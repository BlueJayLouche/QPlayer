//! Main application state and eframe integration.

use qplayer_core::{Cue, ShowFile};
use rust_decimal::Decimal;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A full snapshot of editable state for undo/redo.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub show_file: ShowFile,
    pub project_path: Option<PathBuf>,
    pub selected_cue_id: Option<Decimal>,
    pub show_mode: ShowMode,
    pub dirty: bool,
}

impl Snapshot {
    pub fn from_state(state: &SharedState) -> Self {
        Self {
            show_file: state.show_file.clone(),
            project_path: state.project_path.clone(),
            selected_cue_id: state.selected_cue_id,
            show_mode: state.show_mode,
            dirty: state.dirty,
        }
    }

    pub fn apply(self, state: &mut SharedState) {
        state.show_file = self.show_file;
        state.project_path = self.project_path;
        state.selected_cue_id = self.selected_cue_id;
        state.show_mode = self.show_mode;
        state.dirty = self.dirty;
    }
}

/// Undo/redo history with a configurable max depth.
#[derive(Debug, Clone)]
pub struct UndoRedo {
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    max_depth: usize,
    /// When true, snapshot capture is suppressed (used during undo/redo itself)
    pub suppress: bool,
}

impl UndoRedo {
    pub fn new(max_depth: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_depth,
            suppress: false,
        }
    }

    /// Push a snapshot onto the undo stack, clearing the redo stack.
    pub fn push(&mut self, snapshot: Snapshot) {
        if self.suppress {
            return;
        }
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// Pop the most recent snapshot and return it, pushing current state to redo.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let prev = self.undo_stack.pop()?;
        self.redo_stack.push(current);
        Some(prev)
    }

    /// Pop the most recent redo snapshot and return it, pushing current state to undo.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo_stack.pop()?;
        self.undo_stack.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

impl Default for UndoRedo {
    fn default() -> Self {
        Self::new(50)
    }
}

/// Lightweight info about a cue currently playing, synced from the audio engine.
#[derive(Debug, Clone, Default)]
pub struct ActiveCueInfo {
    pub qid: Decimal,
    pub name: String,
    /// Linear volume (0.0 – 1.0+).
    pub volume: f32,
    /// True if the cue is currently paused.
    pub paused: bool,
}

/// Master meter data synced from the audio engine.
#[derive(Debug, Clone, Copy, Default)]
pub struct GuiMeterData {
    pub peak_l_db: f32,
    pub peak_r_db: f32,
    pub rms_l_db: f32,
    pub rms_r_db: f32,
    pub clipped: bool,
}

/// Central mutable state shared between GUI and audio/control threads.
#[derive(Debug)]
pub struct SharedState {
    pub show_file: ShowFile,
    pub project_path: Option<PathBuf>,
    pub selected_cue_id: Option<Decimal>,
    pub command_queue: Vec<AppCommand>,
    pub show_mode: ShowMode,
    pub dirty: bool,
    pub undo_redo: UndoRedo,
    pub active_cues: Vec<ActiveCueInfo>,
    pub meter_data: GuiMeterData,
    /// Recently opened/saved project paths (most recent first, max 10).
    pub recent_files: Vec<PathBuf>,
    /// Whether the project settings window is open.
    pub show_settings_window: bool,
    /// Current audio output device name.
    pub audio_device_name: String,
    /// Cached waveform peaks: path → Vec<(min, max)>.
    pub waveform_cache: std::collections::HashMap<String, Vec<(f32, f32)>>,
    /// Paths currently being processed for waveform generation.
    pub pending_waveforms: std::collections::HashSet<String>,
    /// Available audio output device names (populated at startup).
    pub audio_devices: Vec<String>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            show_file: ShowFile::default(),
            project_path: None,
            selected_cue_id: None,
            command_queue: Vec::new(),
            show_mode: ShowMode::Edit,
            dirty: false,
            undo_redo: UndoRedo::default(),
            active_cues: Vec::new(),
            meter_data: GuiMeterData::default(),
            recent_files: Vec::new(),
            show_settings_window: false,
            audio_device_name: String::new(),
            waveform_cache: std::collections::HashMap::new(),
            pending_waveforms: std::collections::HashSet::new(),
            audio_devices: Vec::new(),
        }
    }
}

