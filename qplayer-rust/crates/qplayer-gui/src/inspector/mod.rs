//! Cue inspector — right-side panel showing details for the selected cue.

use crate::app::SharedStateHandle;
use egui::RichText;

pub fn show(ui: &mut egui::Ui, state: &SharedStateHandle) {
    ui.heading("Inspector");
    ui.separator();

    let Ok(state) = state.lock() else { return };
    let Some(cue) = state.selected_cue() else {
        ui.label("Select a cue to edit its properties.");
        return;
    };

    let base = cue.base();
    let qid_str = base.qid.to_string();
    ui.label(RichText::new(format!("Q{}", qid_str)).strong().size(18.0));
    ui.add_space(8.0);

    match cue {
        qplayer_core::Cue::Sound { path, volume, pan, .. } => {
            ui.label(RichText::new("Sound Cue").monospace().size(12.0));
            ui.horizontal(|ui| { ui.label("Name:"); ui.label(&base.name); });
            ui.horizontal(|ui| { ui.label("File:"); ui.monospace(path); });
            ui.horizontal(|ui| { ui.label("Volume:"); ui.label(format!("{:.1} dB", 20.0 * volume.log10())); });
            ui.horizontal(|ui| { ui.label("Pan:"); ui.label(format!("{:.0}", pan * 100.0)); });
            ui.horizontal(|ui| { ui.label("Enabled:"); ui.label(if base.enabled { "Yes" } else { "No" }); });
        }
        qplayer_core::Cue::Video { path, volume, pan, .. } => {
            ui.label(RichText::new("Video Cue").monospace().size(12.0));
            ui.horizontal(|ui| { ui.label("Name:"); ui.label(&base.name); });
            ui.horizontal(|ui| { ui.label("File:"); ui.monospace(path); });
            ui.horizontal(|ui| { ui.label("Volume:"); ui.label(format!("{:.1} dB", 20.0 * volume.log10())); });
            ui.horizontal(|ui| { ui.label("Pan:"); ui.label(format!("{:.0}", pan * 100.0)); });
            ui.horizontal(|ui| { ui.label("Enabled:"); ui.label(if base.enabled { "Yes" } else { "No" }); });
        }
        qplayer_core::Cue::Group { .. } => {
            ui.label(RichText::new("Group Cue").monospace().size(12.0));
            ui.horizontal(|ui| { ui.label("Name:"); ui.label(&base.name); });
        }
        qplayer_core::Cue::Stop { stop_qid, .. } => {
            ui.label(RichText::new("Stop Cue").monospace().size(12.0));
            ui.horizontal(|ui| { ui.label("Name:"); ui.label(&base.name); });
            ui.horizontal(|ui| { ui.label("Stops Q#:"); ui.label(stop_qid.to_string()); });
        }
        qplayer_core::Cue::Volume { sound_qid, volume, .. } => {
            ui.label(RichText::new("Volume Cue").monospace().size(12.0));
            ui.horizontal(|ui| { ui.label("Name:"); ui.label(&base.name); });
            ui.horizontal(|ui| { ui.label("Target Q#:"); ui.label(sound_qid.to_string()); });
            ui.horizontal(|ui| { ui.label("Target dB:"); ui.label(format!("{:.1}", 20.0 * volume.log10())); });
        }
        qplayer_core::Cue::Dummy { .. } => {
            ui.label(RichText::new("Dummy Cue").monospace().size(12.0));
            ui.horizontal(|ui| { ui.label("Name:"); ui.label(&base.name); });
        }
        qplayer_core::Cue::TimeCode { .. } => {
            ui.label(RichText::new("TimeCode Cue").monospace().size(12.0));
            ui.horizontal(|ui| { ui.label("Name:"); ui.label(&base.name); });
        }
    }
}
