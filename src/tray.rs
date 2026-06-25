//! System tray icon (minimize / close to tray on Windows and Linux).

use eframe::egui::Context;
use once_cell::sync::OnceCell;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};

static WAKE_CTX: OnceCell<Context> = OnceCell::new();
static WAKE_HANDLERS: OnceCell<()> = OnceCell::new();

pub const MENU_SHOW_ID: &str = "rustdl_tray_show";
pub const MENU_QUIT_ID: &str = "rustdl_tray_quit";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayAction {
    Show,
    Quit,
}

/// Holds the native tray icon alive for the session (Windows/macOS) or the GTK thread (Linux).
pub struct SystemTray {
    #[cfg(not(target_os = "linux"))]
    _icon: TrayIcon,
}

impl SystemTray {
    pub fn register_wake_context(ctx: &Context) {
        let _ = WAKE_CTX.set(ctx.clone());
        install_wake_handlers();
    }

    pub fn try_build() -> Option<Self> {
        let icon = crate::app_icon::tray_icon();
        let menu = build_menu()?;
        match build_tray(icon, menu) {
            Ok(tray) => Some(tray),
            Err(e) => {
                eprintln!("rustdl: failed to create system tray icon: {e}");
                None
            }
        }
    }

    pub fn poll() -> Option<TrayAction> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id.as_ref() == MENU_SHOW_ID {
                return Some(TrayAction::Show);
            }
            if event.id.as_ref() == MENU_QUIT_ID {
                return Some(TrayAction::Quit);
            }
        }
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                event,
                TrayIconEvent::Click { .. } | TrayIconEvent::DoubleClick { .. }
            ) {
                return Some(TrayAction::Show);
            }
        }
        None
    }
}

fn install_wake_handlers() {
    WAKE_HANDLERS.get_or_init(|| {
        TrayIconEvent::set_event_handler(Some(|_event| wake_ui()));
        MenuEvent::set_event_handler(Some(|_event| wake_ui()));
    });
}

fn wake_ui() {
    if let Some(ctx) = WAKE_CTX.get() {
        ctx.request_repaint();
    }
}

fn build_menu() -> Option<Menu> {
    let show = MenuItem::with_id(MENU_SHOW_ID, "Show rustdl", true, None);
    let quit = MenuItem::with_id(MENU_QUIT_ID, "Quit", true, None);
    let separator = PredefinedMenuItem::separator();
    Menu::with_items(&[&show, &separator, &quit]).ok()
}

#[cfg(not(target_os = "linux"))]
fn build_tray(icon: tray_icon::Icon, menu: Menu) -> Result<SystemTray, tray_icon::Error> {
    let icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("rustdl")
        .with_icon(icon)
        .build()?;
    Ok(SystemTray { _icon: icon })
}

#[cfg(target_os = "linux")]
fn build_tray(icon: tray_icon::Icon, menu: Menu) -> Result<SystemTray, tray_icon::Error> {
    static LINUX_TRAY: OnceCell<()> = OnceCell::new();
    LINUX_TRAY.get_or_init(|| {
        std::thread::spawn(move || {
            if gtk::init().is_err() {
                eprintln!("rustdl: failed to initialize GTK for the system tray");
                return;
            }
            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("rustdl")
                .with_icon(icon)
                .build();
            if let Err(e) = tray {
                eprintln!("rustdl: failed to create system tray icon: {e}");
                return;
            }
            gtk::main();
        });
    });
    Ok(SystemTray {})
}
