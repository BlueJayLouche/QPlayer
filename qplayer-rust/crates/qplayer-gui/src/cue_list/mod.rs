//! Cue list — the main view replacing WPF's DataGrid.

use crate::app::{AppCommand, CueType, SharedStateHandle};
use egui::{Color32, RichText};
use qplayer_core::Cue;
use rust_decimal::Decimal;

pub fn show(ui: &mut egui::Ui, state: &SharedStateHandle) {
    let (cues, selected_id, show_mode) = {
        let Ok(state) = state.lock() else { return };
        (
            state.show_file.cues.clone(),
            state.selected_cue_id,
            state.show_mode,
        )
    };

    ui.heading(format!("Cues ({})", cues.len()));
    ui.separator();

    // Toolbar
    if show_mode == crate::app::ShowMode::Edit {
        ui.horizontal(|ui| {
            if ui.button("+ Sound").clicked() {
                queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Sound });
            }
            if ui.button("+ Video").clicked() {
                queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Video });
            }
            if ui.button("+ Stop").clicked() {
                queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Stop });
            }
            if ui.button("+ Volume").clicked() {
                queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Volume });
            }
            if ui.button("+ Group").clicked() {
                queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Group });
            }
            if ui.button("+ Dummy").clicked() {
                queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Dummy });
            }
        });
        ui.separator();
    }

    // Header row
    ui.horizontal(|ui| {
        ui.label(RichText::new("#").strong());
        ui.separator();
        ui.label(RichText::new("Name").strong());
        ui.separator();
        ui.label(RichText::new("Type").strong());
        ui.separator();
        ui.label(RichText::new("").strong());
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (idx, cue) in cues.iter().enumerate() {
            let base = cue.base();
            let qid = base.qid;
            let is_selected = selected_id == Some(qid);
            let name = &base.name;
            let cue_type = cue_type_label(cue);
            let colour = colour_to_egui(base.colour);

            let bg = if is_selected {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().panel_fill
            };

            let frame = egui::Frame::new()
                .fill(bg)
                .inner_margin(egui::Margin::same(4));

            let (drop_response, dropped_payload) = ui.dnd_drop_zone::<usize, ()>(frame, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(20.0);

                    // Drag handle (only in edit mode)
                    if show_mode == crate::app::ShowMode::Edit {
                        let drag_id = ui.auto_id_with(("drag", idx));
                        ui.dnd_drag_source(drag_id, idx, |ui| {
                            ui.label(egui::RichText::new("≡").monospace().size(14.0));
                        });
                    }

                    // Q# column
                    let qid_str = qid.to_string();
                    let response = ui.selectable_label(is_selected, &qid_str);
                    if response.clicked() {
                        queue_select(state, qid);
                    }
                    ui.separator();

                    // Name column
                    let response = ui.selectable_label(is_selected, name.as_str());
                    if response.clicked() {
                        queue_select(state, qid);
                    }
                    ui.separator();

                    // Type column
                    ui.label(RichText::new(cue_type).monospace().size(10.0));
                    ui.separator();

                    // Colour swatch
                    let (rect, _response) = ui.allocate_exact_size(
                        egui::vec2(16.0, 16.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, 4.0, colour);
                });
            });

            // Context menu on the entire row (right-click anywhere in the frame)
            if show_mode == crate::app::ShowMode::Edit {
                drop_response.response.context_menu(|ui| {
                    if ui.button("Move Up").clicked() {
                        queue_cmd(state, AppCommand::MoveSelectedCueUp);
                        ui.close();
                    }
                    if ui.button("Move Down").clicked() {
                        queue_cmd(state, AppCommand::MoveSelectedCueDown);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Duplicate").clicked() {
                        queue_cmd(state, AppCommand::DuplicateSelectedCue);
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        queue_cmd(state, AppCommand::DeleteSelectedCue);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Add Sound Cue").clicked() {
                        queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Sound });
                        ui.close();
                    }
                    if ui.button("Add Video Cue").clicked() {
                        queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Video });
                        ui.close();
                    }
                    if ui.button("Add Stop Cue").clicked() {
                        queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Stop });
                        ui.close();
                    }
                    if ui.button("Add Volume Cue").clicked() {
                        queue_cmd(state, AppCommand::AddCue { cue_type: CueType::Volume });
                        ui.close();
                    }
                });
            }

            // Handle dropped payload for reordering
            if show_mode == crate::app::ShowMode::Edit {
                if let Some(source_idx) = dropped_payload {
                    let source = *source_idx;
                    if source != idx {
                        queue_cmd(state, AppCommand::MoveCue { from_idx: source, to_idx: idx });
                    }
                }
            }
        }
    });

    if show_mode == crate::app::ShowMode::Show {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::YELLOW, "● SHOW MODE");
            ui.label("Editing disabled");
        });
    }
}

fn queue_select(state: &SharedStateHandle, qid: Decimal) {
    if let Ok(mut state) = state.lock() {
        state.command_queue.push(AppCommand::SelectCue(qid));
    }
}

fn queue_cmd(state: &SharedStateHandle, cmd: AppCommand) {
    if let Ok(mut state) = state.lock() {
        state.command_queue.push(cmd);
    }
}

fn cue_type_label(cue: &Cue) -> &'static str {
    match cue {
        Cue::Group { .. } => "GRP",
        Cue::Sound { .. } => "SND",
        Cue::Video { .. } => "VID",
        Cue::Stop { .. } => "STP",
        Cue::Volume { .. } => "VOL",
        Cue::Dummy { .. } => "DUM",
        Cue::TimeCode { .. } => "TC",
    }
}

fn colour_to_egui(c: qplayer_core::SerializedColour) -> Color32 {
    Color32::from_rgba_premultiplied(
        (c.r * 255.0) as u8,
        (c.g * 255.0) as u8,
        (c.b * 255.0) as u8,
        (c.a * 255.0) as u8,
    )
}
