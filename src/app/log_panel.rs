use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{danger_button, secondary_button};
use crate::ui_icons;

use super::{InputLineInfo, InputLineKind, PydlApp, ICON_MISSING, ICON_OK};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogFilter {
    All,
    Important,
    Errors,
}

impl LogFilter {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LogFilter::All => "All",
            LogFilter::Important => "Important",
            LogFilter::Errors => "Errors",
        }
    }

    pub(crate) fn accepts(self, line: &str) -> bool {
        match self {
            LogFilter::All => true,
            LogFilter::Errors => is_error_line(line),
            LogFilter::Important => {
                let lower = line.to_ascii_lowercase();
                is_error_line(line)
                    || lower.contains("metadata fetch failed")
                    || lower.contains("download failed")
                    || lower.contains("starting")
                    || lower.contains("completed")
                    || lower.contains("done")
                    || lower.contains("queue")
            }
        }
    }
}

/// Muted semantic text on dark backgrounds (matches activity log).
pub(crate) const LOG_COLOR_ERROR: Color32 = Color32::from_rgb(255, 138, 128);
pub(crate) const LOG_COLOR_WARN: Color32 = Color32::from_rgb(255, 193, 120);
pub(crate) const LOG_COLOR_OK: Color32 = Color32::from_rgb(132, 204, 154);
pub(crate) const LOG_COLOR_DIM: Color32 = Color32::from_rgb(168, 170, 178);

pub(crate) fn log_line_color(line: &str) -> Color32 {
    if is_error_line(line) {
        LOG_COLOR_ERROR
    } else if is_warning_line(line) {
        LOG_COLOR_WARN
    } else if is_success_line(line) {
        LOG_COLOR_OK
    } else {
        LOG_COLOR_DIM
    }
}

fn log_line_widget(line: &str, color: Color32, ui: &egui::Ui) -> egui::WidgetText {
    let font_px = egui::TextStyle::Small.resolve(ui.style()).size;
    let font_id = egui::FontId::new(font_px, egui::FontFamily::Monospace);
    let job = egui::text::LayoutJob::single_section(
        line.to_owned(),
        egui::text::TextFormat {
            font_id,
            color,
            ..Default::default()
        },
    );
    egui::WidgetText::from(job)
}

pub(crate) fn is_error_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("error")
        || lower.contains("failed")
        || lower.contains("not found")
        || lower.contains("invalid")
        || lower.contains("missing")
}

fn is_warning_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("warning") || lower.contains("deprecated") || lower.contains("caution")
}

fn is_success_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    // Only treat 100% as success on yt-dlp-style progress lines (avoids a solid wall of green).
    if lower.contains("100%") {
        if !(lower.contains("[download]") || lower.contains("[merger]")) {
            return false;
        }
        return true;
    }
    lower.contains("already been downloaded")
        || lower.contains("already downloaded")
        || lower.starts_with("deleted file:")
        || lower.contains("download complete")
        || lower.contains("finished downloading")
        || lower.contains("post-processing is already complete")
        || lower.contains("has already been recorded in the archive")
        || (lower.contains("merge") && lower.contains("complete"))
}

impl PydlApp {
    pub(super) fn draw_logs_window(&mut self, ctx: &egui::Context) {
        if !self.logs_open {
            return;
        }
        let mut logs_open = self.logs_open;
        egui::Window::new("Activity log")
            .open(&mut logs_open)
            .default_size([640.0, 440.0])
            .min_width(400.0)
            .min_height(260.0)
            .show(ctx, |ui| {
                self.draw_activity_log_panel(ui);
            });
        self.logs_open = logs_open;
    }

