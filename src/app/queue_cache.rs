use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::app_state::{self, StatusCounts, TransferTotals};
use crate::models::ItemStatus;

use super::PydlApp;

const TRANSFER_TOTALS_REFRESH: Duration = Duration::from_millis(500);
pub(super) const MAX_TEXTURES: usize = 128;
pub(super) const THUMBNAIL_QUEUE_SOFT_CAP: usize = 50;
pub(super) const THUMBNAIL_DECODE_MAX_WIDTH: u32 = 320;

impl PydlApp {
    pub(super) fn rebuild_item_index(&mut self) {
        self.item_index_by_id = app_state::rebuild_item_index_map(&self.items);
    }

    pub(super) fn item_idx(&self, item_id: u64) -> Option<usize> {
        self.item_index_by_id.get(&item_id).copied()
    }

    pub(super) fn rebuild_dedupe_keys_cache(&mut self) {
        self.cached_dedupe_keys = app_state::rebuild_dedupe_keys_set(&self.items);
    }

    pub(super) fn dedupe_keys(&self) -> &HashSet<String> {
        &self.cached_dedupe_keys
    }

    pub(super) fn invalidate_queue_caches(&mut self) {
        self.rebuild_item_index();
        self.rebuild_dedupe_keys_cache();
        self.transfer_totals_dirty = true;
    }

    pub(super) fn apply_status_delta(&mut self, old: ItemStatus, new: ItemStatus) {
        if old == new {
            return;
        }
        app_state::dec_status_count(&mut self.status_counts, old);
        app_state::inc_status_count(&mut self.status_counts, new);
        self.sync_status_fields_from_counts();
    }

    pub(super) fn on_item_removed(&mut self, item: &QueueItem) {
        app_state::dec_status_count(&mut self.status_counts, item.status);
        self.sync_status_fields_from_counts();
        self.invalidate_queue_caches();
    }

    pub(super) fn on_item_inserted(&mut self, item: &QueueItem) {
        app_state::inc_status_count(&mut self.status_counts, item.status);
        self.sync_status_fields_from_counts();
        self.invalidate_queue_caches();
    }

    pub(super) fn recompute_status(&mut self) {
        self.status_counts = app_state::compute_status_counts(&self.items);
        self.sync_status_fields_from_counts();
    }

    pub(super) fn sync_status_fields_from_counts(&mut self) {
        self.status_resolving = self.status_counts.resolving;
        self.status_ready = self.status_counts.ready;
        self.status_queued = self.status_counts.queued;
        self.status_active = self.status_counts.active;
        self.status_done = self.status_counts.done;
        self.status_failed = self.status_counts.failed;
    }

    pub(super) fn update_status(&mut self) {
        self.recompute_status();
    }

    pub(super) fn compute_transfer_totals(&self) -> TransferTotals {
        app_state::compute_transfer_totals(&self.items)
    }

    pub(super) fn transfer_totals(&mut self) -> TransferTotals {
        let downloading =
            self.status_active > 0 || self.status_queued > 0 || self.queue_running > 0;
        let now = Instant::now();
        let stale = self
            .last_transfer_totals_at
            .is_none_or(|t| now.saturating_duration_since(t) >= TRANSFER_TOTALS_REFRESH);
        if self.transfer_totals_dirty || (downloading && stale) {
            self.cached_transfer_totals = self.compute_transfer_totals();
            self.transfer_totals_dirty = false;
            self.last_transfer_totals_at = Some(now);
        }
        self.cached_transfer_totals.clone()
    }

    pub(super) fn set_item_status_at(&mut self, idx: usize, new: ItemStatus) {
        let old = self.items[idx].status;
        if old != new {
            self.items[idx].status = new;
            self.apply_status_delta(old, new);
        }
    }

    pub(super) fn set_item_status_by_id(&mut self, item_id: u64, new: ItemStatus) -> bool {
        let Some(idx) = self.item_idx(item_id) else {
            return false;
        };
        self.set_item_status_at(idx, new);
        true
    }

    pub(super) fn mark_transfer_totals_dirty(&mut self) {
        self.transfer_totals_dirty = true;
    }

    pub(super) fn evict_textures_if_needed(&mut self) {
        if self.textures.len() <= MAX_TEXTURES {
            return;
        }
        let mut done_ids: Vec<u64> = self
            .items
            .iter()
            .filter(|it| it.status == ItemStatus::Done)
            .map(|it| it.item_id)
            .collect();
        done_ids.sort_unstable();
        while self.textures.len() > MAX_TEXTURES {
            let Some(id) = done_ids.first().copied() else {
                break;
            };
            done_ids.remove(0);
            self.textures.remove(&id);
            self.thumbnail_attempted.remove(&id);
            self.thumbnail_inflight.remove(&id);
        }
    }

    pub(super) fn should_poll_done_lookup(&self) -> bool {
        self.status_done > 0 || self.items.iter().any(|it| it.status == ItemStatus::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state;
    use crate::models::{ItemStatus, QueueItem};

    fn sample_item(id: u64, status: ItemStatus) -> QueueItem {
        QueueItem {
            item_id: id,
            status,
            ..Default::default()
        }
    }

    #[test]
    fn item_index_map_tracks_order() {
        let items = vec![
            sample_item(10, ItemStatus::Idle),
            sample_item(20, ItemStatus::Done),
        ];
        let map = app_state::rebuild_item_index_map(&items);
        assert_eq!(map.get(&10), Some(&0));
        assert_eq!(map.get(&20), Some(&1));
    }

    #[test]
    fn dedupe_keys_skips_resolving() {
        let items = vec![
            sample_item(1, ItemStatus::Resolving),
            QueueItem {
                item_id: 2,
                source_line: "https://youtube.com/watch?v=abc".to_owned(),
                status: ItemStatus::Idle,
                ..Default::default()
            },
        ];
        let keys = app_state::rebuild_dedupe_keys_set(&items);
        assert!(!keys.is_empty());
    }
}
