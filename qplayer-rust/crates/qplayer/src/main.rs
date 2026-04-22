//! QPlayer binary — custom winit event loop with dual windows.
//!
//! - Control window: egui UI (replaces eframe)
//! - Video output window: wgpu fullscreen blit (lazy-created on first video)
//! - Audio engine: cpal output with master clock for A/V sync
//! - Video decode: background thread that sleeps until frame PTS, then sends
//!   frame to main thread via winit user event.

use qplayer_audio::{AudioEngine, FfmpegDecoder};
use qplayer_gui::{AppCommand, QPlayerApp, SharedStateHandle};
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
    fn handle_go(&mut self, event_loop: &ActiveEventLoop) {
        let cue = {
            let state = self.qplayer.state().lock().unwrap();
            state.selected_cue().cloned()
        };

        let Some(cue) = cue else {
            log::info!("Go pressed but no cue selected");
            return;
        };

        let qid_i32: i32 = cue.base().qid.try_into().unwrap_or(0);
        if let Some(pm) = self.plugin_manager.as_mut() {
            pm.on_go(qid_i32);
        }

        match cue {
            qplayer_core::Cue::Sound { path, .. } => {
                log::info!("Go SoundCue: {}", path);
                self.play_audio(&path);
            }
            qplayer_core::Cue::Video { path, .. } => {
                log::info!("Go VideoCue: {}", path);
                self.play_audio(&path);
                self.play_video(&path, event_loop);
            }
            other => {
                log::info!("Go on unsupported cue type: {:?}", std::mem::discriminant(&other));
            }
        }
    }

    fn play_audio(&self, path: &str) {
        match FfmpegDecoder::open(path) {
            Ok(decoder) => {
                self.audio_engine.play(Box::new(decoder));
            }
            Err(e) => {
                log::error!("Failed to open audio for {}: {}", path, e);
            }
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
        // TODO: stop audio playback
    }

    fn handle_dropped_file(&mut self, path: &Path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase());
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

            let next_qid = state
                .show_file
                .cues
                .iter()
                .map(|c| c.base().qid)
                .max()
                .unwrap_or(rust_decimal::Decimal::ZERO)
                + rust_decimal::Decimal::ONE;

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
                    OscEvent::Pause { .. } => {}
                    OscEvent::Unpause { .. } => {}
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
        while running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(60));
            if !running.load(Ordering::Relaxed) {
                break;
            }
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

    // Notify plugins before shutdown
    if let Some(pm) = app.plugin_manager.as_mut() {
        pm.on_unload();
    }

    // Signal autosave thread to stop
    app.autosave_running.store(false, Ordering::Relaxed);

    Ok(())
}
