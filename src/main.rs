#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod dsp;
mod gui;
mod settings;

use gui::BassBridgeApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([560.0, 620.0])
            .with_min_inner_size([480.0, 520.0])
            .with_title(format!("BassBridge v{}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };

    eframe::run_native(
        "BassBridge",
        options,
        Box::new(|cc| Ok(Box::new(BassBridgeApp::new(cc)))),
    )
}
