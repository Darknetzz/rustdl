//! Headless and GUI logic for rustdl. The binary entry point is [`main_entry`].

pub mod app;
pub mod app_actions;
pub mod app_icon;
pub mod app_parsing;
pub mod app_state;
pub mod app_ui;
pub mod cli;
pub mod config;
pub mod convert_presets;
pub mod convert_size_limit;
pub mod convert_state;
pub mod disk_space;
pub mod domain;
pub mod download_organize;
pub mod external_tools;
pub mod filename_rewrite;
pub mod http_client;
pub mod log_filter;
pub mod media_metadata;
pub mod models;
pub mod pkg_version;
pub mod profiles;
pub mod queue_templates;
pub(crate) mod service;
pub mod system_usage;
pub mod theme;
pub mod thumbnail_store;
pub mod time_format;
pub mod transcode;
#[cfg(any(windows, target_os = "linux"))]
pub mod tray;
pub mod ui_icons;
pub mod watch_folder;
pub mod watchlist;
#[cfg(windows)]
pub mod win_drop_target;
#[cfg(windows)]
pub mod win_icon;
#[cfg(windows)]
pub mod win_window;
pub mod ytdlp;
pub mod ytdlp_download_args;
pub mod ytdlp_errors;

use std::process;
use std::sync::Arc;

use eframe::egui;
use tokio::runtime::Runtime;

pub fn run_gui(runtime: Arc<Runtime>) -> eframe::Result<()> {
    // eframe only clamps restored window positions on Windows; off-screen restore on Linux
    // can leave the window invisible. Always center instead of restoring position.
    let mut native_options = eframe::NativeOptions {
        // Center on first launch so the window is easy to spot (especially on multi-monitor setups).
        centered: true,
        viewport: egui::ViewportBuilder::default()
            .with_app_id("rustdl")
            .with_title("rustdl")
            .with_icon(app_icon::window_icon())
            .with_inner_size([1280.0, 880.0])
            .with_min_inner_size(app_ui::VIEWPORT_MIN_INNER)
            .with_active(true),
        #[cfg(target_os = "linux")]
        persist_window: false,
        ..Default::default()
    };

    // winit 0.30 prefers Wayland when WAYLAND_DISPLAY is set. On some GNOME/Zorin setups the
    // Wayland surface shows a dock icon but never maps a visible window. Prefer XWayland unless
    // the user opts into native Wayland with RUSTDL_USE_WAYLAND=1.
    #[cfg(target_os = "linux")]
    {
        let prefer_wayland = std::env::var_os("RUSTDL_USE_WAYLAND")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if !prefer_wayland {
            use winit::platform::x11::EventLoopBuilderExtX11;
            native_options.event_loop_builder = Some(Box::new(|b| {
                b.with_x11();
            }));
        }
    }

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

    #[cfg(windows)]
    cli::detach_console_for_gui();

    #[cfg(target_os = "linux")]
    warn_if_embedded_ide_terminal();

    if let Err(e) = run_gui(runtime) {
        #[cfg(windows)]
        cli::reattach_console_for_error();
        eprintln!("Failed to run app: {e}");
        process::exit(1);
    }

    // Tokio / web-server threads can outlive eframe on some platforms unless we exit explicitly.
    process::exit(0);
}

#[cfg(target_os = "linux")]
fn warn_if_embedded_ide_terminal() {
    let from_cursor = std::env::var_os("CURSOR_TRACE_ID").is_some()
        || std::env::var_os("VSCODE_IPC_HOOK").is_some();
    if from_cursor {
        eprintln!(
            "rustdl: launched from Cursor/VS Code terminal — if no window appears, \
             use a system terminal (Alt+T) or run: killall rustdl && ./target/release/rustdl"
        );
    }
}
