//! Synthetic queue benchmarks for large-list performance regressions.
//!
//! Manual GUI profile checklist (set `RUSTDL_PROFILE=1`):
//!
//! 1. Empty queue baseline FPS.
//! 2. Restore/import 200-item queue, list layout, Done group expanded.
//! 3. Active download with log autoscroll and multiple workers.
//!
//! Slow frames (>8ms) log to stderr for `process_events` and `draw_grouped_cards`.

use rustdl::app_state::{
    compute_status_counts, compute_transfer_totals, rebuild_item_index_map, synthetic_queue_items,
};

#[test]
fn synthetic_200_item_status_counts() {
    let items = synthetic_queue_items(200);
    let counts = compute_status_counts(&items);
    assert_eq!(
        counts.resolving
            + counts.ready
            + counts.queued
            + counts.active
            + counts.done
            + counts.failed,
        200
    );
}

#[test]
fn synthetic_200_item_index_rebuild() {
    let items = synthetic_queue_items(200);
    let map = rebuild_item_index_map(&items);
    assert_eq!(map.len(), 200);
    assert_eq!(map.get(&1), Some(&0));
    assert_eq!(map.get(&200), Some(&199));
}

#[test]
fn transfer_totals_empty_when_no_progress_text() {
    let items = synthetic_queue_items(50);
    let totals = compute_transfer_totals(&items);
    assert_eq!(totals.with_known_total, 0);
}
