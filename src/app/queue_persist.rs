use std::time::{Duration, Instant};

use crate::config::{activity_log_file_path, save_activity_log, save_queue_items};

use super::PydlApp;

const QUEUE_SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

impl PydlApp {
    pub(super) fn schedule_queue_save(&mut self) {
        self.queue_save_deadline = Some(Instant::now() + QUEUE_SAVE_DEBOUNCE);
    }

    pub(super) fn maybe_flush_queue_save(&mut self) {
        if let Some(deadline) = self.queue_save_deadline {
            if Instant::now() >= deadline {
                self.queue_save_deadline = None;
                self.flush_queue_to_disk();
            }
        }
    }

    pub(super) fn flush_queue_to_disk(&mut self) {
        self.queue_save_deadline = None;
        if let Err(err) = save_queue_items(&self.items) {
            self.append_log(&format!("Failed to save queue state: {err}"));
        }
    }

    pub(super) fn schedule_log_save(&mut self) {
        self.log_save_deadline = Some(Instant::now() + QUEUE_SAVE_DEBOUNCE);
    }

    pub(super) fn maybe_flush_log_save(&mut self) {
        if let Some(deadline) = self.log_save_deadline {
            if Instant::now() >= deadline {
                self.log_save_deadline = None;
                self.flush_log_to_disk();
            }
        }
    }

    pub(super) fn flush_log_to_disk(&mut self) {
        self.log_save_deadline = None;
        if let Err(err) = save_activity_log(&self.log_lines) {
            eprintln!("rustdl: failed to save activity log: {err}");
        }
    }

    /// AV1 queue persistence lives on `DownloadCore`; this mirrors the GUI textarea buffer
    /// into the core and flushes (used on exit and when toggling the remember setting).
    pub(super) fn flush_av1_queue_to_disk(&mut self) {
        let mut core = self.shared_core.lock();
        core.av1_input_paths = self.av1_input_paths.clone();
        core.flush_av1_queue_to_disk();
    }

    pub(super) fn clear_activity_log(&mut self) {
        self.log_lines.clear();
        self.flush_log_to_disk();
    }

    pub(super) fn open_activity_log_file(&mut self) {
        let path = activity_log_file_path();
        if !path.exists() {
            let _ = save_activity_log(&self.log_lines);
        }
        if let Err(e) = crate::app_actions::open_path(&path) {
            self.append_log(&format!("Failed to open activity log: {e}"));
        }
    }
}
