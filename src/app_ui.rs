use eframe::egui;
use eframe::egui::{Color32, Response, RichText};

use crate::models::ItemStatus;

pub fn status_color(s: ItemStatus) -> Color32 {
    match s {
        ItemStatus::Resolving => Color32::from_rgb(120, 144, 156),
        ItemStatus::Idle => Color32::GRAY,
        ItemStatus::Queued => Color32::from_rgb(255, 193, 7),
        ItemStatus::Downloading => Color32::from_rgb(66, 165, 245),
        ItemStatus::Done => Color32::from_rgb(102, 187, 106),
        ItemStatus::Failed => Color32::from_rgb(239, 83, 80),
    }
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

pub fn secondary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    colored_button(
        ui,
        label,
        enabled,
        Color32::from_rgb(227, 242, 253),
        Color32::from_rgb(30, 136, 229),
    )
}
