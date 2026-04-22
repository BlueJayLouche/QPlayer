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
        }
    }
}

impl SharedState {
    pub fn new() -> Self {
        Self::default()
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
            if modifiers.command && i.key_pressed(egui::Key::Z) {
                if modifiers.shift {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::Redo);
                    }
                } else {
                    if let Ok(mut state) = self.state.lock() {
                        state.command_queue.push(AppCommand::Undo);
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
                    log::info!("Open project: {:?}", path);
                    match std::fs::read_to_string(&path) {
                        Ok(data) => {
                            if let Ok(mut state) = self.state.lock() {
                                let snapshot = Snapshot::from_state(&state);
                                state.undo_redo.push(snapshot);
                                if let Err(e) = state.load_show_file(&path, &data) {
                                    log::error!("Failed to parse show file: {}", e);
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
                AppCommand::Go => {
                    log::info!("Go!");
                }
                AppCommand::Stop => {
                    log::info!("Stop");
                }
                AppCommand::Pause => {
                    log::info!("Pause");
                }
                AppCommand::SelectCue(id) => {
                    if let Ok(mut state) = self.state.lock() {
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
