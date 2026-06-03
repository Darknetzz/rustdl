//! Pure AV1 queue helpers shared by the desktop GUI, the shared core, and the web API.
//!
//! Nothing here depends on egui so the same logic drives both the windowed app and the
//! headless `--web-only` server.

use std::collections::HashSet;
use std::path::Path;

use crate::app_parsing::human_bytes_ui;
use crate::models::{Av1QueueItem, ItemStatus};

/// A finished item that was intentionally skipped (already AV1, would not shrink, etc.).
pub fn av1_item_is_skipped(item: &Av1QueueItem) -> bool {
    item.status == ItemStatus::Done && item.detail.to_ascii_lowercase().starts_with("skipped")
}

/// Short status label for an AV1 queue row.
pub fn av1_item_status_label(item: &Av1QueueItem) -> &'static str {
    match item.status {
        ItemStatus::Idle => "Ready",
        ItemStatus::Queued => "Queued",
        ItemStatus::Downloading => "Running",
        ItemStatus::Done if av1_item_is_skipped(item) => "Skipped",
        ItemStatus::Done => "Done",
        ItemStatus::Failed => "Failed",
        ItemStatus::Resolving => "Resolving",
    }
}

/// Human-readable savings line for a finished transcode.
pub fn format_av1_saved_detail(input_bytes: u64, output_bytes: u64) -> String {
    if input_bytes == 0 {
        return format!("Output {}", human_bytes_ui(output_bytes));
    }
    if output_bytes <= input_bytes {
        let saved = input_bytes - output_bytes;
        let pct = (saved as f64 / input_bytes as f64) * 100.0;
        format!("Saved {} ({pct:.1}%)", human_bytes_ui(saved))
    } else {
        let growth = output_bytes - input_bytes;
        let grow_pct = (growth as f64 / input_bytes as f64) * 100.0;
        format!("Output +{} (+{grow_pct:.1}%)", human_bytes_ui(growth))
    }
}

/// Aggregated counters used by the batch-summary row in both UIs.
#[derive(Clone, Copy, Debug, Default)]
pub struct Av1BatchSummary {
    pub completed: usize,
    pub completed_input_bytes: u64,
    pub completed_output_bytes: u64,
    pub pending_count: usize,
    pub pending_input_bytes: u64,
}

pub fn compute_av1_batch_summary(items: &[Av1QueueItem]) -> Av1BatchSummary {
    let mut summary = Av1BatchSummary::default();
    for item in items {
        let pending = matches!(
            item.status,
            ItemStatus::Idle | ItemStatus::Queued | ItemStatus::Downloading | ItemStatus::Resolving
        );
        if pending {
            summary.pending_count += 1;
            summary.pending_input_bytes =
                summary.pending_input_bytes.saturating_add(item.input_bytes);
            continue;
        }
        if item.status != ItemStatus::Done || av1_item_is_skipped(item) {
            continue;
        }
        let Some(output_bytes) = item.output_bytes else {
            continue;
        };
        summary.completed += 1;
        summary.completed_input_bytes = summary
            .completed_input_bytes
            .saturating_add(item.input_bytes);
        summary.completed_output_bytes =
            summary.completed_output_bytes.saturating_add(output_bytes);
    }
    summary
}

/// Case/seperator-insensitive key for matching the same source file across input lines.
pub fn normalize_av1_source_key(path: &str) -> String {
    Path::new(path)
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

/// Drops already-scanned lines from a newline-separated input buffer.
pub fn remove_scanned_av1_input_lines(input: &mut String, scanned: &[String]) {
    if scanned.is_empty() {
        return;
    }
    let remove: HashSet<String> = scanned
        .iter()
        .map(|s| normalize_av1_source_key(s))
        .collect();
    let remaining: Vec<String> = input
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| !remove.contains(&normalize_av1_source_key(s)))
        .map(str::to_owned)
        .collect();
    *input = if remaining.is_empty() {
        String::new()
    } else {
        format!("{}\n", remaining.join("\n"))
    };
}

fn format_av1_duration_clock(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn format_av1_rate_display(fps_raw: &str, speed_raw: &str) -> String {
    let mut parts = Vec::new();
    if let Ok(fps) = fps_raw.trim().parse::<f64>() {
        if fps > 0.0 {
            parts.push(format!("{} fps", fps.round() as i64));
        }
    }
    if let Some(speed) = crate::av1_transcode::parse_ffmpeg_speed(speed_raw) {
        parts.push(format!("{speed:.2}x"));
    } else if !speed_raw.trim().is_empty() {
        parts.push(speed_raw.trim().to_owned());
    }
    parts.join(" · ")
}

/// Renders the `progress` detail line shown on running AV1 cards.
pub fn format_av1_progress_detail(
    progress: &str,
    current_secs: Option<f64>,
    total_secs: Option<f64>,
    fps_raw: &str,
    speed_raw: &str,
    percent: Option<f32>,
) -> String {
    let rate = format_av1_rate_display(fps_raw, speed_raw);
    let pct = percent
        .map(|p| format!("{p:.0}%"))
        .unwrap_or_else(|| "…".to_owned());
    let time = match (current_secs, total_secs) {
        (Some(c), Some(t)) => format!(
            "{} / {}",
            format_av1_duration_clock(c),
            format_av1_duration_clock(t)
        ),
        (Some(c), None) => format_av1_duration_clock(c),
        _ => String::new(),
    };
    let rate_part = if rate.is_empty() {
        String::new()
    } else {
        format!(" · {rate}")
    };
    if progress == "end" {
        format!("{pct} · Done{rate_part}")
    } else if time.is_empty() {
        format!("{pct}{rate_part}")
    } else {
        format!("{pct} · {time}{rate_part}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Av1QueueItem;

    #[test]
    fn skipped_detection_matches_skipped_prefix() {
        let item = Av1QueueItem {
            status: ItemStatus::Done,
            detail: "Skipped: already AV1".to_owned(),
            ..Default::default()
        };
        assert!(av1_item_is_skipped(&item));
        assert_eq!(av1_item_status_label(&item), "Skipped");
    }

    #[test]
    fn saved_detail_reports_shrink_and_growth() {
        assert!(format_av1_saved_detail(1000, 400).starts_with("Saved"));
        assert!(format_av1_saved_detail(1000, 1500).starts_with("Output +"));
    }

    #[test]
    fn remove_scanned_lines_is_case_insensitive() {
        let mut input = "D:/Videos/A.mkv\nD:\\Videos\\B.mkv\n".to_owned();
        remove_scanned_av1_input_lines(&mut input, &["d:\\videos\\a.mkv".to_owned()]);
        assert_eq!(input, "D:\\Videos\\B.mkv\n");
    }

    #[test]
    fn progress_detail_includes_percent_and_time() {
        let out = format_av1_progress_detail(
            "continue",
            Some(30.0),
            Some(60.0),
            "30",
            "1.5x",
            Some(50.0),
        );
        assert!(out.contains("50%"));
        assert!(out.contains("0:30 / 1:00"));
        assert!(out.contains("1.50x"));
    }
}
