//! QPlayer binary — custom winit event loop with dual windows.
//!
//! - Control window: egui UI (replaces eframe)
//! - Video output window: wgpu fullscreen blit (lazy-created on first video)
//! - Audio engine: cpal output with master clock for A/V sync
//! - Video decode: background thread that sleeps until frame PTS, then sends
//!   frame to main thread via winit user event.

use qplayer_audio::{AudioEngine, FfmpegDecoder, SampleProvider};
use qplayer_gui::{AppCommand, QPlayerApp, SharedStateHandle};
use qplayer_gui::app::CueState;
use qplayer_protocols::msc::{MscCommandFlags, MscEvent, MscManager};
use qplayer_protocols::osc::{OscEvent, OscManager};
use qplayer_video::{Renderer, Texture, VideoFrame, VideoSource};
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use human_panic::Metadata;

mod plugin_manager;

/// User events sent to the main event loop from background threads.
#[derive(Debug)]
enum AppEvent {
    /// A decoded video frame ready for display.
    VideoFrame(VideoFrame),
    /// Video stream reached EOF.
    VideoEof,
}

/// Per-window identifiers so we can route events.
struct WindowIds {
    control: WindowId,
    video: Option<WindowId>,
}

#[derive(Clone)]
struct ActiveCue {
    qid: rust_decimal::Decimal,
    name: String,
    input: std::sync::Arc<qplayer_audio::MixerInput>,
    state: CueState,
}

/// A cue that is waiting for its delay timer to expire before playing.
struct DelayedCue {
    cue: qplayer_core::Cue,
    start_at: std::time::Instant,
}

struct App {
    // ── wgpu core ──
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,

    // ── control window (egui) ──
    control_window: Option<Arc<Window>>,
    control_surface: Option<wgpu::Surface<'static>>,
    control_config: Option<wgpu::SurfaceConfiguration>,

    // ── video window (wgpu blit) ──
    video_window: Option<Arc<Window>>,
    video_surface: Option<wgpu::Surface<'static>>,
    video_config: Option<wgpu::SurfaceConfiguration>,

    // ── egui ──
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,

    // ── app state ──
    qplayer: QPlayerApp,
    window_ids: Option<WindowIds>,

    // ── audio ──
    audio_engine: AudioEngine,
    active_cues: Vec<ActiveCue>,
    delayed_cues: Vec<DelayedCue>,
    paused: bool,

    // ── video playback ──
    event_loop_proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    video_texture: Option<Texture>,
    video_renderer: Option<Renderer>,
    latest_video_frame: Option<VideoFrame>,
    video_frame_dirty: bool,
    video_start_clock: Option<Duration>,
    video_stop_flag: Arc<AtomicBool>,

    // ── protocols ──
    osc_manager: Option<OscManager>,
    osc_rx: Option<std::sync::mpsc::Receiver<OscEvent>>,
    #[allow(dead_code)]
    msc_manager: Option<MscManager>,
    msc_rx: Option<std::sync::mpsc::Receiver<MscEvent>>,
    last_discovery: Instant,

    // ── polish ──
    last_window_title: String,
    autosave_running: Arc<AtomicBool>,

    // ── plugins ──
    plugin_manager: Option<plugin_manager::PluginManager>,
    last_slow_update: Instant,
}

impl App {
    fn new(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    ) -> Self {
        let audio_engine = AudioEngine::new_default().expect("audio engine init failed");
        let qplayer = QPlayerApp::new();

        // Sync audio device info into GUI state
        {
            let devices: Vec<String> = AudioEngine::list_devices().into_iter().map(|(n, _)| n).collect();
            let device_name = audio_engine.device_name().to_string();
            if let Ok(mut state) = qplayer.state().lock() {
                state.audio_devices = devices;
                state.audio_device_name = device_name;
            }
        }

        // Default protocol settings (TODO: load from project settings)
        let nic = Ipv4Addr::new(127, 0, 0, 1);
        let subnet = Ipv4Addr::new(255, 255, 255, 0);

        let (osc_manager, osc_rx) = {
            let (tx, rx) = std::sync::mpsc::channel();
            match OscManager::new(nic, 9000, 9001, subnet, tx) {
                Ok(m) => {
                    log::info!("OSC manager started on {}:9000", nic);
                    (Some(m), Some(rx))
                }
                Err(e) => {
                    log::error!("Failed to start OSC manager: {e}");
                    (None, Some(rx))
                }
            }
        };

        let (msc_manager, msc_rx) = {
            let (tx, rx) = std::sync::mpsc::channel();
            match MscManager::new(nic, 7000, 7001, subnet, tx.clone()) {
                Ok(m) => {
                    log::info!("MSC manager started on {}:7000", nic);
                    // Wire default MSC subscriptions
                    m.subscribe(MscCommandFlags::GO | MscCommandFlags::TIMED_GO, move |pkt| {
                        let event = match &pkt.data {
                            qplayer_protocols::msc::MscData::Go { qid, executor, page } => {
                                Some(MscEvent::Go { qid: qid.clone(), executor: *executor, page: *page })
                            }
                            qplayer_protocols::msc::MscData::TimedGo { qid, executor, page, time } => {
                                Some(MscEvent::TimedGo { qid: qid.clone(), executor: *executor, page: *page, time: *time })
                            }
                            _ => None,
                        };
                        if let Some(ev) = event {
                            let _ = tx.send(ev);
                        }
                    });
                    (Some(m), Some(rx))
                }
                Err(e) => {
                    log::error!("Failed to start MSC manager: {e}");
                    (None, Some(rx))
                }
            }
        };

        let autosave_running = Arc::new(AtomicBool::new(true));
        spawn_autosave_thread(Arc::clone(&qplayer.state()), Arc::clone(&autosave_running));

        let mut plugin_manager = plugin_manager::PluginManager::new().ok();
        if let Some(pm) = plugin_manager.as_mut() {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            pm.load_from_dir(&exe_dir.join("plugins"));
        }

        Self {
            instance,
            adapter,
            device,
            queue,
            control_window: None,
            control_surface: None,
            control_config: None,
            video_window: None,
            video_surface: None,
            video_config: None,
            egui_ctx: egui::Context::default(),
            egui_state: None,
            egui_renderer: None,
            qplayer,
            window_ids: None,
            audio_engine,
            event_loop_proxy: proxy,
            video_texture: None,
            video_renderer: None,
            latest_video_frame: None,
            video_frame_dirty: false,
            video_start_clock: None,
            video_stop_flag: Arc::new(AtomicBool::new(false)),
            osc_manager,
            osc_rx,
            msc_manager,
            msc_rx,
            last_discovery: Instant::now(),
            last_window_title: String::new(),
            autosave_running,
            plugin_manager,
            last_slow_update: Instant::now(),
            active_cues: Vec::new(),
            delayed_cues: Vec::new(),
            paused: false,
        }
    }