impl SharedState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a path to the recent files list, moving it to the front if it already exists.
    pub fn push_recent_file(&mut self, path: &std::path::Path) {
        let path_buf = path.to_path_buf();
        self.recent_files.retain(|p| p != &path_buf);
        self.recent_files.insert(0, path_buf);
        self.recent_files.truncate(10);
    }

    pub fn load_show_file(&mut self, path: &std::path::Path, data: &str) -> Result<(), serde_json::Error> {
        let show: ShowFile = serde_json::from_str(data)?;
        self.show_file = show;
        self.project_path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    pub fn selected_cue(&self) -> Option<&Cue> {
        let id = self.selected_cue_id?;
        self.show_file.cues.iter().find(|c| c.base().qid == id)
    }

    pub fn selected_cue_mut(&mut self) -> Option<&mut Cue> {
        let id = self.selected_cue_id?;
        self.show_file.cues.iter_mut().find(|c| c.base().qid == id)
    }
}

pub type SharedStateHandle = Arc<Mutex<SharedState>>;

#[derive(Debug, Clone)]
pub enum AppCommand {
    NewProject,
    OpenProject { path: PathBuf },
    SaveProject,
    SaveProjectAs { path: PathBuf },
    Go,
    Stop,
    Pause,
    SelectCue(Decimal),
    Undo,
    Redo,
    AddCue { cue_type: CueType },
    DeleteSelectedCue,
    DuplicateSelectedCue,
    MoveSelectedCueUp,
    MoveSelectedCueDown,
    MoveCue { from_idx: usize, to_idx: usize },
    SetLimiterThreshold(f32),
    SetAudioDevice(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueType {
    Sound,
    Video,
    Stop,
    Volume,
    Group,
    Dummy,
    TimeCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowMode {
    Edit,
    Show,
}

/// The main egui application.
pub struct QPlayerApp {
    state: SharedStateHandle,
}

impl Default for QPlayerApp {
    fn default() -> Self {
        Self::new()
    }
}

impl QPlayerApp {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SharedState::new())),
        }
    }

    pub fn with_show_file(show: ShowFile, path: Option<PathBuf>) -> Self {
        Self {
            state: Arc::new(Mutex::new(SharedState {
                show_file: show,
                project_path: path,
                ..SharedState::default()
            })),
        }
    }

    pub fn state(&self) -> &SharedStateHandle {
        &self.state
    }
}

impl QPlayerApp {
    pub fn update(&mut self, ctx: &egui::Context) {
        // Keyboard shortcuts
        ctx.input(|i| {
            let modifiers = i.modifiers;

            // Undo / Redo
            if modifiers.command && i.key_pressed(egui::Key::Z) {
                let cmd = if modifiers.shift { AppCommand::Redo } else { AppCommand::Undo };
                if let Ok(mut state) = self.state.lock() {
                    state.command_queue.push(cmd);
                }
            }

            // New / Open / Save
            if modifiers.command && i.key_pressed(egui::Key::N) {
                if let Ok(mut state) = self.state.lock() {
                    state.command_queue.push(AppCommand::NewProject);
                }
            }
            if modifiers.command && i.key_pressed(egui::Key::O) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("QPlayer project", &["qproj"])
                    .pick_file()
                {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::OpenProject { path });
                    }
                }
            }
            if modifiers.command && i.key_pressed(egui::Key::S) {
                if let Ok(mut state) = self.state.lock() {
                    state.command_queue.push(AppCommand::SaveProject);
                }
            }

            // Delete selected cue
            if i.key_pressed(egui::Key::Delete) {
                if let Ok(mut state) = self.state.lock() {
                    state.command_queue.push(AppCommand::DeleteSelectedCue);
                }
            }

            // Duplicate selected cue
            if modifiers.command && i.key_pressed(egui::Key::D) {
                if let Ok(mut state) = self.state.lock() {
                    state.command_queue.push(AppCommand::DuplicateSelectedCue);
                }
            }

