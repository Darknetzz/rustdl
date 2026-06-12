//! Video converter orchestration on the shared [`DownloadCore`].
//!
//! Owning this state in the core (instead of the egui app) lets the LAN web UI and the
//! headless `--web-only` server drive the converter queue without a window. The desktop GUI mirrors
//! this state via `sync_core_to_app` and delegates its buttons here.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::app::background_spawn;
use crate::app_parsing::human_bytes_ui;
use crate::config::{save_convert_queue_snapshot, ConvertQueueSnapshot};
use crate::convert_state::{
    normalize_convert_source_key, remove_scanned_convert_input_lines, reset_skipped_convert_items,
};
use crate::models::{ConvertQueueItem, ItemStatus};
use crate::transcode::{self, ConvertConfig, ConvertInput};

use super::core::DownloadCore;

fn convert_encoder_detect_key(
    ffmpeg_path: &str,
    encoder_override: &str,
    target_codec: &str,
) -> String {
    format!("{ffmpeg_path}\0{encoder_override}\0{target_codec}")
}

impl DownloadCore {
    pub fn convert_config(&self) -> ConvertConfig {
        ConvertConfig {
            ffmpeg_path: self.settings.ffmpeg_path.clone(),
            ffprobe_path: self.settings.ffprobe_path.clone(),
            output_dir: self.output_dir.clone(),
            recursive: self.settings.convert_recursive,
            dry_run: self.settings.convert_dry_run,
            delete_original: self.settings.convert_delete_original,
            rename_original: self.settings.convert_rename_original,
            overwrite: self.settings.convert_overwrite,
            reencode_target: self.settings.convert_reencode_target,
            target_codec: self.settings.convert_target_codec.clone(),
            use_recommended_container: self.settings.convert_use_recommended_container,
            target_bitrate: self.settings.convert_target_bitrate.clone(),
            max_width: self.settings.convert_max_width,
            size_preset: self.settings.convert_size_preset.clone(),
            min_shrink_percent: self.settings.convert_min_shrink_percent,
            encoder_override: self.settings.convert_encoder_override.clone(),
            cpu_threads: transcode::resolve_convert_cpu_threads(
                self.settings.convert_cpu_threads,
                self.settings.convert_parallel,
            ),
            subprocess_priority: self.settings.subprocess_priority.clone(),
        }
    }

    /// Caches the selected encoder so the web UI can show the indicator without re-probing.
    pub fn refresh_convert_encoder_detection(&mut self) {
        if !self.has_ffmpeg {
            self.convert_encoder_choice = None;
            self.convert_encoder_detect_key.clear();
            return;
        }
        let key = convert_encoder_detect_key(
            &self.settings.ffmpeg_path,
            &self.settings.convert_encoder_override,
            &self.settings.convert_target_codec,
        );
        if self.convert_encoder_detect_key == key && self.convert_encoder_choice.is_some() {
            return;
        }
        self.convert_encoder_choice = Some(transcode::detect_encoder_with_override(
            &self.settings.ffmpeg_path,
            &self.settings.convert_encoder_override,
            &self.settings.convert_target_codec,
        ));
        self.convert_encoder_detect_key = key;
    }

    pub fn queue_convert_media_probe(&mut self, item_id: u64, file_path: PathBuf) {
        if !self.has_ffprobe || self.convert_media_inflight.contains(&item_id) {
            return;
        }
        self.convert_media_inflight.insert(item_id);
        background_spawn::spawn_convert_media_probe(
            &self.runtime,
            &self.ui_event_bus(),
            self.convert_probe_semaphore.clone(),
            item_id,
            file_path,
            self.settings.ffprobe_path.clone(),
        );
    }

    /// Re-probe restored rows that are missing metadata (after a restart).
    pub fn queue_convert_restored_assets(&mut self) {
        if self.convert_items.is_empty() {
            return;
        }
        for item in self.convert_items.clone() {
            if item.video_codec.is_empty() {
                self.queue_convert_media_probe(item.item_id, PathBuf::from(&item.source_path));
            }
        }
    }

