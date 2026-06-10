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

/// Full-width batch progress bar with a caller-supplied caption.
pub fn draw_batch_progress_bar(
    ui: &mut egui::Ui,
    fraction: f32,
    caption: impl AsRef<str>,
    fill: Color32,
    animate: bool,
) -> Response {
    ui.add(
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
    ui.painter()
        .circle_stroke(center, radius, egui::Stroke::new(1.0, shade(color, 0.72)));
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
            pulse: false,
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
    let dot_alpha = if info.pulse {
        let t = ui.input(|i| i.time);
        let phase = (t * std::f64::consts::TAU / 1.4).sin() * 0.5 + 0.5;
        (0.55 + 0.45 * phase) as f32
    } else {
        1.0
    };
    egui::Frame::none()
        .stroke(egui::Stroke::new(1.0, border_color))
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
                        .color(text_color),
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
    egui::Stroke::new(1.0, shade(bg_fill, 0.78))
}

fn grouped_button_stroke() -> egui::Stroke {
    egui::Stroke::NONE
}

pub fn danger_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    let bg = Color32::from_rgb(183, 28, 28);
    colored_button(
        ui,
        label,
        enabled,
        Color32::from_rgb(255, 235, 238),
        bg,
        egui::Rounding::same(6.0),
        standalone_button_stroke(bg),
    )
}

pub fn success_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    let bg = Color32::from_rgb(46, 125, 50);
    colored_button(
        ui,
        label,
        enabled,
        Color32::from_rgb(232, 245, 233),
        bg,
        egui::Rounding::same(6.0),
        standalone_button_stroke(bg),
    )
}

pub fn warning_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    let bg = Color32::from_rgb(245, 124, 0);
    colored_button(
        ui,
        label,
        enabled,
        Color32::from_rgb(255, 255, 255),
        bg,
        egui::Rounding::same(6.0),
        standalone_button_stroke(bg),
    )
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
pub fn show_mode_panel<R>(
    ui: &mut egui::Ui,
    theme: &str,
    av1: bool,
    colors: ModePanelColors<'_>,
    inner_margin: egui::Margin,
    rounding: f32,
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
        stroke: Stroke::new(1.0, border),
    };

    egui::Frame::none()
        .inner_margin(inner_margin)
        .show(ui, |ui| {
            let bg_idx = ui.painter().add(Shape::Noop);
            let ret = add_contents(ui);
            let paint_rect = ui.min_rect() + inner_margin;
            if ui.is_rect_visible(paint_rect) {
                paint_mode_panel_background(ui.painter(), bg_idx, paint_rect, &style);
            }
            ret
        })
}

const MIN_CONTROLS_SCROLL_H: f32 = 100.0;
const VIDEOS_DOCKED_HEIGHT_RATIO: f32 = 0.52;

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
pub const VIDEOS_DOCK_PANEL_ID: &str = "rustdl_videos_dock_v3";
/// [`egui::TopBottomPanel`] id for the undocked queue footer strip.
pub const UNDOCKED_FOOTER_PANEL_ID: &str = "rustdl_undocked_footer_v3";

