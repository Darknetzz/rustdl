use std::collections::{HashMap, HashSet};

use crate::models::{ItemStatus, QueueItem};
use crate::ytdlp;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct BatchProgress {
    /// Weighted completion in `[0.0, 1.0]` (active items contribute partial credit).
    pub fraction: f32,
    /// Items included in the batch (excludes download metadata rows still resolving).
    pub total: usize,
    /// Items fully processed (done, failed, or skipped).
    pub finished: usize,
    /// Items currently running (downloading, or queued/downloading for convert).
    pub active: usize,
}

impl BatchProgress {
    pub fn percent(&self) -> f32 {
        (self.fraction * 100.0).clamp(0.0, 100.0)
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }
}

fn download_item_batch_fraction(item: &QueueItem) -> Option<f32> {
    match item.status {
        ItemStatus::Resolving => None,
        ItemStatus::Done | ItemStatus::Failed => Some(1.0),
        ItemStatus::Downloading => Some((item.percent / 100.0).clamp(0.0, 1.0)),
        ItemStatus::Queued | ItemStatus::Idle => Some(0.0),
    }
}

/// Weighted batch progress for the downloader queue (includes partial credit for active rows).
pub fn compute_download_batch_progress(items: &[QueueItem]) -> BatchProgress {
    let mut sum = 0.0f32;
    let mut total = 0usize;
    let mut finished = 0usize;
    let mut active = 0usize;
    for item in items {
        let Some(frac) = download_item_batch_fraction(item) else {
            continue;
        };
        total += 1;
        sum += frac;
        if frac >= 1.0 {
            finished += 1;
        }
        if item.status == ItemStatus::Downloading {
            active += 1;
        }
    }
    BatchProgress {
        fraction: if total > 0 { sum / total as f32 } else { 0.0 },
        total,
        finished,
        active,
    }
}

#[derive(Default, Clone)]
pub struct TransferTotals {
    pub downloaded_bytes: u64,
    pub known_total_bytes: u64,
    pub with_known_total: usize,
}

#[derive(Clone, Copy, Default)]
pub struct StatusCounts {
    pub resolving: usize,
    pub ready: usize,
    pub queued: usize,
    pub active: usize,
    pub done: usize,
    pub failed: usize,
}

pub fn compute_status_counts(items: &[QueueItem]) -> StatusCounts {
    let mut out = StatusCounts::default();
    for it in items {
        inc_status_count(&mut out, it.status);
    }
    out
}

pub fn inc_status_count(counts: &mut StatusCounts, status: ItemStatus) {
    match status {
        ItemStatus::Resolving => counts.resolving += 1,
        ItemStatus::Idle => counts.ready += 1,
        ItemStatus::Queued => counts.queued += 1,
        ItemStatus::Downloading => counts.active += 1,
        ItemStatus::Done => counts.done += 1,
        ItemStatus::Failed => counts.failed += 1,
    }
}

pub fn dec_status_count(counts: &mut StatusCounts, status: ItemStatus) {
    match status {
        ItemStatus::Resolving => counts.resolving = counts.resolving.saturating_sub(1),
        ItemStatus::Idle => counts.ready = counts.ready.saturating_sub(1),
        ItemStatus::Queued => counts.queued = counts.queued.saturating_sub(1),
        ItemStatus::Downloading => counts.active = counts.active.saturating_sub(1),
        ItemStatus::Done => counts.done = counts.done.saturating_sub(1),
        ItemStatus::Failed => counts.failed = counts.failed.saturating_sub(1),
    }
}

pub fn unix_secs_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Updates `status` and maintains `completed_at` when entering or leaving Done.
pub fn transition_queue_item_status(item: &mut QueueItem, new: ItemStatus) {
    let old = item.status;
    if old == new {
        return;
    }
    if new == ItemStatus::Done {
        item.completed_at = Some(unix_secs_now());
    } else if old == ItemStatus::Done {
        item.completed_at = None;
    }
    item.status = new;
}

const QUEUE_ITEM_ERROR_SUMMARY_MAX: usize = 500;

/// Truncates long failure text for the queue row `error` field (full text stays in `detail`).
pub fn queue_item_error_summary(detail: &str) -> String {
    let trimmed = detail.trim();
    if trimmed.len() <= QUEUE_ITEM_ERROR_SUMMARY_MAX {
        return trimmed.to_owned();
    }
    format!(
        "{}…",
        &trimmed[..QUEUE_ITEM_ERROR_SUMMARY_MAX.saturating_sub(1)]
    )
}