    /// Toolbar (filter, copy) + scrollable colored activity log (shown inside [`Self::draw_logs_window`]).
    pub(super) fn draw_activity_log_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if danger_button(ui, &format!("{} Clear log", ui_icons::CLEAR_LOG), true).clicked() {
                self.clear_activity_log();
            }
            ui.label("Filter");
            egui::ComboBox::from_id_salt("log_filter")
                .selected_text(self.log_filter.as_str())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.log_filter, LogFilter::All, "All");
                    ui.selectable_value(&mut self.log_filter, LogFilter::Important, "Important");
                    ui.selectable_value(&mut self.log_filter, LogFilter::Errors, "Errors");
                });
            if secondary_button(
                ui,
                &format!("{} Copy last error", ui_icons::COPY_CLIPBOARD),
                true,
            )
            .clicked()
            {
                if let Some(last) = self
                    .log_lines
                    .iter()
                    .rev()
                    .find(|line| is_error_line(line))
                {
                    ui.ctx().copy_text(last.clone());
                }
            }
        });
        let scroll_h = ui.available_height().max(120.0);
        egui::Frame::dark_canvas(ui.style())
            .fill(Color32::from_rgb(28, 28, 32))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(56, 56, 64)))
            .inner_margin(egui::Margin::same(10.0))
            .rounding(egui::Rounding::same(6.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(scroll_h);
                ui.spacing_mut().item_spacing.y = 3.0;
                egui::ScrollArea::vertical()
                    .max_height(scroll_h)
                    .animated(true)
                    .auto_shrink([false, false])
                    .stick_to_bottom(self.settings.autoscroll_log)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for line in &self.log_lines {
                            if !self.log_filter.accepts(line) {
                                continue;
                            }
                            let color = log_line_color(line);
                            // Selectable labels paint with a single theme color; use LayoutJob + non-selectable
                            // so per-line semantic colors actually reach the tessellator.
                            ui.add(
                                egui::Label::new(log_line_widget(line, color, ui))
                                    .wrap()
                                    .selectable(false),
                            );
                        }
                    });
            });
    }
}

pub(crate) fn draw_input_line_summary(ui: &mut egui::Ui, lines: &[InputLineInfo]) {
    if lines.is_empty() {
        return;
    }
    let valid = lines
        .iter()
        .filter(|x| x.kind == InputLineKind::Valid)
        .count();
    let dup = lines
        .iter()
        .filter(|x| {
            matches!(
                x.kind,
                InputLineKind::DuplicateExisting | InputLineKind::DuplicateInInput
            )
        })
        .count();
    let invalid = lines
        .iter()
        .filter(|x| x.kind == InputLineKind::Invalid)
        .count();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{valid} valid")).color(Color32::from_rgb(102, 187, 106)));
        ui.label(RichText::new(format!("{dup} duplicates")).color(Color32::from_rgb(255, 193, 7)));
        ui.label(RichText::new(format!("{invalid} invalid")).color(Color32::from_rgb(239, 83, 80)));
    });
}

/// Queue clipboard text for the next frame. Context menus run after widgets are built; pushing
/// [`egui::Event::Paste`] in the same frame happens too late for [`egui::TextEdit`] to consume it.
pub(crate) fn attach_paste_context_menu(
    response: &egui::Response,
    deferred_paste: &mut Option<String>,
) {
    response.context_menu(|ui| {
        if secondary_button(ui, &format!("{} Paste", ui_icons::COPY_CLIPBOARD), true).clicked() {
            // Focus the field so the deferred paste targets it on the next frame.
            response.request_focus();

            let from_clipboard = arboard::Clipboard::new()
                .ok()
                .and_then(|mut cb| cb.get_text().ok())
                .filter(|t| !t.is_empty());

            if let Some(text) = from_clipboard {
                *deferred_paste = Some(text);
            } else {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            ui.close_menu();
        }
    });
}

pub(crate) fn draw_precheck_status(
    ui: &mut egui::Ui,
    tool_name: &str,
    ok: bool,
    version: &str,
) {
    let (icon, color, text) = if ok {
        (ICON_OK, Color32::from_rgb(102, 187, 106), "OK")
    } else {
        (ICON_MISSING, Color32::from_rgb(239, 83, 80), "Missing")
    };
    let v = version.trim();
    let body = if ok && !v.is_empty() {
        format!("{icon} {tool_name}: {text} — {v}")
    } else {
        format!("{icon} {tool_name}: {text}")
    };
    let response = ui.label(RichText::new(body).small().color(color));
    if ok && !v.is_empty() {
        response.on_hover_text(v);
    }
}