    /// Create the control window + surface + egui state.
    fn create_control_window(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    winit::window::WindowAttributes::default()
                        .with_title("QPlayer")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0)),
                )
                .expect("create control window"),
        );

        let surface = self
            .instance
            .create_surface(Arc::clone(&window))
            .expect("create control surface");

        let size = window.inner_size();
        let config = surface
            .get_default_config(&self.adapter, size.width, size.height)
            .expect("control surface config");
        surface.configure(&self.device, &config);

        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            None,
            None,
            None,
        );

        let egui_renderer = egui_wgpu::Renderer::new(
            &self.device,
            config.format,
            None,
            1,
            false,
        );

        let control_id = window.id();
        self.control_window = Some(window);
        self.control_surface = Some(surface);
        self.control_config = Some(config);
        self.egui_state = Some(egui_state);
        self.egui_renderer = Some(egui_renderer);

        let video_id = self.video_window.as_ref().map(|w| w.id());
        self.window_ids = Some(WindowIds {
            control: control_id,
            video: video_id,
        });
    }

    /// Create (or recreate) the fullscreen video output window.
    fn create_video_window(&mut self, event_loop: &ActiveEventLoop) {
        if self.video_window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    winit::window::WindowAttributes::default()
                        .with_title("QPlayer Video Output")
                        .with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)))
                        .with_visible(true),
                )
                .expect("create video window"),
        );

        let surface = self
            .instance
            .create_surface(Arc::clone(&window))
            .expect("create video surface");

        let size = window.inner_size();
        let config = surface
            .get_default_config(&self.adapter, size.width, size.height)
            .expect("video surface config");
        surface.configure(&self.device, &config);

        let video_id = window.id();
        self.video_window = Some(window);
        self.video_surface = Some(surface);
        self.video_config = Some(config);

        if let Some(ids) = self.window_ids.as_mut() {
            ids.video = Some(video_id);
        }
    }



    /// Handle a `Go` command: start audio (and video if cue is VideoCue).
    /// Also handles `WithLast` trigger mode for subsequent cues.
    fn handle_go(&mut self, event_loop: &ActiveEventLoop) {
        let (start_qid, start_idx) = {
            let state = self.qplayer.state().lock().unwrap();
            let qid = state.selected_cue_id;
            let idx = qid.and_then(|q| state.show_file.cues.iter().position(|c| c.base().qid == q));
            (qid, idx)
        };

        let Some(start_qid) = start_qid else {
            log::info!("Go pressed but no cue selected");
            return;
        };
        let Some(start_idx) = start_idx else {
            log::warn!("Selected cue Q{} not found in cue list", start_qid);
            return;
        };

        let qid_i32: i32 = start_qid.try_into().unwrap_or(0);
        if let Some(pm) = self.plugin_manager.as_mut() {
            pm.on_go(qid_i32);
        }

        // Play the selected cue and all consecutive WithLast followers
        let cues_to_play = {
            let state = self.qplayer.state().lock().unwrap();
            let mut result = Vec::new();
            for i in start_idx..state.show_file.cues.len() {
                let cue = &state.show_file.cues[i];
                if i == start_idx || cue.base().trigger == qplayer_core::TriggerMode::WithLast {
                    result.push(cue.clone());
                } else {
                    break;
                }
            }
            result
        };

        for cue in cues_to_play {
            self.play_cue(&cue, event_loop);
        }

        // Check for AfterLast cues and schedule them
        let after_last = {
            let state = self.qplayer.state().lock().unwrap();
            let mut after_last_qids = Vec::new();
            for i in (start_idx + 1)..state.show_file.cues.len() {
                let cue = &state.show_file.cues[i];
                if cue.base().trigger == qplayer_core::TriggerMode::AfterLast {
                    after_last_qids.push(cue.base().qid);
                } else {
                    break;
                }
            }
            after_last_qids
        };
        for qid in after_last {
            log::info!("AfterLast cue Q{} scheduled (TODO: auto-trigger when previous finishes)", qid);
        }
    }

    fn play_cue(&mut self, cue: &qplayer_core::Cue, event_loop: &ActiveEventLoop) {
        let qid = cue.base().qid;
        let name = cue.base().name.clone();
        let delay = cue.base().delay;

        // If cue has a delay, schedule it instead of playing immediately
        if delay.as_secs_f64() > 0.0 {
            log::info!("Delaying cue Q{} by {:.2}s", qid, delay.as_secs_f64());
            self.delayed_cues.push(DelayedCue {
                cue: cue.clone(),
                start_at: std::time::Instant::now() + std::time::Duration::from_secs_f64(delay.as_secs_f64()),
            });
            return;
        }

        match cue {
            qplayer_core::Cue::Sound { path, start_time, duration, .. } => {
                log::info!("Go SoundCue: {}", path);
                self.play_audio(path, qid, &name, cue.base().loop_mode, cue.base().loop_count, *start_time, *duration);
            }
            qplayer_core::Cue::Video { path, start_time, duration, .. } => {
                log::info!("Go VideoCue: {}", path);
                self.play_audio(path, qid, &name, cue.base().loop_mode, cue.base().loop_count, *start_time, *duration);
                self.play_video(path, event_loop);
            }
            qplayer_core::Cue::Stop { stop_qid, fade_out_time, fade_type, .. } => {
                log::info!("Go StopCue -> stop Q{}", stop_qid);
                self.handle_stop_cue(*stop_qid, *fade_out_time, *fade_type);
            }
            qplayer_core::Cue::Volume { sound_qid, volume, fade_time, fade_type, .. } => {
                log::info!("Go VolumeCue -> adjust Q{} to {:.1} dB", sound_qid, 20.0 * volume.log10());
                self.handle_volume_cue(*sound_qid, *volume, *fade_time, fade_type);
            }
            other => {
                log::info!("Go on unsupported cue type: {:?}", std::mem::discriminant(other));
            }
        }
    }

    fn play_audio(
        &mut self,
        path: &str,
        qid: rust_decimal::Decimal,
        name: &str,
        loop_mode: qplayer_core::LoopMode,
        loop_count: i32,
        start_time: qplayer_core::Timespan,
        duration: qplayer_core::Timespan,
    ) {
        match FfmpegDecoder::open(path) {
            Ok(decoder) => {
                let sample_rate = decoder.sample_rate();
                let loop_proc = qplayer_audio::LoopProcessor::new(Box::new(decoder));
                let start_frame = (start_time.as_secs_f64() * sample_rate as f64) as u64;
                let end_frame = if duration.as_secs_f64() > 0.0 {
                    start_frame + (duration.as_secs_f64() * sample_rate as f64) as u64
                } else {
                    0 // auto-detect from source length
                };
                loop_proc.set_loop(start_frame, end_frame, loop_mode, loop_count as u32);
                let input = self.audio_engine.play(Box::new(loop_proc));
                let state = if loop_mode == qplayer_core::LoopMode::Looped || loop_mode == qplayer_core::LoopMode::LoopedInfinite {
                    CueState::PlayingLooped
                } else {
                    CueState::Playing
                };
                self.active_cues.push(ActiveCue { qid, name: name.to_string(), input, state });
            }
            Err(e) => {
                log::error!("Failed to open audio for {}: {}", path, e);
            }
        }
    }

    fn handle_stop_cue(&mut self, stop_qid: rust_decimal::Decimal, fade_out_time: f32, fade_type: qplayer_core::FadeType) {
        let idx = self.active_cues.iter().position(|ac| ac.qid == stop_qid);
        if let Some(idx) = idx {
            let input = self.active_cues.remove(idx).input;
            if fade_out_time > 0.0 {
                let initial_volume = input.volume();
                let steps = 20usize;
                let sleep_ms = (fade_out_time * 1000.0) / steps as f32;
                let _fade_type = fade_type;
                std::thread::spawn(move || {
                    for i in 0..=steps {
                        let t = i as f32 / steps as f32;
                        let vol = initial_volume * (1.0 - t);
                        input.set_volume(vol.max(0.0));
                        if i < steps {
                            std::thread::sleep(Duration::from_millis(sleep_ms as u64));
                        }
                    }
                    input.set_active(false);
                });
            } else {
                input.set_active(false);
            }
        } else {
            log::warn!("StopCue target Q{} not found in active cues", stop_qid);
        }
    }

    /// Restart the audio engine with a specific device.
    fn restart_audio_engine(&mut self, device: &cpal::Device) {
        self.stop_all();
        match AudioEngine::new(device) {
            Ok(new_engine) => {
                let name = new_engine.device_name().to_string();
                self.audio_engine = new_engine;
                if let Ok(mut state) = self.qplayer.state().lock() {
                    state.audio_device_name = name;
                }
                log::info!("Switched audio output device");
            }
            Err(e) => {
                log::error!("Failed to switch audio device: {}. Attempting fallback to default.", e);
                if let Ok(fallback) = AudioEngine::new_default() {
                    let name = fallback.device_name().to_string();
                    self.audio_engine = fallback;
                    if let Ok(mut state) = self.qplayer.state().lock() {
                        state.audio_device_name = name;
                    }
                }
            }
        }
    }

    /// Check for cues that have finished playing naturally and trigger AfterLast chains.
    fn check_finished_cues(&mut self, event_loop: &ActiveEventLoop) {
        // Mark finished cues as Done and collect their QIDs
        let finished_qids: Vec<rust_decimal::Decimal> = {
            let mut qids = Vec::new();
            for ac in &mut self.active_cues {
                if ac.input.is_finished() {
                    ac.state = CueState::Done;
                    qids.push(ac.qid);
                }
            }
            qids
        };

        for qid in finished_qids {
            // Remove finished cue from active list
            self.active_cues.retain(|ac| ac.qid != qid);
            log::info!("Cue Q{} finished naturally — checking AfterLast chain", qid);

            // Find the cue's position in the show file
            let state = self.qplayer.state().lock().unwrap();
            let Some(idx) = state.show_file.cues.iter().position(|c| c.base().qid == qid) else {
                continue;
            };

            // Collect consecutive AfterLast cues after this one
            let mut after_last_cues = Vec::new();
            for i in (idx + 1)..state.show_file.cues.len() {
                let cue = &state.show_file.cues[i];
                if cue.base().trigger == qplayer_core::TriggerMode::AfterLast {
                    after_last_cues.push(cue.clone());
                } else {
                    break;
                }
            }
            drop(state);

            // Play AfterLast chain: non-audio cues fire immediately in a burst,
            // then the first audio cue starts and will trigger its own chain when it finishes.
            for cue in after_last_cues {
                let is_audio = matches!(cue, qplayer_core::Cue::Sound { .. } | qplayer_core::Cue::Video { .. });
                self.play_cue(&cue, event_loop);
                if is_audio {
                    break; // wait for this audio cue to finish before continuing the chain
                }
            }
        }
    }

    fn handle_volume_cue(&mut self, sound_qid: rust_decimal::Decimal, target_volume: f32, fade_time: f32, fade_type: &qplayer_core::FadeType) {
        let target = self.active_cues.iter().find(|ac| ac.qid == sound_qid).cloned();
        if let Some(input) = target.map(|ac| ac.input) {
            if fade_time > 0.0 {
                let initial_volume = input.volume();
                let steps = 20usize;
                let sleep_ms = (fade_time * 1000.0) / steps as f32;
                let _fade_type = *fade_type;
                std::thread::spawn(move || {
                    for i in 0..=steps {
                        let t = i as f32 / steps as f32;
                        // Simple linear interpolation for now; fade_type ignored in thread-based fade
                        let vol = initial_volume + (target_volume - initial_volume) * t;
                        input.set_volume(vol.max(0.0));
                        if i < steps {
                            std::thread::sleep(Duration::from_millis(sleep_ms as u64));
                        }
                    }
                });
            } else {
                input.set_volume(target_volume.max(0.0));
            }
        } else {
            log::warn!("VolumeCue target Q{} not found in active cues", sound_qid);
        }
    }

    fn play_video(&mut self, path: &str, event_loop: &ActiveEventLoop) {
        self.create_video_window(event_loop);
        self.video_stop_flag.store(false, Ordering::Relaxed);
        self.video_start_clock = Some(self.audio_engine.playback_time());
        self.latest_video_frame = None;
        self.video_frame_dirty = false;

        // Create video texture/renderer if not yet created
        if self.video_texture.is_none() {
            let texture = Texture::new(&self.device, 1920, 1080);
            let renderer = Renderer::new(&self.device, texture.bind_group_layout());
            self.video_texture = Some(texture);
            self.video_renderer = Some(renderer);
        }

        // Spawn decode thread
        let path = path.to_string();
        let clock = {
            let mixer = Arc::clone(self.audio_engine.mixer());
            Arc::new(move || mixer.playback_time()) as Arc<dyn Fn() -> Duration + Send + Sync>
        };
        let start = self.video_start_clock.unwrap();
        let stop_flag = Arc::clone(&self.video_stop_flag);
        let proxy = self.event_loop_proxy.clone();

        std::thread::Builder::new()
            .name("video-decode".into())
            .spawn(move || {
                video_decode_thread(&path, clock, start, stop_flag, proxy);
            })
            .expect("spawn video decode thread");
    }

    fn stop_all(&mut self) {
        self.video_stop_flag.store(true, Ordering::Relaxed);
        self.latest_video_frame = None;
        self.video_frame_dirty = false;
        self.video_start_clock = None;
        self.audio_engine.stop_all();
        self.active_cues.clear();
        self.delayed_cues.clear();
        self.paused = false;
    }

    fn pause_all(&mut self) {
        for ac in &mut self.active_cues {
            ac.input.set_active(false);
            if ac.state == CueState::Playing || ac.state == CueState::PlayingLooped {
                ac.state = CueState::Paused;
            }
        }
        self.paused = true;
        log::info!("Paused {} cue(s)", self.active_cues.len());
    }

    fn resume_all(&mut self) {
        for ac in &mut self.active_cues {
            ac.input.set_active(true);
            if ac.state == CueState::Paused {
                ac.state = CueState::Playing;
            }
        }
        self.paused = false;
        log::info!("Resumed {} cue(s)", self.active_cues.len());
    }

    fn handle_dropped_file(&mut self, path: &Path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase());

        // Open project files directly
        if ext.as_deref() == Some("qproj") {
            if let Ok(mut state) = self.qplayer.state().lock() {
                state.command_queue.push(qplayer_gui::AppCommand::OpenProject {
                    path: path.to_path_buf(),
                });
            }
            return;
        }

        let is_video = matches!(ext.as_deref(), Some("mp4") | Some("mov") | Some("mkv") | Some("avi"));
        let is_audio = matches!(
            ext.as_deref(),
            Some("wav") | Some("mp3") | Some("flac") | Some("ogg") | Some("aiff") | Some("wma")
        );
        if !is_video && !is_audio {
            log::warn!("Dropped file has unsupported extension: {:?}", path);
            return;
        }

        let path_str = path.to_string_lossy().to_string();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Dropped")
            .to_string();

        if let Ok(mut state) = self.qplayer.state().lock() {
            let snapshot = qplayer_gui::app::Snapshot::from_state(&state);
            state.undo_redo.push(snapshot);

            let next_qid = state.show_file.choose_qid(state.selected_cue_id);

            let base = qplayer_core::CueBase {
                qid: next_qid,
                name,
                ..Default::default()
            };

            let cue = if is_video {
                qplayer_core::Cue::Video {
                    base,
                    path: path_str,
                    start_time: qplayer_core::Timespan::ZERO,
                    duration: qplayer_core::Timespan::ZERO,
                    volume: 0.0,
                    pan: 0.0,
                    fade_in: 0.0,
                    fade_out: 0.0,
                    fade_type: qplayer_core::FadeType::Linear,
                    eq: None,
                }
            } else {
                qplayer_core::Cue::Sound {
                    base,
                    path: path_str,
                    start_time: qplayer_core::Timespan::ZERO,
                    duration: qplayer_core::Timespan::ZERO,
                    volume: 0.0,
                    pan: 0.0,
                    fade_in: 0.0,
                    fade_out: 0.0,
                    fade_type: qplayer_core::FadeType::Linear,
                    eq: None,
                }
            };

            state.show_file.cues.push(cue);
            state.dirty = true;
            log::info!("Added dropped file as cue {}: {:?}", next_qid, path);
        }
    }

    /// Drain any AppCommands queued by the UI and execute them.
    fn process_commands(&mut self, event_loop: &ActiveEventLoop) {
        let commands = {
            let Ok(mut state) = self.qplayer.state().lock() else { return };
            let cmds = state.command_queue.clone();
            state.command_queue.clear();
            cmds
        };

        for cmd in commands {
            match cmd {
                AppCommand::Go => self.handle_go(event_loop),
                AppCommand::Stop => self.stop_all(),
                AppCommand::Pause => {
                    if self.paused {
                        self.resume_all();
                    } else {
                        self.pause_all();
                    }
                }
                AppCommand::SetLimiterThreshold(threshold) => {
                    self.audio_engine.set_limiter_threshold(threshold);
                    log::info!("Set master limiter threshold to {:.2} dB", 20.0 * threshold.log10());
                }
                AppCommand::SetAudioDevice(name) => {
                    let devices = AudioEngine::list_devices();
                    if let Some((_, device)) = devices.into_iter().find(|(n, _)| n == &name) {
                        self.restart_audio_engine(&device);
                    } else {
                        log::warn!("Audio device '{}' not found", name);
                    }
                }
                AppCommand::SaveProject | AppCommand::SaveProjectAs { .. } => {
                    if let Some(pm) = self.plugin_manager.as_mut() {
                        pm.on_save();
                    }
                }
                _ => {}
            }
        }
    }

    /// Drain OSC/MSC events and translate them into AppCommands.
    fn process_protocol_events(&mut self) {
        if let Some(rx) = &self.osc_rx {
            while let Ok(ev) = rx.try_recv() {
                log::debug!("OSC event: {ev:?}");
                match ev {
                    OscEvent::Go { qid } => {
                        if let Some(qid_str) = qid {
                            if let Ok(qid_dec) = qid_str.parse::<rust_decimal::Decimal>() {
                                let _ = self.qplayer.state().lock().map(|mut s| s.selected_cue_id = Some(qid_dec));
                            }
                        }
                        if let Ok(mut state) = self.qplayer.state().lock() {
                            state.command_queue.push(AppCommand::Go);
                        }
                    }
                    OscEvent::Stop { qid: _ } => {
                        if let Ok(mut state) = self.qplayer.state().lock() {
                            state.command_queue.push(AppCommand::Stop);
                        }
                    }
                    OscEvent::Pause { .. } => {
                        if let Ok(mut state) = self.qplayer.state().lock() {
                            state.command_queue.push(AppCommand::Pause);
                        }
                    }
                    OscEvent::Unpause { .. } => {
                        if self.paused {
                            if let Ok(mut state) = self.qplayer.state().lock() {
                                state.command_queue.push(AppCommand::Pause);
                            }
                        }
                    }
                    OscEvent::Select { qid } => {
                        if let Ok(qid_dec) = qid.parse::<rust_decimal::Decimal>() {
                            let _ = self.qplayer.state().lock().map(|mut s| s.selected_cue_id = Some(qid_dec));
                        }
                    }
                    OscEvent::Up => {}
                    OscEvent::Down => {}
                    OscEvent::Save => {
                        if let Ok(mut state) = self.qplayer.state().lock() {
                            state.command_queue.push(AppCommand::SaveProject);
                        }
                    }
                    OscEvent::RemotePing => {
                        if let Some(osc) = &self.osc_manager {
                            let _ = osc.send(rosc::OscMessage {
                                addr: "/qplayer/remote/pong".into(),
                                args: vec![],
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        if let Some(rx) = &self.msc_rx {
            while let Ok(ev) = rx.try_recv() {
                log::debug!("MSC event: {ev:?}");
                match ev {
                    MscEvent::Go { qid, .. } | MscEvent::TimedGo { qid, .. } => {
                        if let Ok(qid_dec) = qid.parse::<rust_decimal::Decimal>() {
                            let _ = self.qplayer.state().lock().map(|mut s| s.selected_cue_id = Some(qid_dec));
                        }
                        if let Ok(mut state) = self.qplayer.state().lock() {
                            state.command_queue.push(AppCommand::Go);
                        }
                    }
                    MscEvent::Stop { .. } => {
                        if let Ok(mut state) = self.qplayer.state().lock() {
                            state.command_queue.push(AppCommand::Stop);
                        }
                    }
                    MscEvent::Resume { .. } => {}
                    _ => {}
                }
            }
        }

        // Discovery broadcast every 1 second
        if self.last_discovery.elapsed() >= Duration::from_secs(1) {
            self.last_discovery = Instant::now();
            if let Some(osc) = &self.osc_manager {
                let _ = osc.send(rosc::OscMessage {
                    addr: "/qplayer/remote/discovery".into(),
                    args: vec![rosc::OscType::String("QPlayer-Rust".into())],
                });
            }
        }
    }

    /// Render the control window (egui).
    fn update_window_title(&mut self) {
        let (path, dirty) = {
            let Ok(state) = self.qplayer.state().lock() else { return };
            (state.project_path.clone(), state.dirty)
        };
        let name = path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled");
        let title = if dirty {
            format!("QPlayer — {} *", name)
        } else {
            format!("QPlayer — {}", name)
        };
        if self.last_window_title != title {
            self.last_window_title = title.clone();
            if let Some(window) = self.control_window.as_ref() {
                window.set_title(&title);
            }
        }
    }

    fn render_control(&mut self, event_loop: &ActiveEventLoop) {
        self.check_finished_cues(event_loop);

        // Check for delayed cues whose timer has expired
        {
            let now = std::time::Instant::now();
            let mut ready = Vec::new();
            self.delayed_cues.retain(|dc| {
                if dc.start_at <= now {
                    ready.push(dc.cue.clone());
                    false
                } else {
                    true
                }
            });
            for cue in ready {
                self.play_cue(&cue, event_loop);
            }
        }

        self.update_window_title();
        let Some(surface) = self.control_surface.as_ref() else { return };
        let Some(config) = self.control_config.as_ref() else { return };
        let Some(window) = self.control_window.as_ref() else { return };
        let Some(egui_state) = self.egui_state.as_mut() else { return };
        let Some(egui_renderer) = self.egui_renderer.as_mut() else { return };

        let output = match surface.get_current_texture() {
            Ok(o) => o,
            Err(e) => {
                log::warn!("Control surface acquire failed: {e}");
                return;
            }
        };
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let raw_input = egui_state.take_egui_input(window);
        // Sync active cue state into the GUI shared state
        {
            let gui_active: Vec<qplayer_gui::ActiveCueInfo> = self.active_cues.iter().map(|ac| {
                qplayer_gui::ActiveCueInfo {
                    qid: ac.qid,
                    name: ac.name.clone(),
                    volume: ac.input.volume(),
                    paused: !ac.input.is_active(),
                    position: ac.input.position(),
                    length: ac.input.length(),
                    state: ac.state,
                }
            }).collect();
            if let Ok(mut state) = self.qplayer.state().lock() {
                state.active_cues = gui_active;
            }
        }

        // Sync master meter data into the GUI shared state
        {
            let meters = self.audio_engine.read_meters();
            let peak_l_db = if meters.peak_l > 0.0 { 20.0 * meters.peak_l.log10() } else { -f32::INFINITY };
            let peak_r_db = if meters.peak_r > 0.0 { 20.0 * meters.peak_r.log10() } else { -f32::INFINITY };
            let rms_l_db = if meters.rms_l > 0.0 { 20.0 * meters.rms_l.log10() } else { -f32::INFINITY };
            let rms_r_db = if meters.rms_r > 0.0 { 20.0 * meters.rms_r.log10() } else { -f32::INFINITY };
            if let Ok(mut state) = self.qplayer.state().lock() {
                state.meter_data = qplayer_gui::GuiMeterData {
                    peak_l_db,
                    peak_r_db,
                    rms_l_db,
                    rms_r_db,
                    clipped: false, // TODO: expose clip flag from MeteringProcessor
                };
            }
        }

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            self.qplayer.update(ctx);
        });
        egui_state.handle_platform_output(window, full_output.platform_output);

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [config.width, config.height],
            pixels_per_point: window.scale_factor() as f32 * self.egui_ctx.zoom_factor(),
        };

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("control-encoder"),
        });

        let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        for (id, image_delta) in &full_output.textures_delta.set {
            egui_renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }
        egui_renderer.update_buffers(&self.device, &self.queue, &mut encoder, &paint_jobs, &screen_descriptor);

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("control-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            egui_renderer.render(&mut render_pass.forget_lifetime(), &paint_jobs, &screen_descriptor);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // Process commands that were queued during the UI frame
        self.process_commands(event_loop);
    }

    /// Render the video output window.
    fn render_video(&mut self) {
        let Some(surface) = self.video_surface.as_ref() else { return };
        let Some(texture) = self.video_texture.as_mut() else { return };
        let Some(renderer) = self.video_renderer.as_ref() else { return };

        if self.video_frame_dirty {
            if let Some(frame) = self.latest_video_frame.as_ref() {
                texture.upload(&self.queue, frame);
            }
            self.video_frame_dirty = false;
        }

        let output = match surface.get_current_texture() {
            Ok(o) => o,
            Err(e) => {
                log::warn!("Video surface acquire failed: {e}");
                return;
            }
        };
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("video-encoder"),
        });

        renderer.render(&mut encoder, &view, texture.current_bind_group());
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.control_window.is_none() {
            self.create_control_window(event_loop);
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::VideoFrame(frame) => {
                self.latest_video_frame = Some(frame);
                self.video_frame_dirty = true;
                if let Some(window) = self.video_window.as_ref() {
                    window.request_redraw();
                }
            }
            AppEvent::VideoEof => {
                log::info!("Video EOF");
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let is_control = self
            .window_ids
            .as_ref()
            .map(|ids| ids.control == window_id)
            .unwrap_or(false);
        let is_video = self
            .window_ids
            .as_ref()
            .map(|ids| ids.video == Some(window_id))
            .unwrap_or(false);

        if is_control {
            if let (Some(egui_state), Some(window)) = (self.egui_state.as_mut(), self.control_window.as_ref()) {
                let _response = egui_state.on_window_event(window, &event);
            }

            match event {
                WindowEvent::CloseRequested => {
                    let has_running = !self.active_cues.is_empty();
                    if has_running {
                        let choice = rfd::MessageDialog::new()
                            .set_title("Running Cues")
                            .set_description("There are cues currently playing. Stop them and exit?")
                            .set_buttons(rfd::MessageButtons::OkCancel)
                            .show();
                        if !matches!(choice, rfd::MessageDialogResult::Ok) {
                            return;
                        }
                        self.stop_all();
                    }
                    let dirty = self.qplayer.state().lock().map(|s| s.dirty).unwrap_or(false);
                    if dirty {
                        let choice = rfd::MessageDialog::new()
                            .set_title("Unsaved Changes")
                            .set_description("You have unsaved changes. Discard them?")
                            .set_buttons(rfd::MessageButtons::OkCancel)
                            .show();
                        if !matches!(choice, rfd::MessageDialogResult::Ok) {
                            return;
                        }
                    }
                    event_loop.exit();
                }
                WindowEvent::Resized(size) => {
                    if size.width > 0 && size.height > 0 {
                        if let Some(config) = self.control_config.as_mut() {
                            config.width = size.width;
                            config.height = size.height;
                        }
                        if let Some(surface) = self.control_surface.as_ref() {
                            if let Some(config) = self.control_config.as_ref() {
                                surface.configure(&self.device, config);
                            }
                        }
                    }
                }
                WindowEvent::DroppedFile(path) => {
                    self.handle_dropped_file(&path);
                }
                WindowEvent::RedrawRequested => {
                    self.render_control(event_loop);
                    if let Some(window) = self.control_window.as_ref() {
                        window.request_redraw();
                    }
                }
                _ => {}
            }
        } else if is_video {
            match event {
                WindowEvent::CloseRequested => {
                    self.video_window = None;
                    self.video_surface = None;
                    self.video_config = None;
                    if let Some(ids) = self.window_ids.as_mut() {
                        ids.video = None;
                    }
                }
                WindowEvent::Resized(size) => {
                    if size.width > 0 && size.height > 0 {
                        if let Some(config) = self.video_config.as_mut() {
                            config.width = size.width;
                            config.height = size.height;
                        }
                        if let Some(surface) = self.video_surface.as_ref() {
                            if let Some(config) = self.video_config.as_ref() {
                                surface.configure(&self.device, config);
                            }
                        }
                    }
                }
                WindowEvent::RedrawRequested => {
                    self.render_video();
                    if let Some(window) = self.video_window.as_ref() {
                        window.request_redraw();
                    }
                }
                _ => {}
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.process_protocol_events();

        // Plugin slow update every 250 ms
        if self.last_slow_update.elapsed() >= Duration::from_millis(250) {
            self.last_slow_update = Instant::now();
            if let Some(pm) = self.plugin_manager.as_mut() {
                pm.on_slow_update();
            }
        }

        // Continuously redraw both windows when active.
        if let Some(window) = self.control_window.as_ref() {
            window.request_redraw();
        }
        if let Some(window) = self.video_window.as_ref() {
            window.request_redraw();
        }
    }
}

/// Video decode thread: sleeps until each frame's PTS, then sends it to the main loop.
fn video_decode_thread(
    path: &str,
    clock: Arc<dyn Fn() -> Duration + Send + Sync>,
    start_clock: Duration,
    stop_flag: Arc<AtomicBool>,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
) {
    let mut source = match VideoSource::open(path, 1920, 1080) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Failed to open video source {}: {e}", path);
            return;
        }
    };

    while !stop_flag.load(Ordering::Relaxed) {
        match source.read_frame() {
            Some(frame) => {
                let elapsed = clock().saturating_sub(start_clock);
                let frame_due = Duration::from_secs_f64(frame.pts.max(0.0));

                if frame_due > elapsed {
                    let sleep_for = frame_due - elapsed;
                    // Cap sleep to avoid missing stop signals for too long
                    std::thread::sleep(sleep_for.min(Duration::from_millis(50)));
                }

                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                if proxy.send_event(AppEvent::VideoFrame(frame)).is_err() {
                    break;
                }
            }
            None => {
                let _ = proxy.send_event(AppEvent::VideoEof);
                break;
            }
        }
    }
}

/// Autosave background thread: writes dirty show file to rotating backups every 60 s.
fn spawn_autosave_thread(state: SharedStateHandle, running: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut slot = 0usize;
        let mut elapsed = 0u64;
        while running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
            if !running.load(Ordering::Relaxed) {
                break;
            }
            elapsed += 1;
            if elapsed < 60 {
                continue;
            }
            elapsed = 0;
            let (should_save, path) = {
                let Ok(state) = state.lock() else { continue };
                (state.dirty, state.project_path.clone())
            };
            if !should_save {
                continue;
            }
            let Some(_project_path) = path else { continue };

            let dir = dirs::data_dir()
                .unwrap_or_else(|| std::env::temp_dir())
                .join("QPlayer");
            if let Err(e) = std::fs::create_dir_all(&dir) {
                log::warn!("Autosave: failed to create dir {:?}: {}", dir, e);
                continue;
            }

            slot = (slot % 5) + 1;
            let backup_path = dir.join(format!("autoback_{}.qproj", slot));
            let json = {
                let Ok(state) = state.lock() else { continue };
                match serde_json::to_string_pretty(&state.show_file) {
                    Ok(j) => j,
                    Err(e) => {
                        log::warn!("Autosave: serialization failed: {}", e);
                        continue;
                    }
                }
            };
            if let Err(e) = std::fs::write(&backup_path, json) {
                log::warn!("Autosave: failed to write {:?}: {}", backup_path, e);
            } else {
                log::info!("Autosaved to {:?}", backup_path);
            }
        }
    });
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct AppSettings {
    recent_files: Vec<std::path::PathBuf>,
}

