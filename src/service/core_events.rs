//! Applies download-queue `UiEvent`s to [`DownloadCore`] so web API and GUI stay in sync.

use std::sync::Arc;

use tokio::runtime::Runtime;
use tokio::sync::broadcast::error::RecvError;

use crate::app::UiEvent;
use crate::app_parsing::parse_speed_eta;
use crate::models::{ItemStatus, QueueItem};
use crate::ytdlp;

use super::core::SharedCore;

pub fn spawn_core_event_loop(runtime: Arc<Runtime>, core: SharedCore) {
    runtime.spawn(async move {
        let mut rx = {
            let guard = core.lock();
            guard.subscribe_events()
        };
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let mut c = core.lock();
                    c.apply_ui_event(ev);
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        }
    });
}

impl super::core::DownloadCore {
    pub(crate) fn apply_ui_event(&mut self, ev: UiEvent) {
        match ev {
            UiEvent::AddResolved { rows, source_line } => {
                self.handle_add_resolved(rows, source_line);
            }
            UiEvent::AddProgress {
                processed,
                total,
                current,
            } => {
                self.add_processed_urls = processed.min(total);
                self.add_total_urls = total;
                self.add_current_url = current;
                self.bump_generation();
            }
            UiEvent::AddDone => {
                self.add_in_progress = false;
                self.add_processed_urls = self.add_total_urls;
                self.add_current_url = None;
                self.maybe_auto_start_downloads();
                self.bump_generation();
            }
            UiEvent::DownloadLine { item_id, line } => {
                self.handle_download_line(item_id, &line);
            }
            UiEvent::DownloadDone {
                item_id,
                ok,
                detail,
            } => {
                self.handle_download_done(item_id, ok, &detail);
            }
            _ => {}
        }
    }

    fn handle_add_resolved(&mut self, rows: Vec<crate::models::VideoPreview>, source_line: String) {
        let Some(iid) = self.pending_resolve_ids.remove(&source_line) else {
            return;
        };
        if let Some(idx) = self.item_idx(iid) {
            self.items.remove(idx);
            self.rebuild_item_index();
        }
        // Pending rows contribute source_line to cached_dedupe_keys at add time; rebuild
        // after removing the placeholder so we do not treat this URL as a duplicate of itself.
        self.rebuild_dedupe_keys_cache();
        let keys = ytdlp::dedupe_previews(&self.cached_dedupe_keys, &rows);
        if keys.is_empty() {
            self.append_log(&format!("No new videos found for: {source_line}"));
            let iid = self.next_item_id;
            self.next_item_id += 1;
            let item = if rows.is_empty() {
                QueueItem {
                    item_id: iid,
                    source_line: source_line.clone(),
                    title: source_line.clone(),
                    error: Some("No preview returned for this URL.".to_owned()),
                    status: ItemStatus::Idle,
                    ..Default::default()
                }
            } else {
                let mut it = QueueItem::from_preview(iid, rows[0].clone());
                it.error = Some(
                    "This video is already in the list (same as a finished or pending item)."
                        .to_owned(),
                );
                it
            };
            self.items.insert(0, item);
        } else {
            for pv in keys {
                let iid = self.next_item_id;
                self.next_item_id += 1;
                if let Some(ref err) = pv.error {
                    self.append_log(&format!("[item {iid}] Metadata fetch failed: {err}"));
                }
                let item = QueueItem::from_preview(iid, pv);
                self.items.insert(0, item);
            }
        }
        self.rebuild_item_index();
        self.invalidate_queue_caches();
        self.update_status();
        self.schedule_queue_save();
        self.maybe_auto_start_downloads();
        self.bump_generation();
    }

