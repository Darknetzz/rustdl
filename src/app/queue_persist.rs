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
                self.spawn_queue_save_to_disk();
            }
        }
        if let Some(mut core) = self.shared_core.try_lock() {
            core.maybe_flush_queue_save();
        }
    }

    pub(super) fn maybe_flush_convert_queue_save(&mut self) {
        if let Some(mut core) = self.shared_core.try_lock() {
            core.maybe_flush_convert_queue_save();
            self.convert_save_deadline = core.convert_save_deadline;
        }
    }

    pub(super) fn flush_queue_to_disk(&mut self) {
        self.queue_save_deadline = None;
        if let Err(err) = save_queue_items(&self.items) {
            self.append_log(&format!("Failed to save queue state: {err}"));
        }
    }

    fn spawn_queue_save_to_disk(&mut self) {
        let items = self.items.clone();
        let rt = self.runtime.clone();
        rt.spawn(async move {
            let save_result = tokio::task::spawn_blocking(move || save_queue_items(&items)).await;
            if let Ok(Err(err)) = save_result {
                eprintln!("rustdl: failed to save queue state: {err}");
            }
        });
    }

    pub(super) fn maybe_flush_log_save(&mut self) {
        let snapshot = {
            let Some(mut core) = self.shared_core.try_lock() else {
                return;
            };
            let Some(deadline) = core.log_save_deadline else {
                return;
            };
            if Instant::now() < deadline {
                return;
            }
            core.log_save_deadline = None;
            core.log_lines.clone()
        };
        let rt = self.runtime.clone();
        rt.spawn(async move {
            if let Ok(Err(err)) = tokio::task::spawn_blocking(move || save_activity_log(&snapshot))
                .await
            {
                eprintln!("rustdl: failed to save activity log: {err}");
            }
        });
    }

    pub(super) fn flush_log_to_disk(&mut self) {
        let mut core = self.shared_core.lock();
        core.log_save_deadline = None;
        if let Err(err) = save_activity_log(&core.log_lines) {
            eprintln!("rustdl: failed to save activity log: {err}");
        }
    }

    /// Convert queue persistence lives on `DownloadCore`; this mirrors the GUI textarea buffer
    /// into the core and flushes (used on exit and when toggling the remember setting).
    pub(super) fn flush_convert_queue_to_disk(&mut self) {
        let mut core = self.shared_core.lock();
        core.convert_input_paths = self.convert_input_paths.clone();
        core.flush_convert_queue_to_disk();
    }

    pub(super) fn clear_activity_log(&mut self) {
        let mut core = self.shared_core.lock();
        core.clear_activity_log();
        self.log_lines = core.log_lines.clone();
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

    pub(super) fn export_activity_log(&mut self) {
        let path = activity_log_file_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body: String = self
            .log_lines
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        match std::fs::write(&path, &body) {
            Ok(()) => self.append_log(&format!(
                "Exported activity log ({} lines) to {}",
                self.log_lines.len(),
                path.display()
            )),
            Err(e) => self.append_log(&format!("Export activity log failed: {e}")),
        }
    }
}
