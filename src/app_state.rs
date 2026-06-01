use std::collections::{HashMap, HashSet};

use crate::models::{ItemStatus, QueueItem};
use crate::ytdlp;

#[derive(Default, Clone)]
pub struct TransferTotals {
    pub downloaded_bytes: u64,
    pub known_total_bytes: u64,
    pub with_known_total: usize,
}

#[derive(Default)]
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
        if it.status == ItemStatus::Resolving {
            continue;
        }
        keys.insert(ytdlp::normalize_url_for_dedupe(&it.source_line));
        if !it.webpage_url.is_empty() {
            keys.insert(ytdlp::normalize_url_for_dedupe(&it.webpage_url));
        }
        if !it.video_id.is_empty() {
            keys.insert(format!("vid:{}", it.video_id));
        }
    }
    keys.into_iter().filter(|k| !k.is_empty()).collect()
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
    fn status_delta_round_trips() {
        let mut counts = StatusCounts::default();
        inc_status_count(&mut counts, ItemStatus::Queued);
        inc_status_count(&mut counts, ItemStatus::Downloading);
        dec_status_count(&mut counts, ItemStatus::Queued);
        assert_eq!(counts.queued, 0);
        assert_eq!(counts.active, 1);
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
}