fn settings_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|p| p.join("QPlayer").join("settings.json"))
}

fn load_settings() -> AppSettings {
    if let Some(path) = settings_path() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str(&data) {
                return settings;
            }
        }
    }
    AppSettings::default()
}

fn save_settings(settings: &AppSettings) {
    if let Some(path) = settings_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string_pretty(settings) {
            let _ = std::fs::write(path, data);
        }
    }
}

/// Attempt an emergency save before the process exits.
fn emergency_save(state: &SharedStateHandle) {
    let (json, path) = {
        let Ok(state) = state.lock() else { return };
        let json = match serde_json::to_string_pretty(&state.show_file) {
            Ok(j) => j,
            Err(e) => {
                log::error!("Emergency save: serialization failed: {}", e);
                return;
            }
        };
        (json, state.project_path.clone())
    };

    let dir = dirs::data_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("QPlayer");
    let _ = std::fs::create_dir_all(&dir);

    // Prefer crash_recovery.qproj, but if a project_path exists, also save there
    let crash_path = dir.join("crash_recovery.qproj");
    if let Err(e) = std::fs::write(&crash_path, &json) {
        log::error!("Emergency save: failed to write {:?}: {}", crash_path, e);
    } else {
        log::info!("Emergency save written to {:?}", crash_path);
    }

    if let Some(project_path) = path {
        if let Err(e) = std::fs::write(&project_path, &json) {
            log::error!("Emergency save: failed to overwrite {:?}: {}", project_path, e);
        } else {
            log::info!("Emergency save overwritten {:?}", project_path);
        }
    }
}

