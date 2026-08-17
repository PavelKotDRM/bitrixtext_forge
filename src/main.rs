//! BitrixText Forge — Markdown → Bitrix24 BBCode.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bitrixtext_forge::app;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("BitrixText Forge"),
        ..Default::default()
    };
    eframe::run_native(
        "BitrixText Forge",
        options,
        Box::new(|cc| Ok(Box::new(app::ForgeApp::new(cc)))),
    )
}
