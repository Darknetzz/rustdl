use std::time::{Duration, Instant};

use crate::config::save_settings;

use super::PydlApp;

const SETTINGS_SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

impl PydlApp {
    /// Debounced disk write for layout geometry (dock heights, float window size).
    pub(super) fn schedule_settings_save(&mut self) {
        self.sync_settings_mirror_fields();
        self.settings_save_deadline = Some(Instant::now() + SETTINGS_SAVE_DEBOUNCE);
        self.mark_settings_dirty();
    }

    pub(super) fn maybe_flush_settings_save(&mut self) {
        if let Some(deadline) = self.settings_save_deadline {
            if Instant::now() >= deadline {
                self.settings_save_deadline = None;
                self.spawn_settings_save_to_disk();
            }
        }
    }

    pub(super) fn sync_settings_mirror_fields(&mut self) {
        self.settings.output_dir = self.output_dir.clone();
        self.settings.worker_count = self.worker_count.clamp(1, 6);
        self.settings.settings_tab = super::settings_tab_to_str(self.settings_tab).to_owned();
        self.settings.queue_search = self.queue_search.clone();
        self.settings.log_filter = self.log_filter.slug().to_owned();
    }

    fn spawn_settings_save_to_disk(&mut self) {
        self.sync_settings_mirror_fields();
        let settings = self.settings.clone();
        let rt = self.runtime.clone();
        rt.spawn(async move {
            let save_result =
                tokio::task::spawn_blocking(move || save_settings(&settings)).await;
            if let Ok(Err(err)) = save_result {
                eprintln!("rustdl: failed to save settings: {err}");
            }
        });
    }
}
