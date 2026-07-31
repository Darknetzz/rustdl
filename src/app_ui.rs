use std::hash::Hash;

use eframe::egui;
use eframe::egui::{Color32, Id, InnerResponse, Response, RichText, Shape, Stroke};
use egui::layers::ShapeIdx;
use egui::popup::{popup_above_or_below_widget, PopupCloseBehavior};
use egui::{AboveOrBelow, Frame, TextWrapMode};

use crate::disk_space::{DiskSpace, DiskSpaceLevel};
use crate::models::ItemStatus;
use crate::theme::{
    mode_accent_for, mode_border, mode_soft_tint, panel_border, panel_fill, text_muted,
    ModePanelColors,
};
use crate::ui_icons;

pub const HEADER_STATUS_FONT_SIZE: f32 = 13.0;

/// Slightly larger than `.small()` for the main header status row (tools, disk, activity).
pub fn header_status_rich(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).size(HEADER_STATUS_FONT_SIZE)
}

/// Text color for the free-space figure (green → amber → red by [`DiskSpaceLevel`]).
pub fn disk_space_free_color(level: DiskSpaceLevel) -> Color32 {
    match level {
        DiskSpaceLevel::Ok => Color32::from_rgb(129, 199, 132),
        DiskSpaceLevel::Low => ALERT_WARNING_TEXT,
        DiskSpaceLevel::Critical => ALERT_DANGER_TEXT,
    }
}

/// Fill color for the free-space progress bar (green → amber → red by [`DiskSpaceLevel`]).
pub fn disk_space_bar_fill_color(level: DiskSpaceLevel) -> Color32 {
    match level {
        DiskSpaceLevel::Ok => Color32::from_rgb(102, 187, 106),
        DiskSpaceLevel::Low => Color32::from_rgb(255, 167, 38),
        DiskSpaceLevel::Critical => Color32::from_rgb(229, 57, 53),
    }
}

fn disk_space_bar_label_color(level: DiskSpaceLevel) -> Color32 {
    match level {
        DiskSpaceLevel::Ok | DiskSpaceLevel::Critical => Color32::WHITE,
        DiskSpaceLevel::Low => Color32::from_rgb(24, 24, 24),
    }
}

fn disk_space_bar_track_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(48, 50, 56)
    } else {
        Color32::from_rgb(210, 212, 218)
    }
}

/// Thin progress bar showing used disk percentage; fill color reflects used-space level.
pub fn draw_disk_space_progress_bar(
    ui: &mut egui::Ui,
    percent_free: f64,
    _level: DiskSpaceLevel,
    width: f32,
) -> Response {
    let percent_used = (100.0 - percent_free).clamp(0.0, 100.0);
    let fraction = (percent_used / 100.0).clamp(0.0, 1.0) as f32;
    let bar_level = DiskSpace::bar_level_from_used(percent_used);
    let height = 12.0;
    let rounding = height * 0.5;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

    let track = disk_space_bar_track_color(ui);
    let fill_color = disk_space_bar_fill_color(bar_level);
    let label_color = disk_space_bar_label_color(bar_level);

    ui.painter().rect_filled(rect, rounding, track);

    if fraction > 0.0 {
        let fill_w = (rect.width() * fraction)
            .max(if fraction >= 1.0 {
                rect.width()
            } else {
                rounding * 2.0
            })
            .min(rect.width());
        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
        ui.painter().rect_filled(fill_rect, rounding, fill_color);

        let pct_text = format!("{:.0}%", percent_used);
        let font = egui::FontId::proportional(12.0);
        let galley = ui.painter().layout_no_wrap(pct_text, font, label_color);
        let text_home = if fraction > 0.0 && fill_w >= galley.size().x + 6.0 {
            fill_rect
        } else {
            rect
        };
        let pos = text_home.center() - galley.size() * 0.5;
        ui.painter().galley(pos, galley, label_color);
    } else {
        let pct_text = format!("{:.0}%", percent_used);
        let font = egui::FontId::proportional(12.0);
        let galley = ui.painter().layout_no_wrap(pct_text, font, label_color);
        let pos = rect.center() - galley.size() * 0.5;
        ui.painter().galley(pos, galley, label_color);
    }

    response.on_hover_text(format!(
        "{:.1}% used · {:.1}% free space remaining",
        percent_used, percent_free
    ))
}

fn draw_usage_stat_label(
    ui: &mut egui::Ui,
    icon: &str,
    name: &str,
    percent: Option<f32>,
    muted: Color32,
) {
    let text = format!(
        "{icon} {name} {}",
        crate::system_usage::format_usage_percent(percent)
    );
    let color = percent
        .map(crate::system_usage::usage_level_color)
        .unwrap_or(muted);
    let hover = percent
        .map(|v| format!("{name} utilization: {v:.1}%"))
        .unwrap_or_else(|| format!("{name} utilization: measuring…"));
    ui.add(
        egui::Label::new(header_status_rich(text).color(color).strong())
            .sense(egui::Sense::hover())
            .selectable(false),
    )
    .on_hover_text(hover);
}

/// CPU / RAM / GPU utilization chips for the main header (polled in the background).
pub fn draw_system_usage_header(
    ui: &mut egui::Ui,
    usage: &crate::system_usage::SystemUsageSnapshot,
    theme: &str,
    vertical: bool,
) {
    let muted = text_muted(theme);
    let draw = |ui: &mut egui::Ui| {
        draw_usage_stat_label(ui, ui_icons::USAGE_CPU, "CPU", usage.cpu_percent, muted);
        draw_usage_stat_label(ui, ui_icons::USAGE_RAM, "RAM", usage.ram_percent, muted);
        #[cfg(windows)]
        draw_usage_stat_label(ui, ui_icons::USAGE_GPU, "GPU", usage.gpu_percent, muted);
    };
    if vertical {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            draw(ui);
        });
    } else {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            draw_usage_stat_label(ui, ui_icons::USAGE_CPU, "CPU", usage.cpu_percent, muted);
            ui.label(
                RichText::new("·")
                    .size(HEADER_STATUS_FONT_SIZE)
                    .color(muted),
            );
            draw_usage_stat_label(ui, ui_icons::USAGE_RAM, "RAM", usage.ram_percent, muted);
            #[cfg(windows)]
            {
                ui.label(
                    RichText::new("·")
                        .size(HEADER_STATUS_FONT_SIZE)
                        .color(muted),
                );
                draw_usage_stat_label(ui, ui_icons::USAGE_GPU, "GPU", usage.gpu_percent, muted);
            }
        });
    }
}

pub fn status_color(s: ItemStatus) -> Color32 {
    match s {
        ItemStatus::Resolving => Color32::from_rgb(120, 144, 156),
        ItemStatus::Idle => Color32::GRAY,
        ItemStatus::Queued => Color32::from_rgb(255, 193, 7),
        ItemStatus::Downloading => Color32::from_rgb(66, 165, 245),
        ItemStatus::Done => Color32::from_rgb(129, 199, 132),
        ItemStatus::Failed => Color32::from_rgb(239, 83, 80),
    }
}

/// Small keyboard-key chip (HTML `<kbd>`-like).
pub fn draw_kbd_key(ui: &mut egui::Ui, label: &str) -> Response {
    let (fill, border, text) = kbd_chip_colors(ui);
    egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, border))
        .inner_margin(egui::Margin::symmetric(5.0, 2.0))
        .rounding(egui::Rounding::same(4.0))
        .show(ui, |ui| {
            ui.label(RichText::new(label).monospace().size(11.0).color(text));
        })
        .response
}

fn kbd_chip_colors(ui: &egui::Ui) -> (Color32, Color32, Color32) {
    if ui.visuals().dark_mode {
        (
            Color32::from_rgb(40, 42, 50),
            Color32::from_rgb(78, 82, 94),
            Color32::from_rgb(232, 234, 240),
        )
    } else {
        (
            Color32::from_rgb(248, 249, 252),
            Color32::from_rgb(186, 192, 204),
            Color32::from_rgb(32, 34, 40),
        )
    }
}

fn draw_kbd_sequence(ui: &mut egui::Ui, keys: &[&str], theme: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        let muted = text_muted(theme);
        for (i, key) in keys.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("+").small().color(muted));
            }
            draw_kbd_key(ui, key);
        }
    });
}

/// One shortcut row: key chips, optional macOS alternate, then description.
pub fn draw_keyboard_shortcut_row(
    ui: &mut egui::Ui,
    theme: &str,
    windows_keys: &[&str],
    mac_keys: Option<&[&str]>,
    description: &str,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        draw_kbd_sequence(ui, windows_keys, theme);
        if let Some(mac) = mac_keys {
            ui.label(RichText::new("(").small().color(text_muted(theme)));
            draw_kbd_sequence(ui, mac, theme);
            ui.label(RichText::new(")").small().color(text_muted(theme)));
        }
        ui.label(
            RichText::new(format!(": {description}"))
                .small()
                .color(text_muted(theme)),
        );
    });
}

/// Full-width batch progress bar with a caller-supplied caption.
pub fn draw_batch_progress_bar(
    ui: &mut egui::Ui,
    fraction: f32,
    caption: impl AsRef<str>,
    fill: Color32,
    animate: bool,
) -> Response {
    let w = clip_bounded_width(ui);
    ui.add_sized(
        [w, ui.spacing().interact_size.y],
        egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
            .fill(fill)
            .animate(animate)
            .text(caption.as_ref()),
    )
}

/// Small filled circle aligned with status summary text (e.g. download counts).
pub fn draw_status_dot(ui: &mut egui::Ui, color: Color32) {
    let dot = 8.0;
    let line_h = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(dot, line_h), egui::Sense::hover());
    let center = rect.center();
    let radius = dot * 0.38;
    ui.painter().circle_filled(center, radius, color);
    ui.painter().circle_stroke(
        center,
        radius,
        egui::Stroke::new(1.0_f32, shade(color, 0.72)),
    );
}

/// Status dot immediately before colored label text.
pub fn status_dot_with_label(
    ui: &mut egui::Ui,
    text: impl AsRef<str>,
    color: Color32,
    strong: bool,
) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        draw_status_dot(ui, color);
        let mut rt = RichText::new(text.as_ref()).color(color);
        if strong {
            rt = rt.strong();
        }
        ui.label(rt)
    })
    .response
}

pub fn status_chip_icon(status: ItemStatus) -> &'static str {
    match status {
        ItemStatus::Resolving => ui_icons::STATUS_RESOLVING,
        ItemStatus::Idle => ui_icons::STATUS_IDLE,
        ItemStatus::Queued => ui_icons::STATUS_QUEUED,
        ItemStatus::Downloading => ui_icons::STATUS_DOWNLOADING,
        ItemStatus::Done => ui_icons::STATUS_DONE,
        ItemStatus::Failed => ui_icons::STATUS_FAILED,
    }
}

/// Label color on status chips (matches web `.status-chip` light backgrounds).
pub fn status_chip_text_color(status: ItemStatus) -> Color32 {
    match status {
        ItemStatus::Idle | ItemStatus::Queued | ItemStatus::Done => Color32::from_rgb(26, 26, 26),
        _ => Color32::WHITE,
    }
}

/// Top-bar activity state (aligned with web `#navbar-status`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavbarStatusSlug {
    Idle,
    Adding,
    Resolving,
    Queued,
    Paused,
    Downloading,
    Converting,
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct NavbarStatusInfo {
    pub slug: NavbarStatusSlug,
    pub label: &'static str,
    pub pulse: bool,
    pub title: String,
}

pub struct NavbarStatusInputs {
    pub shutdown_pending: bool,
    pub add_in_progress: bool,
    pub convert_running: bool,
    pub convert_paused: bool,
    pub convert_has_pending: bool,
    pub convert_resolving: bool,
    pub status_resolving: usize,
    pub status_queued: usize,
    pub status_active: usize,
    pub status_ready: usize,
    pub downloads_paused: bool,
    pub queue_running: usize,
}

