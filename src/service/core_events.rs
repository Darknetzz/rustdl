//! Applies download-queue `UiEvent`s to [`DownloadCore`] so web API and GUI stay in sync.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::runtime::Runtime;
use tokio::sync::broadcast::error::RecvError;

use crate::app_parsing::{
    convert_detail_is_user_cancellation, parse_speed_eta, reset_convert_item_to_ready,
};
use crate::convert_state::{
    convert_source_path_missing, format_convert_progress_detail, format_convert_saved_detail,
};
use crate::domain::events::is_throttled_download_log_line;
use crate::domain::UiEvent;
use crate::models::{ItemStatus, QueueItem};
use crate::ytdlp;

use super::core::SharedCore;

/// Minimum interval between full-queue UI syncs for a single convert progress stream.
const CONVERT_PROGRESS_BUMP_MIN_SECS: f64 = 0.25;

fn unix_now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn should_bump_convert_progress(
    throttle: &mut std::collections::HashMap<u64, (f64, f32)>,
    item_id: u64,
    percent: Option<f32>,
    force: bool,
) -> bool {
    if force {
        if let Some(p) = percent {
            throttle.insert(item_id, (unix_now_secs(), p));
        }
        return true;
    }
    let now = unix_now_secs();
    let (last_t, last_pct) = throttle.get(&item_id).copied().unwrap_or((-1_000.0, -1.0));
    if now - last_t >= CONVERT_PROGRESS_BUMP_MIN_SECS {
        throttle.insert(item_id, (now, percent.unwrap_or(last_pct)));
        return true;
    }
    if let Some(p) = percent {
        if (p - last_pct).abs() >= 1.0 {
            throttle.insert(item_id, (now, p));
            return true;
        }
    }
    false
}

pub fn spawn_core_event_loop(runtime: Arc<Runtime>, core: SharedCore) {
    runtime.spawn(async move {
        let mut rx = {
            let guard = core.lock();
            guard.subscribe_events()
        };
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let prefetch_ids = {
                        let mut c = core.lock();
                        c.apply_ui_event(ev)
                    };
                    for item_id in prefetch_ids {
                        crate::service::background_spawn::spawn_queue_thumbnail_prefetch(
                            core.clone(),
                            item_id,
                        );
                    }
                    core.lock().maybe_flush_convert_queue_save();
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        }
    });
}

