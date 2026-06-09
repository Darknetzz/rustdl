//! Windows main-window helpers (restore from taskbar, etc.).

use eframe::egui::{self, Context};
use raw_window_handle::HasWindowHandle;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

use crate::win_icon::hwnd_from_frame;

/// Work around winit/eframe sometimes leaving the HWND iconic after a taskbar restore click.
pub fn maybe_restore_main_window(frame: &impl HasWindowHandle, ctx: &Context) {
    let should_try = ctx.input(|i| {
        i.events
            .iter()
            .any(|e| matches!(e, egui::Event::WindowFocused(true)))
            || (i.focused && i.viewport().minimized == Some(true))
    });
    if !should_try {
        return;
    }
    let Some(hwnd) = hwnd_from_frame(frame) else {
        return;
    };
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.request_repaint();
}