pub fn derive_navbar_status(input: NavbarStatusInputs) -> NavbarStatusInfo {
    if input.shutdown_pending {
        return NavbarStatusInfo {
            slug: NavbarStatusSlug::Shutdown,
            label: "Shutting down",
            pulse: true,
            title: "rustdl is saving state and exiting".to_owned(),
        };
    }
    if input.add_in_progress {
        return NavbarStatusInfo {
            slug: NavbarStatusSlug::Adding,
            label: "Adding URLs",
            pulse: true,
            title: "Fetching metadata for new URLs".to_owned(),
        };
    }
    if input.convert_running {
        return NavbarStatusInfo {
            slug: NavbarStatusSlug::Converting,
            label: "Converting",
            pulse: true,
            title: "Convert batch encode in progress".to_owned(),
        };
    }
    if input.status_active > 0 || (input.queue_running > 0 && !input.downloads_paused) {
        return NavbarStatusInfo {
            slug: NavbarStatusSlug::Downloading,
            label: "Downloading",
            pulse: true,
            title: format!(
                "{} active · {} worker slot(s)",
                input.status_active, input.queue_running
            ),
        };
    }
    if input.status_resolving > 0 || input.convert_resolving {
        return NavbarStatusInfo {
            slug: NavbarStatusSlug::Resolving,
            label: "Resolving",
            pulse: true,
            title: "Probing media metadata".to_owned(),
        };
    }
    if input.convert_paused && input.convert_has_pending && !input.convert_running {
        return NavbarStatusInfo {
            slug: NavbarStatusSlug::Paused,
            label: "Paused",
            pulse: false,
            title: "Convert batch paused — resume to continue".to_owned(),
        };
    }
    if input.downloads_paused
        && (input.status_queued > 0 || input.status_active > 0 || input.status_ready > 0)
    {
        return NavbarStatusInfo {
            slug: NavbarStatusSlug::Paused,
            label: "Paused",
            pulse: false,
            title: "Downloads paused — resume to continue".to_owned(),
        };
    }
    if input.status_queued > 0 {
        return NavbarStatusInfo {
            slug: NavbarStatusSlug::Queued,
            label: "Queued",
            pulse: true,
            title: format!("{} item(s) waiting to download", input.status_queued),
        };
    }
    NavbarStatusInfo {
        slug: NavbarStatusSlug::Idle,
        label: "Idle",
        pulse: false,
        title: "No active downloads or conversions".to_owned(),
    }
}

fn navbar_status_colors(slug: NavbarStatusSlug) -> (Color32, Color32, Color32) {
    let (text, dot) = match slug {
        NavbarStatusSlug::Idle => (Color32::GRAY, Color32::GRAY),
        NavbarStatusSlug::Adding | NavbarStatusSlug::Resolving => (
            Color32::from_rgb(120, 144, 156),
            Color32::from_rgb(120, 144, 156),
        ),
        NavbarStatusSlug::Queued | NavbarStatusSlug::Paused | NavbarStatusSlug::Shutdown => (
            Color32::from_rgb(255, 193, 7),
            Color32::from_rgb(255, 193, 7),
        ),
        NavbarStatusSlug::Downloading => (
            Color32::from_rgb(66, 165, 245),
            Color32::from_rgb(66, 165, 245),
        ),
        NavbarStatusSlug::Converting => (
            Color32::from_rgb(171, 71, 188),
            Color32::from_rgb(171, 71, 188),
        ),
    };
    let border = Color32::from_rgba_unmultiplied(dot.r(), dot.g(), dot.b(), (255.0 * 0.35) as u8);
    (text, dot, border)
}

fn fade_color(c: Color32, alpha: f32) -> Color32 {
    let a = alpha.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        (c.r() as f32 * a) as u8,
        (c.g() as f32 * a) as u8,
        (c.b() as f32 * a) as u8,
        (c.a() as f32 * a) as u8,
    )
}

pub fn draw_navbar_status_badge(ui: &mut egui::Ui, info: &NavbarStatusInfo) -> Response {
    if info.pulse {
        ui.ctx().request_repaint();
    }
    let (text_color, dot_color, border_color) = navbar_status_colors(info.slug);
    let pulse_phase = if info.pulse {
        let t = ui.input(|i| i.time);
        (t * std::f64::consts::TAU / 1.4).sin() * 0.5 + 0.5
    } else {
        1.0
    };
    let dot_alpha = if info.pulse {
        (0.35 + 0.65 * pulse_phase) as f32
    } else {
        1.0
    };
    let border_alpha = if info.pulse {
        (0.45 + 0.55 * pulse_phase) as f32
    } else {
        1.0
    };
    let label_alpha = if info.pulse {
        (0.62 + 0.38 * pulse_phase) as f32
    } else {
        1.0
    };
    egui::Frame::none()
        .stroke(egui::Stroke::new(
            1.0_f32,
            fade_color(border_color, border_alpha),
        ))
        .rounding(egui::Rounding::same(999.0))
        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                draw_status_dot(ui, fade_color(dot_color, dot_alpha));
                ui.label(
                    RichText::new(info.label)
                        .size(HEADER_STATUS_FONT_SIZE)
                        .strong()
                        .color(fade_color(text_color, label_alpha)),
                );
            })
        })
        .response
        .on_hover_text(&info.title)
}

pub fn draw_status_chip(ui: &mut egui::Ui, status: ItemStatus) {
    let label = format!("{} {}", status_chip_icon(status), status.as_str());
    let text = RichText::new(label)
        .small()
        .color(status_chip_text_color(status));
    let fill = status_color(status);
    egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.label(text);
        });
}

#[derive(Clone, Copy)]
pub enum MetaBadgeKind {
    Resolution,
    SizeEstimate,
    Codec,
    FrameRate,
    FileSize,
    Bitrate,
    SizePreset,
    ShrinkPercent,
    ConvertWillSkip,
    FileMissing,
}

fn parse_resolution_height(label: &str) -> u32 {
    if let Some((_, h)) = label.split_once('x') {
        return h.trim().parse().unwrap_or(0);
    }
    if let Some(h) = label.trim().strip_suffix('p') {
        return h.parse().unwrap_or(0);
    }
    if let Some(h) = label.trim().strip_suffix('w') {
        return h.parse().unwrap_or(0);
    }
    0
}

fn resolution_badge_colors(label: &str) -> (Color32, Color32) {
    match parse_resolution_height(label) {
        h if h >= 2160 => (
            Color32::from_rgb(90, 45, 130),
            Color32::from_rgb(240, 220, 255),
        ),
        h if h >= 1080 => (
            Color32::from_rgb(38, 90, 136),
            Color32::from_rgb(230, 240, 255),
        ),
        h if h >= 720 => (
            Color32::from_rgb(25, 100, 90),
            Color32::from_rgb(210, 248, 240),
        ),
        h if h >= 480 => (
            Color32::from_rgb(100, 85, 40),
            Color32::from_rgb(255, 244, 210),
        ),
        _ => (
            Color32::from_rgb(70, 70, 80),
            Color32::from_rgb(220, 220, 228),
        ),
    }
}

fn codec_badge_colors(label: &str) -> (Color32, Color32) {
    let c = label.to_ascii_lowercase().replace(['.', '-', ' ', '_'], "");
    if c.contains("convert") {
        (
            Color32::from_rgb(40, 110, 60),
            Color32::from_rgb(215, 255, 225),
        )
    } else if c.contains("hevc") || c.contains("h265") || c.contains("265") {
        (
            Color32::from_rgb(130, 75, 25),
            Color32::from_rgb(255, 232, 200),
        )
    } else if c.contains("h264") || c.contains("avc") || c.contains("264") {
        (
            Color32::from_rgb(35, 75, 140),
            Color32::from_rgb(220, 235, 255),
        )
    } else if c.contains("vp9") {
        (
            Color32::from_rgb(85, 50, 120),
            Color32::from_rgb(235, 220, 255),
        )
    } else if c.contains("vp8") {
        (
            Color32::from_rgb(70, 70, 100),
            Color32::from_rgb(230, 230, 240),
        )
    } else {
        (
            Color32::from_rgb(65, 65, 75),
            Color32::from_rgb(230, 230, 235),
        )
    }
}

fn fps_badge_colors(label: &str) -> (Color32, Color32) {
    let fps: f32 = label
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    if fps >= 50.0 {
        (
            Color32::from_rgb(20, 120, 100),
            Color32::from_rgb(200, 255, 240),
        )
    } else if fps >= 28.0 {
        (
            Color32::from_rgb(45, 95, 160),
            Color32::from_rgb(220, 235, 255),
        )
    } else if fps >= 23.0 {
        (
            Color32::from_rgb(120, 85, 40),
            Color32::from_rgb(255, 240, 210),
        )
    } else {
        (
            Color32::from_rgb(70, 70, 80),
            Color32::from_rgb(220, 220, 228),
        )
    }
}

fn size_preset_badge_colors(label: &str) -> (Color32, Color32) {
    match label.trim().to_ascii_lowercase().as_str() {
        "light" => (
            Color32::from_rgb(30, 100, 70),
            Color32::from_rgb(210, 255, 230),
        ),
        "aggressive" => (
            Color32::from_rgb(130, 55, 30),
            Color32::from_rgb(255, 225, 205),
        ),
        _ => (
            Color32::from_rgb(38, 90, 136),
            Color32::from_rgb(230, 240, 255),
        ),
    }
}

fn shrink_percent_badge_colors(label: &str) -> (Color32, Color32) {
    let pct: f32 = label.trim().trim_end_matches('%').parse().unwrap_or(0.0);
    if pct <= 0.0 {
        (
            Color32::from_rgb(70, 70, 80),
            Color32::from_rgb(220, 220, 228),
        )
    } else if pct >= 50.0 {
        (
            Color32::from_rgb(130, 70, 25),
            Color32::from_rgb(255, 232, 200),
        )
    } else {
        (
            Color32::from_rgb(100, 85, 40),
            Color32::from_rgb(255, 244, 210),
        )
    }
}

fn meta_badge_colors(kind: MetaBadgeKind, label: &str) -> (Color32, Color32) {
    match kind {
        MetaBadgeKind::Resolution => resolution_badge_colors(label),
        MetaBadgeKind::SizeEstimate | MetaBadgeKind::FileSize => (
            Color32::from_rgb(120, 75, 20),
            Color32::from_rgb(255, 236, 200),
        ),
        MetaBadgeKind::Codec => codec_badge_colors(label),
        MetaBadgeKind::FrameRate => fps_badge_colors(label),
        MetaBadgeKind::Bitrate => (
            Color32::from_rgb(75, 55, 110),
            Color32::from_rgb(235, 225, 255),
        ),
        MetaBadgeKind::SizePreset => size_preset_badge_colors(label),
        MetaBadgeKind::ShrinkPercent => shrink_percent_badge_colors(label),
        MetaBadgeKind::ConvertWillSkip => (
            Color32::from_rgb(120, 70, 20),
            Color32::from_rgb(255, 220, 180),
        ),
        MetaBadgeKind::FileMissing => (
            Color32::from_rgb(110, 32, 32),
            Color32::from_rgb(255, 210, 210),
        ),
    }
}

/// Small pill label for resolution, codec, fps, and related metadata on video cards.
pub fn draw_meta_badge(ui: &mut egui::Ui, label: &str, kind: MetaBadgeKind) {
    let (fill, text_color) = meta_badge_colors(kind, label);
    egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(5.0))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(label).small().strong().color(text_color));
        });
}

/// Muted prefix label plus a colored value pill (e.g. encode settings summary).
pub fn draw_labeled_meta_badge(
    ui: &mut egui::Ui,
    prefix: &str,
    value: &str,
    kind: MetaBadgeKind,
    prefix_color: Color32,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(prefix).small().color(prefix_color));
        draw_meta_badge(ui, value, kind);
    });
}

fn shade(color: Color32, factor: f32) -> Color32 {
    let [r, g, b, a] = color.to_array();
    let scale = |v: u8| -> u8 { ((v as f32 * factor).round()).clamp(0.0, 255.0) as u8 };
    Color32::from_rgba_unmultiplied(scale(r), scale(g), scale(b), a)
}

fn colored_button(
    ui: &mut egui::Ui,
    label: impl Into<RichText>,
    enabled: bool,
    text_color: Color32,
    bg_fill: Color32,
    rounding: egui::Rounding,
    stroke: egui::Stroke,
) -> Response {
    let label = label.into();
    let (fill, stroke, text) = if enabled {
        (bg_fill, stroke, text_color)
    } else {
        (
            shade(bg_fill, 0.45),
            egui::Stroke::new(stroke.width, shade(stroke.color, 0.35)),
            shade(text_color, 0.70),
        )
    };

    let button = egui::Button::new(label.color(text))
        .frame(true)
        .fill(fill)
        .stroke(stroke)
        .rounding(rounding);
    ui.add_enabled(enabled, button)
}

fn standalone_button_stroke(bg_fill: Color32) -> egui::Stroke {
    egui::Stroke::new(1.0_f32, shade(bg_fill, 0.78))
}

fn grouped_button_stroke() -> egui::Stroke {
    egui::Stroke::NONE
}

/// Bootstrap 5 `.alert-warning` palette (`#fff3cd` / `#ffecb5` / `#664d03`).
const ALERT_WARNING_BG: Color32 = Color32::from_rgb(255, 243, 205);
const ALERT_WARNING_BORDER: Color32 = Color32::from_rgb(255, 236, 181);
pub const ALERT_WARNING_TEXT: Color32 = Color32::from_rgb(102, 77, 3);

/// Bootstrap 5 `.alert-danger` palette (`#f8d7da` / `#f5c2c7` / `#842029`).
const ALERT_DANGER_BG: Color32 = Color32::from_rgb(248, 215, 218);
const ALERT_DANGER_BORDER: Color32 = Color32::from_rgb(245, 194, 199);
pub const ALERT_DANGER_TEXT: Color32 = Color32::from_rgb(132, 32, 41);