impl super::core::DownloadCore {
    pub(crate) fn apply_ui_event(&mut self, ev: UiEvent) -> Vec<u64> {
        let mut prefetch = Vec::new();
        match ev {
            UiEvent::AddResolved { rows, source_line } => {
                prefetch.extend(self.handle_add_resolved(rows, source_line));
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
            UiEvent::ConvertLine { item_id, line } => {
                self.handle_convert_line(item_id, &line);
            }
            UiEvent::ConvertDuration {
                item_id,
                duration_ms,
            } => {
                if duration_ms > 0 {
                    self.convert_duration_ms.insert(item_id, duration_ms);
                }
            }
            UiEvent::ConvertMediaProbed { item_id, media } => {
                self.handle_convert_media_probed(item_id, media);
            }
            UiEvent::ConvertDone {
                item_id,
                ok,
                detail,
                final_output_path,
            } => {
                self.handle_convert_done(item_id, ok, detail, final_output_path);
            }
            UiEvent::ConvertBatchDone => {
                self.handle_convert_batch_done();
            }
            _ => {}
        }
        self.maybe_finish_shutdown();
        prefetch
    }

    fn handle_convert_line(&mut self, item_id: u64, line: &str) {
        if line.starts_with("starting with ") || line.starts_with("skip_reason=") {
            if let Some(idx) = self.convert_item_idx(item_id) {
                let it = &mut self.convert_items[idx];
                it.status = ItemStatus::Downloading;
                it.detail = line.to_owned();
            }
            self.append_log(&format!("[convert {item_id}] {line}"));
            self.update_convert_status();
            self.bump_generation();
            return;
        }

        let Some((key, value)) = line.split_once('=') else {
            if line.starts_with("dry-run:") {
                if let Some(idx) = self.convert_item_idx(item_id) {
                    self.convert_items[idx].detail = line.chars().take(160).collect();
                }
                self.append_log(&format!("[convert {item_id}] {line}"));
                self.update_convert_status();
                self.bump_generation();
            }
            return;
        };

        let key = key.trim();
        let value = value.trim();
        if key != "progress" {
            self.convert_progress_state
                .entry(item_id)
                .or_default()
                .insert(key.to_owned(), value.to_owned());
            return;
        }

        let state = self
            .convert_progress_state
            .remove(&item_id)
            .unwrap_or_default();
        let current_secs = state
            .get("out_time")
            .and_then(|v| crate::transcode::parse_ffmpeg_out_time_secs(v));
        let total_secs = self
            .convert_duration_ms
            .get(&item_id)
            .copied()
            .map(|ms| ms as f64 / 1000.0);

        let percent = if value == "end" {
            Some(100.0)
        } else if let (Some(current), Some(total)) = (current_secs, total_secs) {
            if total > 0.0 {
                Some(((current / total) * 100.0).clamp(0.0, 100.0) as f32)
            } else {
                None
            }
        } else {
            None
        };

        let fps_raw = state.get("fps").map(String::as_str).unwrap_or("");
        let speed_raw = state.get("speed").map(String::as_str).unwrap_or("");
        let detail = format_convert_progress_detail(
            value,
            current_secs,
            total_secs,
            fps_raw,
            speed_raw,
            percent,
        );

        if let Some(idx) = self.convert_item_idx(item_id) {
            let it = &mut self.convert_items[idx];
            it.status = ItemStatus::Downloading;
            if let Some(p) = percent {
                it.percent = p;
            }
            it.detail = detail;
        }
        let force_bump = value == "end";
        if should_bump_convert_progress(
            &mut self.convert_progress_throttle,
            item_id,
            percent,
            force_bump,
        ) {
            self.update_convert_status();
            self.bump_generation();
        }
    }

    fn handle_convert_media_probed(
        &mut self,
        item_id: u64,
        media: crate::transcode::ConvertInputMedia,
    ) {
        self.convert_media_inflight.remove(&item_id);
        if let Some(idx) = self.convert_item_idx(item_id) {
            let it = &mut self.convert_items[idx];
            it.video_codec = media.codec;
            it.width = media.width;
            it.height = media.height;
            it.fps = media.fps;
            it.bitrate_bps = media.bitrate_bps;
            if it.video_codec.is_empty() {
                it.source_missing = convert_source_path_missing(&it.source_path);
            }
        }
        if let Some(ms) = media.duration_ms.filter(|ms| *ms > 0) {
            self.convert_duration_ms.insert(item_id, ms);
        }
        self.update_convert_status();
        self.schedule_convert_queue_save();
        self.bump_generation();
    }

    fn handle_convert_done(
        &mut self,
        item_id: u64,
        ok: bool,
        detail: String,
        final_output_path: Option<String>,
    ) {
        if let Some(idx) = self.convert_item_idx(item_id) {
            let it = &mut self.convert_items[idx];
            if !ok && convert_detail_is_user_cancellation(&detail) {
                reset_convert_item_to_ready(it);
            } else {
                it.status = if ok {
                    ItemStatus::Done
                } else {
                    ItemStatus::Failed
                };
                it.percent = if ok { 100.0 } else { it.percent };
                if let Some(path) = final_output_path {
                    it.output_path = path;
                }
                let skipped = detail.to_ascii_lowercase().starts_with("skipped");
                if ok && !skipped {
                    if let Ok(meta) = std::fs::metadata(&it.output_path) {
                        let output_bytes = meta.len();
                        it.output_bytes = Some(output_bytes);
                        it.detail = if it.input_bytes > 0 {
                            format_convert_saved_detail(it.input_bytes, output_bytes)
                        } else {
                            detail.clone()
                        };
                    } else {
                        it.detail = detail.clone();
                    }
                    let source = it.source_path.clone();
                    let out = it.output_path.clone();
                    self.apply_convert_post_encode_actions(&source, &out);
                } else {
                    it.detail = detail.clone();
                }
            }
        }
        self.convert_duration_ms.remove(&item_id);
        self.convert_progress_state.remove(&item_id);
        self.convert_progress_throttle.remove(&item_id);
        if !ok && !convert_detail_is_user_cancellation(&detail) {
            self.append_log(&format!("[convert {item_id}] {detail}"));
        }
        self.update_convert_status();
        self.schedule_convert_queue_save();
        self.bump_generation();
    }

    fn handle_convert_batch_done(&mut self) {
        self.convert_progress_throttle.clear();
        self.convert_running = false;
        for item in &mut self.convert_items {
            if matches!(item.status, ItemStatus::Queued | ItemStatus::Downloading) {
                reset_convert_item_to_ready(item);
            }
        }
        self.update_convert_status();
        self.convert_save_deadline = None;
        self.flush_convert_queue_to_disk();
        self.bump_generation();
    }

    fn handle_add_resolved(
        &mut self,
        rows: Vec<crate::models::VideoPreview>,
        source_line: String,
    ) -> Vec<u64> {
        let mut prefetch = Vec::new();
        let Some(iid) = self.pending_resolve_ids.remove(&source_line) else {
            return prefetch;
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
            if rows.is_empty() {
                let item = QueueItem {
                    item_id: iid,
                    source_line: source_line.clone(),
                    title: source_line.clone(),
                    webpage_url: source_line.clone(),
                    error: Some("No preview returned for this URL.".to_owned()),
                    status: ItemStatus::Idle,
                    ..Default::default()
                };
                self.bump_item_id_floor(iid);
                self.items.insert(0, item);
            } else {
                self.append_log(&format!("Already in queue (duplicate): {source_line}"));
            }
        } else {
            for (n, pv) in keys.into_iter().enumerate() {
                let assign_iid = if n == 0 {
                    iid
                } else {
                    let id = self.next_item_id;
                    self.next_item_id += 1;
                    id
                };
                if n == 0 {
                    self.bump_item_id_floor(assign_iid);
                }
                if let Some(ref err) = pv.error {
                    self.append_log(&format!("[item {assign_iid}] Metadata fetch failed: {err}"));
                }
                let item = QueueItem::from_preview(assign_iid, pv);
                self.items.insert(0, item);
                prefetch.push(assign_iid);
            }
        }
        self.rebuild_item_index();
        self.invalidate_queue_caches();
        self.update_status();
        self.schedule_queue_save();
        self.maybe_auto_start_downloads();
        self.bump_generation();
        prefetch
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
        if let Some(detail) = ytdlp::format_download_card_detail(line) {
            it.detail = detail.chars().take(160).collect();
        }
        if let Some(raw) = ytdlp::parse_output_path_from_download_log_line(line) {
            let raw = raw.to_string_lossy();
            if let Some(path) =
                crate::app::done_file_index::resolve_path_under_output(&self.output_dir, &raw)
            {
                it.local_path = Some(path.to_string_lossy().into_owned());
            }
        }
        self.transfer_totals_dirty = true;
        self.maybe_append_download_line_log(item_id, line);
        self.bump_generation();
    }

    fn maybe_append_download_line_log(&mut self, item_id: u64, line: &str) {
        if is_throttled_download_log_line(line) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let last = self
                .download_log_throttle
                .get(&item_id)
                .copied()
                .unwrap_or(-1_000.0);
            if now - last < 0.25 {
                return;
            }
            self.download_log_throttle.insert(item_id, now);
        }
        self.append_log(&format!("[item {item_id}] {line}"));
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

        let mut completed = ok;
        let mut final_detail = detail.to_owned();
        if completed {
            self.done_file_index.force_refresh();
            self.refresh_done_file_lookup();
            self.bind_local_path_for_item(item_id);
            self.apply_post_download_organize_for_item(item_id);
            if let Some(msg) = self.verify_done_item_streams(item_id) {
                completed = false;
                final_detail = msg;
            } else {
                self.probe_saved_file_media_for_item(item_id);
            }
        }
        if let Some(idx) = self.item_idx(item_id) {
            let new_status = if completed {
                ItemStatus::Done
            } else {
                ItemStatus::Failed
            };
            crate::app_state::transition_queue_item_status(&mut self.items[idx], new_status);
            if completed {
                self.items[idx].percent = 100.0;
                self.items[idx].eta_text = "0s".to_owned();
            }
            self.items[idx].detail = final_detail.clone();
        }
        if completed {
            self.enqueue_completed_download_to_convert(item_id);
        }
        if !completed {
            let summary = final_detail.trim();
            if summary.is_empty() {
                self.append_log(&format!("[item {item_id}] Download failed."));
            } else if ok {
                self.append_log(&format!(
                    "[item {item_id}] Post-download verification failed: {summary}"
                ));
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

    pub fn verify_done_item_streams(&self, item_id: u64) -> Option<String> {
        if !self.settings.verify_output_video_audio
            || self.settings.ffmpeg_extract_audio_mp3
            || !self.has_ffprobe
        {
            return None;
        }
        let idx = self.item_idx(item_id)?;
        let item = &self.items[idx];
        if item.video_id.trim().is_empty() {
            return None;
        }
        let output_dir = self.effective_output_dir();
        let (path, _) = self
            .done_file_index
            .find_path_for_queue_item(&output_dir, item)?;
        let path_str = path.to_string_lossy().to_string();
        let (has_video, has_audio) = match ytdlp::probe_video_audio_stream_presence(
            &path_str,
            &self.settings.ffprobe_path,
        ) {
            Some(v) => v,
            None => {
                return Some(
                    "ffprobe failed or could not parse output. Check the file and ffprobe path."
                        .to_owned(),
                );
            }
        };
        streams_incomplete_message(has_video, has_audio)
    }
}

fn streams_incomplete_message(has_video: bool, has_audio: bool) -> Option<String> {
    if !has_video && !has_audio {
        Some("File has neither video nor audio streams according to ffprobe.".to_owned())
    } else if !has_video {
        Some(
            "Download has audio only (no video stream). Try yt-dlp -f \"bv*+ba/b\" with ffmpeg merge, or check available formats (-F)."
                .to_owned(),
        )
    } else if !has_audio {
        Some(
            "Download has video but no audio stream. Try a different format or merge (bestvideo+bestaudio)."
                .to_owned(),
        )
    } else {
        None
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
