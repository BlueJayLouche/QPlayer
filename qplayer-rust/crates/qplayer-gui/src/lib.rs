//! QPlayer GUI — egui + wgpu immediate-mode interface.
//!
//! Replaces all WPF Views and ViewModels.

pub mod app;
pub mod cue_list;
pub mod inspector;
pub mod transport;
pub mod waveform;

pub use app::{AppCommand, QPlayerApp, SharedState, SharedStateHandle, ShowMode};
