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
    ui.painter()
        .circle_filled(center, radius, color);
    ui.painter().circle_stroke(
        center,
        radius,
        egui::Stroke::new(1.0, shade(color, 0.72)),
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
}

/// Small pill label for resolution or estimated size on video cards.
pub fn draw_meta_badge(ui: &mut egui::Ui, label: &str, kind: MetaBadgeKind) {
    let (fill, text_color) = match kind {
        MetaBadgeKind::Resolution => (
            Color32::from_rgb(38, 90, 136),
            Color32::from_rgb(230, 240, 255),
        ),
        MetaBadgeKind::SizeEstimate => (
            Color32::from_rgb(120, 75, 20),
            Color32::from_rgb(255, 236, 200),
        ),
    };
    egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(5.0))
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(label).small().strong().color(text_color));
        });
}

fn shade(color: Color32, factor: f32) -> Color32 {
    let [r, g, b, a] = color.to_array();
    let scale = |v: u8| -> u8 { ((v as f32 * factor).round()).clamp(0.0, 255.0) as u8 };
    Color32::from_rgba_unmultiplied(scale(r), scale(g), scale(b), a)
}

fn colored_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    text_color: Color32,
    bg_fill: Color32,
) -> Response {
    let (fill, stroke, text) = if enabled {
        (
            bg_fill,
            egui::Stroke::new(1.0, shade(bg_fill, 0.78)),
            text_color,
        )
    } else {
        (
            shade(bg_fill, 0.45),
            egui::Stroke::new(1.0, shade(bg_fill, 0.35)),
            shade(text_color, 0.70),
        )
    };

    let button = egui::Button::new(RichText::new(label).color(text))
        .frame(true)
        .fill(fill)
        .stroke(stroke);
    ui.add_enabled(enabled, button)
}

pub fn danger_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    colored_button(
        ui,
        label,
        enabled,
        Color32::from_rgb(255, 235, 238),
        Color32::from_rgb(183, 28, 28),
    )
}

pub fn success_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    colored_button(
        ui,
        label,
        enabled,
        Color32::from_rgb(232, 245, 233),
        Color32::from_rgb(46, 125, 50),
    )
}

pub fn warning_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    colored_button(
        ui,
        label,
        enabled,
        Color32::from_rgb(40, 24, 0),
        Color32::from_rgb(255, 167, 38),
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

fn alert_box<R>(
    ui: &mut egui::Ui,
    bg: Color32,
    border: Color32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let width = ui.available_width();
    ui.scope(|ui| {
        ui.set_width(width);
        egui::Frame::none()
            .fill(bg)
            .stroke(egui::Stroke::new(1.0, border))
            .rounding(egui::Rounding::same(6.0))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, add_contents)
            .inner
    })
    .inner
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
        sizing_ui.horizontal(&mut add_contents).response.rect.width()
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
    colored_button(
        ui,
        label,
        enabled,
        Color32::from_rgb(227, 242, 253),
        Color32::from_rgb(30, 136, 229),
    )
}