/// Horizontal inset for the main window content area ([`content_panel_frame`]).
pub const CONTENT_MARGIN_LEFT: f32 = 20.0;
pub const CONTENT_MARGIN_RIGHT: f32 = 32.0;
pub const CONTENT_MARGIN_V: f32 = 12.0;

pub fn content_panel_frame() -> egui::Frame {
    egui::Frame::default().inner_margin(egui::Margin {
        left: CONTENT_MARGIN_LEFT,
        right: CONTENT_MARGIN_RIGHT,
        top: CONTENT_MARGIN_V,
        bottom: CONTENT_MARGIN_V,
    })
}

/// Horizontal inset for docked bottom panels (queue / footer) to match [`content_panel_frame`].
pub fn dock_panel_horizontal_frame() -> egui::Frame {
    egui::Frame::default().inner_margin(egui::Margin {
        left: CONTENT_MARGIN_LEFT,
        right: CONTENT_MARGIN_RIGHT,
        top: 0.0,
        bottom: 0.0,
    })
}

/// Dimmed fill for queue status groups (Active, Ready, Done, …).
pub fn queue_group_section_fill(theme: &str) -> Color32 {
    if theme == "light" {
        Color32::from_rgb(224, 226, 234)
    } else {
        Color32::from_rgb(16, 17, 22)
    }
}

/// Wraps a collapsible queue group in a rounded panel with a dimmed background.
pub fn show_queue_group_section<R>(
    ui: &mut egui::Ui,
    theme: &str,
    accent: Color32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let max_w = clip_bounded_width(ui);
    egui::Frame::none()
        .fill(queue_group_section_fill(theme))
        .stroke(Stroke::new(1.0_f32, mode_border(accent)))
        .inner_margin(egui::Margin::symmetric(10.0, 8.0))
        .outer_margin(egui::Margin {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 8.0,
        })
        .rounding(egui::Rounding::same(8.0))
        .show(ui, |ui| {
            ui.set_max_width(max_w);
            ui.set_width(max_w);
            add_contents(ui)
        })
}

fn mode_panel_gradient_shape(rect: egui::Rect, left: Color32, right: Color32) -> Shape {
    let mut mesh = egui::Mesh::default();
    let base = mesh.vertices.len() as u32;
    mesh.colored_vertex(rect.left_top(), left);
    mesh.colored_vertex(rect.right_top(), right);
    mesh.colored_vertex(rect.right_bottom(), right);
    mesh.colored_vertex(rect.left_bottom(), left);
    mesh.add_triangle(base, base + 1, base + 2);
    mesh.add_triangle(base, base + 2, base + 3);
    Shape::mesh(mesh)
}

struct ModePanelStyle {
    accent: Color32,
    panel: Color32,
    soft: Color32,
    rounding: egui::Rounding,
    stroke: Stroke,
}

fn paint_mode_panel_background(
    painter: &egui::Painter,
    shape_idx: ShapeIdx,
    rect: egui::Rect,
    style: &ModePanelStyle,
) {
    let ModePanelStyle {
        accent,
        panel,
        soft,
        rounding,
        stroke,
    } = *style;
    let mut shapes = vec![Shape::rect_filled(rect, rounding, panel)];
    let fade_w = rect.width() * 0.28;
    if fade_w > 1.0 {
        let grad_rect = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + fade_w, rect.bottom()),
        );
        shapes.push(mode_panel_gradient_shape(grad_rect, soft, panel));
    }
    let stripe_w = 3.0f32.min(rect.width());
    if stripe_w > 0.0 {
        let stripe = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + stripe_w, rect.bottom()),
        );
        shapes.push(Shape::rect_filled(
            stripe,
            egui::Rounding {
                nw: rounding.nw,
                ne: 0.0,
                sw: rounding.sw,
                se: 0.0,
            },
            accent,
        ));
    }
    shapes.push(Shape::rect_stroke(rect, rounding, stroke));
    painter.set(shape_idx, Shape::Vec(shapes));
}

/// Panel chrome for Downloader / AV1 sections (muted tint + left accent; matches web `.panel`).
///
/// Use `fill_height = true` for docked/floating queue shells so the accent frame fills the
/// allocated panel; main-column panels should pass `false` (shrink-wrap).
pub fn show_mode_panel<R>(
    ui: &mut egui::Ui,
    theme: &str,
    av1: bool,
    colors: ModePanelColors<'_>,
    inner_margin: egui::Margin,
    rounding: f32,
    fill_height: bool,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> InnerResponse<R> {
    let accent = mode_accent_for(av1, &colors);
    let border = mode_border(accent);
    let panel = panel_fill(theme);
    let soft = mode_soft_tint(accent, theme);
    let rounding = egui::Rounding::same(rounding);
    let style = ModePanelStyle {
        accent,
        panel,
        soft,
        rounding,
        stroke: Stroke::new(1.0_f32, border),
    };

    egui::Frame::none()
        .inner_margin(inner_margin)
        .show(ui, |ui| {
            let draw = |ui: &mut egui::Ui| {
                let bg_idx = ui.painter().add(Shape::Noop);
                let ret = add_contents(ui);
                let mut paint_rect = ui.min_rect() + inner_margin;
                if fill_height {
                    let max = ui.max_rect();
                    if max.height().is_finite() && max.height() > paint_rect.height() + 2.0 {
                        paint_rect.max.y =
                            (max.max.y - inner_margin.bottom).min(ui.clip_rect().bottom());
                    }
                }
                paint_rect.max.x = paint_rect.max.x.min(ui.clip_rect().right());
                paint_rect = paint_rect.intersect(ui.clip_rect());
                if ui.is_rect_visible(paint_rect) {
                    paint_mode_panel_background(ui.painter(), bg_idx, paint_rect, &style);
                }
                ret
            };
            if fill_height {
                with_full_panel(ui, draw)
            } else {
                with_full_width(ui, draw)
            }
        })
}

/// Virtualized evenly spaced rows inside an outer [`egui::ScrollArea`] (no nested scroll).
///
/// Reserves `total_rows × (row_height + spacing)` and only paints rows that intersect the clip rect.
pub fn show_virtualized_rows(
    ui: &mut egui::Ui,
    row_height: f32,
    total_rows: usize,
    mut add_row: impl FnMut(&mut egui::Ui, usize),
) {
    if total_rows == 0 || !row_height.is_finite() || row_height < 1.0 {
        return;
    }
    let spacing_y = ui.spacing().item_spacing.y.max(0.0);
    let row_step = row_height + spacing_y;
    let total_h = (row_step * total_rows as f32 - spacing_y).max(0.0);
    let width = clip_bounded_width(ui).max(1.0);
    let top = ui.cursor().min.y;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, total_h), egui::Sense::hover());
    let clip = ui.clip_rect().intersect(rect);
    if clip.height() < 0.5 || clip.width() < 0.5 {
        return;
    }
    let mut min_row = ((clip.top() - top) / row_step).floor().max(0.0) as usize;
    let mut max_row = ((clip.bottom() - top) / row_step).ceil() as usize + 1;
    if max_row > total_rows {
        let diff = max_row.saturating_sub(min_row);
        max_row = total_rows;
        min_row = total_rows.saturating_sub(diff);
    }
    min_row = min_row.min(max_row);
    for row in min_row..max_row {
        let y = top + row as f32 * row_step;
        let row_rect =
            egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(width, row_height));
        ui.allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(row_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.set_max_width(width);
                ui.set_min_height(row_height);
                add_row(ui, row);
            },
        );
    }
}

pub const VIDEOS_DOCKED_HEIGHT_RATIO: f32 = 0.52;
/// Review layout preset: taller docked queue panel.
pub const REVIEW_DOCK_HEIGHT_RATIO: f32 = 0.60;
/// Minimal layout preset: shorter docked queue panel.
pub const MINIMAL_DOCK_HEIGHT_RATIO: f32 = 0.40;
/// Shared absolute min/max for docked queue and undocked footer (log docked).
pub const BOTTOM_PANEL_MIN_H: f32 = 180.0;
/// Minimum bottom panel when the activity log is docked under the queue (list + log chrome).
pub const BOTTOM_PANEL_MIN_H_WITH_DOCKED_LOG: f32 = 360.0;
pub const BOTTOM_PANEL_MAX_H: f32 = 800.0;

/// Downloader queue list row height (virtualized list layout).
pub const QUEUE_DL_LIST_ROW_H: f32 = 42.0;
/// Convert queue list row height (full detail).
pub const QUEUE_CONVERT_LIST_ROW_H: f32 = 140.0;
/// Convert queue list row height (compact panels / minimal preset).
pub const QUEUE_CONVERT_LIST_ROW_COMPACT_H: f32 = 88.0;
pub const QUEUE_FLOATING_LIST_MIN_H: f32 = 80.0;
pub const QUEUE_DOCKED_LIST_MIN_PAD: f32 = 8.0;

/// Force list layout when outer scroll is shorter than this (downloader).
pub const QUEUE_DL_SHORT_PANEL_LIST_THRESHOLD: f32 = 220.0;
/// Force list layout when outer scroll is shorter than this (convert).
pub const QUEUE_CONVERT_SHORT_PANEL_LIST_THRESHOLD: f32 = 280.0;
pub const QUEUE_DL_LIST_FALLBACK_THRESHOLD: f32 = 200.0;
pub const QUEUE_CONVERT_LIST_FALLBACK_THRESHOLD: f32 = 280.0;
/// Max list scroll height used in layout math (guards bad body_bottom in float windows).
pub const QUEUE_LIST_LAYOUT_MAX_H: f32 = 1200.0;
/// Skip per-group nested scroll cap when the outer queue scroll is shorter than this.
pub const QUEUE_FLATTEN_NESTED_SCROLL_THRESHOLD: f32 = 320.0;
/// Use compact convert list rows below this outer scroll height (or compact_cards).
pub const QUEUE_COMPACT_CONVERT_ROW_THRESHOLD: f32 = 260.0;

pub const QUEUE_STATUS_COMPACT_DL_THRESHOLD: f32 = 120.0;
pub const QUEUE_STATUS_COMPACT_CONVERT_THRESHOLD: f32 = 200.0;
pub const MODE_NAV_COMPACT_BREAKPOINT: f32 = 720.0;
const LAYOUT_UI_SCALE_FLOOR: f32 = 0.85;

/// Minimum main-window inner width (see [`VIEWPORT_MIN_INNER`]).
pub const VIEWPORT_MIN_INNER_WIDTH: f32 = 920.0;
/// Minimum main-window inner height (see [`VIEWPORT_MIN_INNER`]).
pub const VIEWPORT_MIN_INNER_HEIGHT: f32 = 760.0;
/// Minimum inner size passed to [`egui::ViewportBuilder::with_min_inner_size`].
pub const VIEWPORT_MIN_INNER: [f32; 2] = [VIEWPORT_MIN_INNER_WIDTH, VIEWPORT_MIN_INNER_HEIGHT];
/// Extra inner size required before auto re-docking the video queue after a size-driven undock.
const AUTO_REDOCK_VIDEOS_MARGIN: f32 = 80.0;

/// Logical size of the primary window viewport (points).
pub fn main_viewport_size(ctx: &egui::Context) -> egui::Vec2 {
    ctx.input(|i| {
        i.viewport()
            .inner_rect
            .map(|r| r.size())
            .unwrap_or_else(|| ctx.screen_rect().size())
    })
}

/// True when the docked video queue leaves too little room for the main controls.
pub fn viewport_too_small_for_docked_videos(size: egui::Vec2) -> bool {
    size.y <= VIEWPORT_MIN_INNER_HEIGHT || size.x <= VIEWPORT_MIN_INNER_WIDTH
}

/// True when the main window is large enough to restore a size-driven undock (hysteresis).
pub fn viewport_large_enough_to_redock_videos(size: egui::Vec2) -> bool {
    size.y >= VIEWPORT_MIN_INNER_HEIGHT + AUTO_REDOCK_VIDEOS_MARGIN
        && size.x >= VIEWPORT_MIN_INNER_WIDTH + AUTO_REDOCK_VIDEOS_MARGIN
}

/// [`egui::TopBottomPanel`] id for the docked video queue.
pub const VIDEOS_DOCK_PANEL_ID: &str = "rustdl_videos_dock_v6";
/// [`egui::TopBottomPanel`] id for the undocked queue footer strip.
pub const UNDOCKED_FOOTER_PANEL_ID: &str = "rustdl_undocked_footer_v6";

/// Use the parent's width without fixing height or locking horizontal resize.
pub fn fill_allocated_rect(ui: &mut egui::Ui) -> egui::Vec2 {
    let w = ui.max_rect().width().max(1.0);
    let h = ui.max_rect().height().max(1.0);
    ui.set_max_width(w);
    egui::vec2(w, h)
}