/// Maps raw yt-dlp failure text to a short row `error` plus expanded `detail`.
pub fn format_queue_download_failure(raw: &str) -> (String, String) {
    let raw = raw.trim();
    if raw.is_empty() {
        return (String::new(), String::new());
    }
    if let Some(hint) = crate::ytdlp_errors::download_failure_user_hint(raw) {
        let detail = if raw.contains(hint) {
            raw.to_owned()
        } else {
            format!("{raw}\n\n{hint}")
        };
        return (hint.to_owned(), detail);
    }
    (queue_item_error_summary(raw), raw.to_owned())
}

/// Primary failure text for a downloader row (metadata `error`, else failed `detail`).
pub fn queue_item_failure_text(item: &QueueItem) -> Option<&str> {
    if let Some(err) = item.error.as_deref() {
        let t = err.trim();
        if !t.is_empty() {
            return Some(t);
        }
    }
    if item.status == ItemStatus::Failed {
        let d = item.detail.trim();
        if !d.is_empty() {
            return Some(d);
        }
    }
    None
}

/// Sort key for Done rows: newest completion first (left in the card strip).
pub fn done_item_sort_key(item: &QueueItem) -> (u64, u64) {
    let t = item.completed_at.unwrap_or(item.item_id);
    (t, item.item_id)
}

pub fn compute_transfer_totals(items: &[QueueItem]) -> TransferTotals {
    use crate::app_parsing::parse_item_size_text;
    let mut totals = TransferTotals::default();
    for it in items {
        if let Some((downloaded, total)) = parse_item_size_text(&it.size_text) {
            totals.downloaded_bytes += downloaded;
            if let Some(t) = total {
                totals.known_total_bytes += t;
                totals.with_known_total += 1;
            }
        }
    }
    totals
}

pub fn rebuild_item_index_map(items: &[QueueItem]) -> HashMap<u64, usize> {
    let mut map = HashMap::with_capacity(items.len());
    for (i, it) in items.iter().enumerate() {
        map.insert(it.item_id, i);
    }
    map
}

pub fn rebuild_dedupe_keys_set(items: &[QueueItem]) -> HashSet<String> {
    let mut keys = HashSet::new();
    for it in items {
        insert_item_dedupe_keys(&mut keys, it);
    }
    keys.into_iter().filter(|k| !k.is_empty()).collect()
}

fn insert_item_dedupe_keys(keys: &mut HashSet<String>, it: &QueueItem) {
    keys.insert(ytdlp::normalize_url_for_dedupe(&it.source_line));
    if !it.webpage_url.is_empty() {
        keys.insert(ytdlp::normalize_url_for_dedupe(&it.webpage_url));
    }
    if !it.video_id.is_empty() {
        keys.insert(format!("vid:{}", it.video_id));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlLineClass {
    Valid,
    DuplicateInInput,
    DuplicateExisting,
    Invalid,
}

/// Best URL to pass to yt-dlp for a queue row (validated webpage URL, http(s) source line, or YouTube id).
pub fn resolve_item_download_url(item: &QueueItem) -> Option<String> {
    let web = item.webpage_url.trim();
    if is_queueable_http_url(web) {
        return Some(web.to_owned());
    }
    let src = item.source_line.trim();
    if is_queueable_http_url(src) {
        return Some(src.to_owned());
    }
    ytdlp::youtube_video_id_from_item(item)
        .map(|id| format!("https://www.youtube.com/watch?v={id}"))
}

pub fn item_has_redownload_target(item: &QueueItem) -> bool {
    resolve_item_download_url(item).is_some()
}

/// True when `line` is an absolute http(s) URL with a host (rejects `error:`, `help:`, etc.).
pub fn is_queueable_http_url(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    let Ok(url) = url::Url::parse(line) else {
        return false;
    };
    match url.scheme().to_ascii_lowercase().as_str() {
        "http" | "https" => {}
        _ => return false,
    }
    match url.host() {
        Some(url::Host::Domain(d)) => !d.is_empty(),
        Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)) => true,
        None => false,
    }
}