fn main() -> anyhow::Result<()> {
    // Single instance guard
    let single = single_instance::SingleInstance::new("QPlayer_rust_port").unwrap();
    if !single.is_single() {
        log::warn!("Another instance of QPlayer is already running. Exiting.");
        return Ok(());
    }

    human_panic::setup_panic!(
        Metadata::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
            .authors("QPlayer Contributors")
            .homepage("https://github.com/BlueJayLouche/QPlayer")
    );

    env_logger::init();

    let event_loop = EventLoop::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let proxy = event_loop.create_proxy();

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    // Create a headless adapter first (we'll create surfaces after windows exist)
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|e| anyhow::anyhow!("no wgpu adapter: {e}"))?;

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("qplayer-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        },
    ))?;

    let mut app = App::new(instance, adapter, device, queue, proxy);

    // Load persisted settings and sync audio device name
    let settings = load_settings();
    let device_name = app.audio_engine.device_name().to_string();
    if let Ok(mut state) = app.qplayer.state().lock() {
        state.recent_files = settings.recent_files;
        state.audio_device_name = device_name;
    }

    // Ctrl-C / SIGTERM handler for graceful emergency save
    {
        let state = Arc::clone(app.qplayer.state());
        ctrlc::set_handler(move || {
            log::info!("SIGINT received, performing emergency save...");
            emergency_save(&state);
            std::process::exit(0);
        })?;
    }

    event_loop.run_app(&mut app)?;

    // Save persisted settings
    let recent_files = app.qplayer.state().lock().map(|s| s.recent_files.clone()).unwrap_or_default();
    save_settings(&AppSettings { recent_files });

    // Notify plugins before shutdown
    if let Some(pm) = app.plugin_manager.as_mut() {
        pm.on_unload();
    }

    // Signal autosave thread to stop
    app.autosave_running.store(false, Ordering::Relaxed);

    Ok(())
}
