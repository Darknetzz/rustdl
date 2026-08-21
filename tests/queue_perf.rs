//! Manual queue/mirror perf checks (ignored by default).
//!
//! Run with:
//! `cargo test --test queue_perf -- --ignored`
//!
//! Manual GUI profile checklist (set `RUSTDL_PROFILE=1`):
//!
//! 1. Empty queue baseline FPS.
//! 2. Restore/import 200-item queue, list layout, Done group expanded.
//! 3. Active download with log autoscroll and multiple workers.
//!
//! Slow frames (>8ms) log to stderr for `process_events`, `draw_grouped_cards`, and
//! `draw_convert_grouped_cards`.

use rustdl::app_state::{rebuild_item_index_map, synthetic_queue_items};

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
    let core_by_id: HashMap<u64, &QueueItem> =
        core_items.iter().map(|it| (it.item_id, it)).collect();
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
