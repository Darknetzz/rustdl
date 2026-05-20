//! Shared UI colors for dark panels (used across cards, logs, and chrome).

use eframe::egui::{Color32, Visuals};

pub const BG_CANVAS: Color32 = Color32::from_rgb(22, 22, 26);
pub const BG_LOG: Color32 = Color32::from_rgb(28, 28, 32);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(56, 56, 64);
pub const BORDER_PANEL: Color32 = Color32::from_rgb(58, 58, 66);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(168, 170, 178);
pub const TEXT_HINT: Color32 = Color32::from_rgb(150, 152, 160);
pub const THUMB_PLACEHOLDER: Color32 = Color32::from_gray(32);
pub const DONE_CARD_FILL: Color32 = Color32::from_rgba_unmultiplied(56, 142, 60, 45);

/// Base dark egui visuals (call once at startup).
pub fn dark_visuals() -> Visuals {
    Visuals::dark()
}
