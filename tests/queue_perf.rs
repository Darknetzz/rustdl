//! Synthetic queue benchmarks for large-list performance regressions.
//!
//! Manual GUI profile checklist (set `RUSTDL_PROFILE=1`):
//!
//! 1. Empty queue baseline FPS.
//! 2. Restore/import 200-item queue, list layout, Done group expanded.
//! 3. Active download with log autoscroll and multiple workers.
//!
//! Slow frames (>8ms) log to stderr for `process_events`, `draw_grouped_cards`, and
//! `draw_convert_grouped_cards`.

use rustdl::app_state::{
    compute_status_counts, compute_transfer_totals, rebuild_item_index_map, synthetic_queue_items,
};
use rustdl::convert_state::{
    compute_convert_status_counts, rebuild_convert_item_index_map, synthetic_convert_items,
};

#[test]
fn synthetic_200_convert_status_counts() {
    let items = synthetic_convert_items(200);
    let counts = compute_convert_status_counts(&items);
    assert_eq!(
        counts.ready
            + counts.queued
            + counts.running
            + counts.done
            + counts.skipped
            + counts.failed,
        200
    );
}

#[test]
fn synthetic_200_convert_index_rebuild() {
    let items = synthetic_convert_items(200);
    let map = rebuild_convert_item_index_map(&items);
    assert_eq!(map.len(), 200);
    assert_eq!(map.get(&1), Some(&0));
    assert_eq!(map.get(&200), Some(&199));
}

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

#[test]
fn synthetic_500_item_index_rebuild() {
    let items = synthetic_queue_items(500);
    let map = rebuild_item_index_map(&items);
    assert_eq!(map.len(), 500);
}

#[test]
#[ignore = "manual perf check; run with `cargo test --ignored synthetic_500_item_index_rebuild_bench`"]
fn synthetic_500_item_index_rebuild_bench() {
    use std::time::Instant;

    let items = synthetic_queue_items(500);
    let t0 = Instant::now();
    for _ in 0..50 {
        let _ = rebuild_item_index_map(&items);
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    eprintln!("rustdl perf: 50x index rebuild at N=500 in {ms:.1}ms");
    assert!(ms < 500.0, "index rebuild too slow: {ms:.1}ms");
}

#[test]
#[ignore = "manual perf check; run with `cargo test --ignored synthetic_500_dirty_mirror_sync_bench`"]
fn synthetic_500_dirty_mirror_sync_bench() {
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    use rustdl::models::QueueItem;

    let app_items = synthetic_queue_items(500);
    let core_items = app_items.clone();
    let dirty: HashSet<u64> = core_items.iter().take(100).map(|it| it.item_id).collect();
    let core_by_id: HashMap<u64, &QueueItem> = core_items.iter().map(|it| (it.item_id, it)).collect();
    let t0 = Instant::now();
    for _ in 0..50 {
        let mut mirror = app_items.clone();
        for app_it in mirror.iter_mut() {
            if dirty.contains(&app_it.item_id) {
                if let Some(core_it) = core_by_id.get(&app_it.item_id) {
                    *app_it = (*core_it).clone();
                }
            }
        }
    }
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    eprintln!("rustdl perf: 50x dirty mirror sync at N=500 in {ms:.1}ms");
    assert!(ms < 500.0, "dirty mirror sync too slow: {ms:.1}ms");
}
