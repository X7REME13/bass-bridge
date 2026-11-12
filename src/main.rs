#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod dsp;
mod gui;
mod settings;

use gui::BassBridgeApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("BassBridge - Subwoofer & Transducer Router")
            .with_inner_size([540.0, 720.0])
            .with_min_inner_size([460.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "BassBridge",
        native_options,
        Box::new(|cc| Ok(Box::new(BassBridgeApp::new(cc)))),
    )
}
