//! System tray icon (minimize / close to tray on Windows and Linux).

use eframe::egui::Context;
use once_cell::sync::OnceCell;

#[cfg(target_os = "linux")]
use crossbeam_channel::{Receiver, TryRecvError};

static WAKE_CTX: OnceCell<Context> = OnceCell::new();

pub const MENU_SHOW_ID: &str = "rustdl_tray_show";
pub const MENU_QUIT_ID: &str = "rustdl_tray_quit";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayAction {
    Show,
    Quit,
}

/// Holds the native tray icon alive for the session.
pub struct SystemTray {
    #[cfg(windows)]
    _icon: tray_icon::TrayIcon,
    #[cfg(target_os = "linux")]
    _handle: ksni::Handle<LinuxTray>,
}

impl SystemTray {
    pub fn register_wake_context(ctx: &Context) {
        let _ = WAKE_CTX.set(ctx.clone());
        install_wake_handlers();
    }

    pub fn try_build() -> Option<Self> {
        match build_tray() {
            Ok(tray) => Some(tray),
            Err(e) => {
                eprintln!("rustdl: failed to create system tray icon: {e}");
                None
            }
        }
    }

    pub fn poll() -> Option<TrayAction> {
        poll_tray_action()
    }
}

fn install_wake_handlers() {
    #[cfg(windows)]
    {
        static WAKE_HANDLERS: OnceCell<()> = OnceCell::new();
        WAKE_HANDLERS.get_or_init(|| {
            use tray_icon::{menu::MenuEvent, TrayIconEvent};
            TrayIconEvent::set_event_handler(Some(|_event| wake_ui()));
            MenuEvent::set_event_handler(Some(|_event| wake_ui()));
        });
    }
}

fn wake_ui() {
    if let Some(ctx) = WAKE_CTX.get() {
        ctx.request_repaint();
    }
}

#[cfg(windows)]
fn build_tray() -> Result<SystemTray, tray_icon::Error> {
    let icon = crate::app_icon::tray_icon();
    let menu = build_tray_icon_menu()?;
    let icon = tray_icon::TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("rustdl")
        .with_icon(icon)
        .build()?;
    Ok(SystemTray { _icon: icon })
}

#[cfg(windows)]
fn build_tray_icon_menu() -> Result<tray_icon::menu::Menu, tray_icon::Error> {
    use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
    let show = MenuItem::with_id(MENU_SHOW_ID, "Show rustdl", true, None);
    let quit = MenuItem::with_id(MENU_QUIT_ID, "Quit", true, None);
    let separator = PredefinedMenuItem::separator();
    Menu::with_items(&[&show, &separator, &quit]).map_err(|e| {
        tray_icon::Error::OsError(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))
    })
}

#[cfg(windows)]
fn poll_tray_action() -> Option<TrayAction> {
    use tray_icon::{menu::MenuEvent, TrayIconEvent};
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

#[cfg(target_os = "linux")]
static TRAY_ACTION_RX: OnceCell<Receiver<TrayAction>> = OnceCell::new();

#[cfg(target_os = "linux")]
struct LinuxTray {
    icon: ksni::Icon,
    action_tx: crossbeam_channel::Sender<TrayAction>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "rustdl".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.icon.clone()]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "rustdl".into(),
            description: "yt-dlp download manager".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.action_tx.send(TrayAction::Show);
        wake_ui();
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let show_tx = self.action_tx.clone();
        let quit_tx = self.action_tx.clone();
        vec![
            StandardItem {
                label: "Show rustdl".into(),
                activate: Box::new(move |_| {
                    let _ = show_tx.send(TrayAction::Show);
                    wake_ui();
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(move |_| {
                    let _ = quit_tx.send(TrayAction::Quit);
                    wake_ui();
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(target_os = "linux")]
fn build_tray() -> Result<SystemTray, String> {
    static LINUX_TRAY: OnceCell<()> = OnceCell::new();
    let (action_tx, action_rx) = crossbeam_channel::unbounded();
    let _ = TRAY_ACTION_RX.set(action_rx);
    let service = ksni::TrayService::new(LinuxTray {
        icon: crate::app_icon::ksni_tray_icon(),
        action_tx,
    });
    let handle = service.handle();
    LINUX_TRAY.get_or_init(|| {
        service.spawn();
    });
    Ok(SystemTray { _handle: handle })
}

#[cfg(target_os = "linux")]
fn poll_tray_action() -> Option<TrayAction> {
    let rx = TRAY_ACTION_RX.get()?;
    match rx.try_recv() {
        Ok(action) => Some(action),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => None,
    }
}