/// Lock child layout to the parent's allocated [`egui::Ui::max_rect`] (floating windows / fixed panels).
pub fn pin_allocated_rect(ui: &mut egui::Ui) -> egui::Vec2 {
    let r = ui.max_rect();
    let w = finite_ui_span(r.width(), 1.0).max(1.0);
    let h = finite_ui_span(r.height(), 1.0).max(1.0);
    ui.set_max_width(w);
    if h < MAX_REASONABLE_UI_SPAN {
        ui.set_max_height(h);
    }
    egui::vec2(w, h)
}

/// Inner body size for a resizable floating window (prefer clip rect; fall back to stored size).
pub fn float_window_inner_size(
    ui: &egui::Ui,
    stored: egui::Vec2,
    min: egui::Vec2,
    max: egui::Vec2,
) -> egui::Vec2 {
    let clip = ui.clip_rect().size();
    let pick = |raw: f32, fallback: f32, lo: f32, hi: f32| {
        let use_raw = raw.is_finite() && raw >= lo && raw <= hi + 2.0;
        (if use_raw { raw } else { fallback }).clamp(lo, hi)
    };
    egui::vec2(
        pick(clip.x, stored.x, min.x, max.x),
        pick(clip.y, stored.y, min.y, max.y),
    )
}

/// Fill leftover space so resizable panels/windows keep their dragged size.
///
/// See egui docs: put `ui.allocate_space(ui.available_size())` **last** in resizable panel/window code.
pub fn consume_remaining_ui_space(ui: &mut egui::Ui) {
    let mut size = ui.available_size();
    if !size.x.is_finite() || size.x < 0.0 {
        size.x = 0.0;
    }
    if !size.y.is_finite() || size.y < 0.0 {
        size.y = remaining_ui_height(ui);
    }
    if size.y > MAX_REASONABLE_UI_SPAN {
        size.y = remaining_ui_height(ui);
    }
    let to_bottom = (ui.max_rect().bottom() - ui.cursor().min.y).max(0.0);
    if to_bottom.is_finite() && size.y > to_bottom {
        size.y = to_bottom;
    }
    if size.x > 0.5 || size.y > 0.5 {
        ui.allocate_space(size);
    }
}

/// Remember the panel's allocated height for [`patch_resizable_panel_state_height`].
pub fn note_resizable_panel_height(ctx: &egui::Context, panel_id: &str, height: f32) {
    if height.is_finite() && height >= 1.0 {
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new(panel_id).with("allocated_h"), height);
        });
    }
}

/// egui stores [`egui::containers::panel::PanelState`] height from shrink-wrapped content; patch after show.
pub fn patch_resizable_panel_state_height(ctx: &egui::Context, panel_id: &str) {
    let id = egui::Id::new(panel_id);
    let Some(height) = ctx.data(|d| d.get_temp::<f32>(id.with("allocated_h"))) else {
        return;
    };
    if !height.is_finite() || height < 1.0 {
        return;
    }
    let Some(mut state) = egui::containers::panel::PanelState::load(ctx, id) else {
        return;
    };
    if (state.rect.height() - height).abs() <= 0.5 {
        return;
    }
    state.rect = egui::Rect::from_min_size(
        state.rect.min,
        egui::vec2(state.rect.width().max(1.0), height),
    );
    ctx.data_mut(|d| d.insert_persisted(id, state));
}

/// Returns `(width, height)` when a resizable floating window should persist a new size.
pub fn persist_resizable_window_size(
    pointer_down: bool,
    size: egui::Vec2,
    min: egui::Vec2,
    max: egui::Vec2,
    current: (f32, f32),
) -> Option<(f32, f32)> {
    if pointer_down || !size.x.is_finite() || !size.y.is_finite() {
        return None;
    }
    if size.x < min.x || size.y < min.y || size.x > max.x || size.y > max.y {
        return None;
    }
    // Reject runaway growth from shrink-wrapped content (layout feedback loop).
    if current.1 > min.y + 1.0 && size.y > current.1 * 1.35 + 24.0 {
        return None;
    }
    if current.0 > min.x + 1.0 && size.x > current.0 * 1.35 + 24.0 {
        return None;
    }
    if (current.0 - size.x).abs() <= 0.5 && (current.1 - size.y).abs() <= 0.5 {
        return None;
    }
    Some((size.x, size.y))
}

pub struct PersistedFloatWindowParams {
    pub title: String,
    pub window_id: egui::Id,
    pub min_size: egui::Vec2,
    pub max_size: egui::Vec2,
    pub default_size: egui::Vec2,
    pub stored_size: (f32, f32),
    pub item_spacing_y: f32,
}

pub struct PersistedFloatWindowOutcome {
    pub open: bool,
    pub size: Option<(f32, f32)>,
}

/// Shared resizable floating-window shell (size init, body allocation, persist on resize).
pub fn show_persisted_resizable_window(
    ctx: &egui::Context,
    open: &mut bool,
    params: &PersistedFloatWindowParams,
    body: impl FnOnce(&mut egui::Ui),
) -> PersistedFloatWindowOutcome {
    let init_id = params.window_id.with("size_init");
    let needs_default = ctx.data(|d| d.get_temp::<egui::Vec2>(init_id).is_none());
    let pointer_down = ctx.input(|i| i.pointer.any_down());
    let mut window = egui::Window::new(params.title.clone())
        .id(params.window_id)
        .open(open)
        .min_width(params.min_size.x)
        .min_height(params.min_size.y)
        .max_width(params.max_size.x)
        .max_height(params.max_size.y)
        .max_size(params.max_size)
        .resizable(true);
    if needs_default {
        window = window.default_size(params.default_size);
        ctx.data_mut(|d| {
            d.insert_temp(init_id, egui::vec2(1.0, 1.0));
        });
    }
    let response = window.show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = params.item_spacing_y;
        let inner = float_window_inner_size(
            ui,
            egui::vec2(params.stored_size.0, params.stored_size.1),
            params.min_size,
            params.max_size,
        );
        allocate_top_down_rect(ui, inner, |ui| {
            pin_allocated_rect(ui);
            body(ui);
            consume_remaining_ui_space(ui);
        });
        consume_remaining_ui_space(ui);
    });
    let size = response.as_ref().and_then(|inner| {
        persist_resizable_window_size(
            pointer_down,
            inner.response.rect.size(),
            params.min_size,
            params.max_size,
            params.stored_size,
        )
    });
    if response.is_none() && *open {
        *open = false;
    }
    PersistedFloatWindowOutcome { open: *open, size }
}

pub struct QueueStatusPart {
    pub name: &'static str,
    pub count: usize,
    pub color: Color32,
    pub group: &'static str,
}

pub enum QueueStatusRowAction {
    ShowAll,
    Focus(&'static str),
}

/// Shared downloader/convert queue status count row.
pub fn draw_queue_status_row(
    ui: &mut egui::Ui,
    heading: &str,
    parts: &[QueueStatusPart],
    group_focused: bool,
) -> Option<QueueStatusRowAction> {
    if parts.is_empty() {
        return None;
    }
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(heading).color(crate::theme::TEXT_MUTED));
        if group_focused
            && ui
                .small_button(format!("{} Show all", ui_icons::SHOW_ALL))
                .clicked()
        {
            action = Some(QueueStatusRowAction::ShowAll);
        }
        for (idx, part) in parts.iter().enumerate() {
            let suffix = if idx + 1 == parts.len() { "" } else { "," };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                draw_status_dot(ui, part.color);
                let label = format!("{} {}{suffix}", part.count, part.name);
                let r = ui.add(
                    egui::Label::new(RichText::new(label).color(part.color))
                        .sense(egui::Sense::click()),
                );
                if r.clicked() {
                    action = Some(QueueStatusRowAction::Focus(part.group));
                }
                r.on_hover_text(format!("Show {} items", part.group));
            });
        }
    });
    action
}

/// Single-line queue status summary when vertical space is tight.
pub fn draw_queue_status_compact_row(ui: &mut egui::Ui, heading: &str, parts: &[QueueStatusPart]) {
    if parts.is_empty() {
        return;
    }
    let summary: String = parts
        .iter()
        .map(|p| format!("{} {}", p.count, p.name))
        .collect::<Vec<_>>()
        .join(" · ");
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(heading)
                .small()
                .color(crate::theme::TEXT_MUTED),
        );
        ui.label(RichText::new(summary).small().weak());
    });
}

/// Vertical space from the layout cursor to the bottom of the clip rect (always finite).
pub fn remaining_ui_height(ui: &egui::Ui) -> f32 {
    let y = ui.cursor().min.y;
    let to_bottom = |bottom: f32| (bottom - y).max(0.0);
    let mut h = to_bottom(ui.clip_rect().bottom());
    let max = ui.max_rect();
    if max.is_finite() {
        h = h.min(to_bottom(max.bottom()));
    }
    h
}

/// Upper bound for layout math when egui reports unbounded parents (floating windows, first frame).
const MAX_REASONABLE_UI_SPAN: f32 = 16_000.0;

pub(crate) fn finite_ui_span(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value.min(MAX_REASONABLE_UI_SPAN)
    } else {
        fallback.min(MAX_REASONABLE_UI_SPAN)
    }
}

/// Like [`remaining_ui_height`] but ignores unbounded `available_height()` from content-sized parents.
pub fn bounded_ui_height(ui: &egui::Ui, min_h: f32) -> f32 {
    let min_h = min_h.clamp(1.0, MAX_REASONABLE_UI_SPAN);
    let cap = finite_ui_span(remaining_ui_height(ui), MAX_REASONABLE_UI_SPAN);
    let avail = ui.available_height();
    let h = if avail.is_finite() && avail > 0.0 && avail < MAX_REASONABLE_UI_SPAN {
        avail.min(cap).max(min_h)
    } else {
        cap.max(min_h)
    };
    finite_ui_span(h, min_h)
}

/// Compact Videos strip in the main window when the queue is undocked (no docked log).
pub const UNDOCKED_VIDEOS_STRIP_H: f32 = 100.0;
/// Header row, height slider, and filter toolbar above docked log lines.
pub const DOCKED_LOG_CHROME_H: f32 = 100.0;
/// Spacing + separator above the activity log when docked under the video queue.
pub const DOCKED_LOG_UNDER_VIDEOS_SEPARATOR_H: f32 = 11.0;
/// "Activity log" placement row above docked log chrome (under queue or in footer frame).
pub const DOCKED_LOG_HEADING_H: f32 = 24.0;
/// Soft floor for docked log *lines* when the panel has room (slider / preference).
pub const DOCKED_LOG_LINES_PREF_MIN_H: f32 = 80.0;
/// Absolute minimum visible log lines height when the dock is cramped (still above chrome).
pub const DOCKED_LOG_LINES_ABS_MIN_H: f32 = 40.0;
/// Outer spacing + frame margins for log docked in the undocked main footer (no under-queue separator).
pub const UNDOCKED_LOG_SECTION_FRAME_H: f32 = 25.0;
/// Main header switches from one row to stacked layout at this content width.
pub const LAYOUT_WIDE_BREAKPOINT: f32 = 1040.0;
/// Footer toolbar reserve uses a taller wrapped layout below this width.
pub const LAYOUT_FOOTER_WIDE_BREAKPOINT: f32 = 900.0;
/// Footer toolbar reserve uses the tallest wrapped layout below this width.
pub const LAYOUT_FOOTER_MEDIUM_BREAKPOINT: f32 = 600.0;
/// Inner margin on each side of the activity log lines frame (see `log_panel.rs`).
pub const ACTIVITY_LOG_LINES_FRAME_INNER_MARGIN: f32 = 10.0;
/// Stroke width around the activity log lines frame.
pub const ACTIVITY_LOG_LINES_FRAME_STROKE: f32 = 1.0;
/// Vertical chrome inside the log scroll allocation (frame margins + stroke).
pub const ACTIVITY_LOG_LINES_SCROLL_CHROME_H: f32 =
    ACTIVITY_LOG_LINES_FRAME_INNER_MARGIN * 2.0 + ACTIVITY_LOG_LINES_FRAME_STROKE;

/// Layout breakpoint adjusted for UI zoom (widgets grow with `ctx.set_zoom_factor`).
pub fn layout_breakpoint(base: f32, ui_scale: f32) -> f32 {
    base / ui_scale.max(LAYOUT_UI_SCALE_FLOOR)
}

/// Minimum queue list scroll height for at least one full row.
pub fn queue_list_min_scroll_h(docked: bool, convert_mode: bool, compact_convert_row: bool) -> f32 {
    let row = if convert_mode {
        if compact_convert_row {
            QUEUE_CONVERT_LIST_ROW_COMPACT_H
        } else {
            QUEUE_CONVERT_LIST_ROW_H
        }
    } else {
        QUEUE_DL_LIST_ROW_H
    };
    let min = row + QUEUE_DOCKED_LIST_MIN_PAD;
    if docked {
        min
    } else {
        min.max(QUEUE_FLOATING_LIST_MIN_H)
    }
}