    pub fn scan_convert_paths_into_queue(&mut self, path_lines: &[String]) {
        let lines: Vec<String> = path_lines
            .iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if lines.is_empty() {
            return;
        }

        let cfg = self.convert_config();
        let inputs: Vec<ConvertInput> = lines
            .iter()
            .map(|source_path| ConvertInput {
                source_path: source_path.clone(),
            })
            .collect();
        let plan = transcode::collect_plan(&inputs, &cfg);
        if plan.is_empty() {
            self.append_log("Convert: no supported video files found in added path(s).");
            remove_scanned_convert_input_lines(&mut self.convert_input_paths, &lines);
            self.schedule_convert_queue_save();
            self.bump_generation();
            return;
        }

        let added = self.push_convert_plan_items(plan);
        if added > 0 {
            self.append_log(&format!(
                "Convert: added {added} video(s) to queue as ready."
            ));
            self.maybe_auto_start_convert_batch();
        } else {
            self.append_log("Convert: all video(s) from path(s) are already in the queue.");
        }
        remove_scanned_convert_input_lines(&mut self.convert_input_paths, &lines);
        self.schedule_convert_queue_save();
        self.bump_generation();
    }

    /// Starts the convert batch when [`AppSettings::convert_auto_start_on_add`] is enabled and tools are ready.
    pub fn maybe_auto_start_convert_batch(&mut self) {
        if !self.settings.convert_auto_start_on_add || self.convert_running || self.convert_paused {
            return;
        }
        if !self.has_ffmpeg || !self.has_ffprobe {
            return;
        }
        if !self
            .convert_items
            .iter()
            .any(|item| item.status == ItemStatus::Idle)
        {
            return;
        }
        self.start_convert_batch();
    }

    /// Adds plan items not already in the converter queue. Returns how many were added.
    pub fn push_convert_plan_items(&mut self, plan: Vec<transcode::ConvertPlanItem>) -> usize {
        if plan.is_empty() {
            return 0;
        }
        let existing: HashSet<String> = self
            .convert_items
            .iter()
            .map(|item| normalize_convert_source_key(&item.source_path))
            .collect();
        let mut added = 0usize;
        for plan_item in plan {
            let source = plan_item.input.to_string_lossy().to_string();
            if existing.contains(&normalize_convert_source_key(&source)) {
                continue;
            }

            let item_id = self.convert_next_item_id;
            self.convert_next_item_id = self.convert_next_item_id.saturating_add(1);
            self.queue_convert_media_probe(item_id, plan_item.input.clone());

            let input_bytes = std::fs::metadata(&plan_item.input)
                .map(|m| m.len())
                .unwrap_or(0);
            let ready_detail = if input_bytes > 0 {
                format!("Ready · {}", human_bytes_ui(input_bytes))
            } else {
                "Ready".to_owned()
            };

            self.convert_items.push(ConvertQueueItem {
                item_id,
                source_path: source,
                output_path: plan_item.output.to_string_lossy().to_string(),
                status: ItemStatus::Idle,
                percent: 0.0,
                detail: ready_detail,
                input_bytes,
                output_bytes: None,
                video_codec: String::new(),
                width: None,
                height: None,
                fps: None,
                bitrate_bps: None,
                source_missing: !plan_item.input.is_file(),
            });
            added += 1;
        }
        if added > 0 {
            self.update_convert_status();
            self.schedule_convert_queue_save();
            self.bump_generation();
        }
        added
    }

    /// Adds a freshly downloaded video to the converter queue when the user opted in.
    pub fn enqueue_completed_download_to_convert(&mut self, item_id: u64) {
        if !self.settings.enqueue_downloads_to_convert || self.settings.ffmpeg_extract_audio_mp3 {
            return;
        }
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        let item = self.items[idx].clone();
        let output_dir = self.output_dir.clone();
        let Some((path, _)) = self
            .done_file_index
            .find_path_for_queue_item(&output_dir, &item)
        else {
            return;
        };
        if !transcode::is_video_path(&path) {
            return;
        }
        let source = path.to_string_lossy().into_owned();
        let plan = transcode::collect_plan(
            &[ConvertInput {
                source_path: source,
            }],
            &self.convert_config(),
        );
        let added = self.push_convert_plan_items(plan);
        if added > 0 {
            let label = item.title.trim();
            let label = if label.is_empty() {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("download")
            } else {
                label
            };
            self.append_log(&format!(
                "Convert: enqueued \"{label}\" from completed download."
            ));
        }
    }

