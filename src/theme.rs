//! Shared UI colors and egui visuals for dark/light themes.

use eframe::egui::{Color32, Stroke, Visuals};

pub const BG_CANVAS: Color32 = Color32::from_rgb(22, 22, 26);
pub const BG_LOG: Color32 = Color32::from_rgb(28, 28, 32);
pub const BG_INPUT_DARK: Color32 = Color32::from_rgb(32, 34, 42);
pub const BORDER_INPUT_DARK: Color32 = Color32::from_rgb(72, 78, 92);
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(56, 56, 64);
pub const BORDER_PANEL: Color32 = Color32::from_rgb(58, 58, 66);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(168, 170, 178);
pub const TEXT_HINT: Color32 = Color32::from_rgb(150, 152, 160);
pub const THUMB_PLACEHOLDER: Color32 = Color32::from_gray(32);

pub const BG_CANVAS_LIGHT: Color32 = Color32::from_rgb(245, 246, 248);
pub const BG_LOG_LIGHT: Color32 = Color32::from_rgb(236, 238, 242);
pub const BG_INPUT_LIGHT: Color32 = Color32::from_rgb(255, 255, 255);
pub const BORDER_INPUT_LIGHT: Color32 = Color32::from_rgb(186, 192, 204);
pub const BORDER_PANEL_LIGHT: Color32 = Color32::from_rgb(200, 204, 212);
pub const TEXT_MUTED_LIGHT: Color32 = Color32::from_rgb(90, 94, 102);
pub const TEXT_HINT_LIGHT: Color32 = Color32::from_rgb(110, 114, 122);
#[allow(dead_code)]
pub const THUMB_PLACEHOLDER_LIGHT: Color32 = Color32::from_gray(220);

pub fn text_hint(theme: &str) -> Color32 {
    if theme == "light" {
        TEXT_HINT_LIGHT
    } else {
        TEXT_HINT
    }
}

pub fn canvas_bg(theme: &str) -> Color32 {
    if theme == "light" {
        BG_CANVAS_LIGHT
    } else {
        BG_CANVAS
    }
}

pub fn log_bg(theme: &str) -> Color32 {
    if theme == "light" {
        BG_LOG_LIGHT
    } else {
        BG_LOG
    }
}

pub fn panel_border(theme: &str) -> Color32 {
    if theme == "light" {
        BORDER_PANEL_LIGHT
    } else {
        BORDER_PANEL
    }
}

pub fn text_muted(theme: &str) -> Color32 {
    if theme == "light" {
        TEXT_MUTED_LIGHT
    } else {
        TEXT_MUTED
    }
}

pub fn done_card_fill(theme: &str) -> Color32 {
    if theme == "light" {
        Color32::from_rgba_unmultiplied(56, 142, 60, 35)
    } else {
        Color32::from_rgba_unmultiplied(56, 142, 60, 45)
    }
}

pub fn dark_visuals() -> Visuals {
    let mut v = Visuals::dark();
    style_text_fields(&mut v, true);
    v.panel_fill = BG_CANVAS;
    v.window_fill = BG_CANVAS;
    v
}

pub fn light_visuals() -> Visuals {
    let mut v = Visuals::light();
    style_text_fields(&mut v, false);
    v
}

/// Text fields and combo boxes: slightly raised fill + visible outline vs panel background.
fn style_text_fields(v: &mut Visuals, dark: bool) {
    let (bg, border, bg_hover, border_hover, bg_active, border_active) = if dark {
        (
            BG_INPUT_DARK,
            BORDER_INPUT_DARK,
            Color32::from_rgb(38, 40, 50),
            Color32::from_rgb(90, 98, 114),
            Color32::from_rgb(36, 40, 52),
            Color32::from_rgb(66, 133, 244),
        )
    } else {
        (
            BG_INPUT_LIGHT,
            BORDER_INPUT_LIGHT,
            Color32::from_rgb(248, 249, 252),
            Color32::from_rgb(150, 160, 178),
            BG_INPUT_LIGHT,
            Color32::from_rgb(30, 136, 229),
        )
    };

    v.widgets.inactive.bg_fill = bg;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, border);
    v.widgets.hovered.bg_fill = bg_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, border_hover);
    v.widgets.active.bg_fill = bg_active;
    v.widgets.active.bg_stroke = Stroke::new(1.5, border_active);
}

pub fn visuals_for_theme(theme: &str) -> Visuals {
    match theme.trim().to_ascii_lowercase().as_str() {
        "light" => light_visuals(),
        "system" => {
            #[cfg(windows)]
            {
                use std::process::Command;

                use crate::external_tools::no_console_window;
                let mut cmd = Command::new("powershell");
                no_console_window(&mut cmd);
                let dark = cmd
                    .args([
                        "-NoProfile",
                        "-Command",
                        "(Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize').AppsUseLightTheme -eq 0",
                    ])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim() == "True")
                    .unwrap_or(true);
                if dark {
                    dark_visuals()
                } else {
                    light_visuals()
                }
            }
            #[cfg(not(windows))]
            {
                dark_visuals()
            }
        }
        _ => dark_visuals(),
    }
}

pub fn apply_ui_theme(ctx: &eframe::egui::Context, theme: &str) {
    ctx.set_visuals(visuals_for_theme(theme));
    ctx.style_mut(|style| {
        style.spacing.item_spacing = eframe::egui::vec2(9.0, 7.0);
        style.spacing.button_padding = eframe::egui::vec2(14.0, 8.0);
        let r = eframe::egui::Rounding::same(6.0);
        style.visuals.widgets.noninteractive.rounding = r;
        style.visuals.widgets.inactive.rounding = r;
        style.visuals.widgets.hovered.rounding = r;
        style.visuals.widgets.active.rounding = r;
        style.visuals.window_rounding = eframe::egui::Rounding::same(10.0);
    });
}