pub fn convert_list_row_height(compact: bool) -> f32 {
    if compact {
        QUEUE_CONVERT_LIST_ROW_COMPACT_H
    } else {
        QUEUE_CONVERT_LIST_ROW_H
    }
}

pub fn compact_convert_list_row(compact_cards: bool, outer_scroll_h: f32) -> bool {
    compact_cards || outer_scroll_h < QUEUE_COMPACT_CONVERT_ROW_THRESHOLD
}

pub fn should_flatten_nested_group_scroll(outer_scroll_h: f32) -> bool {
    outer_scroll_h < QUEUE_FLATTEN_NESTED_SCROLL_THRESHOLD
}

pub fn effective_panel_list_layout(
    settings_card_list: bool,
    item_count: usize,
    outer_scroll_h: f32,
    convert_mode: bool,
    auto_threshold: usize,
) -> bool {
    if settings_card_list || item_count > auto_threshold {
        return true;
    }
    if convert_mode {
        outer_scroll_h < QUEUE_CONVERT_SHORT_PANEL_LIST_THRESHOLD
    } else {
        outer_scroll_h < QUEUE_DL_SHORT_PANEL_LIST_THRESHOLD
    }
}

pub fn queue_short_panel_list_fallback(outer_scroll_h: f32, convert_mode: bool) -> bool {
    if convert_mode {
        outer_scroll_h < QUEUE_CONVERT_LIST_FALLBACK_THRESHOLD
    } else {
        outer_scroll_h < QUEUE_DL_LIST_FALLBACK_THRESHOLD
    }
}

pub fn queue_status_compact(list_h: f32, convert_mode: bool) -> bool {
    if convert_mode {
        list_h < QUEUE_STATUS_COMPACT_CONVERT_THRESHOLD
    } else {
        list_h < QUEUE_STATUS_COMPACT_DL_THRESHOLD
    }
}

pub fn max_bottom_panel_height(viewport_h: f32) -> f32 {
    if !viewport_h.is_finite() || viewport_h < 1.0 {
        return BOTTOM_PANEL_MAX_H;
    }
    (viewport_h * VIDEOS_DOCKED_HEIGHT_RATIO).clamp(BOTTOM_PANEL_MIN_H, BOTTOM_PANEL_MAX_H)
}

pub fn main_body_scroll_min(viewport_h: f32) -> f32 {
    if !viewport_h.is_finite() || viewport_h < 1.0 {
        return 120.0;
    }
    (viewport_h * 0.25).clamp(120.0, 280.0)
}

pub fn url_input_height(viewport_h: f32) -> f32 {
    if !viewport_h.is_finite() || viewport_h < 1.0 {
        return 88.0;
    }
    (viewport_h * 0.12).clamp(72.0, 140.0)
}

/// Cap log *lines* height so it does not dominate a small bottom panel.
///
/// `available_for_log` is the lines-only budget (chrome / heading / separator already excluded).
pub fn scaled_log_dock_height(user_pref: f32, available_for_log: f32) -> f32 {
    let max_lines = available_for_log.max(0.0).min(480.0);
    if max_lines < 1.0 {
        return 0.0;
    }
    let soft_min = if max_lines >= DOCKED_LOG_LINES_PREF_MIN_H {
        DOCKED_LOG_LINES_PREF_MIN_H
    } else {
        DOCKED_LOG_LINES_ABS_MIN_H.min(max_lines)
    };
    user_pref
        .min(max_lines * 0.55)
        .clamp(soft_min, 480.0)
        .min(max_lines)
}

/// Chrome above docked-under-videos log lines (separator + heading + slider/toolbar).
pub fn docked_log_under_videos_chrome_h() -> f32 {
    DOCKED_LOG_UNDER_VIDEOS_SEPARATOR_H + DOCKED_LOG_HEADING_H + DOCKED_LOG_CHROME_H
}

/// User log line height scaled to remaining docked queue body space.
///
/// Reserves `min_list_h` for the queue list first so a short panel does not collapse the cards.
pub fn queue_log_lines_for_dock_layout(
    user_pref: f32,
    body_bottom: f32,
    content_top: f32,
    footer_h: f32,
    min_list_h: f32,
) -> f32 {
    let avail_body = finite_ui_span(body_bottom - content_top, 0.0);
    let log_budget =
        (avail_body - footer_h - min_list_h.max(0.0) - docked_log_under_videos_chrome_h()).max(0.0);
    scaled_log_dock_height(user_pref, log_budget)
}

/// Docked-under-videos log block that fits after footer + minimum list height.
///
/// Returns `(lines_h, block_h)`. `block_h` is 0 when the panel is too short for a usable log.
pub fn queue_docked_under_videos_log_fit(
    dock_log: bool,
    user_pref: f32,
    body_bottom: f32,
    content_top: f32,
    footer_h: f32,
    min_list_h: f32,
) -> (f32, f32) {
    if !dock_log {
        return (0.0, 0.0);
    }
    let avail = finite_ui_span(body_bottom - content_top, 0.0);
    let max_block = (avail - footer_h - min_list_h.max(0.0)).max(0.0);
    let chrome = docked_log_under_videos_chrome_h();
    // Need chrome plus a sliver of lines, otherwise skip the log reservation this frame.
    if max_block < chrome + DOCKED_LOG_LINES_ABS_MIN_H * 0.5 {
        return (0.0, 0.0);
    }
    let lines =
        queue_log_lines_for_dock_layout(user_pref, body_bottom, content_top, footer_h, min_list_h);
    let preferred = queue_log_block_height(true, lines, true);
    let block = preferred.min(max_block);
    let fitted_lines = (block - chrome).clamp(0.0, 480.0);
    (fitted_lines, block)
}

/// Card width for wrapped grid layout (`columns` in 1..=4).
pub fn queue_card_grid_width(avail: f32, compact: bool) -> f32 {
    let (card_min, card_max) = if compact {
        (260.0, 320.0)
    } else {
        (280.0, 360.0)
    };
    let gutter = 8.0;
    let columns = ((avail - gutter) / (card_min + gutter))
        .floor()
        .clamp(1.0, 4.0) as u32;
    if columns <= 1 {
        return (avail * 0.45).clamp(card_min, card_max);
    }
    let gutters = gutter * (columns as f32 - 1.0);
    ((avail - gutters) / columns as f32).clamp(card_min, card_max)
}

/// Estimated vertical space for the video queue footer toolbar (dock/hide + batch actions).
pub fn queue_footer_toolbar_reserve(
    content_width: f32,
    convert_mode: bool,
    docked: bool,
    ui_scale: f32,
) -> f32 {
    let wide = layout_breakpoint(LAYOUT_FOOTER_WIDE_BREAKPOINT, ui_scale);
    let medium = layout_breakpoint(LAYOUT_FOOTER_MEDIUM_BREAKPOINT, ui_scale);
    let base = if content_width >= wide {
        72.0
    } else if content_width >= medium {
        96.0
    } else {
        130.0
    };
    let convert_extra = if convert_mode { 40.0 } else { 0.0 };
    let docked_extra = if docked && !convert_mode && content_width < wide {
        24.0
    } else {
        0.0
    };
    base + convert_extra + docked_extra
}

/// Footer toolbar reserve: prefer last frame's measured height when available.
///
/// Falls back to the width/mode estimate only before the first measure so an inflated
/// estimate does not leave empty space under the footer buttons.
pub fn queue_footer_reserve(
    content_width: f32,
    convert_mode: bool,
    docked: bool,
    measured_h: Option<f32>,
    ui_scale: f32,
) -> f32 {
    let est = queue_footer_toolbar_reserve(content_width, convert_mode, docked, ui_scale) + 2.0;
    measured_h
        .filter(|m| m.is_finite() && *m > 0.0)
        .unwrap_or(est)
}

/// Undocked videos strip reserve (compact strip in main footer when queue is floating).
pub fn queue_undocked_strip_reserve(measured_h: Option<f32>) -> f32 {
    match measured_h {
        Some(m) if m.is_finite() && m > 0.0 => m.max(UNDOCKED_VIDEOS_STRIP_H),
        _ => UNDOCKED_VIDEOS_STRIP_H,
    }
}

/// Vertical space for a docked log block (scroll lines + chrome + heading; optional placement extras).
pub fn queue_log_block_height(dock_log: bool, log_dock_height: f32, under_videos: bool) -> f32 {
    if !dock_log {
        return 0.0;
    }
    let lines = log_dock_height.clamp(0.0, 480.0);
    if lines < 1.0 && under_videos {
        // Caller asked for an empty under-videos log; do not reserve chrome alone.
        return 0.0;
    }
    let mut h = lines + DOCKED_LOG_CHROME_H + DOCKED_LOG_HEADING_H;
    if under_videos {
        h += DOCKED_LOG_UNDER_VIDEOS_SEPARATOR_H;
    } else {
        h += UNDOCKED_LOG_SECTION_FRAME_H;
    }
    h
}

/// List and bottom-stack heights from a fixed content top and pinned footer/log blocks.
pub fn queue_panel_layout_heights(
    content_top: f32,
    body_bottom: f32,
    footer_h: f32,
    log_block_h: f32,
) -> (f32, f32) {
    let list_h = queue_list_height_from_layout(content_top, body_bottom, footer_h, log_block_h);
    let stack_h = footer_h + log_block_h;
    (list_h, stack_h)
}

/// Clamp persisted dock panel heights when the main viewport shrinks (skip while dragging).
pub fn clamp_dock_heights_for_viewport(
    viewport_height: f32,
    videos_dock_height: &mut f32,
    undocked_footer_height: &mut f32,
    log_dock_height: &mut f32,
    pointer_down: bool,
) -> bool {
    if pointer_down || !viewport_height.is_finite() || viewport_height < 1.0 {
        return false;
    }
    let max_panel = max_bottom_panel_height(viewport_height);
    let mut changed = false;
    if *videos_dock_height > max_panel {
        *videos_dock_height = max_panel;
        changed = true;
    }
    if *undocked_footer_height > max_panel {
        *undocked_footer_height = max_panel;
        changed = true;
    }
    let log_budget = (max_panel * 0.55).max(80.0);
    let scaled = scaled_log_dock_height(*log_dock_height, log_budget);
    if changed {
        if (scaled - *log_dock_height).abs() > 0.5 {
            *log_dock_height = scaled;
        }
    } else if *log_dock_height > scaled + 0.5 {
        *log_dock_height = scaled;
        changed = true;
    }
    changed
}

/// Suggested docked queue panel height for a main-window viewport (factory default scaling).
pub fn default_videos_dock_height_for_viewport(viewport_height: f32) -> f32 {
    if !viewport_height.is_finite() || viewport_height < 1.0 {
        return 360.0;
    }
    (viewport_height * VIDEOS_DOCKED_HEIGHT_RATIO).clamp(BOTTOM_PANEL_MIN_H, BOTTOM_PANEL_MAX_H)
}

/// Suggested dock height for a layout preset ratio.
pub fn dock_height_for_viewport_ratio(viewport_height: f32, ratio: f32) -> f32 {
    if !viewport_height.is_finite() || viewport_height < 1.0 {
        return 360.0;
    }
    (viewport_height * ratio).clamp(BOTTOM_PANEL_MIN_H, BOTTOM_PANEL_MAX_H)
}

/// Apply a named layout preset (`compact`, `review`, `minimal`) to settings.
pub fn apply_layout_preset(
    settings: &mut crate::config::AppSettings,
    preset: &str,
    viewport_height: Option<f32>,
) {
    match preset {
        "compact" => {
            settings.card_list_layout = true;
            settings.compact_cards = true;
            settings.hide_card_subtitle = true;
            settings.show_thumbnails = true;
            settings.log_dock_height = settings.log_dock_height.clamp(80.0, 120.0);
        }
        "review" => {
            settings.card_list_layout = false;
            settings.compact_cards = false;
            settings.hide_card_subtitle = false;
            settings.show_thumbnails = true;
            settings.logs_open = true;
            settings.logs_docked = true;
            if let Some(vh) = viewport_height {
                settings.videos_dock_height =
                    dock_height_for_viewport_ratio(vh, REVIEW_DOCK_HEIGHT_RATIO);
                let log_budget = (settings.videos_dock_height * 0.45).max(80.0);
                settings.log_dock_height = scaled_log_dock_height(200.0, log_budget).max(120.0);
            } else {
                settings.log_dock_height = 200.0;
            }
        }
        "minimal" => {
            settings.card_list_layout = true;
            settings.compact_cards = true;
            settings.hide_card_subtitle = true;
            settings.show_thumbnails = false;
            if let Some(vh) = viewport_height {
                settings.videos_dock_height =
                    dock_height_for_viewport_ratio(vh, MINIMAL_DOCK_HEIGHT_RATIO);
            }
        }
        _ => {}
    }
}

