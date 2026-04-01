use std::sync::Arc;

use eframe::egui;
use tokio::runtime::Runtime;

mod app;
mod app_actions;
mod app_icon;
mod app_parsing;
mod app_state;
mod app_ui;
mod config;
mod models;
mod pkg_version;
mod ui_icons;
#[cfg(windows)]
mod win_icon;
mod ytdlp;

fn main() {
    let runtime = match Runtime::new() {
        Ok(rt) => Arc::new(rt),
        Err(e) => {
            eprintln!("Failed to create Tokio runtime: {e}");
            return;
        }
    };
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rustdl")
            .with_icon(app_icon::window_icon())
            .with_inner_size([1280.0, 880.0])
            .with_min_inner_size([860.0, 760.0]),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "rustdl",
        native_options,
        Box::new(move |cc| {
            egui_material_icons::initialize(&cc.egui_ctx);
            app::PydlApp::apply_ui_smoothness(&cc.egui_ctx);
            Ok(Box::new(app::PydlApp::new(cc, runtime.clone())))
        }),
    ) {
        eprintln!("Failed to run app: {e}");
    }
}
