//! Cue inspector — right-side panel showing details for the selected cue.

use crate::app::SharedStateHandle;
use egui::RichText;

pub fn show(ui: &mut egui::Ui, state: &SharedStateHandle) {
    ui.heading("Inspector");
    ui.separator();

    let Ok(mut state) = state.lock() else { return };
    let Some(cue) = state.selected_cue_mut() else {
        ui.label("Select a cue to edit its properties.");
        return;
    };

    let base = cue.base_mut();
    let mut changed = false;

    ui.label(RichText::new(format!("Q{}", base.qid)).strong().size(18.0));
    ui.add_space(8.0);

    // Common fields
    ui.horizontal(|ui| {
        ui.label("Name:");
        let response = ui.text_edit_singleline(&mut base.name);
        changed |= response.changed();
    });
    ui.horizontal(|ui| {
        ui.label("QID:");
        let mut qid_str = base.qid.to_string();
        let response = ui.text_edit_singleline(&mut qid_str);
        if response.lost_focus() {
            if let Ok(new_qid) = qid_str.parse::<rust_decimal::Decimal>() {
                if new_qid != base.qid {
                    base.qid = new_qid;
                    changed = true;
                }
            }
        }
    });
    ui.horizontal(|ui| {
        let mut enabled = base.enabled;
        let response = ui.checkbox(&mut enabled, "Enabled");
        if response.changed() {
            base.enabled = enabled;
            changed = true;
        }
    });

    ui.separator();

    match cue {
        qplayer_core::Cue::Sound { path, volume, pan, fade_in, fade_out, fade_type, .. } => {
            ui.label(RichText::new("Sound Cue").monospace().size(12.0));
            ui.horizontal(|ui| {
                ui.label("File:");
                let response = ui.text_edit_singleline(path);
                changed |= response.changed();
            });
            ui.horizontal(|ui| {
                ui.label("Volume (dB):");
                let mut db = 20.0 * volume.log10();
                let response = ui.add(egui::Slider::new(&mut db, -60.0..=12.0));
                if response.changed() {
                    *volume = 10.0f32.powf(db / 20.0);
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Pan:");
                let response = ui.add(egui::Slider::new(pan, -1.0..=1.0));
                changed |= response.changed();
            });
            ui.horizontal(|ui| {
                ui.label("Fade In (s):");
                let response = ui.add(egui::DragValue::new(fade_in).speed(0.1));
                changed |= response.changed();
            });
            ui.horizontal(|ui| {
                ui.label("Fade Out (s):");
                let response = ui.add(egui::DragValue::new(fade_out).speed(0.1));
                changed |= response.changed();
            });
            ui.horizontal(|ui| {
                ui.label("Fade Type:");
                egui::ComboBox::from_id_salt("fade_type")
                    .selected_text(format!("{:?}", fade_type))
                    .show_ui(ui, |ui| {
                        for variant in [qplayer_core::FadeType::Linear, qplayer_core::FadeType::SCurve, qplayer_core::FadeType::Square, qplayer_core::FadeType::InverseSquare] {
                            if ui.selectable_value(fade_type, variant, format!("{:?}", variant)).clicked() {
                                changed = true;
                            }
                        }
                    });
            });
        }
        qplayer_core::Cue::Video { path, volume, pan, fade_in, fade_out, fade_type, .. } => {
            ui.label(RichText::new("Video Cue").monospace().size(12.0));
            ui.horizontal(|ui| {
                ui.label("File:");
                let response = ui.text_edit_singleline(path);
                changed |= response.changed();
            });
            ui.horizontal(|ui| {
                ui.label("Volume (dB):");
                let mut db = 20.0 * volume.log10();
                let response = ui.add(egui::Slider::new(&mut db, -60.0..=12.0));
                if response.changed() {
                    *volume = 10.0f32.powf(db / 20.0);
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Pan:");
                let response = ui.add(egui::Slider::new(pan, -1.0..=1.0));
                changed |= response.changed();
            });
            ui.horizontal(|ui| {
                ui.label("Fade In (s):");
                let response = ui.add(egui::DragValue::new(fade_in).speed(0.1));
                changed |= response.changed();
            });
            ui.horizontal(|ui| {
                ui.label("Fade Out (s):");
                let response = ui.add(egui::DragValue::new(fade_out).speed(0.1));
                changed |= response.changed();
            });
            ui.horizontal(|ui| {
                ui.label("Fade Type:");
                egui::ComboBox::from_id_salt("fade_type")
                    .selected_text(format!("{:?}", fade_type))
                    .show_ui(ui, |ui| {
                        for variant in [qplayer_core::FadeType::Linear, qplayer_core::FadeType::SCurve, qplayer_core::FadeType::Square, qplayer_core::FadeType::InverseSquare] {
                            if ui.selectable_value(fade_type, variant, format!("{:?}", variant)).clicked() {
                                changed = true;
                            }
                        }
                    });
            });
        }
        qplayer_core::Cue::Group { .. } => {
            ui.label(RichText::new("Group Cue").monospace().size(12.0));
        }
        qplayer_core::Cue::Stop { stop_qid, .. } => {
            ui.label(RichText::new("Stop Cue").monospace().size(12.0));
            ui.horizontal(|ui| {
                ui.label("Stops Q#:");
                let mut qid_str = stop_qid.to_string();
                let response = ui.text_edit_singleline(&mut qid_str);
                if response.lost_focus() {
                    if let Ok(new_qid) = qid_str.parse::<rust_decimal::Decimal>() {
                        if new_qid != *stop_qid {
                            *stop_qid = new_qid;
                            changed = true;
                        }
                    }
                }
            });
        }
        qplayer_core::Cue::Volume { sound_qid, volume, .. } => {
            ui.label(RichText::new("Volume Cue").monospace().size(12.0));
            ui.horizontal(|ui| {
                ui.label("Target Q#:");
                let mut qid_str = sound_qid.to_string();
                let response = ui.text_edit_singleline(&mut qid_str);
                if response.lost_focus() {
                    if let Ok(new_qid) = qid_str.parse::<rust_decimal::Decimal>() {
                        if new_qid != *sound_qid {
                            *sound_qid = new_qid;
                            changed = true;
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Target dB:");
                let mut db = 20.0 * volume.log10();
                let response = ui.add(egui::Slider::new(&mut db, -60.0..=12.0));
                if response.changed() {
                    *volume = 10.0f32.powf(db / 20.0);
                    changed = true;
                }
            });
        }
        qplayer_core::Cue::Dummy { .. } => {
            ui.label(RichText::new("Dummy Cue").monospace().size(12.0));
        }
        qplayer_core::Cue::TimeCode { start_time, duration, .. } => {
            ui.label(RichText::new("TimeCode Cue").monospace().size(12.0));
            ui.horizontal(|ui| {
                ui.label("Start (s):");
                let mut secs = start_time.as_secs_f64();
                let response = ui.add(egui::DragValue::new(&mut secs).speed(0.1));
                if response.changed() {
                    *start_time = qplayer_core::Timespan::from_secs_f64(secs);
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Duration (s):");
                let mut secs = duration.as_secs_f64();
                let response = ui.add(egui::DragValue::new(&mut secs).speed(0.1));
                if response.changed() {
                    *duration = qplayer_core::Timespan::from_secs_f64(secs);
                    changed = true;
                }
            });
        }
    }

    if changed {
        state.dirty = true;
    }
}
