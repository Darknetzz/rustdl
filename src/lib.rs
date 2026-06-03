//! Headless and GUI logic for rustdl. The binary entry point is [`main_entry`].

pub mod app;
pub(crate) mod service;
pub mod app_actions;
pub mod app_icon;
pub mod app_parsing;
pub mod app_state;
pub mod app_ui;
pub mod av1_state;
pub mod av1_transcode;
pub mod cli;
pub mod config;
pub mod external_tools;
pub mod models;
pub mod pkg_version;
pub mod profiles;
pub mod theme;
pub mod time_format;
pub mod ui_icons;
#[cfg(windows)]
pub mod win_drop_target;
#[cfg(windows)]
pub mod win_icon;
pub mod ytdlp;
pub mod ytdlp_download_args;

use std::process;
use std::sync::Arc;

use eframe::egui;
use tokio::runtime::Runtime;

/// Minimum inner size so header/actions, the non-wrapping output-folder row, and queue controls stay visible.
const VIEWPORT_MIN_INNER: [f32; 2] = [920.0, 760.0];

pub fn run_gui(runtime: Arc<Runtime>) -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rustdl")
            .with_icon(app_icon::window_icon())
            .with_inner_size([1280.0, 880.0])
            .with_min_inner_size(VIEWPORT_MIN_INNER),
        ..Default::default()
    };

    eframe::run_native(
        "rustdl",
        native_options,
        Box::new(move |cc| {
            egui_material_icons::initialize(&cc.egui_ctx);
            let settings = config::load_settings();
            theme::apply_ui_theme(&cc.egui_ctx, &settings.theme);
            Ok(Box::new(app::PydlApp::new(cc, runtime.clone())))
        }),
    )
}

pub fn main_entry() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if cli::args_want_console(&args) {
        cli::attach_parent_console();
    }
    // CLI modes must exit explicitly: eframe/winit can leave threads running on Windows
    // after main returns, which makes --help and other headless commands appear hung.
    if !args.is_empty() && cli::run_cli_or_exit(args) {
        process::exit(0);
    }

    let runtime = match Runtime::new() {
        Ok(rt) => Arc::new(rt),
        Err(e) => {
            eprintln!("Failed to create Tokio runtime: {e}");
            process::exit(1);
        }
    };

    if let Err(e) = run_gui(runtime) {
        eprintln!("Failed to run app: {e}");
        process::exit(1);
    }
}
