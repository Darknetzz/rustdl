//! Shared log-line helpers and header/input chrome (desktop activity log is external-only).

use eframe::egui;
use eframe::egui::{Color32, RichText};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::app_ui::{button_group, secondary_button};
use crate::theme::TEXT_MUTED;
use crate::time_format::log_message_body;
use crate::ui_icons;

use super::{InputLineInfo, InputLineKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogFilter {
    All,
    Important,
    Errors,
}

impl LogFilter {
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
}

pub(crate) use crate::log_filter::is_error_line;

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