/// Use the parent's width without fixing height or locking horizontal resize.
pub fn fill_allocated_rect(ui: &mut egui::Ui) -> egui::Vec2 {
    let w = ui.max_rect().width().max(1.0);
    let h = ui.max_rect().height().max(1.0);
    ui.set_max_width(w);
    egui::vec2(w, h)
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
        size.y = 0.0;
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
    if let Some(mut state) = egui::containers::panel::PanelState::load(ctx, id) {
        if (state.rect.height() - height).abs() > 0.5 {
            state.rect = egui::Rect::from_min_size(
                state.rect.min,
                egui::vec2(state.rect.width().max(1.0), height),
            );
            ctx.data_mut(|d| d.insert_persisted(id, state));
        }
    }
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
    if (current.0 - size.x).abs() <= 0.5 && (current.1 - size.y).abs() <= 0.5 {
        return None;
    }
    Some((size.x, size.y))
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

/// Like [`remaining_ui_height`] but ignores unbounded `available_height()` from content-sized parents.
pub fn bounded_ui_height(ui: &egui::Ui, min: f32) -> f32 {
    let cap = remaining_ui_height(ui);
    let avail = ui.available_height();
    if avail.is_finite() && avail > 0.0 && avail < 50_000.0 {
        avail.min(cap).max(min)
    } else {
        cap.max(min)
    }
}

/// Split remaining main-panel height between scrollable controls and a pinned footer.
pub struct MainColumnSplit {
    pub controls_max_height: f32,
    /// Height reserved for the pinned footer (docked video queue and/or undocked strip + docked log).
    pub footer_height: f32,
}

const UNDOCKED_VIDEOS_STRIP_H: f32 = 100.0;
/// Header row, height slider, and filter toolbar above docked log lines.
const DOCKED_LOG_CHROME_H: f32 = 100.0;

/// Estimated vertical space for the video queue footer toolbar (dock/hide + batch actions).
pub fn queue_footer_toolbar_reserve(content_width: f32) -> f32 {
    if content_width >= 900.0 {
        72.0
    } else if content_width >= 600.0 {
        96.0
    } else {
        130.0
    }
}

pub fn compute_main_column_split(
    available_height: f32,
    videos_docked: bool,
    compact_cards: bool,
    log_docked_separate: bool,
    log_docked_under_videos: bool,
    log_dock_height: f32,
) -> MainColumnSplit {
    let h = available_height.max(0.0);
    let log_h = log_dock_height.clamp(80.0, 480.0) + DOCKED_LOG_CHROME_H;
    let log_footer = if log_docked_separate { log_h } else { 0.0 };
    if !videos_docked {
        let footer = UNDOCKED_VIDEOS_STRIP_H + log_footer;
        return MainColumnSplit {
            controls_max_height: (h - footer).max(MIN_CONTROLS_SCROLL_H),
            footer_height: footer,
        };
    }
    let log_reserve = if log_docked_under_videos { log_h } else { 0.0 };
    let min_videos = if compact_cards { 180.0 } else { 220.0 };
    let min_videos = min_videos + log_reserve;
    if h <= MIN_CONTROLS_SCROLL_H + min_videos {
        let videos_h = (h * 0.45).clamp(120.0, (h - 60.0).max(120.0));
        let controls_h = (h - videos_h).max(60.0);
        return MainColumnSplit {
            controls_max_height: controls_h,
            footer_height: videos_h,
        };
    }
    let videos_h = (h * VIDEOS_DOCKED_HEIGHT_RATIO)
        .max(min_videos)
        .min(h - MIN_CONTROLS_SCROLL_H);
    MainColumnSplit {
        controls_max_height: h - videos_h,
        footer_height: videos_h,
    }
}

/// Full-width Downloader / Video Converter tabs with a fixed 50/50 split.
pub fn draw_mode_nav_bar(
    ui: &mut egui::Ui,
    theme: &str,
    dl_active: bool,
    av1_active: bool,
    colors: ModePanelColors<'_>,
) -> (bool, bool) {
    let dl_accent = mode_accent_for(false, &colors);
    let convert_accent = mode_accent_for(true, &colors);
    let mut dl_clicked = false;
    let mut av1_clicked = false;
    let row_w = content_width(ui).max(1.0);
    let muted = text_muted(theme);
    let group_border = panel_border(theme);
    let group_fill = if theme == "light" {
        Color32::from_rgba_unmultiplied(0, 0, 0, 10)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 8)
    };
    ui.allocate_ui_with_layout(
        egui::vec2(row_w, 38.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(row_w);
            let btn_w = row_w * 0.5;
            egui::Frame::none()
                .fill(group_fill)
                .stroke(Stroke::new(1.0, group_border))
                .rounding(egui::Rounding::same(6.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let dl_text = if dl_active { Color32::WHITE } else { muted };
                        let dl_label = RichText::new(format!(
                            "{} Downloader",
                            crate::ui_icons::NAV_DOWNLOADER
                        ))
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
                        if dl.clicked() {
                            dl_clicked = true;
                        }
                        let av1_text = if av1_active { Color32::WHITE } else { muted };
                        let av1_label =
                            RichText::new(format!("{} Video Converter", crate::ui_icons::NAV_AV1))
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
                        if av1.clicked() {
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

/// Cap layout width without forcing horizontal expansion (preserves panel margins).
pub fn constrain_content_width(ui: &mut egui::Ui, max_content_width: f32) -> f32 {
    let mut w = content_width(ui);
    if max_content_width > 0.0 {
        w = w.min(max_content_width);
    }
    ui.set_max_width(w);
    w
}

/// Allocate a top-down child region with an explicit size (avoids shrink-wrapped `max_rect`).
pub fn allocate_top_down_rect<R>(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let size = egui::vec2(size.x.max(1.0), size.y.max(1.0));
    let rect = egui::Rect::from_min_size(ui.cursor().min, size);
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
    (bottom_y - ui.cursor().min.y).max(0.0)
}

/// Lay out children across the full width of the parent (egui vertical layouts default to shrink-wrap).
pub fn with_full_width<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let width = content_width(ui);
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
            .stroke(egui::Stroke::new(1.0, border))
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

fn show_menu_popup<R>(
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
    pub fn url_menu(
        &mut self,
        url: &str,
        copy_clicked: &mut bool,
        open_clicked: &mut bool,
    ) -> Response {
        let compact = self.compact;
        let label = format!("{} URL...", crate::ui_icons::PAGE_URL);
        self.add(|ui| {
            let popup_id = ui.make_persistent_id("url_menu");
            grouped_popup_menu(ui, popup_id, &label, true, compact, false, |ui| {
                if ui
                    .button(format!("{} Copy URL", crate::ui_icons::COPY_CLIPBOARD))
                    .on_hover_text(url)
                    .clicked()
                {
                    *copy_clicked = true;
                }
                if ui
                    .button(format!("{} Open URL", crate::ui_icons::UPDATE_OPEN))
                    .on_hover_text("Open in your default browser")
                    .clicked()
                {
                    *open_clicked = true;
                }
            })
            .on_hover_text(url)
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

    /// Verify saved file streams and optionally re-download (done rows).
    pub fn verify_menu(
        &mut self,
        show_verify_file: bool,
        can_verify_file: bool,
        show_redownload: bool,
        can_redownload: bool,
        verify_clicked: &mut bool,
        redownload_clicked: &mut bool,
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
                });
            }
            button.on_hover_text("Verify the saved file or re-download this URL")
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

/// Row of one or more [`button_group`]s with spacing between groups.
pub fn button_toolbar<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        add(ui)
    })
    .inner
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
    fn main_column_split_fits_viewport() {
        let split = compute_main_column_split(600.0, true, false, false, false, 120.0);
        assert!(split.controls_max_height >= 100.0);
        assert!(split.footer_height >= 220.0);
        assert!((split.controls_max_height + split.footer_height - 600.0).abs() < 0.01);
    }

    #[test]
    fn main_column_split_never_exceeds_available() {
        let split = compute_main_column_split(280.0, true, false, false, false, 120.0);
        assert!(split.controls_max_height + split.footer_height <= 280.0 + 0.01);
    }

    #[test]
    fn main_column_split_undocked_uses_full_height() {
        let split = compute_main_column_split(600.0, false, false, false, false, 120.0);
        assert_eq!(
            split.controls_max_height,
            (500.0_f32).max(MIN_CONTROLS_SCROLL_H)
        );
        assert_eq!(split.footer_height, UNDOCKED_VIDEOS_STRIP_H);
    }

    #[test]
    fn main_column_split_undocked_reserves_docked_log() {
        let split = compute_main_column_split(600.0, false, false, true, false, 120.0);
        assert_eq!(
            split.footer_height,
            UNDOCKED_VIDEOS_STRIP_H + 120.0 + DOCKED_LOG_CHROME_H
        );
        assert!((split.controls_max_height + split.footer_height - 600.0).abs() < 0.01);
    }

    #[test]
    fn viewport_auto_undock_at_min_inner_size() {
        let at_min = egui::vec2(VIEWPORT_MIN_INNER_WIDTH, VIEWPORT_MIN_INNER_HEIGHT);
        assert!(viewport_too_small_for_docked_videos(at_min));
        assert!(!viewport_large_enough_to_redock_videos(at_min));
    }

    #[test]
    fn queue_footer_reserve_scales_with_width() {
        assert_eq!(queue_footer_toolbar_reserve(1000.0), 72.0);
        assert_eq!(queue_footer_toolbar_reserve(900.0), 72.0);
        assert_eq!(queue_footer_toolbar_reserve(750.0), 96.0);
        assert_eq!(queue_footer_toolbar_reserve(600.0), 96.0);
        assert_eq!(queue_footer_toolbar_reserve(480.0), 130.0);
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
}
