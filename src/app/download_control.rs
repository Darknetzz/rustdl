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
        self.download_core_action(|core| core.start_downloads());
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
                core.append_log(&format!("Re-download failed: {err:?}"));
            }
        });
    }

    pub(super) fn retry_download_item_id(&mut self, item_id: u64) {
        self.download_core_action(|core| core.retry_download_item_id(item_id));
    }

    pub(super) fn retry_failed_items(&mut self) {
        self.download_core_action(|core| core.retry_failed_items());
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
}