            // Add new sound cue
            if modifiers.command && i.key_pressed(egui::Key::T) {
                if let Ok(mut state) = self.state.lock() {
                    state.command_queue.push(AppCommand::AddCue { cue_type: CueType::Sound });
                }
            }

            // Move selected cue up/down
            if modifiers.command {
                if i.key_pressed(egui::Key::ArrowUp) {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::MoveSelectedCueUp);
                    }
                }
                if i.key_pressed(egui::Key::ArrowDown) {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::MoveSelectedCueDown);
                    }
                }
            }

            // Go / Stop / Pause (transport shortcuts)
            if !modifiers.command && !modifiers.alt {
                if i.key_pressed(egui::Key::Space) {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::Go);
                    }
                }
                if i.key_pressed(egui::Key::Escape) {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::Stop);
                    }
                }
            }
        });

        // Top menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.menu_bar(ui);
        });

        // Transport controls
        egui::TopBottomPanel::top("transport").show(ctx, |ui| {
            crate::transport::show(ui, &self.state);
        });

        // Active cues panel (left side)
        egui::SidePanel::left("active_cues")
            .default_width(220.0)
            .show(ctx, |ui| {
                crate::active_cues::show(ui, &self.state);
            });

        // Cue inspector (right side)
        egui::SidePanel::right("inspector")
            .default_width(280.0)
            .show(ctx, |ui| {
                crate::inspector::show(ui, &self.state);
            });

        // Main cue list
        egui::CentralPanel::default().show(ctx, |ui| {
            crate::cue_list::show(ui, &self.state);
        });

        // Project settings window
        let mut show_settings = if let Ok(state) = self.state.lock() {
            state.show_settings_window
        } else {
            false
        };
        if show_settings {
            let mut settings_changed = false;
            let mut limiter_cmd: Option<AppCommand> = None;
            let mut audio_device_cmd: Option<AppCommand> = None;
            egui::Window::new("Project Settings")
                .collapsible(false)
                .resizable(true)
                .default_size([380.0, 520.0])
                .open(&mut show_settings)
                .show(ctx, |ui| {
                    if let Ok(mut state) = self.state.lock() {
                        let devices = state.audio_devices.clone();
                        let current_device = state.audio_device_name.clone();
                        let threshold = state.command_queue.iter().rev().find_map(|cmd| {
                            if let AppCommand::SetLimiterThreshold(t) = cmd { Some(*t) } else { None }
                        }).unwrap_or(0.95);
                        let settings = &mut state.show_file.show_settings;

                        egui::CollapsingHeader::new("Show Info").default_open(true).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Title:");
                                settings_changed |= ui.text_edit_singleline(&mut settings.title).changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("Author:");
                                settings_changed |= ui.text_edit_singleline(&mut settings.author).changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("Description:");
                                settings_changed |= ui.text_edit_singleline(&mut settings.description).changed();
                            });
                        });
                        ui.separator();

                        egui::CollapsingHeader::new("Audio").default_open(true).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Latency (ms):");
                                settings_changed |= ui.add(egui::DragValue::new(&mut settings.audio_latency).speed(1).range(10..=500)).changed();
                            });
                            settings_changed |= ui.checkbox(&mut settings.exclusive_mode, "Exclusive Mode").changed();

                            ui.horizontal(|ui| {
                                ui.label("Output Device:");
                                egui::ComboBox::from_id_salt("audio_device")
                                    .selected_text(&current_device)
                                    .width(200.0)
                                    .show_ui(ui, |ui| {
                                        for name in &devices {
                                            if ui.selectable_label(name == &current_device, name).clicked() {
                                                audio_device_cmd = Some(AppCommand::SetAudioDevice(name.clone()));
                                            }
                                        }
                                    });
                            });

                            ui.label("Master Limiter Threshold:");
                            let mut db = 20.0 * threshold.log10();
                            let response = ui.add(egui::Slider::new(&mut db, -24.0..=0.0).text("dB"));
                            if response.changed() {
                                let linear = 10.0f32.powf(db / 20.0);
                                limiter_cmd = Some(AppCommand::SetLimiterThreshold(linear));
                            }
                        });
                        ui.separator();

                        egui::CollapsingHeader::new("OSC / Remote").default_open(false).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("NIC:");
                                settings_changed |= ui.text_edit_singleline(&mut settings.osc_nic).changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("RX Port:");
                                settings_changed |= ui.add(egui::DragValue::new(&mut settings.osc_rx_port).speed(1)).changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("TX Port:");
                                settings_changed |= ui.add(egui::DragValue::new(&mut settings.osc_tx_port).speed(1)).changed();
                            });
                            settings_changed |= ui.checkbox(&mut settings.enable_remote_control, "Enable Remote Control").changed();
                            ui.horizontal(|ui| {
                                ui.label("Node Name:");
                                settings_changed |= ui.text_edit_singleline(&mut settings.node_name).changed();
                            });
                        });
                        ui.separator();

                        egui::CollapsingHeader::new("MSC").default_open(false).show(ui, |ui| {
                            settings_changed |= ui.checkbox(&mut settings.enable_msc, "Enable MSC").changed();
                            ui.horizontal(|ui| {
                                ui.label("RX Port:");
                                settings_changed |= ui.add(egui::DragValue::new(&mut settings.msc_rx_port).speed(1)).changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("TX Port:");
                                settings_changed |= ui.add(egui::DragValue::new(&mut settings.msc_tx_port).speed(1)).changed();
                            });
                        });
                    }
                });
            if let Ok(mut state) = self.state.lock() {
                state.show_settings_window = show_settings;
                if settings_changed {
                    state.dirty = true;
                }
                if let Some(cmd) = limiter_cmd {
                    state.command_queue.push(cmd);
                }
                if let Some(cmd) = audio_device_cmd {
                    state.command_queue.push(cmd);
                }
            }
        }

        // Process any commands queued during the frame
        self.process_commands(ctx);
    }
}

