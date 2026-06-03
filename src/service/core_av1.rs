//! AV1 converter orchestration on the shared [`DownloadCore`].
//!
//! Owning this state in the core (instead of the egui app) lets the LAN web UI and the
//! headless `--web-only` server drive the AV1 queue without a window. The desktop GUI mirrors
//! this state via `sync_core_to_app` and delegates its buttons here.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use crate::app::background_spawn;
use crate::app_parsing::human_bytes_ui;
use crate::av1_state::{normalize_av1_source_key, remove_scanned_av1_input_lines};
use crate::av1_transcode::{self, Av1Config, Av1Input};
use crate::config::{save_av1_queue_snapshot, Av1QueueSnapshot};
use crate::models::{Av1QueueItem, ItemStatus};

use super::core::DownloadCore;

fn av1_encoder_detect_key(ffmpeg_path: &str, encoder_override: &str) -> String {
    format!("{ffmpeg_path}\0{encoder_override}")
}

impl DownloadCore {
    pub fn av1_config(&self) -> Av1Config {
        Av1Config {
            ffmpeg_path: self.settings.ffmpeg_path.clone(),
            ffprobe_path: self.settings.ffprobe_path.clone(),
            output_dir: self.output_dir.clone(),
            recursive: self.settings.av1_recursive,
            dry_run: self.settings.av1_dry_run,
            delete_original: self.settings.av1_delete_original,
            rename_original: self.settings.av1_rename_original,
            overwrite: self.settings.av1_overwrite,
            reencode_av1: self.settings.av1_reencode_av1,
            target_bitrate: self.settings.av1_target_bitrate.clone(),
            max_width: self.settings.av1_max_width,
            size_preset: self.settings.av1_size_preset.clone(),
            min_shrink_percent: self.settings.av1_min_shrink_percent,
            encoder_override: self.settings.av1_encoder_override.clone(),
        }
    }

    /// Caches the selected encoder so the web UI can show the indicator without re-probing.
    pub fn refresh_av1_encoder_detection(&mut self) {
        if !self.has_ffmpeg {
            self.av1_encoder_choice = None;
            self.av1_encoder_detect_key.clear();
            return;
        }
        let key = av1_encoder_detect_key(
            &self.settings.ffmpeg_path,
            &self.settings.av1_encoder_override,
        );
        if self.av1_encoder_detect_key == key && self.av1_encoder_choice.is_some() {
            return;
        }
        self.av1_encoder_choice = Some(av1_transcode::detect_encoder_with_override(
            &self.settings.ffmpeg_path,
            &self.settings.av1_encoder_override,
        ));
        self.av1_encoder_detect_key = key;
    }

    pub fn queue_av1_media_probe(&mut self, item_id: u64, file_path: PathBuf) {
        if !self.has_ffprobe || self.av1_media_inflight.contains(&item_id) {
            return;
        }
        self.av1_media_inflight.insert(item_id);
        background_spawn::spawn_av1_media_probe(
            &self.runtime,
            &self.ui_event_bus(),
            item_id,
            file_path,
            self.settings.ffprobe_path.clone(),
        );
    }

    /// Re-probe restored rows that are missing metadata (after a restart).
    pub fn queue_av1_restored_assets(&mut self) {
        if self.av1_items.is_empty() {
            return;
        }
        for item in self.av1_items.clone() {
            if item.video_codec.is_empty() {
                self.queue_av1_media_probe(item.item_id, PathBuf::from(&item.source_path));
            }
        }
    }

    pub fn scan_av1_paths_into_queue(&mut self, path_lines: &[String]) {
        let lines: Vec<String> = path_lines
            .iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if lines.is_empty() {
            return;
        }

        let cfg = self.av1_config();
        let inputs: Vec<Av1Input> = lines
            .iter()
            .map(|source_path| Av1Input {
                source_path: source_path.clone(),
            })
            .collect();
        let plan = av1_transcode::collect_plan(&inputs, &cfg);
        if plan.is_empty() {
            self.append_log("AV1: no supported video files found in added path(s).");
            remove_scanned_av1_input_lines(&mut self.av1_input_paths, &lines);
            self.schedule_av1_queue_save();
            self.bump_generation();
            return;
        }

        let added = self.push_av1_plan_items(plan);
        if added > 0 {
            self.append_log(&format!("AV1: added {added} video(s) to queue as ready."));
        } else {
            self.append_log("AV1: all video(s) from path(s) are already in the queue.");
        }
        remove_scanned_av1_input_lines(&mut self.av1_input_paths, &lines);
        self.schedule_av1_queue_save();
        self.bump_generation();
    }

