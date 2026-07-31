//! Pure video converter queue helpers shared by the desktop GUI, the shared core, and the web API.
//!
//! Nothing here depends on egui so the same logic drives both the windowed app and the
//! headless `--web-only` server.

use std::collections::HashSet;
use std::path::Path;

use crate::app_parsing::{human_bytes_ui, reset_convert_item_to_ready};
use std::collections::HashMap;

use crate::app_state::BatchProgress;
use crate::models::{ConvertQueueItem, ItemStatus};
use crate::transcode::{codec_matches_target, normalize_target_codec, target_codec_label};

/// A finished item that was intentionally skipped (already target codec, would not shrink, etc.).
pub fn convert_item_is_skipped(item: &ConvertQueueItem) -> bool {
    item.status == ItemStatus::Done && item.detail.to_ascii_lowercase().starts_with("skipped")
}

/// Reset convert rows matching `matches` back to Ready.
pub fn reset_convert_items_matching(
    items: &mut [ConvertQueueItem],
    mut matches: impl FnMut(&ConvertQueueItem) -> bool,
) -> usize {
    let mut count = 0usize;
    for item in items.iter_mut() {
        if matches(item) {
            reset_convert_item_to_ready(item);
            count += 1;
        }
    }
    count
}

/// Move skipped rows back to Ready so they can be encoded again (e.g. after lowering min shrink %).
pub fn reset_skipped_convert_items(items: &mut [ConvertQueueItem]) -> usize {
    reset_convert_items_matching(items, convert_item_is_skipped)
}

/// Move failed rows back to Ready so they can be encoded again.
pub fn reset_failed_convert_items(items: &mut [ConvertQueueItem]) -> usize {
    reset_convert_items_matching(items, |item| item.status == ItemStatus::Failed)
}

/// Move failed rows among `ids` back to Ready.
pub fn reset_failed_convert_items_by_ids(items: &mut [ConvertQueueItem], ids: &[u64]) -> usize {
    if ids.is_empty() {
        return 0;
    }
    let id_set: HashSet<u64> = ids.iter().copied().collect();
    reset_convert_items_matching(items, |item| {
        item.status == ItemStatus::Failed && id_set.contains(&item.item_id)
    })
}

/// True when the queue row points at a path that is not an existing file on disk.
pub fn convert_source_path_missing(source_path: &str) -> bool {
    let p = Path::new(source_path.trim());
    !p.is_file()
}

/// Local file / folder targets for Open actions (desktop GUI and LAN web UI).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConvertOpenTargets {
    pub file: Option<std::path::PathBuf>,
    pub folder: Option<std::path::PathBuf>,
}

/// Prefer encoded output when done; otherwise the source file; folder-only when files are missing.
pub fn convert_item_open_targets(item: &ConvertQueueItem) -> ConvertOpenTargets {
    let mut out = ConvertOpenTargets::default();
    let done = item.status == ItemStatus::Done && !convert_item_is_skipped(item);

    if done {
        let output = Path::new(item.output_path.trim());
        if output.is_file() {
            out.file = Some(output.to_path_buf());
            out.folder = output.parent().map(|p| p.to_path_buf());
            return out;
        }
    }

    let source = Path::new(item.source_path.trim());
    if !item.source_missing && source.is_file() {
        out.file = Some(source.to_path_buf());
        out.folder = source.parent().map(|p| p.to_path_buf());
        return out;
    }

    if done {
        if let Some(parent) = Path::new(item.output_path.trim()).parent() {
            if parent.is_dir() {
                out.folder = Some(parent.to_path_buf());
                return out;
            }
        }
    }

    if let Some(parent) = source.parent() {
        if parent.is_dir() {
            out.folder = Some(parent.to_path_buf());
        }
    }
    out
}

