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
mod config;
mod models;
mod pkg_version;
mod ui_icons;
#[cfg(windows)]
mod win_icon;
mod ytdlp;

fn print_version() {
    println!("rustdl {}", pkg_version::VERSION);
    println!("Build: {}", pkg_version::BUILD_DATE);
}

fn print_help() {
    println!(
        "rustdl {} — desktop GUI for yt-dlp (egui).\n",
        pkg_version::VERSION
    );
    println!("Usage:");
    println!("  rustdl              Start the graphical interface");
    println!("  rustdl [OPTIONS]    Print version or help and exit\n");
    println!("Options:");
    println!("  -h, --help       Print this help message");
    println!("  -V, --version    Print version and build date (UTC)");
}

fn try_cli_and_exit() -> bool {
    let mut args = std::env::args();
    let _exe = args.next();
    let Some(first) = args.next() else {
        return false;
    };
    match first.as_str() {
        "--version" | "-V" => {
            print_version();
            true
        }
        "--help" | "-h" => {
            print_help();
            true
        }
        s if s.starts_with('-') => {
            eprintln!("Unknown option: {s}");
            eprintln!("Try `rustdl --help`.");
            process::exit(2);
        }
        _ => {
            eprintln!("Unexpected argument: {first}");
            eprintln!("Try `rustdl --help`.");
            process::exit(2);
        }
    }
}

fn main() {
    if try_cli_and_exit() {
        return;
    }

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
