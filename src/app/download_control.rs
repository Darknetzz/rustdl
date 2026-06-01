use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use url::Url;

use crate::app_state;
use crate::models::{ItemStatus, QueueItem};
use crate::ytdlp;
use crate::ytdlp_download_args::output_filename_template;

use super::background_spawn;
use super::{CancelPostAction, PydlApp};

impl PydlApp {
    pub(super) fn pause_all_downloads(&mut self) {
        if self.downloads_paused {
            return;
        }
        self.downloads_paused = true;
        self.cancel_all_active(CancelPostAction::Ready);
        self.append_log("Downloads paused (active items moved back to ready).");
    }

    pub(super) fn resume_all_downloads(&mut self) {
        if !self.downloads_paused {
            return;
        }
        self.downloads_paused = false;
        self.session_complete_notified = false;
        self.append_log("Downloads resumed.");
        self.start_downloads();
    }

    pub(super) fn maybe_auto_start_downloads(&mut self) {
        if self.downloads_paused || !self.settings.auto_start_downloads {
            return;
        }
        let has_idle_items = self
            .items
            .iter()
            .any(|x| x.status == ItemStatus::Idle && x.error.is_none());
        if has_idle_items {
            self.start_downloads();
        }
    }

    fn collect_idle_download_item_ids(&self) -> Vec<u64> {
        let mut ids: Vec<(u64, u64)> = self
            .items
            .iter()
            .filter(|it| it.status == ItemStatus::Idle && it.error.is_none())
            .map(|it| {
                let order = if it.sort_order == 0 {
                    it.item_id
                } else {
                    it.sort_order
                };
                (order, it.item_id)
            })
            .collect();
        ids.sort_by_key(|(order, _)| *order);
        ids.into_iter().map(|(_, id)| id).collect()
    }

    pub(super) fn spawn_download_workers(&mut self, pending_ids: Vec<u64>) {
        if self.downloads_paused {
            self.append_log("Downloads are paused. Click Resume to continue.");
            return;
        }
        if pending_ids.is_empty() {
            self.append_log("Nothing to download.");
            return;
        }
        self.session_complete_notified = false;

        for id in &pending_ids {
            if let Some(idx) = self.item_idx(*id) {
                self.set_item_status_at(idx, ItemStatus::Queued);
                self.items[idx].detail = "Queued".to_owned();
            }
        }
        self.update_status();
        self.schedule_queue_save();

        let mut groups = vec![Vec::<u64>::new(); self.worker_count.max(1)];
        let groups_len = groups.len();
        for (idx, iid) in pending_ids.into_iter().enumerate() {
            groups[idx % groups_len].push(iid);
        }
        let download_args = self.download_extra_args();
        let yt_dlp_bin = self.yt_dlp_bin();
        let ffmpeg_bin = self.ffmpeg_bin();
        let output_template = output_filename_template(&self.settings);

        for ids in groups.into_iter().filter(|g| !g.is_empty()) {
            self.queue_running += 1;
            let urls = ids
                .iter()
                .filter_map(|iid| {
                    self.items.iter().find(|x| x.item_id == *iid).map(|x| {
                        let cancel_flag = self
                            .download_cancel_flags
                            .entry(*iid)
                            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
                            .clone();
                        (
                            *iid,
                            x.webpage_url.clone(),
                            x.source_line.clone(),
                            cancel_flag,
                        )
                    })
                })
                .collect::<Vec<_>>();
            background_spawn::spawn_download_worker(
                &self.runtime,
                &self.tx,
                self.output_dir.clone(),
                output_template.clone(),
                download_args.clone(),
                yt_dlp_bin.clone(),
                ffmpeg_bin.clone(),
                urls,
            );
        }
    }

    pub(super) fn start_downloads(&mut self) {
        if self.downloads_paused {
            self.append_log("Downloads are paused. Click Resume first.");
            return;
        }
        if self.items.is_empty() {
            self.append_log("Add URLs first.");
            return;
        }
        if !Path::new(&self.output_dir).is_dir() {
            self.append_log("Choose a valid output folder.");
            return;
        }
        self.persist_settings();
        let pending_ids = self.collect_idle_download_item_ids();
        self.spawn_download_workers(pending_ids);
    }

    pub(super) fn remove_item_by_id(&mut self, item_id: u64) -> bool {
        let Some(idx) = self.item_idx(item_id) else {
            return false;
        };
        if self.items[idx].status == ItemStatus::Resolving {
            self.pending_resolve_ids.retain(|_, iid| *iid != item_id);
        }
        let item = self.items[idx].clone();
        app_state::dec_status_count(&mut self.status_counts, item.status);
        self.sync_status_fields_from_counts();
        self.items.remove(idx);
        self.textures.remove(&item_id);
        self.thumbnail_attempted.remove(&item_id);
        self.thumbnail_inflight.remove(&item_id);
        self.download_cancel_flags.remove(&item_id);
        self.cancel_post_actions.remove(&item_id);
        self.invalidate_queue_caches();
        true
    }

