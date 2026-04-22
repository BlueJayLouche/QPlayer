//! Transport controls — Go, Stop, Pause buttons.

use crate::app::{AppCommand, SharedStateHandle};
use egui::{Button, Color32, RichText, Vec2};

pub fn show(ui: &mut egui::Ui, state: &SharedStateHandle) {
    ui.horizontal(|ui| {
        let button_size = Vec2::new(60.0, 32.0);

        let go_btn = Button::new(RichText::new("▶ GO").strong().color(Color32::WHITE))
            .fill(Color32::from_rgb(0, 180, 0))
            .min_size(button_size);
        if ui.add(go_btn).clicked() {
            if let Ok(mut state) = state.lock() {
                state.command_queue.push(AppCommand::Go);
            }
        }

        let stop_btn = Button::new(RichText::new("⏹ STOP").strong())
            .fill(Color32::from_rgb(200, 0, 0))
            .min_size(button_size);
        if ui.add(stop_btn).clicked() {
            if let Ok(mut state) = state.lock() {
                state.command_queue.push(AppCommand::Stop);
            }
        }

        let pause_btn = Button::new(RichText::new("⏸ PAUSE"))
            .min_size(button_size);
        if ui.add(pause_btn).clicked() {
            if let Ok(mut state) = state.lock() {
                state.command_queue.push(AppCommand::Pause);
            }
        }

        ui.separator();

        // Show / Edit mode toggle
        let mode = {
            let Ok(state) = state.lock() else { return };
            state.show_mode
        };

        let mode_label = match mode {
            crate::app::ShowMode::Edit => "Edit Mode",
            crate::app::ShowMode::Show => "Show Mode",
        };
        let mode_color = match mode {
            crate::app::ShowMode::Edit => Color32::from_rgb(60, 60, 60),
            crate::app::ShowMode::Show => Color32::from_rgb(180, 140, 0),
        };

        let mode_btn = Button::new(RichText::new(mode_label).strong().color(Color32::WHITE))
            .fill(mode_color)
            .min_size(Vec2::new(100.0, 32.0));
        if ui.add(mode_btn).clicked() {
            if let Ok(mut state) = state.lock() {
                state.show_mode = match state.show_mode {
                    crate::app::ShowMode::Edit => crate::app::ShowMode::Show,
                    crate::app::ShowMode::Show => crate::app::ShowMode::Edit,
                };
            }
        }
    });
}