/// List scroll height between fixed `content_top` and a bottom stack (footer + optional log).
pub fn queue_list_height_from_layout(
    content_top: f32,
    body_bottom: f32,
    footer_h: f32,
    log_block_h: f32,
) -> f32 {
    if !content_top.is_finite() || !body_bottom.is_finite() {
        return 0.0;
    }
    let stack_h = footer_h + log_block_h;
    let stack_top = body_bottom - stack_h;
    finite_ui_span(stack_top - content_top, 0.0).clamp(0.0, QUEUE_LIST_LAYOUT_MAX_H)
}

/// Max activity-log *lines* height from remaining space that still includes chrome (slider+toolbar).
///
/// Returns 0 when `remaining_h` cannot cover chrome — callers must not floor this back to 80px
/// or a short Videos dock will collapse the queue list.
pub fn docked_log_lines_max_h(remaining_h: f32) -> f32 {
    (remaining_h - DOCKED_LOG_CHROME_H).clamp(0.0, 480.0)
}

/// Allocate a fixed-height region anchored to `body_bottom` (bottom-up layout).
pub fn allocate_bottom_up_rect<R>(
    ui: &mut egui::Ui,
    body_bottom: f32,
    width: f32,
    height: f32,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let height = finite_ui_span(height, 1.0).max(1.0);
    let max_w = clip_bounded_width(ui);
    let mut width = finite_ui_span(width, 1.0).max(1.0).min(max_w);
    let left = ui.max_rect().min.x;
    let right = (left + width).min(ui.clip_rect().right());
    width = (right - left).max(1.0);
    let top = (body_bottom - height).max(ui.clip_rect().min.y);
    let rect =
        egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + width, body_bottom));
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_max_width(width);
            add(ui)
        },
    )
    .inner
}

/// Full-width Downloader / Video Converter tabs with a fixed 50/50 split.
pub fn draw_mode_nav_bar(
    ui: &mut egui::Ui,
    theme: &str,
    dl_active: bool,
    av1_active: bool,
    colors: ModePanelColors<'_>,
    ui_scale: f32,
) -> (bool, bool) {
    let dl_accent = mode_accent_for(false, &colors);
    let convert_accent = mode_accent_for(true, &colors);
    let mut dl_clicked = false;
    let mut av1_clicked = false;
    let row_w = clip_bounded_width(ui);
    let compact = row_w < layout_breakpoint(MODE_NAV_COMPACT_BREAKPOINT, ui_scale);
    let muted = text_muted(theme);
    let group_border = panel_border(theme);
    let group_fill = if theme == "light" {
        Color32::from_rgba_unmultiplied(0, 0, 0, 10)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 8)
    };
    let (dl_name, convert_name, dl_tip, convert_tip) = if compact {
        (
            "Downloader",
            "Converter",
            "Downloader mode",
            "Video Converter mode",
        )
    } else {
        (
            "Downloader",
            "Video Converter",
            "Downloader mode",
            "Video Converter mode",
        )
    };
    ui.allocate_ui_with_layout(
        egui::vec2(row_w, 38.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(row_w);
            let btn_w = row_w * 0.5;
            egui::Frame::none()
                .fill(group_fill)
                .stroke(Stroke::new(1.0_f32, group_border))
                .rounding(egui::Rounding::same(6.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let dl_text = if dl_active { Color32::WHITE } else { muted };
                        let dl_label =
                            RichText::new(format!("{} {dl_name}", crate::ui_icons::NAV_DOWNLOADER))
                                .color(dl_text)
                                .size(14.0)
                                .strong();
                        let dl = ui.add_sized(
                            [btn_w, 34.0],
                            egui::Button::new(dl_label)
                                .fill(if dl_active {
                                    dl_accent
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .stroke(Stroke::NONE)
                                .rounding(egui::Rounding::same(6.0)),
                        );
                        if dl.on_hover_text(dl_tip).clicked() {
                            dl_clicked = true;
                        }
                        let av1_text = if av1_active { Color32::WHITE } else { muted };
                        let av1_label =
                            RichText::new(format!("{} {convert_name}", crate::ui_icons::NAV_AV1))
                                .color(av1_text)
                                .size(14.0)
                                .strong();
                        let av1 = ui.add_sized(
                            [btn_w, 34.0],
                            egui::Button::new(av1_label)
                                .fill(if av1_active {
                                    convert_accent
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .stroke(Stroke::NONE)
                                .rounding(egui::Rounding::same(6.0)),
                        );
                        if av1.on_hover_text(convert_tip).clicked() {
                            av1_clicked = true;
                        }
                    });
                });
        },
    );
    (dl_clicked, av1_clicked)
}

/// Width of the current layout region (respects [`content_panel_frame`] margins).
pub fn content_width(ui: &egui::Ui) -> f32 {
    let max_w = ui.max_rect().width();
    if max_w.is_finite() && max_w > 0.0 {
        // Shrink-wrapped nested rows inside a scroll area report a tiny `max_rect`; widen then only.
        if ui.stack().contained_in(egui::UiKind::ScrollArea) {
            let clip_w = ui.clip_rect().width();
            if clip_w.is_finite() && clip_w > max_w + 4.0 && max_w < clip_w * 0.75 {
                return clip_w;
            }
        }
        return max_w;
    }
    let avail = ui.available_width();
    if avail.is_finite() && avail > 0.0 && avail < 50_000.0 {
        return avail;
    }
    ui.clip_rect().width().max(0.0)
}

/// [`content_width`] capped by the current clip rect and remaining horizontal space.
pub fn clip_bounded_width(ui: &egui::Ui) -> f32 {
    let mut w = content_width(ui);
    let clip = ui.clip_rect().width();
    if clip.is_finite() && clip > 0.0 {
        w = w.min(clip);
    }
    let avail = ui.available_width();
    if avail.is_finite() && avail > 0.0 && avail < w {
        w = avail;
    }
    w.max(1.0)
}

/// Cap layout width without forcing horizontal expansion (preserves panel margins).
pub fn constrain_content_width(ui: &mut egui::Ui, max_content_width: f32) -> f32 {
    let mut w = clip_bounded_width(ui);
    if max_content_width > 0.0 {
        w = w.min(max_content_width);
    }
    ui.set_max_width(w);
    w
}

/// Like [`constrain_content_width`], but also sets explicit width (mode/queue panels).
pub fn constrain_panel_width(ui: &mut egui::Ui, max_content_width: f32) -> f32 {
    let w = constrain_content_width(ui, max_content_width);
    ui.set_width(w);
    w
}

/// Allocate a top-down child region with an explicit size (avoids shrink-wrapped `max_rect`).
fn layout_origin(ui: &egui::Ui) -> egui::Pos2 {
    let cursor = ui.cursor().min;
    if cursor.is_finite() {
        return cursor;
    }
    let clip = ui.clip_rect().min;
    if clip.is_finite() {
        return clip;
    }
    ui.max_rect().min
}

pub fn allocate_top_down_rect<R>(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let size = egui::vec2(
        finite_ui_span(size.x, 1.0).max(1.0),
        finite_ui_span(size.y, 1.0).max(1.0),
    );
    let origin = layout_origin(ui);
    let rect = egui::Rect::from_min_size(origin, size);
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        add,
    )
    .inner
}

/// Vertical space from the cursor to a fixed bottom edge.
pub fn height_to_bottom(ui: &egui::Ui, bottom_y: f32) -> f32 {
    if !bottom_y.is_finite() {
        return 0.0;
    }
    finite_ui_span(bottom_y - ui.cursor().min.y, 0.0)
}

/// Lay out children across the full width of the parent (egui vertical layouts default to shrink-wrap).
pub fn with_full_width<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let width = clip_bounded_width(ui);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_max_width(width);
            add_contents(ui)
        },
    )
    .inner
}

/// Full panel width; also fills a fixed-height parent (docked/floating queue shells).
pub fn with_full_panel<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let width = clip_bounded_width(ui);
    let parent_h = finite_ui_span(ui.max_rect().height(), 0.0);
    let fixed_h = parent_h > 1.0;
    let height = if fixed_h { parent_h } else { 0.0 };
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_max_width(width);
            if fixed_h {
                ui.set_min_height(parent_h);
            }
            add_contents(ui)
        },
    )
    .inner
}

fn alert_box<R>(
    ui: &mut egui::Ui,
    bg: Color32,
    border: Color32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    with_full_width(ui, |ui| {
        let width = content_width(ui);
        egui::Frame::none()
            .fill(bg)
            .stroke(egui::Stroke::new(1.0_f32, border))
            .rounding(egui::Rounding::same(6.0))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.set_max_width(width);
                add_contents(ui)
            })
            .inner
    })
}

/// Bordered warning strip matching Bootstrap `alert alert-warning` (full container width).
pub fn alert_warning<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    alert_box(ui, ALERT_WARNING_BG, ALERT_WARNING_BORDER, add_contents)
}

/// Bordered danger strip matching Bootstrap `alert alert-danger` (full container width).
pub fn alert_danger<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    alert_box(ui, ALERT_DANGER_BG, ALERT_DANGER_BORDER, add_contents)
}

/// Center a compact horizontal row (e.g. dialog Cancel / OK buttons) in the parent width.
pub fn centered_button_row<R>(
    ui: &mut egui::Ui,
    id_salt: impl Hash,
    mut add_contents: impl FnMut(&mut egui::Ui) -> R,
) -> R {
    let avail_w = ui.available_width();
    let row_width = {
        let mut sizing_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("centered_button_row_sizing", id_salt))
                .sizing_pass()
                .invisible(),
        );
        sizing_ui
            .horizontal(&mut add_contents)
            .response
            .rect
            .width()
    };
    let pad = ((avail_w - row_width) * 0.5).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(pad);
        add_contents(ui)
    })
    .inner
}

/// Dim the viewport behind a modal. Call before the modal window so the dialog stays on top.
/// Returns `true` if the user clicked the backdrop.
pub fn modal_backdrop(ctx: &egui::Context, id: egui::Id) -> bool {
    let screen = ctx.screen_rect();
    let response = egui::Area::new(id)
        .order(egui::Order::Middle)
        .fixed_pos(screen.left_top())
        .interactable(true)
        .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(120));
            response
        });
    response.inner.clicked()
}

pub fn secondary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    let bg = Color32::from_rgb(30, 136, 229);
    colored_button(
        ui,
        label,
        enabled,
        Color32::from_rgb(227, 242, 253),
        bg,
        egui::Rounding::same(6.0),
        standalone_button_stroke(bg),
    )
}

fn grouped_button_label(label: &str, compact: bool) -> RichText {
    let text = RichText::new(label);
    if compact {
        text.small()
    } else {
        text
    }
}

fn grouped_secondary_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    compact: bool,
) -> Response {
    let bg = Color32::from_rgb(30, 136, 229);
    colored_button(
        ui,
        grouped_button_label(label, compact),
        enabled,
        Color32::from_rgb(227, 242, 253),
        bg,
        egui::Rounding::ZERO,
        grouped_button_stroke(),
    )
}

fn grouped_success_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    compact: bool,
) -> Response {
    let bg = Color32::from_rgb(46, 125, 50);
    colored_button(
        ui,
        grouped_button_label(label, compact),
        enabled,
        Color32::from_rgb(232, 245, 233),
        bg,
        egui::Rounding::ZERO,
        grouped_button_stroke(),
    )
}

fn grouped_danger_button(ui: &mut egui::Ui, label: &str, enabled: bool, compact: bool) -> Response {
    let bg = Color32::from_rgb(183, 28, 28);
    colored_button(
        ui,
        grouped_button_label(label, compact),
        enabled,
        Color32::from_rgb(255, 235, 238),
        bg,
        egui::Rounding::ZERO,
        grouped_button_stroke(),
    )
}

fn grouped_warning_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    compact: bool,
) -> Response {
    let bg = Color32::from_rgb(245, 124, 0);
    colored_button(
        ui,
        grouped_button_label(label, compact),
        enabled,
        Color32::from_rgb(255, 255, 255),
        bg,
        egui::Rounding::ZERO,
        grouped_button_stroke(),
    )
}

pub(crate) const COPY_FEEDBACK_SECS: f64 = 2.0;

pub(crate) fn copy_feedback_active(ui: &egui::Ui, feedback_id: Id) -> bool {
    let now = ui.input(|i| i.time);
    ui.ctx().data(|d| {
        d.get_temp::<f64>(feedback_id)
            .is_some_and(|until| now < until)
    })
}

pub(crate) fn set_copy_feedback(ui: &mut egui::Ui, feedback_id: Id) {
    let now = ui.input(|i| i.time);
    ui.ctx().data_mut(|d| {
        d.insert_temp(feedback_id, now + COPY_FEEDBACK_SECS);
    });
    ui.ctx().request_repaint();
}