    pub(super) fn request_cancel_item(&mut self, item_id: u64, post_action: CancelPostAction) {
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        match self.items[idx].status {
            ItemStatus::Queued => {
                self.cancel_post_actions.remove(&item_id);
                if matches!(post_action, CancelPostAction::Remove) {
                    let _ = self.remove_item_by_id(item_id);
                    self.append_log(&format!(
                        "[item {item_id}] Cancelled and removed from queue."
                    ));
                } else {
                    self.set_item_status_at(idx, ItemStatus::Idle);
                    let it = &mut self.items[idx];
                    it.percent = 0.0;
                    it.speed_text = "-".to_owned();
                    it.eta_text = "-".to_owned();
                    it.detail = "Cancelled (ready)".to_owned();
                    self.append_log(&format!(
                        "[item {item_id}] Cancelled and moved back to ready."
                    ));
                }
                self.download_cancel_flags.remove(&item_id);
            }
            ItemStatus::Downloading => {
                if let Some(flag) = self.download_cancel_flags.get(&item_id) {
                    flag.store(true, Ordering::Relaxed);
                } else {
                    self.download_cancel_flags
                        .insert(item_id, Arc::new(AtomicBool::new(true)));
                }
                self.cancel_post_actions.insert(item_id, post_action);
                self.items[idx].detail = match post_action {
                    CancelPostAction::Ready => "Cancelling… will return to ready".to_owned(),
                    CancelPostAction::Remove => "Cancelling… will remove row".to_owned(),
                };
                self.append_log(&format!("[item {item_id}] Cancel requested."));
            }
            _ => return,
        }
        self.update_status();
        self.refresh_input_line_info();
        self.schedule_queue_save();
    }

    pub(super) fn cancel_all_active(&mut self, post_action: CancelPostAction) {
        let ids: Vec<u64> = self
            .items
            .iter()
            .filter(|it| matches!(it.status, ItemStatus::Queued | ItemStatus::Downloading))
            .map(|it| it.item_id)
            .collect();
        if ids.is_empty() {
            self.append_log("No active queued/downloading items to cancel.");
            return;
        }
        for item_id in &ids {
            self.request_cancel_item(*item_id, post_action);
        }
        self.append_log(&format!("Cancel requested for {} item(s).", ids.len()));
    }

    pub(super) fn item_has_redownload_target(&self, item: &QueueItem) -> bool {
        let u = item.webpage_url.trim();
        let s = item.source_line.trim();
        !u.is_empty() || (!s.is_empty() && Url::parse(s).is_ok())
    }

    fn prepare_item_redownload_reset(&mut self, item_id: u64) {
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        if let Some((path, _)) = self.find_downloaded_file_for_item(&self.items[idx]) {
            if let Err(e) = fs::remove_file(&path) {
                self.append_log(&format!(
                    "Could not remove old file {}: {e}",
                    path.to_string_lossy()
                ));
            } else {
                self.append_log(&format!("Removed old file: {}", path.to_string_lossy()));
            }
            self.done_file_index.force_refresh();
            self.refresh_done_file_lookup();
        }
        {
            let it = &mut self.items[idx];
            it.error = None;
            it.percent = 0.0;
            it.size_text = "-".to_owned();
            it.speed_text = "-".to_owned();
            it.eta_text = "-".to_owned();
            it.detail = "Re-downloading…".to_owned();
        }
        self.set_item_status_at(idx, ItemStatus::Idle);
    }

    pub(super) fn redownload_item_id(&mut self, item_id: u64) {
        if !Path::new(&self.output_dir).is_dir() {
            self.append_log("Choose a valid output folder.");
            return;
        }
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        if !self.item_has_redownload_target(&self.items[idx]) {
            self.append_log(&format!(
                "[item {item_id}] Cannot re-download: no video URL on this row."
            ));
            return;
        }
        self.persist_settings();
        self.refresh_done_file_lookup();
        self.prepare_item_redownload_reset(item_id);
        self.refresh_done_file_lookup();
        self.update_status();
        self.schedule_queue_save();
        self.spawn_download_workers(vec![item_id]);
    }

    pub(super) fn retry_download_item_id(&mut self, item_id: u64) {
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        if self.items[idx].status != ItemStatus::Failed {
            return;
        }
        self.redownload_item_id(item_id);
    }

    pub(super) fn retry_failed_items(&mut self) {
        if !Path::new(&self.output_dir).is_dir() {
            self.append_log("Choose a valid output folder.");
            return;
        }
        let (has_yt, _, _) = ytdlp::get_external_tools_with_paths(
            &self.settings.yt_dlp_path,
            &self.settings.ffmpeg_path,
            &self.settings.ffprobe_path,
        );
        if !has_yt {
            self.append_log("yt-dlp not found (check PATH or Settings executable path).");
            self.refresh_deps();
            return;
        }
        let failed_no_url = self
            .items
            .iter()
            .filter(|it| it.status == ItemStatus::Failed && !self.item_has_redownload_target(it))
            .count();
        let ids: Vec<u64> = self
            .items
            .iter()
            .filter(|it| it.status == ItemStatus::Failed && self.item_has_redownload_target(it))
            .map(|it| it.item_id)
            .collect();
        if ids.is_empty() {
            if self.status_failed > 0 {
                self.append_log(
                    "No failed items have a video URL to retry. Check the row or re-add the link.",
                );
            } else {
                self.append_log("No failed downloads to retry.");
            }
            return;
        }
        self.persist_settings();
        self.refresh_done_file_lookup();
        for id in &ids {
            self.prepare_item_redownload_reset(*id);
        }
        self.refresh_done_file_lookup();
        self.update_status();
        self.schedule_queue_save();
        self.append_log(&format!(
            "Retrying {} failed download(s).{}",
            ids.len(),
            if failed_no_url > 0 {
                format!(" Skipped {failed_no_url} without a URL.")
            } else {
                String::new()
            }
        ));
        self.spawn_download_workers(ids);
    }
}