pub fn classify_url_line(
    line: &str,
    seen_in_batch: &mut HashSet<String>,
    existing_keys: &HashSet<String>,
) -> UrlLineClass {
    let line = line.trim();
    if line.is_empty() || !is_queueable_http_url(line) {
        return UrlLineClass::Invalid;
    }
    let normalized = ytdlp::normalize_url_for_dedupe(line);
    if !normalized.is_empty() && existing_keys.contains(&normalized) {
        return UrlLineClass::DuplicateExisting;
    }
    if !normalized.is_empty() && seen_in_batch.contains(&normalized) {
        return UrlLineClass::DuplicateInInput;
    }
    if !normalized.is_empty() {
        seen_in_batch.insert(normalized);
    }
    UrlLineClass::Valid
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UrlLineFilterStats {
    pub accepted: usize,
    pub duplicate_in_input: usize,
    pub duplicate_existing: usize,
    pub invalid: usize,
}

pub fn filter_url_lines_for_queue_add(
    lines: impl IntoIterator<Item = String>,
    existing_keys: &HashSet<String>,
) -> (Vec<String>, UrlLineFilterStats) {
    let mut seen_in_batch = HashSet::new();
    let mut accepted = Vec::new();
    let mut stats = UrlLineFilterStats::default();
    for line in lines {
        let line = line.trim().to_owned();
        if line.is_empty() {
            continue;
        }
        match classify_url_line(&line, &mut seen_in_batch, existing_keys) {
            UrlLineClass::Valid => {
                stats.accepted += 1;
                accepted.push(line);
            }
            UrlLineClass::DuplicateInInput => stats.duplicate_in_input += 1,
            UrlLineClass::DuplicateExisting => stats.duplicate_existing += 1,
            UrlLineClass::Invalid => stats.invalid += 1,
        }
    }
    (accepted, stats)
}

/// Build synthetic queue items for profiling / tests (no network).
pub fn synthetic_queue_items(count: usize) -> Vec<QueueItem> {
    (0..count)
        .map(|i| {
            let id = (i + 1) as u64;
            QueueItem {
                item_id: id,
                source_line: format!("https://example.com/watch?v={id}"),
                video_id: format!("vid{id}"),
                title: format!("Synthetic video {id}"),
                webpage_url: format!("https://example.com/watch?v={id}"),
                status: if i % 5 == 0 {
                    ItemStatus::Done
                } else if i % 7 == 0 {
                    ItemStatus::Failed
                } else {
                    ItemStatus::Idle
                },
                ..Default::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_queue_download_failure_uses_hint_for_generic() {
        let raw = "ERROR: [generic] Unable to extract flashvars; please report this issue";
        let (error, detail) = format_queue_download_failure(raw);
        assert!(error.contains("generic extractor"));
        assert!(detail.contains(raw));
        assert!(detail.contains("generic extractor"));
    }

    #[test]
    fn status_delta_round_trips() {
        let mut counts = StatusCounts::default();
        inc_status_count(&mut counts, ItemStatus::Queued);
        inc_status_count(&mut counts, ItemStatus::Downloading);
        dec_status_count(&mut counts, ItemStatus::Queued);
        assert_eq!(counts.queued, 0);
        assert_eq!(counts.active, 1);
    }

    #[test]
    fn transition_queue_item_status_sets_completed_at() {
        let mut item = QueueItem {
            item_id: 7,
            status: ItemStatus::Downloading,
            ..Default::default()
        };
        transition_queue_item_status(&mut item, ItemStatus::Done);
        assert_eq!(item.status, ItemStatus::Done);
        assert!(item.completed_at.is_some());
        transition_queue_item_status(&mut item, ItemStatus::Idle);
        assert_eq!(item.status, ItemStatus::Idle);
        assert!(item.completed_at.is_none());
    }

    #[test]
    fn done_item_sort_key_prefers_newer_completion() {
        let older = QueueItem {
            item_id: 1,
            completed_at: Some(100),
            status: ItemStatus::Done,
            ..Default::default()
        };
        let newer = QueueItem {
            item_id: 2,
            completed_at: Some(200),
            status: ItemStatus::Done,
            ..Default::default()
        };
        assert!(done_item_sort_key(&newer) > done_item_sort_key(&older));
    }

    #[test]
    fn download_batch_progress_weights_active_percent() {
        let items = vec![
            QueueItem {
                item_id: 1,
                status: ItemStatus::Done,
                ..Default::default()
            },
            QueueItem {
                item_id: 2,
                status: ItemStatus::Downloading,
                percent: 50.0,
                ..Default::default()
            },
            QueueItem {
                item_id: 3,
                status: ItemStatus::Idle,
                ..Default::default()
            },
        ];
        let p = compute_download_batch_progress(&items);
        assert_eq!(p.total, 3);
        assert_eq!(p.finished, 1);
        assert_eq!(p.active, 1);
        assert!((p.fraction - 0.5).abs() < 0.001);
    }

    #[test]
    fn download_batch_progress_excludes_resolving() {
        let items = vec![QueueItem {
            item_id: 1,
            status: ItemStatus::Resolving,
            ..Default::default()
        }];
        assert!(compute_download_batch_progress(&items).is_empty());
    }

    #[test]
    fn synthetic_queue_has_expected_len() {
        assert_eq!(synthetic_queue_items(200).len(), 200);
    }

    #[test]
    fn status_delta_matches_full_recompute() {
        let items = synthetic_queue_items(100);
        let full = compute_status_counts(&items);
        let mut incremental = StatusCounts::default();
        for it in &items {
            inc_status_count(&mut incremental, it.status);
        }
        assert_eq!(incremental.resolving, full.resolving);
        assert_eq!(incremental.ready, full.ready);
        assert_eq!(incremental.queued, full.queued);
        assert_eq!(incremental.active, full.active);
        assert_eq!(incremental.done, full.done);
        assert_eq!(incremental.failed, full.failed);
    }

    #[test]
    fn item_index_map_matches_len() {
        let items = synthetic_queue_items(200);
        assert_eq!(rebuild_item_index_map(&items).len(), 200);
    }

    #[test]
    fn filter_url_lines_skips_queue_duplicates_and_youtube_variants() {
        let items = vec![QueueItem {
            item_id: 1,
            source_line: "https://www.youtube.com/watch?v=abc123".to_owned(),
            status: ItemStatus::Done,
            ..Default::default()
        }];
        let keys = rebuild_dedupe_keys_set(&items);
        let (accepted, stats) = filter_url_lines_for_queue_add(
            vec![
                "https://youtu.be/abc123".to_owned(),
                "https://www.youtube.com/watch?v=abc123".to_owned(),
            ],
            &keys,
        );
        assert!(accepted.is_empty());
        assert_eq!(stats.duplicate_existing, 2);
        assert_eq!(stats.duplicate_in_input, 0);
    }

    #[test]
    fn resolve_item_download_url_prefers_valid_webpage_url() {
        let item = QueueItem {
            webpage_url: "https://www.youtube.com/watch?v=abc123".to_owned(),
            source_line: "https://youtu.be/other".to_owned(),
            ..Default::default()
        };
        assert_eq!(
            resolve_item_download_url(&item).as_deref(),
            Some("https://www.youtube.com/watch?v=abc123")
        );
    }

    #[test]
    fn resolve_item_download_url_skips_invalid_webpage_and_uses_source_line() {
        let item = QueueItem {
            webpage_url: "error: stream did not contain valid UTF-8".to_owned(),
            source_line: "https://www.youtube.com/watch?v=abc123".to_owned(),
            ..Default::default()
        };
        assert_eq!(
            resolve_item_download_url(&item).as_deref(),
            Some("https://www.youtube.com/watch?v=abc123")
        );
    }

    #[test]
    fn resolve_item_download_url_falls_back_to_video_id() {
        let item = QueueItem {
            video_id: "dQw4w9WgXcQ".to_owned(),
            webpage_url: "not a url".to_owned(),
            source_line: "playlist import".to_owned(),
            ..Default::default()
        };
        assert_eq!(
            resolve_item_download_url(&item).as_deref(),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
        );
    }

    #[test]
    fn is_queueable_http_url_rejects_rustc_style_lines() {
        assert!(!is_queueable_http_url(
            "error: could not compile `rustdl` (lib) due to 1 previous error"
        ));
        assert!(!is_queueable_http_url(
            "help: consider changing this to be mutable"
        ));
        assert!(!is_queueable_http_url("mailto:test@example.com"));
        assert!(is_queueable_http_url(
            "https://www.youtube.com/watch?v=abc123"
        ));
        assert!(is_queueable_http_url("http://127.0.0.1:8765/"));
    }

    #[test]
    fn classify_url_line_marks_rustc_output_invalid() {
        let mut seen = HashSet::new();
        let keys = HashSet::new();
        assert_eq!(
            classify_url_line(
                "error: could not compile `rustdl` (lib) due to 1 previous error",
                &mut seen,
                &keys
            ),
            UrlLineClass::Invalid
        );
    }

    #[test]
    fn filter_url_lines_skips_duplicate_within_batch() {
        let keys = HashSet::new();
        let (accepted, stats) = filter_url_lines_for_queue_add(
            vec![
                "https://example.com/a".to_owned(),
                "https://example.com/a".to_owned(),
            ],
            &keys,
        );
        assert_eq!(accepted.len(), 1);
        assert_eq!(stats.duplicate_in_input, 1);
    }
}
