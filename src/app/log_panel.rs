use std::time::SystemTime;

use eframe::egui;
use eframe::egui::{Color32, RichText};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::app_ui::{
    allocate_top_down_rect, button_group, button_toolbar_wrapped, compact_button_group,
    consume_remaining_ui_space, content_width, fill_allocated_rect, finite_ui_span,
    height_to_bottom, left_button_row, persist_resizable_window_size, secondary_button,
    with_full_width,
};
use crate::theme::{log_bg, text_hint, BG_CANVAS, BORDER_PANEL, BORDER_SUBTLE, TEXT_MUTED};
use crate::time_format::{format_relative_ago, log_message_body, split_log_line};
use crate::ui_icons;

use super::{InputLineInfo, InputLineKind, PydlApp};

const MAX_LOG_RENDER_LINES: usize = 320;

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

    pub(crate) fn slug(self) -> &'static str {
        match self {
            LogFilter::All => "all",
            LogFilter::Important => "important",
            LogFilter::Errors => "errors",
        }
    }

    pub(crate) fn from_slug(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "important" => LogFilter::Important,
            "errors" => LogFilter::Errors,
            _ => LogFilter::All,
        }
    }

    pub(crate) fn accepts(self, line: &str) -> bool {
        let body = log_message_body(line);
        match self {
            LogFilter::All => true,
            LogFilter::Errors => is_error_line(body),
            LogFilter::Important => {
                let lower = body.to_ascii_lowercase();
                is_error_line(body)
                    || lower.contains("metadata fetch failed")
                    || lower.contains("download failed")
                    || lower.contains("starting")
                    || lower.contains("started")
                    || lower.contains("completed")
                    || lower.contains("done")
                    || lower.contains("queue")
                    || lower.contains("convert")
                    || lower.contains("skipped")
                    || lower.contains("skip_reason")
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
    let body = log_message_body(line);
    if is_error_line(body) {
        LOG_COLOR_ERROR
    } else if is_warning_line(body) {
        LOG_COLOR_WARN
    } else if is_success_line(body) {
        LOG_COLOR_OK
    } else {
        LOG_COLOR_DIM
    }
}

fn format_log_timestamp_display(line: &str, relative: bool) -> String {
    let (ts, body) = split_log_line(line);
    if ts.is_empty() {
        return line.to_owned();
    }
    if !relative {
        return format!("[{ts}] {body}");
    }
    if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S") {
        use chrono::TimeZone;
        if let Some(local) = chrono::Local.from_local_datetime(&parsed).single() {
            let st: SystemTime = local.into();
            return format!("[{}] {}", format_relative_ago(st), body);
        }
    }
    format!("[{ts}] {body}")
}

fn log_line_widget(
    line: &str,
    color: Color32,
    ui: &egui::Ui,
    relative_ts: bool,
) -> egui::WidgetText {
    let display = format_log_timestamp_display(line, relative_ts);
    let font_px = egui::TextStyle::Small.resolve(ui.style()).size;
    let font_id = egui::FontId::new(font_px, egui::FontFamily::Monospace);
    let fmt = |c: Color32| egui::text::TextFormat {
        font_id: font_id.clone(),
        color: c,
        ..Default::default()
    };
    let (ts, body) = split_log_line(&display);
    let mut job = egui::text::LayoutJob::default();
    if !ts.is_empty() {
        job.append(&format!("[{ts}] "), 0.0, fmt(LOG_COLOR_DIM));
        job.append(body, 0.0, fmt(color));
    } else {
        job.append(&display, 0.0, fmt(color));
    }
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
    pub(super) fn draw_log_height_slider(&mut self, ui: &mut egui::Ui, max_log: f32) -> bool {
        let max_log = max_log.max(80.0).round();
        let mut px = self.settings.log_dock_height.round().clamp(80.0, max_log) as i32;
        let max_i = max_log as i32;
        let changed = ui
            .add(egui::Slider::new(&mut px, 80..=max_i.max(80)).text("px"))
            .changed();
        if changed {
            self.settings.log_dock_height = px as f32;
            self.persist_settings();
        }
        changed
    }

    pub(super) fn draw_logs_window(&mut self, ctx: &egui::Context) {
        if !self.settings.logs_open {
            return;
        }
        let mut open = true;
        let window_id = egui::Id::new("rustdl_log_float_v7");
        let init_id = window_id.with("size_init");
        let needs_default = ctx.data(|d| d.get_temp::<egui::Vec2>(init_id).is_none());
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        let mut window = egui::Window::new("Activity log")
            .id(window_id)
            .open(&mut open)
            .min_width(400.0)
            .min_height(260.0)
            .resizable(true);
        if needs_default {
            window = window.default_size(egui::vec2(
                self.settings.log_float_width,
                self.settings.log_float_height,
            ));
            ctx.data_mut(|d| {
                d.insert_temp(init_id, egui::vec2(1.0, 1.0));
            });
        }
        let response = window.show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let body_h = finite_ui_span(
                ui.clip_rect().height().max(ui.max_rect().height()),
                self.settings.log_float_height,
            )
            .max(260.0);
            let body_w = finite_ui_span(
                ui.clip_rect().width().max(ui.max_rect().width()),
                self.settings.log_float_width,
            )
            .max(400.0);
            allocate_top_down_rect(ui, egui::vec2(body_w, body_h), |ui| {
                egui::Frame::dark_canvas(ui.style())
                    .fill(BG_CANVAS)
                    .stroke(egui::Stroke::new(1.0, BORDER_PANEL))
                    .inner_margin(egui::Margin::same(10.0))
                    .rounding(egui::Rounding::same(8.0))
                    .show(ui, |ui| {
                        fill_allocated_rect(ui);
                        let body_bottom = ui.max_rect().bottom();
                        left_button_row(ui, |ui| {
                            self.draw_log_dock_controls_compact(ui);
                        });
                        ui.add_space(4.0);
                        with_full_width(ui, |ui| {
                            self.draw_activity_log_toolbar_inner(ui, true);
                        });
                        ui.add_space(4.0);
                        let log_h = height_to_bottom(ui, body_bottom).max(80.0);
                        self.draw_activity_log_lines_scroll(ui, log_h);
                    });
                consume_remaining_ui_space(ui);
            });
            consume_remaining_ui_space(ui);
        });
        if let Some(inner) = &response {
            if let Some((w, h)) = persist_resizable_window_size(
                pointer_down,
                inner.response.rect.size(),
                egui::vec2(400.0, 260.0),
                egui::vec2(2400.0, 1600.0),
                (
                    self.settings.log_float_width,
                    self.settings.log_float_height,
                ),
            ) {
                self.settings.log_float_width = w;
                self.settings.log_float_height = h;
                self.persist_settings();
            }
        }
        if !open {
            self.settings.logs_open = false;
            self.persist_settings();
        }
        if response.is_none() && self.settings.logs_open {
            // Window was closed via the title-bar X.
            self.settings.logs_open = false;
            self.persist_settings();
        }
    }

    pub(super) fn draw_log_dock_controls(&mut self, ui: &mut egui::Ui) {
        button_toolbar_wrapped(ui, |ui| self.draw_log_dock_controls_inner(ui, false));
    }

    /// Dock/undock only — show/hide is in the main header.
    pub(super) fn draw_log_dock_controls_compact(&mut self, ui: &mut egui::Ui) {
        self.draw_log_dock_controls_inner(ui, true);
    }

    fn draw_log_dock_controls_inner(&mut self, ui: &mut egui::Ui, compact: bool) {
        if !self.settings.logs_open {
            return;
        }
        let draw = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut crate::app_ui::ButtonGroup<'_>)| {
            if compact {
                compact_button_group(ui, "queue_logs_controls", |g| add(g));
            } else {
                button_group(ui, "queue_logs_controls", |g| add(g));
            }
        };
        draw(ui, &mut |g| {
            let log_dock_label = if self.settings.logs_docked {
                format!("{} Undock log", ui_icons::UNDOCK_LOG)
            } else {
                format!("{} Dock log", ui_icons::DOCK_LOG)
            };
            if g.secondary(&log_dock_label, true)
                .on_hover_text(
                    "Dock the log under the queue in the main window, or show it in a separate window",
                )
                .clicked()
            {
                self.settings.logs_docked = !self.settings.logs_docked;
                self.persist_settings();
            }
        });
    }

    pub(super) fn draw_activity_log_toolbar(&mut self, ui: &mut egui::Ui) {
        self.draw_activity_log_toolbar_inner(ui, false);
    }

    fn draw_activity_log_toolbar_inner(&mut self, ui: &mut egui::Ui, compact: bool) {
        let draw = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut crate::app_ui::ButtonGroup<'_>)| {
            if compact {
                compact_button_group(ui, "log_clear", |g| add(g));
            } else {
                button_group(ui, "log_clear", |g| add(g));
            }
        };
        let mut row = |ui: &mut egui::Ui| {
            draw(ui, &mut |g| {
                if g.danger(&format!("{} Clear log", ui_icons::CLEAR_LOG), true)
                    .clicked()
                {
                    self.clear_activity_log();
                }
            });
            ui.label(RichText::new("Filter").small());
            egui::ComboBox::from_id_salt(if compact {
                "log_filter_compact"
            } else {
                "log_filter"
            })
            .selected_text(self.log_filter.as_str())
            .width(if compact { 88.0 } else { 120.0 })
            .show_ui(ui, |ui| {
                let prev = self.log_filter;
                ui.selectable_value(&mut self.log_filter, LogFilter::All, "All");
                ui.selectable_value(&mut self.log_filter, LogFilter::Important, "Important");
                ui.selectable_value(&mut self.log_filter, LogFilter::Errors, "Errors");
                if self.log_filter != prev {
                    self.persist_ui_prefs();
                }
            });
            if !compact
                && ui
                    .checkbox(&mut self.settings.log_relative_time, "Relative timestamps")
                    .changed()
            {
                self.persist_settings();
            }
            let actions =
                |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut crate::app_ui::ButtonGroup<'_>)| {
                    if compact {
                        compact_button_group(ui, "log_actions", |g| add(g));
                    } else {
                        button_group(ui, "log_actions", |g| add(g));
                    }
                };
            actions(ui, &mut |g| {
                if g.secondary(
                    &format!("{} Copy last error", ui_icons::COPY_CLIPBOARD),
                    true,
                )
                .clicked()
                {
                    if let Some(last) = self
                        .log_lines
                        .iter()
                        .rev()
                        .find(|line| is_error_line(log_message_body(line)))
                    {
                        g.ui().ctx().copy_text(last.clone());
                    }
                }
                if g.secondary(&format!("{} Open log file", ui_icons::OPEN_FILE), true)
                    .clicked()
                {
                    self.open_activity_log_file();
                }
                if !compact
                    && g.secondary(
                        &format!("{} Open config folder", ui_icons::OPEN_FOLDER),
                        true,
                    )
                    .clicked()
                {
                    self.open_config_folder();
                }
            });
        };
        if compact {
            ui.horizontal_wrapped(|ui| row(ui));
        } else {
            button_toolbar_wrapped(ui, row);
        }
    }

    /// Docked under the video queue: placement row, height slider, filter/actions, then lines.
    pub(super) fn draw_docked_log_under_videos(&mut self, ui: &mut egui::Ui, max_log_h: f32) {
        left_button_row(ui, |ui| {
            ui.label(RichText::new("Activity log").small().strong());
            self.draw_log_dock_controls_inner(ui, true);
        });
        let max_log = max_log_h.clamp(80.0, 480.0);
        self.draw_log_height_slider(ui, max_log);
        self.draw_activity_log_toolbar_inner(ui, true);
        let log_h = self.settings.log_dock_height.clamp(80.0, max_log);
        self.draw_activity_log_lines_scroll(ui, log_h);
    }

    /// Scrollable log lines only (toolbar is separate).
    pub(super) fn draw_activity_log_lines_scroll(&mut self, ui: &mut egui::Ui, scroll_h: f32) {
        let scroll_h = finite_ui_span(scroll_h, 80.0).max(60.0);
        let w = content_width(ui).max(1.0);
        let inner_h = (scroll_h - 22.0).max(40.0);
        ui.allocate_ui(egui::vec2(w, scroll_h), |ui| {
            ui.set_min_size(egui::vec2(w, scroll_h));
            egui::Frame::dark_canvas(ui.style())
                .fill(log_bg(&self.settings.theme))
                .stroke(egui::Stroke::new(1.0, BORDER_SUBTLE))
                .inner_margin(egui::Margin::same(10.0))
                .rounding(egui::Rounding::same(6.0))
                .show(ui, |ui| {
                    ui.set_min_height(inner_h);
                    ui.set_width(ui.available_width().max(1.0));
                    if self.log_lines.is_empty() {
                        ui.label(
                            RichText::new("Download activity will appear here.")
                                .small()
                                .color(text_hint(&self.settings.theme)),
                        );
                        return;
                    }
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let relative = self.settings.log_relative_time;
                    let filtered: Vec<&String> = self
                        .log_lines
                        .iter()
                        .filter(|line| self.log_filter.accepts(line))
                        .collect();
                    if filtered.is_empty() {
                        ui.label(
                            RichText::new(format!(
                                "No lines match the \"{}\" filter ({} hidden). Switch to All.",
                                self.log_filter.as_str(),
                                self.log_lines.len()
                            ))
                            .small()
                            .color(text_hint(&self.settings.theme)),
                        );
                        ui.add_space(4.0);
                    }
                    let start = filtered.len().saturating_sub(MAX_LOG_RENDER_LINES);
                    let window = if filtered.is_empty() {
                        &filtered[..]
                    } else {
                        &filtered[start..]
                    };
                    egui::ScrollArea::vertical()
                        .id_salt("rustdl_log_lines_scroll")
                        .max_height(inner_h)
                        .animated(true)
                        .auto_shrink([false, false])
                        .stick_to_bottom(self.settings.autoscroll_log)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width().max(1.0));
                            if start > 0 {
                                ui.label(
                                    RichText::new(format!(
                                        "Showing last {} of {} matching lines",
                                        window.len(),
                                        filtered.len()
                                    ))
                                    .small()
                                    .color(text_hint(&self.settings.theme)),
                                );
                            }
                            for line in window {
                                let color = log_line_color(line);
                                let widget = log_line_widget(line, color, ui, relative);
                                let label = egui::Label::new(widget).wrap().selectable(true);
                                let r = ui.add(label);
                                r.context_menu(|ui| {
                                    button_group(ui, "log_copy_line", |g| {
                                        if g.secondary(
                                            &format!("{} Copy line", ui_icons::COPY_CLIPBOARD),
                                            true,
                                        )
                                        .clicked()
                                        {
                                            g.ui().ctx().copy_text((*line).clone());
                                            g.ui().close_menu();
                                        }
                                    });
                                });
                            }
                        });
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

/// Per-line URL validation preview (monospace, color-coded).
pub(crate) fn draw_input_line_preview(ui: &mut egui::Ui, lines: &[InputLineInfo]) {
    if lines.is_empty() {
        return;
    }
    let max_show = 6usize;
    let font_px = egui::TextStyle::Small.resolve(ui.style()).size;
    let font_id = egui::FontId::new(font_px, egui::FontFamily::Monospace);
    for line in lines.iter().take(max_show) {
        let color = match line.kind {
            InputLineKind::Valid => Color32::from_rgb(102, 187, 106),
            InputLineKind::DuplicateInInput | InputLineKind::DuplicateExisting => {
                Color32::from_rgb(255, 193, 7)
            }
            InputLineKind::Invalid => Color32::from_rgb(239, 83, 80),
        };
        let short: String = line.line.chars().take(72).collect();
        let suffix = if line.line.chars().count() > 72 {
            "…"
        } else {
            ""
        };
        ui.label(
            RichText::new(format!("{short}{suffix}"))
                .font(font_id.clone())
                .color(color),
        );
    }
    if lines.len() > max_show {
        ui.label(
            RichText::new(format!("… and {} more line(s)", lines.len() - max_show))
                .small()
                .color(TEXT_MUTED),
        );
    }
}

pub(crate) fn attach_paste_context_menu(
    response: &egui::Response,
    deferred_paste: &mut Option<String>,
) {
    response.context_menu(|ui| {
        button_group(ui, "paste_ctx", |g| {
            if g.secondary(&format!("{} Paste", ui_icons::COPY_CLIPBOARD), true)
                .clicked()
            {
                response.request_focus();
                let from_clipboard = arboard::Clipboard::new()
                    .ok()
                    .and_then(|mut cb| cb.get_text().ok())
                    .filter(|t| !t.is_empty());
                if let Some(text) = from_clipboard {
                    *deferred_paste = Some(text);
                } else {
                    g.ui()
                        .ctx()
                        .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                }
                g.ui().close_menu();
            }
        });
    });
}

static TOOL_VERSION_DATE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\d{4}[-./]\d{2}[-./]\d{2}").expect("date regex"));
static TOOL_VERSION_N_REV: Lazy<Regex> = Lazy::new(|| Regex::new(r"N-\d+").expect("n-rev regex"));

/// Short label for the header tool strip; prefers build/release date, full string on hover.
pub(crate) fn compact_tool_version_display(version: &str) -> String {
    let t = version.trim();
    if t.is_empty() {
        return String::new();
    }
    let lower = t.to_ascii_lowercase();
    if let Some(idx) = lower.find("built on ") {
        let tail = t[idx + "built on ".len()..].trim();
        if let Some(m) = TOOL_VERSION_DATE.find(tail) {
            return m.as_str().replace('.', "-");
        }
    }
    if let Some(m) = TOOL_VERSION_DATE.find_iter(t).last() {
        return m.as_str().replace('.', "-");
    }
    let mut short = t;
    if let Some((head, _)) = short.split_once(" Copyright") {
        short = head.trim();
    }
    if let Some((head, _)) = short.split_once(" built with") {
        short = head.trim();
    }
    if let Some(m) = TOOL_VERSION_N_REV.find(short) {
        return m.as_str().to_owned();
    }
    let n = short.chars().count();
    if n <= 14 {
        short.to_owned()
    } else {
        short.chars().take(13).collect::<String>() + "…"
    }
}

/// Header control: opens the LAN web UI when the embedded server is running.
pub(crate) fn draw_web_ui_header_button(ui: &mut egui::Ui, running: bool, url: &str) -> bool {
    let host = web_ui_link_host(url);
    let label = if host.is_empty() {
        format!("{} Web UI", ui_icons::WEB_UI)
    } else {
        format!("{} Web UI · {host}", ui_icons::WEB_UI)
    };
    let hover = if running {
        format!("Open LAN web UI in browser\n{url}")
    } else {
        "Web UI is enabled in Settings but the server is not running".to_owned()
    };
    secondary_button(ui, &label, running)
        .on_hover_text(hover)
        .clicked()
        && running
}

fn web_ui_link_host(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    let host = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);
    if host.is_empty() {
        String::new()
    } else {
        host.to_owned()
    }
}

pub(crate) fn draw_precheck_status(
    ui: &mut egui::Ui,
    tool_name: &str,
    ok: bool,
    version: &str,
    compact: bool,
) {
    let (icon, fg, text) = if ok {
        ("✔", Color32::from_rgb(132, 235, 156), "OK")
    } else {
        ("✖", Color32::from_rgb(70, 15, 15), "Missing")
    };
    let v = version.trim();
    let cv = compact_tool_version_display(v);
    let body = if compact {
        if ok {
            format!("{icon} {tool_name}")
        } else {
            format!("{icon} {tool_name}: {text}")
        }
    } else if ok && !cv.is_empty() {
        format!("{icon} {tool_name} {text} · {cv}")
    } else if ok {
        format!("{icon} {tool_name} {text}")
    } else {
        format!("{icon} {tool_name}: {text}")
    };
    let response = ui.add(
        egui::Label::new(crate::app_ui::header_status_rich(body).color(fg).strong())
            .sense(egui::Sense::hover())
            .selectable(false),
    );
    if ok && !v.is_empty() {
        response.on_hover_text(v);
    }
}

#[cfg(test)]
mod version_display_tests {
    use super::compact_tool_version_display;

    #[test]
    fn prefers_calendar_date() {
        assert_eq!(
            compact_tool_version_display("2024.05.25.234532"),
            "2024-05-25"
        );
        assert_eq!(
            compact_tool_version_display(
                "ffmpeg version N-124724-g6f1de91492 Copyright (c) built on 2026-06-02"
            ),
            "2026-06-02"
        );
    }

    #[test]
    fn falls_back_to_n_rev() {
        assert_eq!(
            compact_tool_version_display("ffmpeg version N-124724-g6f1de91492"),
            "N-124724"
        );
    }
}

#[cfg(test)]
mod web_ui_link_tests {
    use super::web_ui_link_host;

    #[test]
    fn host_from_url() {
        assert_eq!(web_ui_link_host("http://127.0.0.1:8765/"), "127.0.0.1:8765");
    }
}
