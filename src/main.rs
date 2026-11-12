#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod dsp;
mod gui;
mod settings;

use gui::BassBridgeApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("BassBridge Pro - Subwoofer & Transducer Crossover")
            .with_inner_size([560.0, 760.0])
            .with_min_inner_size([480.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "BassBridge",
        native_options,
        Box::new(|cc| Ok(Box::new(BassBridgeApp::new(cc)))),
    )
}