fn draw_url_menu_popup_items(
    ui: &mut egui::Ui,
    url: &str,
    feedback_id: Id,
    open_clicked: &mut bool,
) {
    if ui
        .button(format!("{} Copy URL", ui_icons::COPY_CLIPBOARD))
        .on_hover_text(url)
        .clicked()
    {
        ui.ctx().copy_text(url.to_owned());
        set_copy_feedback(ui, feedback_id);
    }
    if ui
        .button(format!("{} Open URL", ui_icons::UPDATE_OPEN))
        .on_hover_text("Open in your default browser")
        .clicked()
    {
        *open_clicked = true;
    }
}

pub(crate) fn show_menu_popup<R>(
    ui: &mut egui::Ui,
    popup_id: Id,
    button: &Response,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) {
    popup_above_or_below_widget(
        ui,
        popup_id,
        button,
        AboveOrBelow::Above,
        PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
            Frame::menu(ui.style())
                .show(ui, |ui| {
                    ui.set_min_width(ui.ctx().style().spacing.menu_width);
                    add_contents(ui)
                })
                .inner
        },
    );
}

fn grouped_popup_menu<R>(
    ui: &mut egui::Ui,
    popup_id: Id,
    label: &str,
    enabled: bool,
    compact: bool,
    danger: bool,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Response {
    let button = if danger {
        grouped_danger_button(ui, label, enabled, compact)
    } else {
        grouped_secondary_button(ui, label, enabled, compact)
    };
    if !enabled {
        return button;
    }
    if button.clicked() {
        ui.memory_mut(|mem| mem.toggle_popup(popup_id));
    }
    if ui.memory(|mem| mem.is_popup_open(popup_id)) {
        show_menu_popup(ui, popup_id, &button, add_contents);
    }
    button
}

/// Popup menu for a plain button (e.g. compact list rows).
pub(crate) fn popup_menu_above<R>(
    ui: &mut egui::Ui,
    popup_id: Id,
    button_label: impl Into<egui::WidgetText>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Response {
    let button = ui.button(button_label);
    if button.clicked() {
        ui.memory_mut(|mem| mem.toggle_popup(popup_id));
    }
    if ui.memory(|mem| mem.is_popup_open(popup_id)) {
        show_menu_popup(ui, popup_id, &button, add_contents);
    }
    button
}

/// URL copy/open menu for compact list rows (matches [`ButtonGroup::url_menu`] feedback).
pub(crate) fn url_menu_above(
    ui: &mut egui::Ui,
    popup_id: Id,
    url: &str,
    open_clicked: &mut bool,
) -> Response {
    let feedback_id = ui.id().with("url_copy_feedback");
    let copied = copy_feedback_active(ui, feedback_id);
    if copied {
        ui.ctx().request_repaint();
    }
    let label = if copied {
        format!("{} Copied!", ui_icons::STATUS_DONE)
    } else {
        format!("{} URL...", ui_icons::PAGE_URL)
    };
    let url_owned = url.to_owned();
    if copied {
        ui.colored_label(Color32::from_rgb(46, 125, 50), label)
            .on_hover_text(url)
    } else {
        popup_menu_above(ui, popup_id, label, |ui| {
            draw_url_menu_popup_items(ui, &url_owned, feedback_id, open_clicked);
        })
        .on_hover_text(url)
    }
}

/// Bootstrap-style fused buttons (shared edges, no dividers).
pub struct ButtonGroup<'a> {
    ui: &'a mut egui::Ui,
    compact: bool,
}

impl<'a> ButtonGroup<'a> {
    pub fn ui(&mut self) -> &mut egui::Ui {
        self.ui
    }

    pub fn add<F>(&mut self, add: F) -> Response
    where
        F: FnOnce(&mut egui::Ui) -> Response,
    {
        add(self.ui)
    }

    pub fn secondary(&mut self, label: &str, enabled: bool) -> Response {
        let compact = self.compact;
        self.add(|ui| grouped_secondary_button(ui, label, enabled, compact))
    }

    pub fn success(&mut self, label: &str, enabled: bool) -> Response {
        let compact = self.compact;
        self.add(|ui| grouped_success_button(ui, label, enabled, compact))
    }

    pub fn danger(&mut self, label: &str, enabled: bool) -> Response {
        let compact = self.compact;
        self.add(|ui| grouped_danger_button(ui, label, enabled, compact))
    }

    pub fn warning(&mut self, label: &str, enabled: bool) -> Response {
        let compact = self.compact;
        self.add(|ui| grouped_warning_button(ui, label, enabled, compact))
    }

    /// Copy or open the downloader page URL for this row.
    pub fn url_menu(&mut self, url: &str, open_clicked: &mut bool) -> Response {
        let compact = self.compact;
        self.add(|ui| {
            let feedback_id = ui.id().with("url_copy_feedback");
            let copied = copy_feedback_active(ui, feedback_id);
            if copied {
                ui.ctx().request_repaint();
            }
            let label = if copied {
                format!("{} Copied!", ui_icons::STATUS_DONE)
            } else {
                format!("{} URL...", ui_icons::PAGE_URL)
            };
            let popup_id = ui.make_persistent_id("url_menu");
            let url_owned = url.to_owned();
            if copied {
                grouped_success_button(ui, &label, true, compact).on_hover_text(url)
            } else {
                grouped_popup_menu(ui, popup_id, &label, true, compact, false, |ui| {
                    draw_url_menu_popup_items(ui, &url_owned, feedback_id, open_clicked);
                })
                .on_hover_text(url)
            }
        })
    }

    /// Open saved file and/or its containing folder (done downloads).
    pub fn open_menu(
        &mut self,
        can_open_file: bool,
        can_open_folder: bool,
        open_file_clicked: &mut bool,
        folder_clicked: &mut bool,
    ) -> Response {
        let compact = self.compact;
        let label = format!("{} Open...", crate::ui_icons::OPEN_FILE);
        self.add(|ui| {
            if !can_open_file && !can_open_folder {
                return grouped_success_button(ui, &label, false, compact)
                    .on_disabled_hover_text("No saved file or output folder for this row");
            }
            let popup_id = ui.make_persistent_id("open_menu");
            let button = if can_open_file {
                grouped_success_button(ui, &label, true, compact)
            } else {
                grouped_secondary_button(ui, &label, true, compact)
            };
            if button.clicked() {
                ui.memory_mut(|mem| mem.toggle_popup(popup_id));
            }
            if ui.memory(|mem| mem.is_popup_open(popup_id)) {
                show_menu_popup(ui, popup_id, &button, |ui| {
                    if can_open_file
                        && ui
                            .button(format!("{} Open", crate::ui_icons::OPEN_FILE))
                            .on_hover_text("Open with the default app for this file type")
                            .clicked()
                    {
                        *open_file_clicked = true;
                    }
                    if can_open_folder {
                        let folder_hover = if can_open_file {
                            "Show the file in Explorer / file manager"
                        } else {
                            "Open the output folder for this download"
                        };
                        if ui
                            .button(format!("{} Folder", crate::ui_icons::REVEAL_FOLDER))
                            .on_hover_text(folder_hover)
                            .clicked()
                        {
                            *folder_clicked = true;
                        }
                    }
                });
            }
            let trigger_hover = if can_open_file {
                "Open file or show in folder"
            } else {
                "Open the output folder for this download"
            };
            button.on_hover_text(trigger_hover)
        })
    }

    /// Verify saved file streams, re-download, or watch for better quality (done rows).
    #[allow(clippy::too_many_arguments)]
    pub fn verify_menu(
        &mut self,
        show_verify_file: bool,
        can_verify_file: bool,
        show_redownload: bool,
        can_redownload: bool,
        show_watch: bool,
        can_watch: bool,
        verify_clicked: &mut bool,
        redownload_clicked: &mut bool,
        watch_clicked: &mut bool,
    ) -> Response {
        let compact = self.compact;
        let label = format!("{} Verify...", crate::ui_icons::CHECK_STREAMS);
        self.add(|ui| {
            let popup_id = ui.make_persistent_id("verify_menu");
            let button = grouped_secondary_button(ui, &label, true, compact);
            if button.clicked() {
                ui.memory_mut(|mem| mem.toggle_popup(popup_id));
            }
            if ui.memory(|mem| mem.is_popup_open(popup_id)) {
                show_menu_popup(ui, popup_id, &button, |ui| {
                    if show_verify_file
                        && ui
                            .add_enabled(
                                can_verify_file,
                                egui::Button::new(format!(
                                    "{} Verify file",
                                    crate::ui_icons::CHECK_STREAMS
                                )),
                            )
                            .on_hover_text(
                                "Run ffprobe on the saved file; refresh stream check and media badges.",
                            )
                            .on_disabled_hover_text(
                                "Configure ffprobe in Settings → Executables.",
                            )
                            .clicked()
                    {
                        *verify_clicked = true;
                    }
                    if show_redownload
                        && ui
                            .add_enabled(
                                can_redownload,
                                egui::Button::new(format!(
                                    "{} Re-download",
                                    crate::ui_icons::REDOWNLOAD
                                )),
                            )
                            .on_hover_text(
                                "Deletes the matched file in the output folder (if found), then downloads this URL again.",
                            )
                            .on_disabled_hover_text(
                                "Needs a video URL on this row, a valid output folder, and yt-dlp.",
                            )
                            .clicked()
                    {
                        *redownload_clicked = true;
                    }
                    if show_watch
                        && ui
                            .add_enabled(
                                can_watch,
                                egui::Button::new(format!(
                                    "{} Watch for better quality",
                                    crate::ui_icons::WATCHLIST
                                )),
                            )
                            .on_hover_text(
                                "Add this URL to the quality watchlist; rustdl will re-probe on a schedule and notify you when max resolution increases.",
                            )
                            .on_disabled_hover_text("Needs a video URL and yt-dlp.")
                            .clicked()
                    {
                        *watch_clicked = true;
                    }
                });
            }
            button.on_hover_text("Verify, re-download, or watch for better quality")
        })
    }

    /// Fused "Remove..." menu: queue row removal and optional on-disk file delete.
    pub fn remove_menu(
        &mut self,
        enabled: bool,
        removable: bool,
        show_delete_file: bool,
        remove_clicked: &mut bool,
        delete_clicked: &mut bool,
    ) -> Response {
        let compact = self.compact;
        let label = format!("{} Remove...", crate::ui_icons::REMOVE);
        self.add(|ui| {
            if !enabled {
                return grouped_danger_button(ui, &label, false, compact)
                    .on_disabled_hover_text("Nothing to remove for this row");
            }
            let popup_id = ui.make_persistent_id("remove_menu");
            grouped_popup_menu(ui, popup_id, &label, true, compact, true, |ui| {
                if ui
                    .add_enabled(
                        removable,
                        egui::Button::new(format!(
                            "{} Remove from queue",
                            crate::ui_icons::REMOVE_FROM_QUEUE
                        )),
                    )
                    .on_hover_text(
                        "Remove this row from the list (does not delete the file on disk).",
                    )
                    .clicked()
                {
                    *remove_clicked = true;
                }
                if show_delete_file
                    && ui
                        .button(format!("{} Delete file", crate::ui_icons::DELETE_FILE))
                        .on_hover_text(
                            "Delete only this file; the queue row stays until you remove it.",
                        )
                        .clicked()
                {
                    *delete_clicked = true;
                }
            })
            .on_hover_text("Remove from queue or delete the saved file")
        })
    }

    /// Fused "Import/Export" menu for settings, queue I/O, URL import, etc.
    pub fn import_export_menu<F>(&mut self, enabled: bool, add_items: F) -> Response
    where
        F: FnOnce(&mut egui::Ui),
    {
        let compact = self.compact;
        let label = format!("{} Import/Export", crate::ui_icons::IMPORT_FILE);
        self.add(|ui| {
            if !enabled {
                return grouped_secondary_button(ui, &label, false, compact);
            }
            ui.menu_button(grouped_button_label(&label, compact), add_items)
                .response
        })
    }

    /// Cancel every active/queued download: return to Ready or remove from queue.
    pub fn cancel_all_menu(
        &mut self,
        enabled: bool,
        ready_clicked: &mut bool,
        remove_clicked: &mut bool,
    ) -> Response {
        let compact = self.compact;
        let label = format!("{} Cancel all...", ui_icons::CANCEL_TO_READY);
        self.add(|ui| {
            if !enabled {
                return grouped_warning_button(ui, &label, false, compact)
                    .on_disabled_hover_text("No active or queued downloads to cancel");
            }
            ui.menu_button(grouped_button_label(&label, compact), |ui| {
                if ui
                    .button(format!("{} Cancel all -> Ready", ui_icons::CANCEL_TO_READY))
                    .on_hover_text("Stop active downloads and return items to Ready")
                    .clicked()
                {
                    *ready_clicked = true;
                }
                if ui
                    .button(format!(
                        "{} Cancel all -> Remove",
                        ui_icons::CANCEL_TO_REMOVE
                    ))
                    .on_hover_text("Stop active downloads and remove items from the queue")
                    .clicked()
                {
                    *remove_clicked = true;
                }
            })
            .response
            .on_hover_text("Cancel all active or queued downloads")
        })
    }
}

pub fn button_group<R>(
    ui: &mut egui::Ui,
    id_salt: impl Hash,
    add: impl FnOnce(&mut ButtonGroup<'_>) -> R,
) -> R {
    button_group_sized(ui, id_salt, false, add)
}

/// Fused button row with tighter padding and smaller labels (queue cards).
pub fn compact_button_group<R>(
    ui: &mut egui::Ui,
    id_salt: impl Hash,
    add: impl FnOnce(&mut ButtonGroup<'_>) -> R,
) -> R {
    button_group_sized(ui, id_salt, true, add)
}

fn button_group_sized<R>(
    ui: &mut egui::Ui,
    id_salt: impl Hash,
    compact: bool,
    add: impl FnOnce(&mut ButtonGroup<'_>) -> R,
) -> R {
    ui.push_id(id_salt, |ui| {
        let pad = ui.style().spacing.button_padding;
        if compact {
            ui.style_mut().spacing.button_padding = egui::vec2(8.0, 4.0);
        }
        let inner = egui::Frame::none()
            .rounding(egui::Rounding::same(6.0))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let mut group = ButtonGroup { ui, compact };
                    add(&mut group)
                })
                .inner
            })
            .inner;
        if compact {
            ui.style_mut().spacing.button_padding = pad;
        }
        inner
    })
    .inner
}

