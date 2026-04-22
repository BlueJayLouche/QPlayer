//! QPlayer binary — main entry point.

use qplayer_gui::QPlayerApp;

fn main() {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "QPlayer",
        options,
        Box::new(|_cc| Ok(Box::new(QPlayerApp::new()))),
    )
    .unwrap();
}
