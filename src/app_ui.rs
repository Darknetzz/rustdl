use std::hash::Hash;

use eframe::egui;
use eframe::egui::{Color32, Response, RichText};

use crate::models::ItemStatus;

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

pub fn draw_status_chip(ui: &mut egui::Ui, status: ItemStatus) {
    let text = RichText::new(status.as_str()).small().color(Color32::WHITE);
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
    if c.contains("av1") {
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
        (
            bg_fill,
            stroke,
            text_color,
        )
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

const MIN_CONTROLS_SCROLL_H: f32 = 100.0;
const VIDEOS_DOCKED_HEIGHT_RATIO: f32 = 0.45;

/// Split remaining main-panel height between scrollable controls and a docked video queue.
pub struct MainColumnSplit {
    pub controls_max_height: f32,
    pub videos_height: f32,
}

pub fn compute_main_column_split(
    available_height: f32,
    videos_docked: bool,
    compact_cards: bool,
) -> MainColumnSplit {
    let h = available_height.max(0.0);
    if !videos_docked {
        const UNDOCKED_STRIP: f32 = 72.0;
        return MainColumnSplit {
            controls_max_height: (h - UNDOCKED_STRIP).max(MIN_CONTROLS_SCROLL_H),
            videos_height: 0.0,
        };
    }
    let min_videos = if compact_cards { 180.0 } else { 220.0 };
    if h <= MIN_CONTROLS_SCROLL_H + min_videos {
        let videos_h = (h * 0.45).clamp(120.0, (h - 60.0).max(120.0));
        let controls_h = (h - videos_h).max(60.0);
        return MainColumnSplit {
            controls_max_height: controls_h,
            videos_height: videos_h,
        };
    }
    let videos_h = (h * VIDEOS_DOCKED_HEIGHT_RATIO)
        .max(min_videos)
        .min(h - MIN_CONTROLS_SCROLL_H);
    MainColumnSplit {
        controls_max_height: h - videos_h,
        videos_height: videos_h,
    }
}

/// Full-width Downloader / AV1 Converter tabs with a fixed 50/50 split.
pub fn draw_mode_nav_bar(ui: &mut egui::Ui, dl_active: bool, av1_active: bool) -> (bool, bool) {
    let mut dl_clicked = false;
    let mut av1_clicked = false;
    with_full_width(ui, |ui| {
        let row_w = ui.available_width();
        let btn_w = (row_w * 0.5).max(0.0);
        button_group(ui, "mode_nav", |g| {
            let dl = g.add(|ui| {
                ui.add(
                    egui::Button::new(
                        RichText::new(format!("{} Downloader", crate::ui_icons::NAV_DOWNLOADER))
                            .strong()
                            .color(if dl_active {
                                Color32::from_rgb(10, 32, 10)
                            } else {
                                Color32::from_rgb(210, 220, 235)
                            }),
                    )
                    .min_size(egui::vec2(btn_w, 34.0))
                    .fill(if dl_active {
                        Color32::from_rgb(152, 255, 152)
                    } else {
                        Color32::from_rgb(44, 52, 64)
                    })
                    .stroke(egui::Stroke::NONE)
                    .rounding(egui::Rounding::ZERO),
                )
            });
            if dl.clicked() {
                dl_clicked = true;
            }
            let av1 = g.add(|ui| {
                ui.add(
                    egui::Button::new(
                        RichText::new(format!("{} AV1 Converter", crate::ui_icons::NAV_AV1))
                            .strong()
                            .color(if av1_active {
                                Color32::from_rgb(45, 27, 0)
                            } else {
                                Color32::from_rgb(210, 220, 235)
                            }),
                    )
                    .min_size(egui::vec2(btn_w, 34.0))
                    .fill(if av1_active {
                        Color32::from_rgb(255, 190, 90)
                    } else {
                        Color32::from_rgb(44, 52, 64)
                    })
                    .stroke(egui::Stroke::NONE)
                    .rounding(egui::Rounding::ZERO),
                )
            });
            if av1.clicked() {
                av1_clicked = true;
            }
        });
    });
    (dl_clicked, av1_clicked)
}

/// Width of the current layout region (respects [`content_panel_frame`] margins).
pub fn content_width(ui: &egui::Ui) -> f32 {
    ui.max_rect().width().max(0.0)
}

/// Cap layout width without forcing horizontal expansion (preserves panel margins).
pub fn constrain_content_width(ui: &mut egui::Ui) -> f32 {
    let w = content_width(ui);
    ui.set_max_width(w);
    w
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

fn grouped_secondary_button(ui: &mut egui::Ui, label: &str, enabled: bool, compact: bool) -> Response {
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

fn grouped_success_button(ui: &mut egui::Ui, label: &str, enabled: bool, compact: bool) -> Response {
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

fn grouped_warning_button(ui: &mut egui::Ui, label: &str, enabled: bool, compact: bool) -> Response {
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
    let _ = ui.id().with(id_salt);
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
        let w = constrain_content_width(ui);
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
        let (av1, _) = codec_badge_colors("AV1");
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
        let split = compute_main_column_split(600.0, true, false);
        assert!(split.controls_max_height >= 100.0);
        assert!(split.videos_height >= 220.0);
        assert!((split.controls_max_height + split.videos_height - 600.0).abs() < 0.01);
    }

    #[test]
    fn main_column_split_never_exceeds_available() {
        let split = compute_main_column_split(280.0, true, false);
        assert!(split.controls_max_height + split.videos_height <= 280.0 + 0.01);
    }

    #[test]
    fn main_column_split_undocked_uses_full_height() {
        let split = compute_main_column_split(600.0, false, false);
        assert_eq!(split.controls_max_height, (528.0_f32).max(MIN_CONTROLS_SCROLL_H));
        assert_eq!(split.videos_height, 0.0);
    }
}