/// Left-aligned row for one or more [`button_group`]s (does not consume remaining width).
pub fn left_button_row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| add(ui)).inner
}

pub fn button_toolbar_wrapped<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        let w = constrain_content_width(ui, 0.0);
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
            |ui| {
                ui.set_max_width(w);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 8.0);
                add(ui)
            },
        )
        .inner
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_badge_colors_follow_height_buckets() {
        let (fill_1080, _) = resolution_badge_colors("1920x1080");
        let (fill_720, _) = resolution_badge_colors("1280x720");
        assert_ne!(fill_1080, fill_720);
    }

    #[test]
    fn codec_badge_colors_distinguish_common_codecs() {
        let (av1, _) = codec_badge_colors("convert");
        let (h264, _) = codec_badge_colors("H264");
        let (hevc, _) = codec_badge_colors("HEVC");
        assert_ne!(av1, h264);
        assert_ne!(h264, hevc);
    }

    #[test]
    fn fps_badge_colors_distinguish_common_rates() {
        let (fps60, _) = fps_badge_colors("60.00 fps");
        let (fps30, _) = fps_badge_colors("30.00 fps");
        let (fps24, _) = fps_badge_colors("23.98 fps");
        assert_ne!(fps60, fps30);
        assert_ne!(fps30, fps24);
    }

    #[test]
    fn queue_list_height_from_layout_with_status_block() {
        let list_h = queue_list_height_from_layout(100.0, 500.0, 72.0, 200.0);
        assert!((list_h - 128.0).abs() < 0.01);
        assert!(queue_list_height_from_layout(400.0, 500.0, 72.0, 200.0) >= 0.0);
    }

    #[test]
    fn docked_log_lines_max_h_respects_chrome() {
        assert_eq!(
            docked_log_lines_max_h(200.0),
            (200.0 - DOCKED_LOG_CHROME_H).clamp(0.0, 480.0)
        );
        // Short remaining space must not invent an 80px floor (that crushed the queue list).
        assert_eq!(docked_log_lines_max_h(50.0), 0.0);
        assert_eq!(docked_log_lines_max_h(DOCKED_LOG_CHROME_H + 40.0), 40.0);
    }

    #[test]
    fn queue_docked_under_videos_log_fit_protects_list() {
        let footer = 74.0;
        let min_list = 50.0;
        let content_top = 40.0;
        // ~360px panel body: old math reserved ≥215px log and left the list near zero.
        let body_bottom = 360.0;
        let (lines, block) = queue_docked_under_videos_log_fit(
            true,
            180.0,
            body_bottom,
            content_top,
            footer,
            min_list,
        );
        let list_h = queue_list_height_from_layout(content_top, body_bottom, footer, block);
        assert!(
            list_h + 0.5 >= min_list,
            "list_h={list_h} block={block} lines={lines}"
        );
        assert!(block > 0.0);
        // Very short panel: drop log reservation rather than steal the list.
        let (lines2, block2) =
            queue_docked_under_videos_log_fit(true, 180.0, 200.0, 40.0, footer, min_list);
        let list2 = queue_list_height_from_layout(40.0, 200.0, footer, block2);
        assert_eq!(block2, 0.0);
        assert_eq!(lines2, 0.0);
        assert!(list2 + 0.5 >= min_list, "list2={list2}");
    }

    #[test]
    fn default_videos_dock_height_scales_with_viewport() {
        assert_eq!(
            default_videos_dock_height_for_viewport(880.0),
            880.0 * VIDEOS_DOCKED_HEIGHT_RATIO
        );
        assert!(default_videos_dock_height_for_viewport(300.0) >= 180.0);
    }

    #[test]
    fn viewport_auto_undock_at_min_inner_size() {
        let at_min = egui::vec2(VIEWPORT_MIN_INNER_WIDTH, VIEWPORT_MIN_INNER_HEIGHT);
        assert!(viewport_too_small_for_docked_videos(at_min));
        assert!(!viewport_large_enough_to_redock_videos(at_min));
    }

    #[test]
    fn queue_footer_reserve_scales_with_width() {
        assert_eq!(queue_footer_toolbar_reserve(1000.0, false, true, 1.0), 72.0);
        assert_eq!(queue_footer_toolbar_reserve(900.0, false, true, 1.0), 72.0);
        assert_eq!(
            queue_footer_toolbar_reserve(750.0, false, true, 1.0),
            96.0 + 24.0
        );
        assert_eq!(
            queue_footer_toolbar_reserve(600.0, false, true, 1.0),
            96.0 + 24.0
        );
        assert_eq!(
            queue_footer_toolbar_reserve(480.0, false, true, 1.0),
            130.0 + 24.0
        );
        assert_eq!(
            queue_footer_toolbar_reserve(599.0, false, true, 1.0),
            130.0 + 24.0
        );
        assert_eq!(
            queue_footer_toolbar_reserve(601.0, false, true, 1.0),
            96.0 + 24.0
        );
        assert_eq!(queue_footer_toolbar_reserve(901.0, false, true, 1.0), 72.0);
    }

    #[test]
    fn queue_footer_reserve_convert_mode_taller() {
        assert_eq!(
            queue_footer_toolbar_reserve(1000.0, true, true, 1.0),
            72.0 + 40.0
        );
        assert_eq!(
            queue_footer_toolbar_reserve(480.0, true, false, 1.0),
            130.0 + 40.0
        );
    }

    #[test]
    fn queue_footer_reserve_prefers_measured() {
        let est = queue_footer_reserve(800.0, false, true, None, 1.0);
        let raised = queue_footer_reserve(800.0, false, true, Some(140.0), 1.0);
        assert!(raised >= est);
        assert_eq!(raised, 140.0);
        // Trust measured even when below the width estimate (avoids under-footer void).
        assert_eq!(
            queue_footer_reserve(800.0, false, true, Some(50.0), 1.0),
            50.0
        );
        assert_eq!(queue_footer_reserve(800.0, false, true, None, 1.0), est);
    }

    #[test]
    fn queue_list_height_recovers_when_measured_footer_below_estimate() {
        let content_top = 100.0;
        let body_bottom = 500.0;
        let est = queue_footer_reserve(800.0, false, true, None, 1.0);
        let measured = 50.0;
        assert!(measured < est);
        let old_list =
            queue_list_height_from_layout(content_top, body_bottom, est.max(measured), 0.0);
        let new_footer = queue_footer_reserve(800.0, false, true, Some(measured), 1.0);
        let new_list = queue_list_height_from_layout(content_top, body_bottom, new_footer, 0.0);
        assert!((new_list - old_list - (est - measured)).abs() < 0.01);
    }

    #[test]
    fn queue_list_min_scroll_h_mode_aware() {
        assert!(queue_list_min_scroll_h(true, true, false) >= QUEUE_CONVERT_LIST_ROW_H);
        assert!(queue_list_min_scroll_h(true, false, false) >= QUEUE_DL_LIST_ROW_H);
        assert_eq!(
            queue_list_min_scroll_h(true, true, true),
            QUEUE_CONVERT_LIST_ROW_COMPACT_H + QUEUE_DOCKED_LIST_MIN_PAD
        );
    }

    #[test]
    fn queue_panel_layout_heights_convert_footer_and_log() {
        let (list_h, stack_h) = queue_panel_layout_heights(80.0, 520.0, 112.0, 304.0);
        assert_eq!(stack_h, 416.0);
        assert_eq!(list_h, 520.0 - 80.0 - 416.0);
    }

    #[test]
    fn queue_log_block_height_includes_heading() {
        let h = queue_log_block_height(true, 180.0, true);
        assert!(h >= 180.0 + DOCKED_LOG_CHROME_H + DOCKED_LOG_HEADING_H);
        let footer = queue_log_block_height(true, 180.0, false);
        assert!(footer > h - DOCKED_LOG_UNDER_VIDEOS_SEPARATOR_H);
    }

    #[test]
    fn queue_panel_layout_heights_never_negative_list() {
        let (list_h, stack_h) = queue_panel_layout_heights(100.0, 500.0, 72.0, 200.0);
        assert!(list_h >= 0.0);
        assert_eq!(stack_h, 272.0);
    }

    #[test]
    fn clamp_dock_heights_for_viewport_helper() {
        let mut dock = 800.0;
        let mut undock = 600.0;
        let mut log = 400.0;
        assert!(super::clamp_dock_heights_for_viewport(
            760.0,
            &mut dock,
            &mut undock,
            &mut log,
            false
        ));
        assert!(dock <= max_bottom_panel_height(760.0) + 0.01);
        assert!(undock <= max_bottom_panel_height(760.0) + 0.01);
        let mut dock2 = 200.0;
        let mut undock2 = 200.0;
        let mut log2 = 180.0;
        assert!(!super::clamp_dock_heights_for_viewport(
            760.0,
            &mut dock2,
            &mut undock2,
            &mut log2,
            true
        ));
        assert_eq!(dock2, 200.0);
    }

    #[test]
    fn viewport_auto_redock_after_hysteresis_margin() {
        let big = egui::vec2(
            VIEWPORT_MIN_INNER_WIDTH + 80.0,
            VIEWPORT_MIN_INNER_HEIGHT + 80.0,
        );
        assert!(!viewport_too_small_for_docked_videos(big));
        assert!(viewport_large_enough_to_redock_videos(big));
    }

    fn idle_inputs() -> NavbarStatusInputs {
        NavbarStatusInputs {
            shutdown_pending: false,
            add_in_progress: false,
            convert_running: false,
            convert_paused: false,
            convert_has_pending: false,
            convert_resolving: false,
            status_resolving: 0,
            status_queued: 0,
            status_active: 0,
            status_ready: 0,
            downloads_paused: false,
            queue_running: 0,
        }
    }

    #[test]
    fn navbar_status_idle_by_default() {
        let info = derive_navbar_status(idle_inputs());
        assert_eq!(info.slug, NavbarStatusSlug::Idle);
        assert_eq!(info.label, "Idle");
    }

    #[test]
    fn navbar_status_downloading_when_active() {
        let mut input = idle_inputs();
        input.status_active = 1;
        input.queue_running = 1;
        let info = derive_navbar_status(input);
        assert_eq!(info.slug, NavbarStatusSlug::Downloading);
    }

    #[test]
    fn navbar_status_converting_over_downloading() {
        let mut input = idle_inputs();
        input.status_active = 2;
        input.convert_running = true;
        let info = derive_navbar_status(input);
        assert_eq!(info.slug, NavbarStatusSlug::Converting);
    }

    #[test]
    fn apply_layout_preset_review_opens_docked_log() {
        let mut s = crate::config::AppSettings::default();
        apply_layout_preset(&mut s, "review", Some(900.0));
        assert!(!s.card_list_layout);
        assert!(s.logs_open);
        assert!(s.logs_docked);
        assert!(s.videos_dock_height >= BOTTOM_PANEL_MIN_H);
        assert!(s.log_dock_height >= 120.0);
    }

    #[test]
    fn apply_layout_preset_minimal_hides_thumbnails() {
        let mut s = crate::config::AppSettings::default();
        apply_layout_preset(&mut s, "minimal", Some(800.0));
        assert!(!s.show_thumbnails);
        assert!(s.videos_dock_height >= BOTTOM_PANEL_MIN_H);
    }
}