impl QPlayerApp {
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::NewProject);
                    }
                    ui.close();
                }
                if ui.button("Open…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("QPlayer project", &["qproj"])
                        .pick_file()
                    {
                        if let Ok(mut state) = self.state.lock() {
                            state.command_queue.push(AppCommand::OpenProject { path });
                        }
                    }
                    ui.close();
                }
                if ui.button("Save").clicked() {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::SaveProject);
                    }
                    ui.close();
                }
                if ui.button("Save As…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("QPlayer project", &["qproj"])
                        .save_file()
                    {
                        if let Ok(mut state) = self.state.lock() {
                            state.command_queue.push(AppCommand::SaveProjectAs { path });
                        }
                    }
                    ui.close();
                }

                ui.separator();
                if ui.button("Project Settings…").clicked() {
                    if let Ok(mut state) = self.state.lock() {
                        state.show_settings_window = true;
                    }
                    ui.close();
                }

                // Recent files
                let recent = {
                    let Ok(state) = self.state.lock() else { return };
                    state.recent_files.clone()
                };
                if !recent.is_empty() {
                    ui.separator();
                    ui.label("Recent Files:");
                    for path in &recent {
                        let label = path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Untitled");
                        if ui.button(label).clicked() {
                            if let Ok(mut state) = self.state.lock() {
                                state.command_queue.push(AppCommand::OpenProject { path: path.clone() });
                            }
                            ui.close();
                        }
                    }
                }
            });

            ui.menu_button("Edit", |ui| {
                let (can_undo, can_redo) = {
                    let Ok(state) = self.state.lock() else { return };
                    (state.undo_redo.can_undo(), state.undo_redo.can_redo())
                };
                if ui.add_enabled(can_undo, egui::Button::new("Undo")).clicked() {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::Undo);
                    }
                    ui.close();
                }
                if ui.add_enabled(can_redo, egui::Button::new("Redo")).clicked() {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::Redo);
                    }
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                ui.label("Zoom / Grid options (TODO)");
            });
        });
    }

    fn confirm_discard(state: &SharedStateHandle) -> bool {
        let (dirty, has_running) = {
            let Ok(state) = state.lock() else { return false };
            (state.dirty, !state.active_cues.is_empty())
        };
        if has_running {
            let choice = rfd::MessageDialog::new()
                .set_title("Running Cues")
                .set_description("There are cues currently playing. Stop them and proceed?")
                .set_buttons(rfd::MessageButtons::OkCancel)
                .show();
            if !matches!(choice, rfd::MessageDialogResult::Ok) {
                return false;
            }
        }
        if dirty {
            let choice = rfd::MessageDialog::new()
                .set_title("Unsaved Changes")
                .set_description("You have unsaved changes. Discard them?")
                .set_buttons(rfd::MessageButtons::OkCancel)
                .show();
            if !matches!(choice, rfd::MessageDialogResult::Ok) {
                return false;
            }
        }
        true
    }

    fn process_commands(&mut self, _ctx: &egui::Context) {
        let commands = {
            let Ok(mut state) = self.state.lock() else { return };
            let cmds = state.command_queue.clone();
            state.command_queue.clear();
            cmds
        };

        for cmd in commands {
            match cmd {
                AppCommand::NewProject => {
                    if !Self::confirm_discard(&self.state) {
                        continue;
                    }
                    if let Ok(mut state) = self.state.lock() {
                        let snapshot = Snapshot::from_state(&state);
                        state.undo_redo.push(snapshot);
                        state.show_file = ShowFile::default();
                        state.project_path = None;
                        state.selected_cue_id = None;
                        state.dirty = false;
                    }
                }
                AppCommand::OpenProject { path } => {
                    if !Self::confirm_discard(&self.state) {
                        continue;
                    }
                    log::info!("Open project: {:?}", path);
                    match std::fs::read_to_string(&path) {
                        Ok(data) => {
                            if let Ok(mut state) = self.state.lock() {
                                let snapshot = Snapshot::from_state(&state);
                                state.undo_redo.push(snapshot);
                                if let Err(e) = state.load_show_file(&path, &data) {
                                    log::error!("Failed to parse show file: {}", e);
                                } else {
                                    state.push_recent_file(&path);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to read file: {}", e);
                        }
                    }
                }
                AppCommand::SaveProject => {
                    let path = {
                        let Ok(state) = self.state.lock() else { continue };
                        state.project_path.clone()
                    };
                    if let Some(path) = path {
                        if let Err(e) = self.save_to_path(&path) {
                            log::error!("Failed to save project: {}", e);
                        }
                    } else {
                        // No path yet — prompt Save As
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("QPlayer project", &["qproj"])
                            .save_file()
                        {
                            if let Err(e) = self.save_to_path(&path) {
                                log::error!("Failed to save project: {}", e);
                            }
                        }
                    }
                }
                AppCommand::SaveProjectAs { path } => {
                    if let Err(e) = self.save_to_path(&path) {
                        log::error!("Failed to save project: {}", e);
                    }
                }
                AppCommand::SelectCue(id) => {
                    if let Ok(mut state) = self.state.lock() {
                        // Capture snapshot before switching cues so inspector edits are undoable
                        let snapshot = Snapshot::from_state(&state);
                        state.undo_redo.push(snapshot);
                        state.selected_cue_id = Some(id);
                    }
                }
                AppCommand::Undo => {
                    if let Ok(mut state) = self.state.lock() {
                        let current = Snapshot::from_state(&state);
                        if let Some(prev) = state.undo_redo.undo(current) {
                            state.undo_redo.suppress = true;
                            prev.apply(&mut state);
                            state.undo_redo.suppress = false;
                            log::info!("Undo");
                        }
                    }
                }
                AppCommand::Redo => {
                    if let Ok(mut state) = self.state.lock() {
                        let current = Snapshot::from_state(&state);
                        if let Some(next) = state.undo_redo.redo(current) {
                            state.undo_redo.suppress = true;
                            next.apply(&mut state);
                            state.undo_redo.suppress = false;
                            log::info!("Redo");
                        }
                    }
                }
                AppCommand::AddCue { cue_type } => {
                    if let Ok(mut state) = self.state.lock() {
                        let snapshot = Snapshot::from_state(&state);
                        state.undo_redo.push(snapshot);

                        let next_qid = state.show_file.choose_qid(state.selected_cue_id);

                        let base = qplayer_core::CueBase {
                            qid: next_qid,
                            name: format!("New {:?} Cue", cue_type),
                            ..Default::default()
                        };

                        let cue = match cue_type {
                            CueType::Sound => qplayer_core::Cue::Sound {
                                base,
                                path: String::new(),
                                start_time: qplayer_core::Timespan::ZERO,
                                duration: qplayer_core::Timespan::ZERO,
                                volume: 1.0,
                                pan: 0.0,
                                fade_in: 0.0,
                                fade_out: 0.0,
                                fade_type: qplayer_core::FadeType::Linear,
                                eq: None,
                            },
                            CueType::Video => qplayer_core::Cue::Video {
                                base,
                                path: String::new(),
                                start_time: qplayer_core::Timespan::ZERO,
                                duration: qplayer_core::Timespan::ZERO,
                                volume: 1.0,
                                pan: 0.0,
                                fade_in: 0.0,
                                fade_out: 0.0,
                                fade_type: qplayer_core::FadeType::Linear,
                                eq: None,
                            },
                            CueType::Stop => qplayer_core::Cue::Stop {
                                base,
                                stop_qid: Decimal::ZERO,
                                stop_mode: qplayer_core::StopMode::Immediate,
                                fade_out_time: 0.0,
                                fade_type: qplayer_core::FadeType::Linear,
                            },
                            CueType::Volume => qplayer_core::Cue::Volume {
                                base,
                                sound_qid: Decimal::ZERO,
                                fade_time: 0.0,
                                volume: 0.0,
                                fade_type: qplayer_core::FadeType::Linear,
                            },
                            CueType::Group => qplayer_core::Cue::Group { base },
                            CueType::Dummy => qplayer_core::Cue::Dummy { base },
                            CueType::TimeCode => qplayer_core::Cue::TimeCode {
                                base,
                                start_time: qplayer_core::Timespan::ZERO,
                                duration: qplayer_core::Timespan::ZERO,
                            },
                        };
                        state.show_file.cues.push(cue);
                        state.dirty = true;
                    }
                }
                AppCommand::DeleteSelectedCue => {
                    if let Ok(mut state) = self.state.lock() {
                        if let Some(id) = state.selected_cue_id {
                            let snapshot = Snapshot::from_state(&state);
                            state.undo_redo.push(snapshot);
                            state.show_file.cues.retain(|c| c.base().qid != id);
                            state.selected_cue_id = None;
                            state.dirty = true;
                        }
                    }
                }
                AppCommand::DuplicateSelectedCue => {
                    if let Ok(mut state) = self.state.lock() {
                        if let Some(cue) = state.selected_cue().cloned() {
                            let snapshot = Snapshot::from_state(&state);
                            state.undo_redo.push(snapshot);

                            let mut new_cue = cue;
                            let original_qid = new_cue.base().qid;
                            let next_qid = state.show_file.choose_qid(Some(original_qid));
                            new_cue.base_mut().qid = next_qid;
                            new_cue.base_mut().name.push_str(" (copy)");
                            state.show_file.cues.push(new_cue);
                            state.dirty = true;
                        }
                    }
                }
                AppCommand::MoveSelectedCueUp => {
                    if let Ok(mut state) = self.state.lock() {
                        if let Some(id) = state.selected_cue_id {
                            let idx = state.show_file.cues.iter().position(|c| c.base().qid == id);
                            if let Some(i) = idx {
                                if i > 0 {
                                    let snapshot = Snapshot::from_state(&state);
                                    state.undo_redo.push(snapshot);
                                    state.show_file.cues.swap(i, i - 1);
                                    state.dirty = true;
                                }
                            }
                        }
                    }
                }
                AppCommand::MoveSelectedCueDown => {
                    if let Ok(mut state) = self.state.lock() {
                        if let Some(id) = state.selected_cue_id {
                            let len = state.show_file.cues.len();
                            let idx = state.show_file.cues.iter().position(|c| c.base().qid == id);
                            if let Some(i) = idx {
                                if i + 1 < len {
                                    let snapshot = Snapshot::from_state(&state);
                                    state.undo_redo.push(snapshot);
                                    state.show_file.cues.swap(i, i + 1);
                                    state.dirty = true;
                                }
                            }
                        }
                    }
                }
                AppCommand::MoveCue { from_idx, to_idx } => {
                    if let Ok(mut state) = self.state.lock() {
                        let len = state.show_file.cues.len();
                        if from_idx < len && to_idx < len && from_idx != to_idx {
                            let snapshot = Snapshot::from_state(&state);
                            state.undo_redo.push(snapshot);
                            let cue = state.show_file.cues.remove(from_idx);
                            let insert_idx = if to_idx > from_idx { to_idx } else { to_idx };
                            state.show_file.cues.insert(insert_idx, cue);
                            state.dirty = true;
                        }
                    }
                }
                // Go, Stop, Pause are handled by the main application (qplayer/src/main.rs)
                _ => {}
            }
        }
    }

    fn save_to_path(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = {
            let Ok(state) = self.state.lock() else {
                return Err("failed to lock state".into());
            };
            serde_json::to_string_pretty(&state.show_file)?
        };
        std::fs::write(path, json)?;
        if let Ok(mut state) = self.state.lock() {
            state.project_path = Some(path.to_path_buf());
            state.dirty = false;
            state.push_recent_file(path);
        }
        log::info!("Project saved to {:?}", path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qplayer_core::CueBase;

    #[test]
    fn test_shared_state_default() {
        let state = SharedState::new();
        assert!(state.show_file.cues.is_empty());
        assert_eq!(state.selected_cue_id, None);
    }

    #[test]
    fn test_generate_large_show_file() {
        let mut show = ShowFile::default();
        for i in 0..500 {
            show.cues.push(Cue::Sound {
                base: CueBase {
                    qid: Decimal::from(i + 1),
                    name: format!("Cue {}", i + 1),
                    ..Default::default()
                },
                path: format!("/audio/cue_{}.wav", i + 1),
                start_time: qplayer_core::Timespan::ZERO,
                duration: qplayer_core::Timespan::from_secs_f64(10.0),
                volume: 0.0,
                pan: 0.0,
                fade_in: 0.0,
                fade_out: 0.0,
                fade_type: qplayer_core::FadeType::Linear,
                eq: None,
            });
        }
        assert_eq!(show.cues.len(), 500);
    }

    #[test]
    fn test_undo_redo() {
        let mut state = SharedState::new();
        state.show_file.cues.push(Cue::Sound {
            base: CueBase {
                qid: Decimal::ONE,
                name: "First".into(),
                ..Default::default()
            },
            path: "/audio/first.wav".into(),
            start_time: qplayer_core::Timespan::ZERO,
            duration: qplayer_core::Timespan::ZERO,
            volume: 0.0,
            pan: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            fade_type: qplayer_core::FadeType::Linear,
            eq: None,
        });

        // Capture snapshot, then mutate
        let s1 = Snapshot::from_state(&state);
        state.undo_redo.push(s1);
        state.show_file.cues.push(Cue::Sound {
            base: CueBase {
                qid: Decimal::from(2),
                name: "Second".into(),
                ..Default::default()
            },
            path: "/audio/second.wav".into(),
            start_time: qplayer_core::Timespan::ZERO,
            duration: qplayer_core::Timespan::ZERO,
            volume: 0.0,
            pan: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            fade_type: qplayer_core::FadeType::Linear,
            eq: None,
        });
        assert_eq!(state.show_file.cues.len(), 2);

        // Undo
        let current = Snapshot::from_state(&state);
        let prev = state.undo_redo.undo(current).unwrap();
        prev.apply(&mut state);
        assert_eq!(state.show_file.cues.len(), 1);
        assert_eq!(state.show_file.cues[0].base().name, "First");

        // Redo
        let current = Snapshot::from_state(&state);
        let next = state.undo_redo.redo(current).unwrap();
        next.apply(&mut state);
        assert_eq!(state.show_file.cues.len(), 2);
        assert_eq!(state.show_file.cues[1].base().name, "Second");
    }
}
