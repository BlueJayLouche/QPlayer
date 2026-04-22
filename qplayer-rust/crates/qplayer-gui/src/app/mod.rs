//! Main application state and eframe integration.

use qplayer_core::{Cue, ShowFile};
use rust_decimal::Decimal;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Central mutable state shared between GUI and audio/control threads.
#[derive(Debug)]
pub struct SharedState {
    pub show_file: ShowFile,
    pub project_path: Option<PathBuf>,
    pub selected_cue_id: Option<Decimal>,
    pub command_queue: Vec<AppCommand>,
    pub show_mode: ShowMode,
    pub dirty: bool,
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
    Go,
    Stop,
    Pause,
    SelectCue(Decimal),
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

impl eframe::App for QPlayerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                    log::info!("Save project");
                    // TODO: actual file I/O from main thread
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
            }
        }
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
}