    /// Adds plan items not already in the AV1 queue. Returns how many were added.
    pub fn push_av1_plan_items(&mut self, plan: Vec<av1_transcode::Av1PlanItem>) -> usize {
        if plan.is_empty() {
            return 0;
        }
        let existing: HashSet<String> = self
            .av1_items
            .iter()
            .map(|item| normalize_av1_source_key(&item.source_path))
            .collect();
        let mut added = 0usize;
        for plan_item in plan {
            let source = plan_item.input.to_string_lossy().to_string();
            if existing.contains(&normalize_av1_source_key(&source)) {
                continue;
            }

            let item_id = self.av1_next_item_id;
            self.av1_next_item_id = self.av1_next_item_id.saturating_add(1);
            self.queue_av1_media_probe(item_id, plan_item.input.clone());

            let input_bytes = std::fs::metadata(&plan_item.input)
                .map(|m| m.len())
                .unwrap_or(0);
            let ready_detail = if input_bytes > 0 {
                format!("Ready · {}", human_bytes_ui(input_bytes))
            } else {
                "Ready".to_owned()
            };

            self.av1_items.push(Av1QueueItem {
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
            });
            added += 1;
        }
        if added > 0 {
            self.schedule_av1_queue_save();
            self.bump_generation();
        }
        added
    }

    /// Adds a freshly downloaded video to the AV1 queue when the user opted in.
    pub fn enqueue_completed_download_to_av1(&mut self, item_id: u64) {
        if !self.settings.enqueue_downloads_to_av1 || self.settings.ffmpeg_extract_audio_mp3 {
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
        if !av1_transcode::is_video_path(&path) {
            return;
        }
        let source = path.to_string_lossy().into_owned();
        let plan = av1_transcode::collect_plan(
            &[Av1Input {
                source_path: source,
            }],
            &self.av1_config(),
        );
        let added = self.push_av1_plan_items(plan);
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
                "AV1: enqueued \"{label}\" from completed download."
            ));
        }
    }

    pub fn start_av1_batch(&mut self) {
        let jobs: Vec<(u64, Av1Input, String)> = self
            .av1_items
            .iter()
            .filter(|item| item.status == ItemStatus::Idle)
            .map(|item| {
                (
                    item.item_id,
                    Av1Input {
                        source_path: item.source_path.clone(),
                    },
                    item.output_path.clone(),
                )
            })
            .collect();
        if jobs.is_empty() {
            self.append_log("AV1: no ready items to convert.");
            return;
        }

        self.persist_settings();
        self.av1_cancel_flag.store(false, Ordering::Relaxed);
        let cfg = self.av1_config();

        for (item_id, _, _) in &jobs {
            if let Some(item) = self.av1_items.iter_mut().find(|x| x.item_id == *item_id) {
                item.status = ItemStatus::Queued;
                item.detail = "Queued".to_owned();
            }
        }

        self.av1_running = true;
        background_spawn::spawn_av1_worker(
            &self.runtime,
            &self.ui_event_bus(),
            cfg,
            jobs,
            self.av1_cancel_flag.clone(),
        );
        self.append_log("AV1: batch started.");
        self.schedule_av1_queue_save();
        self.bump_generation();
    }

    pub fn cancel_av1_batch(&mut self) {
        if !self.av1_running {
            return;
        }
        self.av1_cancel_flag.store(true, Ordering::Relaxed);
        self.append_log("AV1: cancel requested.");
        self.bump_generation();
    }

    pub fn clear_av1_queue(&mut self) {
        if self.av1_items.is_empty() && self.av1_input_paths.is_empty() {
            return;
        }
        for item in &self.av1_items {
            self.av1_media_inflight.remove(&item.item_id);
        }
        self.av1_items.clear();
        self.av1_duration_ms.clear();
        self.av1_progress_state.clear();
        self.clear_av1_queue_persistence();
        self.bump_generation();
    }

    // --- persistence ---

    pub fn schedule_av1_queue_save(&mut self) {
        // AV1 save points are infrequent (scan/probe/done/clear), so persist eagerly. This works
        // identically for the GUI and the headless web server, which has no per-frame flush loop.
        self.flush_av1_queue_to_disk();
    }

    pub fn flush_av1_queue_to_disk(&mut self) {
        if !self.settings.av1_remember_queue {
            return;
        }
        let snapshot = Av1QueueSnapshot {
            input_paths: self.av1_input_paths.clone(),
            next_item_id: self.av1_next_item_id,
            items: self.av1_items.clone(),
        };
        if let Err(err) = save_av1_queue_snapshot(&snapshot) {
            self.append_log(&format!("Failed to save AV1 queue state: {err}"));
        }
    }

    pub fn clear_av1_queue_persistence(&mut self) {
        let snapshot = Av1QueueSnapshot::default();
        if let Err(err) = save_av1_queue_snapshot(&snapshot) {
            self.append_log(&format!("Failed to clear AV1 queue state: {err}"));
        }
    }
}
