use crate::models::QueueItem;
use crate::service::core::DownloadCore;

use super::core_sync;
use super::{CancelPostAction, PydlApp};

impl PydlApp {
    /// Push editable GUI fields into the shared core, run an action, then mirror core state back.
    pub(super) fn download_core_action(&mut self, f: impl FnOnce(&mut DownloadCore)) {
        let shared = self.shared_core.clone();
        {
            let mut core = shared.lock();
            core_sync::sync_app_to_core(self, &mut core);
            f(&mut core);
        }
        {
            let mut core = shared.lock();
            core_sync::sync_core_to_app(&mut core, self);
        }
    }

    fn cleanup_item_gui_state(&mut self, item_id: u64) {
        self.textures.remove(&item_id);
        self.thumbnail_attempted.remove(&item_id);
        self.thumbnail_inflight.remove(&item_id);
        self.selected_item_ids.remove(&item_id);
    }

    pub(super) fn pause_all_downloads(&mut self) {
        self.download_core_action(|core| core.pause_all_downloads());
    }

    pub(super) fn resume_all_downloads(&mut self) {
        self.download_core_action(|core| core.resume_all_downloads());
    }

    pub(super) fn start_downloads(&mut self) {
        self.download_core_action(|core| {
            let _ = core.start_downloads();
        });
    }

    pub(super) fn remove_item_by_id(&mut self, item_id: u64) -> bool {
        let mut removed = false;
        self.download_core_action(|core| {
            removed = core.remove_item_by_id(item_id);
        });
        if removed {
            self.cleanup_item_gui_state(item_id);
            self.refresh_input_line_info();
        }
        removed
    }

    pub(super) fn request_cancel_item(&mut self, item_id: u64, post_action: CancelPostAction) {
        self.download_core_action(|core| core.request_cancel_item(item_id, post_action));
        self.refresh_input_line_info();
    }

    pub(super) fn cancel_all_active(&mut self, post_action: CancelPostAction) {
        self.download_core_action(|core| core.cancel_all_active(post_action));
    }

    pub(super) fn item_has_redownload_target(&self, item: &QueueItem) -> bool {
        crate::app_state::item_has_redownload_target(item)
    }

    pub(super) fn redownload_item_id(&mut self, item_id: u64) {
        self.download_core_action(|core| {
            if let Err(err) = core.redownload_item_id(item_id) {
                core.append_log(&format!("Re-download failed: {}", err.message()));
            }
        });
    }

    pub(super) fn retry_download_item_id(&mut self, item_id: u64) {
        self.download_core_action(|core| core.retry_download_item_id(item_id));
    }

    pub(super) fn retry_failed_items(&mut self) {
        self.download_core_action(|core| {
            let _ = core.retry_failed_items();
        });
    }

    pub(super) fn refetch_failed_items(&mut self) {
        self.download_core_action(|core| {
            let _ = core.refetch_failed_items();
        });
    }

    pub(super) fn cancel_url_resolve_pipeline(&mut self) {
        self.download_core_action(|core| core.cancel_url_resolve_pipeline());
    }

    pub(super) fn set_item_download_overrides(
        &mut self,
        item_id: u64,
        format_override: Option<String>,
        profile_override: Option<String>,
    ) {
        self.download_core_action(|core| {
            if let Some(idx) = core.item_idx(item_id) {
                core.items[idx].format_override = format_override;
                core.items[idx].profile_override = profile_override;
                core.schedule_queue_save();
                core.bump_generation();
            }
        });
    }

    pub(super) fn add_queue_item_to_watchlist(&mut self, item_id: u64) {
        self.download_core_action(|core| {
            if let Err(err) = core.add_watchlist_from_queue_item(item_id) {
                core.append_log(&format!("Watchlist: {}", err.message()));
            }
        });
        self.spawn_watchlist_probe_if_enabled();
    }

    pub(super) fn add_url_to_watchlist(&mut self, url: &str) {
        let url = url.to_owned();
        self.download_core_action(|core| {
            if let Err(err) = core.add_watchlist_url(url) {
                core.append_log(&format!("Watchlist: {}", err.message()));
            }
        });
        self.spawn_watchlist_probe_if_enabled();
    }

    pub(super) fn remove_watchlist_entry(&mut self, entry_id: u64) {
        self.download_core_action(|core| {
            if core.remove_watchlist_entry(entry_id) {
                core.append_log("Watchlist: entry removed.");
            }
        });
    }

    pub(super) fn set_watchlist_entry_paused(&mut self, entry_id: u64, paused: bool) {
        self.download_core_action(|core| {
            core.set_watchlist_entry_paused(entry_id, paused);
        });
    }

    pub(super) fn enqueue_watchlist_entry(&mut self, entry_id: u64) {
        self.download_core_action(|core| {
            if core.enqueue_watchlist_entry(entry_id) {
                core.append_log("Watchlist: URL added to the download queue.");
            }
        });
    }

    pub(super) fn probe_watchlist_now(&mut self) {
        self.download_core_action(|core| {
            core.request_watchlist_probe_now();
            core.append_log("Watchlist: checking URLs…");
        });
        if self.settings.watchlist_enabled && self.has_yt_dlp {
            crate::service::background_spawn::spawn_watchlist_poll_cycle(self.shared_core.clone());
        }
    }

    fn spawn_watchlist_probe_if_enabled(&self) {
        if self.settings.watchlist_enabled && self.has_yt_dlp {
            crate::service::background_spawn::spawn_watchlist_poll_cycle(self.shared_core.clone());
        }
    }
}