/// Path to stream in the LAN web UI (output when done, else source).
pub fn convert_item_playable_path(item: &ConvertQueueItem) -> Option<std::path::PathBuf> {
    let path = convert_item_open_targets(item).file?;
    if local_path_is_streamable(&path) {
        Some(path)
    } else {
        None
    }
}

/// `video` or `audio` when the path uses a browser-streamable container.
pub fn convert_item_playable_kind(item: &ConvertQueueItem) -> Option<&'static str> {
    let path = convert_item_playable_path(item)?;
    local_path_playable_kind(&path)
}

fn local_path_is_streamable(path: &Path) -> bool {
    local_path_playable_kind(path).is_some()
}

fn local_path_playable_kind(path: &Path) -> Option<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "webm" | "mkv" | "mov" | "m4v" | "avi" => Some("video"),
        "mp3" | "m4a" | "opus" | "ogg" | "flac" | "wav" | "aac" => Some("audio"),
        _ => None,
    }
}

/// Per-status counters for the converter queue (includes skipped as a Done subset).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConvertStatusCounts {
    pub ready: usize,
    pub queued: usize,
    pub running: usize,
    pub done: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub fn compute_convert_status_counts(items: &[ConvertQueueItem]) -> ConvertStatusCounts {
    let mut counts = ConvertStatusCounts::default();
    for item in items {
        if convert_item_is_skipped(item) {
            counts.skipped += 1;
            continue;
        }
        match item.status {
            ItemStatus::Idle => counts.ready += 1,
            ItemStatus::Queued => counts.queued += 1,
            ItemStatus::Downloading => counts.running += 1,
            ItemStatus::Done => counts.done += 1,
            ItemStatus::Failed => counts.failed += 1,
            ItemStatus::Resolving => {}
        }
    }
    counts
}

pub fn rebuild_convert_item_index_map(items: &[ConvertQueueItem]) -> HashMap<u64, usize> {
    let mut map = HashMap::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        map.insert(item.item_id, idx);
    }
    map
}

/// Synthetic converter rows for performance regression tests.
pub fn synthetic_convert_items(count: usize) -> Vec<ConvertQueueItem> {
    (0..count)
        .map(|i| {
            let item_id = (i + 1) as u64;
            ConvertQueueItem {
                item_id,
                source_path: format!(r"C:\videos\clip_{item_id}.mp4"),
                output_path: format!(r"C:\out\clip_{item_id}.mkv"),
                status: match i % 5 {
                    0 => ItemStatus::Idle,
                    1 => ItemStatus::Queued,
                    2 => ItemStatus::Downloading,
                    3 => ItemStatus::Done,
                    _ => ItemStatus::Failed,
                },
                percent: if i % 5 == 2 { 42.0 } else { 0.0 },
                detail: String::new(),
                input_bytes: 1_000_000,
                output_bytes: if i % 5 == 3 { Some(400_000) } else { None },
                ..Default::default()
            }
        })
        .collect()
}

/// Pending row that will be skipped at encode time (already target codec, re-encode disabled).
pub fn convert_item_will_skip_already_target(
    item: &ConvertQueueItem,
    reencode_target: bool,
    target_codec: &str,
) -> bool {
    if reencode_target {
        return false;
    }
    if !matches!(
        item.status,
        ItemStatus::Idle | ItemStatus::Queued | ItemStatus::Resolving
    ) {
        return false;
    }
    codec_matches_target(&item.video_codec, target_codec)
}

/// Short status label for a converter queue row.
pub fn convert_item_status_label(item: &ConvertQueueItem) -> &'static str {
    match item.status {
        ItemStatus::Idle => "Ready",
        ItemStatus::Queued => "Queued",
        ItemStatus::Downloading => "Running",
        ItemStatus::Done if convert_item_is_skipped(item) => "Skipped",
        ItemStatus::Done => "Done",
        ItemStatus::Failed => "Failed",
        ItemStatus::Resolving => "Resolving",
    }
}