    pub fn start_convert_batch(&mut self) {
        if self.convert_paused {
            self.append_log("Convert: batch is paused. Click Resume first.");
            return;
        }
        let jobs: Vec<(u64, ConvertInput, String)> = self
            .convert_items
            .iter()
            .filter(|item| item.status == ItemStatus::Idle)
            .map(|item| {
                (
                    item.item_id,
                    ConvertInput {
                        source_path: item.source_path.clone(),
                    },
                    item.output_path.clone(),
                )
            })
            .collect();
        if jobs.is_empty() {
            self.append_log("Convert: no ready items to convert.");
            return;
        }

        self.persist_settings();
        self.convert_cancel_flag.store(false, Ordering::Relaxed);
        let cfg = self.convert_config();

        for (item_id, _, _) in &jobs {
            if let Some(item) = self
                .convert_items
                .iter_mut()
                .find(|x| x.item_id == *item_id)
            {
                item.status = ItemStatus::Queued;
                item.detail = "Queued".to_owned();
            }
        }

        self.convert_running = true;
        let parallel = self.settings.convert_parallel.clamp(1, 6);
        background_spawn::spawn_convert_worker(
            &self.runtime,
            &self.ui_event_bus(),
            cfg,
            jobs,
            self.convert_cancel_flag.clone(),
            parallel,
        );
        self.append_log("Convert: batch started.");
        self.update_convert_status();
        self.schedule_convert_queue_save();
        self.bump_generation();
    }

    pub fn cancel_convert_batch(&mut self) {
        self.request_convert_batch_stop(false);
    }

    pub fn pause_convert_batch(&mut self) {
        if self.convert_paused && !self.convert_running {
            return;
        }
        self.convert_paused = true;
        if self.convert_running {
            self.request_convert_batch_stop(true);
            self.append_log(
                "Convert: batch paused (active items return to ready when the current encode stops).",
            );
        } else {
            self.append_log("Convert: batch paused.");
            self.bump_generation();
        }
    }

    pub fn resume_convert_batch(&mut self) {
        if !self.convert_paused {
            return;
        }
        self.convert_paused = false;
        self.append_log("Convert: batch resumed.");
        self.start_convert_batch();
    }

    fn request_convert_batch_stop(&mut self, from_pause: bool) {
        if !self.convert_running {
            if !from_pause {
                self.convert_paused = false;
            }
            return;
        }
        if !from_pause {
            self.convert_paused = false;
            self.append_log("Convert: cancel requested.");
        }
        self.convert_cancel_flag.store(true, Ordering::Relaxed);
        self.bump_generation();
    }

    pub fn retry_skipped_convert_items(&mut self) {
        if self.convert_running {
            self.append_log(
                "Convert: wait for the running batch to finish before retrying skipped items.",
            );
            return;
        }
        let count = reset_skipped_convert_items(&mut self.convert_items);
        if count == 0 {
            self.append_log("Convert: no skipped items to retry.");
            return;
        }
        self.schedule_convert_queue_save();
        self.append_log(&format!(
            "Convert: reset {count} skipped item(s) to ready. Adjust settings if needed, then start the batch."
        ));
        self.update_convert_status();
        self.bump_generation();
    }

    pub fn clear_convert_queue(&mut self) {
        if self.convert_items.is_empty() && self.convert_input_paths.is_empty() {
            return;
        }
        let item_ids: Vec<u64> = self.convert_items.iter().map(|it| it.item_id).collect();
        for item_id in item_ids {
            self.convert_media_inflight.remove(&item_id);
            self.evict_thumbnail(item_id);
        }
        self.convert_items.clear();
        self.convert_duration_ms.clear();
        self.convert_progress_state.clear();
        self.clear_convert_queue_persistence();
        self.update_convert_status();
        self.bump_generation();
    }

    // --- persistence ---

    pub fn schedule_convert_queue_save(&mut self) {
        self.convert_save_deadline = Some(Instant::now() + Duration::from_millis(400));
    }

    pub fn maybe_flush_convert_queue_save(&mut self) {
        if let Some(deadline) = self.convert_save_deadline {
            if Instant::now() >= deadline {
                self.convert_save_deadline = None;
                self.flush_convert_queue_to_disk();
            }
        }
    }

    pub fn flush_convert_queue_to_disk(&mut self) {
        self.convert_save_deadline = None;
        if !self.settings.convert_remember_queue {
            return;
        }
        let snapshot = ConvertQueueSnapshot {
            input_paths: self.convert_input_paths.clone(),
            next_item_id: self.convert_next_item_id,
            items: self.convert_items.clone(),
        };
        if let Err(err) = save_convert_queue_snapshot(&snapshot) {
            self.append_log(&format!("Failed to save converter queue state: {err}"));
        }
    }

    pub fn clear_convert_queue_persistence(&mut self) {
        let snapshot = ConvertQueueSnapshot::default();
        if let Err(err) = save_convert_queue_snapshot(&snapshot) {
            self.append_log(&format!("Failed to clear converter queue state: {err}"));
        }
    }
}