    fn handle_download_line(&mut self, item_id: u64, line: &str) {
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        if self.items[idx].status != ItemStatus::Downloading {
            self.set_item_status_at(idx, ItemStatus::Downloading);
        }
        let (pct, size) = ytdlp::parse_progress_line(line);
        let it = &mut self.items[idx];
        if let Some(p) = pct {
            it.percent = p.clamp(0.0, 100.0);
        }
        if let Some(sz) = size {
            it.size_text = sz;
        }
        if let Some((speed, eta)) = parse_speed_eta(line) {
            it.speed_text = speed;
            it.eta_text = eta;
        }
        it.detail = line.chars().take(160).collect();
        if let Some(raw) = ytdlp::parse_output_path_from_download_log_line(line) {
            let raw = raw.to_string_lossy();
            if let Some(path) =
                crate::app::done_file_index::resolve_path_under_output(&self.output_dir, &raw)
            {
                it.local_path = Some(path.to_string_lossy().into_owned());
            }
        }
        self.transfer_totals_dirty = true;
        self.bump_generation();
    }

    fn handle_download_done(&mut self, item_id: u64, ok: bool, detail: &str) {
        if let Some(post_action) = self.cancel_post_actions.remove(&item_id) {
            self.download_cancel_flags.remove(&item_id);
            match post_action {
                super::core::CancelPostAction::Ready => {
                    if let Some(idx) = self.item_idx(item_id) {
                        self.set_item_status_at(idx, ItemStatus::Idle);
                        let it = &mut self.items[idx];
                        it.percent = 0.0;
                        it.speed_text = "-".to_owned();
                        it.eta_text = "-".to_owned();
                        it.detail = "Cancelled (ready)".to_owned();
                    }
                }
                super::core::CancelPostAction::Remove => {
                    let _ = self.remove_item_by_id(item_id);
                }
            }
            self.queue_running = self.queue_running.saturating_sub(1);
            self.schedule_queue_save();
            self.bump_generation();
            return;
        }

        let completed = ok;
        let final_detail = detail.to_owned();
        if let Some(idx) = self.item_idx(item_id) {
            let new_status = if completed {
                ItemStatus::Done
            } else {
                ItemStatus::Failed
            };
            self.items[idx].status = new_status;
            if completed {
                self.items[idx].percent = 100.0;
                self.items[idx].eta_text = "0s".to_owned();
                self.done_file_index.force_refresh();
                self.refresh_done_file_lookup();
                self.bind_local_path_for_item(item_id);
            }
            self.items[idx].detail = final_detail.clone();
        }
        if !completed {
            let summary = final_detail.trim();
            if summary.is_empty() {
                self.append_log(&format!("[item {item_id}] Download failed."));
            } else {
                self.append_log(&format!("[item {item_id}] Download failed: {summary}"));
            }
        }
        self.download_cancel_flags.remove(&item_id);
        self.queue_running = self.queue_running.saturating_sub(1);
        self.update_status();
        self.schedule_queue_save();
        self.bump_generation();
    }

    pub fn maybe_auto_start_downloads(&mut self) {
        if self.downloads_paused || !self.settings.auto_start_downloads {
            return;
        }
        let has_idle = self
            .items
            .iter()
            .any(|x| x.status == ItemStatus::Idle && x.error.is_none());
        if has_idle {
            self.start_downloads();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app_state;
    use crate::models::{QueueItem, VideoPreview};
    use crate::ytdlp::{self, normalize_url_for_dedupe};

    #[test]
    fn dedupe_cache_without_pending_row_allows_resolved_preview() {
        let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let pending = QueueItem::pending_metadata(1, url.to_owned());
        let keys_with_pending = app_state::rebuild_dedupe_keys_set(&[pending]);
        assert!(keys_with_pending.contains(&normalize_url_for_dedupe(url)));

        let keys_empty = app_state::rebuild_dedupe_keys_set(&[]);
        let preview = VideoPreview {
            webpage_url: url.to_owned(),
            video_id: "dQw4w9WgXcQ".to_owned(),
            ..Default::default()
        };
        let out = ytdlp::dedupe_previews(&keys_with_pending, std::slice::from_ref(&preview));
        assert!(
            out.is_empty(),
            "stale cache from pending row must not be used after removal"
        );
        let out = ytdlp::dedupe_previews(&keys_empty, std::slice::from_ref(&preview));
        assert_eq!(out.len(), 1);
    }
}