/// Human-readable savings line for a finished transcode.
pub fn format_convert_saved_detail(input_bytes: u64, output_bytes: u64) -> String {
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

/// Batch summary savings/growth line (mirrors [`format_convert_saved_detail`] for totals).
pub fn format_convert_batch_saved_line(input_bytes: u64, output_bytes: u64) -> String {
    if input_bytes == 0 {
        return format!("output {}", human_bytes_ui(output_bytes));
    }
    if output_bytes <= input_bytes {
        let saved = input_bytes - output_bytes;
        let pct = (saved as f64 / input_bytes as f64) * 100.0;
        format!("saved {} ({pct:.1}%)", human_bytes_ui(saved))
    } else {
        let growth = output_bytes - input_bytes;
        let grow_pct = (growth as f64 / input_bytes as f64) * 100.0;
        format!("output +{} (+{grow_pct:.1}%)", human_bytes_ui(growth))
    }
}

/// True when batch totals grew rather than shrank.
pub fn convert_batch_totals_grew(input_bytes: u64, output_bytes: u64) -> bool {
    output_bytes > input_bytes && input_bytes > 0
}

/// Aggregated counters used by the batch-summary row in both UIs.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConvertBatchSummary {
    pub completed: usize,
    pub completed_input_bytes: u64,
    pub completed_output_bytes: u64,
    pub pending_count: usize,
    pub pending_input_bytes: u64,
}

