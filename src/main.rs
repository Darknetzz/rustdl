use std::process;
use std::sync::Arc;

use eframe::egui;
use tokio::runtime::Runtime;

mod app;
mod app_actions;
mod app_icon;
mod app_parsing;
mod app_state;
mod app_ui;
mod cli;
mod config;
mod external_tools;
mod models;
mod pkg_version;
mod profiles;
mod theme;
mod time_format;
mod ui_icons;
mod av1_transcode;
#[cfg(windows)]
mod win_icon;
#[cfg(windows)]
mod win_drop_target;
mod ytdlp;

/// Minimum inner size so header/actions, the non-wrapping output-folder row, and queue controls stay visible.
const VIEWPORT_MIN_INNER: [f32; 2] = [920.0, 760.0];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() && cli::run_cli_or_exit(args) {
        return;
    }

    let runtime = match Runtime::new() {
        Ok(rt) => Arc::new(rt),
        Err(e) => {
            eprintln!("Failed to create Tokio runtime: {e}");
            process::exit(1);
        }
    };
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rustdl")
            .with_icon(app_icon::window_icon())
            .with_inner_size([1280.0, 880.0])
            .with_min_inner_size(VIEWPORT_MIN_INNER),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "rustdl",
        native_options,
        Box::new(move |cc| {
            egui_material_icons::initialize(&cc.egui_ctx);
            let settings = config::load_settings();
            theme::apply_ui_theme(&cc.egui_ctx, &settings.theme);
            Ok(Box::new(app::PydlApp::new(cc, runtime.clone())))
        }),
    ) {
        eprintln!("Failed to run app: {e}");
        process::exit(1);
    }
}