pub fn compute_convert_batch_summary(items: &[ConvertQueueItem]) -> ConvertBatchSummary {
    let mut summary = ConvertBatchSummary::default();
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
        if item.status != ItemStatus::Done || convert_item_is_skipped(item) {
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

fn convert_item_batch_fraction(item: &ConvertQueueItem) -> f32 {
    if convert_item_is_skipped(item) {
        return 1.0;
    }
    match item.status {
        ItemStatus::Done | ItemStatus::Failed => 1.0,
        ItemStatus::Downloading => (item.percent / 100.0).clamp(0.0, 1.0),
        ItemStatus::Queued | ItemStatus::Idle | ItemStatus::Resolving => 0.0,
    }
}

/// Weighted batch progress for the converter queue (includes partial credit for running encodes).
pub fn compute_convert_batch_progress(items: &[ConvertQueueItem]) -> BatchProgress {
    let mut sum = 0.0f32;
    let mut total = 0usize;
    let mut finished = 0usize;
    let mut active = 0usize;
    for item in items {
        let frac = convert_item_batch_fraction(item);
        total += 1;
        sum += frac;
        if frac >= 1.0 {
            finished += 1;
        }
        match item.status {
            ItemStatus::Downloading | ItemStatus::Queued => active += 1,
            _ => {}
        }
    }
    BatchProgress {
        fraction: if total > 0 { sum / total as f32 } else { 0.0 },
        total,
        finished,
        active,
    }
}

/// Case/separator-insensitive key for matching the same source file across input lines.
pub fn normalize_convert_source_key(path: &str) -> String {
    Path::new(path)
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

/// Drops already-scanned lines from a newline-separated input buffer.
pub fn remove_scanned_convert_input_lines(input: &mut String, scanned: &[String]) {
    if scanned.is_empty() {
        return;
    }
    let remove: HashSet<String> = scanned
        .iter()
        .map(|s| normalize_convert_source_key(s))
        .collect();
    let remaining: Vec<String> = input
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| !remove.contains(&normalize_convert_source_key(s)))
        .map(str::to_owned)
        .collect();
    *input = if remaining.is_empty() {
        String::new()
    } else {
        format!("{}\n", remaining.join("\n"))
    };
}

fn format_convert_duration_clock(secs: f64) -> String {
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

fn format_convert_rate_display(fps_raw: &str, speed_raw: &str) -> String {
    let mut parts = Vec::new();
    if let Ok(fps) = fps_raw.trim().parse::<f64>() {
        if fps > 0.0 {
            parts.push(format!("{} fps", fps.round() as i64));
        }
    }
    if let Some(speed) = crate::transcode::parse_ffmpeg_speed(speed_raw) {
        parts.push(format!("{speed:.2}x"));
    } else if !speed_raw.trim().is_empty() {
        parts.push(speed_raw.trim().to_owned());
    }
    parts.join(" · ")
}

/// Renders the `progress` detail line shown on running converter cards.
pub fn format_convert_progress_detail(
    progress: &str,
    current_secs: Option<f64>,
    total_secs: Option<f64>,
    fps_raw: &str,
    speed_raw: &str,
    percent: Option<f32>,
) -> String {
    let rate = format_convert_rate_display(fps_raw, speed_raw);
    let pct = percent
        .map(|p| format!("{p:.0}%"))
        .unwrap_or_else(|| "…".to_owned());
    let time = match (current_secs, total_secs) {
        (Some(c), Some(t)) => format!(
            "{} / {}",
            format_convert_duration_clock(c),
            format_convert_duration_clock(t)
        ),
        (Some(c), None) => format_convert_duration_clock(c),
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

/// Label for skip hint on queue cards.
pub fn convert_skip_hint_label(target_codec: &str) -> String {
    format!(
        "Will skip · already {}",
        target_codec_label(normalize_target_codec(target_codec))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ConvertQueueItem;

    #[test]
    fn will_skip_already_target_when_reencode_disabled() {
        let item = ConvertQueueItem {
            status: ItemStatus::Idle,
            video_codec: "av1".to_owned(),
            ..Default::default()
        };
        assert!(convert_item_will_skip_already_target(&item, false, "av1"));
        assert!(!convert_item_will_skip_already_target(&item, true, "av1"));
        assert!(!convert_item_will_skip_already_target(&item, false, "hevc"));
    }

    #[test]
    fn open_targets_prefers_output_when_done() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("in.mkv");
        let out = dir.path().join("out.mp4");
        std::fs::write(&src, b"x").unwrap();
        std::fs::write(&out, b"y").unwrap();
        let item = ConvertQueueItem {
            status: ItemStatus::Done,
            source_path: src.display().to_string(),
            output_path: out.display().to_string(),
            ..Default::default()
        };
        let targets = convert_item_open_targets(&item);
        assert_eq!(targets.file.as_deref(), Some(out.as_path()));
    }

    #[test]
    fn playable_path_uses_source_when_not_done() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("clip.mp4");
        std::fs::write(&src, b"x").unwrap();
        let item = ConvertQueueItem {
            status: ItemStatus::Idle,
            source_path: src.display().to_string(),
            ..Default::default()
        };
        assert_eq!(
            convert_item_playable_path(&item).as_deref(),
            Some(src.as_path())
        );
        assert_eq!(convert_item_playable_kind(&item), Some("video"));
    }

    #[test]
    fn skipped_detection_matches_skipped_prefix() {
        let item = ConvertQueueItem {
            status: ItemStatus::Done,
            detail: "Skipped: already AV1".to_owned(),
            ..Default::default()
        };
        assert!(convert_item_is_skipped(&item));
        assert_eq!(convert_item_status_label(&item), "Skipped");
    }

    #[test]
    fn reset_skipped_moves_rows_to_ready() {
        let mut items = vec![
            ConvertQueueItem {
                item_id: 1,
                status: ItemStatus::Done,
                detail: "Skipped: estimated output would not shrink by at least 50%".to_owned(),
                input_bytes: 1_000_000,
                ..Default::default()
            },
            ConvertQueueItem {
                item_id: 2,
                status: ItemStatus::Done,
                detail: "Saved 500 KiB (50.0%)".to_owned(),
                ..Default::default()
            },
        ];
        assert_eq!(reset_skipped_convert_items(&mut items), 1);
        assert_eq!(items[0].status, ItemStatus::Idle);
        assert!(items[0].detail.starts_with("Ready"));
        assert_eq!(items[1].status, ItemStatus::Done);
    }

    #[test]
    fn reset_failed_moves_rows_to_ready() {
        let mut items = vec![
            ConvertQueueItem {
                item_id: 1,
                status: ItemStatus::Failed,
                detail: "ffmpeg failed with status exit code -1".to_owned(),
                input_bytes: 2_000_000,
                ..Default::default()
            },
            ConvertQueueItem {
                item_id: 2,
                status: ItemStatus::Done,
                detail: "Saved 100 KiB".to_owned(),
                ..Default::default()
            },
            ConvertQueueItem {
                item_id: 3,
                status: ItemStatus::Failed,
                detail: "Invalid data found".to_owned(),
                ..Default::default()
            },
        ];
        assert_eq!(reset_failed_convert_items(&mut items), 2);
        assert_eq!(items[0].status, ItemStatus::Idle);
        assert!(items[0].detail.starts_with("Ready"));
        assert_eq!(items[1].status, ItemStatus::Done);
        assert_eq!(items[2].status, ItemStatus::Idle);
    }

    #[test]
    fn reset_failed_by_ids_only_touches_selected_failed() {
        let mut items = vec![
            ConvertQueueItem {
                item_id: 1,
                status: ItemStatus::Failed,
                detail: "fail a".to_owned(),
                ..Default::default()
            },
            ConvertQueueItem {
                item_id: 2,
                status: ItemStatus::Failed,
                detail: "fail b".to_owned(),
                ..Default::default()
            },
            ConvertQueueItem {
                item_id: 3,
                status: ItemStatus::Idle,
                detail: "Ready".to_owned(),
                ..Default::default()
            },
        ];
        assert_eq!(reset_failed_convert_items_by_ids(&mut items, &[2, 3]), 1);
        assert_eq!(items[0].status, ItemStatus::Failed);
        assert_eq!(items[1].status, ItemStatus::Idle);
        assert_eq!(items[2].status, ItemStatus::Idle);
    }

    #[test]
    fn convert_batch_progress_weights_running_percent() {
        let items = vec![
            ConvertQueueItem {
                item_id: 1,
                status: ItemStatus::Done,
                output_bytes: Some(500),
                ..Default::default()
            },
            ConvertQueueItem {
                item_id: 2,
                status: ItemStatus::Downloading,
                percent: 40.0,
                ..Default::default()
            },
            ConvertQueueItem {
                item_id: 3,
                status: ItemStatus::Queued,
                ..Default::default()
            },
        ];
        let p = compute_convert_batch_progress(&items);
        assert_eq!(p.total, 3);
        assert_eq!(p.finished, 1);
        assert_eq!(p.active, 2);
        assert!((p.fraction - 0.466666).abs() < 0.001);
    }

    #[test]
    fn saved_detail_reports_shrink_and_growth() {
        assert!(format_convert_saved_detail(1000, 400).starts_with("Saved"));
        assert!(format_convert_saved_detail(1000, 1500).starts_with("Output +"));
    }

    #[test]
    fn batch_saved_line_reports_growth_when_output_larger() {
        let line = format_convert_batch_saved_line(5_700_000_000, 9_800_000_000);
        assert!(line.starts_with("output +"));
        assert!(convert_batch_totals_grew(5_700_000_000, 9_800_000_000));
        let shrink = format_convert_batch_saved_line(9_800_000_000, 5_700_000_000);
        assert!(shrink.starts_with("saved "));
    }

    #[test]
    fn remove_scanned_lines_is_case_insensitive() {
        let mut input = "D:/Videos/A.mkv\nD:\\Videos\\B.mkv\n".to_owned();
        remove_scanned_convert_input_lines(&mut input, &["d:\\videos\\a.mkv".to_owned()]);
        assert_eq!(input, "D:\\Videos\\B.mkv\n");
    }

    #[test]
    fn progress_detail_includes_percent_and_time() {
        let out = format_convert_progress_detail(
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
