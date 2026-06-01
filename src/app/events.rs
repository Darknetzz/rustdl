use std::sync::Once;

use crossbeam_channel::Sender;
use eframe::egui;

use crate::app_parsing::{
    av1_detail_is_user_cancellation, parse_speed_eta, reset_av1_item_to_ready,
};
use crate::app_state;

static UI_CHANNEL_CLOSED_WARN: Once = Once::new();

/// Upper bound on UI work per frame so one burst of download lines cannot freeze the window.
const MAX_UI_EVENTS_PER_FRAME: usize = 128;

/// Sends an event to the UI thread. Logs once to stderr if the channel is disconnected.
pub(crate) fn try_send_ui(tx: &Sender<UiEvent>, event: UiEvent) -> bool {
    match tx.send(event) {
        Ok(()) => true,
        Err(_) => {
            UI_CHANNEL_CLOSED_WARN.call_once(|| {
                eprintln!(
                    "rustdl: UI event channel closed; background tasks may not update the window."
                );
            });
            false
        }
    }
}
use crate::models::{ItemStatus, QueueItem, VideoPreview};
use crate::ytdlp;

use super::PydlApp;

#[derive(Clone)]
pub(crate) enum UiEvent {
    AddResolved {
        rows: Vec<VideoPreview>,
        source_line: String,
    },
    AddProgress {
        processed: usize,
        total: usize,
        current: Option<String>,
    },
    AddDone,
    DownloadLine {
        item_id: u64,
        line: String,
    },
    DownloadDone {
        item_id: u64,
        ok: bool,
        detail: String,
    },
    UpdateCheckDone {
        latest_version: Option<String>,
        release_url: Option<String>,
        has_update: bool,
        message: String,
    },
    ThumbnailFetched {
        item_id: u64,
        /// Decoded on a worker thread; GPU upload is deferred (see `pending_thumbnail_uploads`).
        image: Option<egui::ColorImage>,
    },
    Av1Line {
        item_id: u64,
        line: String,
    },
    Av1Duration {
        item_id: u64,
        duration_ms: u64,
    },
    Av1MediaProbed {
        item_id: u64,
        media: crate::av1_transcode::Av1InputMedia,
    },
    Av1Done {
        item_id: u64,
        ok: bool,
        detail: String,
        final_output_path: Option<String>,
    },
    Av1BatchDone,
}

/// yt-dlp progress lines that would flood the log if recorded every event.
fn is_throttled_download_log_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    (l.contains("[download]") && (l.contains('%') || l.contains("frag"))) || l.contains("[merger]")
}

fn format_av1_duration_clock(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn format_av1_rate_display(fps_raw: &str, speed_raw: &str) -> String {
    let mut parts = Vec::new();
    if let Ok(fps) = fps_raw.trim().parse::<f64>() {
        if fps > 0.0 {
            parts.push(format!("{} fps", fps.round() as i64));
        }
    }
    if let Some(speed) = crate::av1_transcode::parse_ffmpeg_speed(speed_raw) {
        parts.push(format!("{speed:.2}x"));
    } else if !speed_raw.trim().is_empty() {
        parts.push(speed_raw.trim().to_owned());
    }
    parts.join(" · ")
}

fn format_av1_progress_detail(
    progress: &str,
    current_secs: Option<f64>,
    total_secs: Option<f64>,
    rate: &str,
    percent: Option<f32>,
) -> String {
    let pct = percent
        .map(|p| format!("{p:.0}%"))
        .unwrap_or_else(|| "…".to_owned());
    let time = match (current_secs, total_secs) {
        (Some(c), Some(t)) => format!(
            "{} / {}",
            format_av1_duration_clock(c),
            format_av1_duration_clock(t)
        ),
        (Some(c), None) => format_av1_duration_clock(c),
        _ => String::new(),
    };
    let rate_part = if rate.is_empty() {
        String::new()
    } else {
        format!(" · {rate}")
    };
    if progress == "end" {
        format!("{pct} · Done{rate_part}")
    } else if time.is_empty() {
        format!("{pct}{rate_part}")
    } else {
        format!("{pct} · {time}{rate_part}")
    }
}

impl PydlApp {
    fn handle_av1_line(&mut self, item_id: u64, line: &str) {
        if line.starts_with("starting with ") || line.starts_with("skip_reason=") {
            if let Some(it) = self.av1_items.iter_mut().find(|x| x.item_id == item_id) {
                it.status = ItemStatus::Downloading;
                it.detail = line.to_owned();
            }
            self.append_log(&format!("[av1 {item_id}] {line}"));
            return;
        }

        let Some((key, value)) = line.split_once('=') else {
            if line.starts_with("dry-run:") {
                if let Some(it) = self.av1_items.iter_mut().find(|x| x.item_id == item_id) {
                    it.detail = line.chars().take(160).collect();
                }
                self.append_log(&format!("[av1 {item_id}] {line}"));
            }
            return;
        };

        let key = key.trim();
        let value = value.trim();
        if key != "progress" {
            self.av1_progress_state
                .entry(item_id)
                .or_default()
                .insert(key.to_owned(), value.to_owned());
            return;
        }

        let state = self.av1_progress_state.remove(&item_id).unwrap_or_default();
        let current_secs = state
            .get("out_time")
            .and_then(|v| crate::av1_transcode::parse_ffmpeg_out_time_secs(v));
        let total_secs = self
            .av1_duration_ms
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
        let rate = format_av1_rate_display(fps_raw, speed_raw);
        let detail = format_av1_progress_detail(value, current_secs, total_secs, &rate, percent);

        if let Some(it) = self.av1_items.iter_mut().find(|x| x.item_id == item_id) {
            it.status = ItemStatus::Downloading;
            if let Some(p) = percent {
                it.percent = p;
            }
            it.detail = detail;
        }
    }

    pub(super) fn process_events(&mut self, ctx: &egui::Context) {
        let profile = std::env::var("RUSTDL_PROFILE").ok().as_deref() == Some("1");
        let t0 = if profile {
            Some(std::time::Instant::now())
        } else {
            None
        };
        let mut processed = 0usize;
        loop {
            if processed >= MAX_UI_EVENTS_PER_FRAME {
                if !self.rx.is_empty() {
                    ctx.request_repaint();
                }
                break;
            }
            let ev = match self.rx.try_recv() {
                Ok(e) => e,
                Err(_) => break,
            };
            processed += 1;
            match ev {
                UiEvent::AddResolved { rows, source_line } => {
                    let Some(iid) = self.pending_resolve_ids.remove(&source_line) else {
                        continue;
                    };
                    if let Some(idx) = self.item_idx(iid) {
                        let removed = self.items.remove(idx);
                        self.on_item_removed(&removed);
                    } else {
                        continue;
                    }
                    let deduped = ytdlp::dedupe_previews(self.dedupe_keys(), &rows);
                    if deduped.is_empty() {
                        self.append_log(&format!("No new videos found for: {source_line}"));
                        // Keep a visible card: otherwise the resolving row vanishes and it looks broken.
                        let iid = self.next_item_id;
                        self.next_item_id += 1;
                        if rows.is_empty() {
                            let item = QueueItem {
                                item_id: iid,
                                source_line: source_line.clone(),
                                title: source_line.clone(),
                                error: Some("No preview returned for this URL.".to_owned()),
                                status: ItemStatus::Idle,
                                ..Default::default()
                            };
                            self.items.insert(0, item.clone());
                            self.on_item_inserted(&item);
                        } else {
                            let mut it = QueueItem::from_preview(iid, rows[0].clone());
                            it.error = Some(
                                "This video is already in the list (same as a finished or pending item)."
                                    .to_owned(),
                            );
                            self.items.insert(0, it.clone());
                            self.on_item_inserted(&it);
                        }
                    } else {
                        for pv in deduped {
                            let iid = self.next_item_id;
                            self.next_item_id += 1;
                            if let Some(ref err) = pv.error {
                                self.append_log(&format!(
                                    "[item {iid}] Metadata fetch failed: {err}"
                                ));
                            }
                            let item = QueueItem::from_preview(iid, pv.clone());
                            self.items.insert(0, item.clone());
                            self.on_item_inserted(&item);
                            if self.settings.show_thumbnails {
                                if let Some(url) = pv.thumbnail_url.clone() {
                                    self.queue_thumbnail_load(iid, url);
                                }
                            }
                        }
                    }
                    self.sync_status_fields_from_counts();
                    self.invalidate_queue_caches();
                    self.refresh_input_line_info();
                    self.schedule_queue_save();
                    self.maybe_auto_start_downloads();
                }
                UiEvent::AddProgress {
                    processed,
                    total,
                    current,
                } => {
                    self.add_processed_urls = processed.min(total);
                    self.add_total_urls = total;
                    self.add_current_url = current;
                }
                UiEvent::AddDone => {
                    self.add_in_progress = false;
                    self.add_processed_urls = self.add_total_urls;
                    self.add_current_url = None;
                    self.maybe_auto_start_downloads();
                }
                UiEvent::DownloadLine { item_id, line } => {
                    if let Some(idx) = self.item_idx(item_id) {
                        let old = self.items[idx].status;
                        if old != ItemStatus::Downloading {
                            self.set_item_status_at(idx, ItemStatus::Downloading);
                        }
                        let it = &mut self.items[idx];
                        let (pct, size) = ytdlp::parse_progress_line(&line);
                        if let Some(p) = pct {
                            it.percent = p.clamp(0.0, 100.0);
                        }
                        if let Some(sz) = size {
                            it.size_text = sz;
                        }
                        if let Some((speed, eta)) = parse_speed_eta(&line) {
                            it.speed_text = speed;
                            it.eta_text = eta;
                        }
                        it.detail = line.chars().take(160).collect::<String>();
                    }
                    self.maybe_append_download_log(ctx, item_id, &line);
                    self.mark_transfer_totals_dirty();
                }
                UiEvent::DownloadDone {
                    item_id,
                    ok,
                    detail,
                } => {
                    if let Some(post_action) = self.cancel_post_actions.remove(&item_id) {
                        self.download_cancel_flags.remove(&item_id);
                        match post_action {
                            super::CancelPostAction::Ready => {
                                if let Some(idx) = self.item_idx(item_id) {
                                    self.set_item_status_at(idx, ItemStatus::Idle);
                                    let it = &mut self.items[idx];
                                    it.percent = 0.0;
                                    it.speed_text = "-".to_owned();
                                    it.eta_text = "-".to_owned();
                                    it.detail = "Cancelled (ready)".to_owned();
                                }
                            }
                            super::CancelPostAction::Remove => {
                                let _ = self.remove_item_by_id(item_id);
                            }
                        }
                        self.queue_running = self.queue_running.saturating_sub(1);
                        self.refresh_input_line_info();
                        self.schedule_queue_save();
                        continue;
                    }
                    let mut completed = ok;
                    let mut final_detail = detail;
                    if ok {
                        self.done_file_index.force_refresh();
                        self.refresh_done_file_lookup();
                        if let Some(msg) = self.verify_done_item_has_video_and_audio(item_id) {
                            completed = false;
                            final_detail = msg.clone();
                            self.append_log(&format!("[item {item_id}] {msg}"));
                        }
                    }
                    if let Some(idx) = self.item_idx(item_id) {
                        let new_status = if completed {
                            ItemStatus::Done
                        } else {
                            ItemStatus::Failed
                        };
                        self.set_item_status_by_id(item_id, new_status);
                        let it = &mut self.items[idx];
                        it.percent = if completed { 100.0 } else { it.percent };
                        it.detail = final_detail.clone();
                        if completed {
                            it.eta_text = "0s".to_owned();
                        }
                    }
                    if !completed {
                        let summary = final_detail.trim();
                        if summary.is_empty() {
                            self.append_log(&format!("[item {item_id}] Download failed."));
                        } else {
                            self.append_log(&format!(
                                "[item {item_id}] Download failed: {summary}"
                            ));
                        }
                    }
                    if completed {
                        self.probe_done_item_resolution_if_missing(item_id);
                    }
                    self.download_cancel_flags.remove(&item_id);
                    self.queue_running = self.queue_running.saturating_sub(1);
                    self.mark_transfer_totals_dirty();
                    self.schedule_queue_save();
                }
                UiEvent::UpdateCheckDone {
                    latest_version,
                    release_url,
                    has_update,
                    message,
                } => {
                    self.update_check_in_progress = false;
                    self.update_latest_version = latest_version;
                    self.update_release_url = release_url;
                    self.update_has_update = has_update;
                    self.update_status_text = message;
                }
                UiEvent::ThumbnailFetched { item_id, image } => {
                    self.thumbnail_inflight.remove(&item_id);
                    let Some(image) = image else {
                        self.thumbnail_attempted.insert(item_id);
                        continue;
                    };
                    self.pending_thumbnail_uploads.push_back((item_id, image));
                }
                UiEvent::Av1Line { item_id, line } => {
                    self.handle_av1_line(item_id, &line);
                }
                UiEvent::Av1Duration {
                    item_id,
                    duration_ms,
                } => {
                    if duration_ms > 0 {
                        self.av1_duration_ms.insert(item_id, duration_ms);
                    }
                }
                UiEvent::Av1MediaProbed { item_id, media } => {
                    self.av1_media_inflight.remove(&item_id);
                    if let Some(it) = self.av1_items.iter_mut().find(|x| x.item_id == item_id) {
                        it.video_codec = media.codec;
                        it.width = media.width;
                        it.height = media.height;
                        it.fps = media.fps;
                        it.bitrate_bps = media.bitrate_bps;
                    }
                    if let Some(ms) = media.duration_ms.filter(|ms| *ms > 0) {
                        self.av1_duration_ms.insert(item_id, ms);
                    }
                    ctx.request_repaint();
                    self.schedule_av1_queue_save();
                }
                UiEvent::Av1Done {
                    item_id,
                    ok,
                    detail,
                    final_output_path,
                } => {
                    if let Some(it) = self.av1_items.iter_mut().find(|x| x.item_id == item_id) {
                        if !ok && av1_detail_is_user_cancellation(&detail) {
                            reset_av1_item_to_ready(it);
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
                                        super::av1_panel::format_av1_saved_detail(
                                            it.input_bytes,
                                            output_bytes,
                                        )
                                    } else {
                                        detail.clone()
                                    };
                                } else {
                                    it.detail = detail.clone();
                                }
                            } else {
                                it.detail = detail.clone();
                            }
                        }
                    }
                    self.av1_duration_ms.remove(&item_id);
                    self.av1_progress_state.remove(&item_id);
                    if !ok && !av1_detail_is_user_cancellation(&detail) {
                        self.append_log(&format!("[av1 {item_id}] {detail}"));
                    }
                    self.schedule_av1_queue_save();
                }
                UiEvent::Av1BatchDone => {
                    self.av1_running = false;
                    for item in &mut self.av1_items {
                        if matches!(item.status, ItemStatus::Queued | ItemStatus::Downloading) {
                            reset_av1_item_to_ready(item);
                        }
                    }
                    self.schedule_av1_queue_save();
                }
            }
        }
        self.drain_pending_thumbnail_uploads(ctx);
        self.evict_textures_if_needed();
        if let Some(t0) = t0 {
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            if ms > 8.0 {
                eprintln!("rustdl profile: process_events {processed} events in {ms:.1}ms");
            }
        }
    }

    fn drain_pending_thumbnail_uploads(&mut self, ctx: &egui::Context) {
        const MAX_UPLOADS_PER_FRAME: usize = 2;
        for _ in 0..MAX_UPLOADS_PER_FRAME {
            let Some((item_id, color_image)) = self.pending_thumbnail_uploads.pop_front() else {
                break;
            };
            let tex = ctx.load_texture(
                format!("thumb-{item_id}"),
                color_image,
                egui::TextureOptions::LINEAR,
            );
            self.textures.insert(item_id, tex);
            self.thumbnail_attempted.remove(&item_id);
        }
        if !self.pending_thumbnail_uploads.is_empty() {
            ctx.request_repaint();
        }
    }

    fn maybe_append_download_log(&mut self, ctx: &egui::Context, item_id: u64, line: &str) {
        if is_throttled_download_log_line(line) {
            let now = ctx.input(|i| i.time);
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
}

#[cfg(test)]
mod tests {
    use super::is_throttled_download_log_line;

    #[test]
    fn throttle_matches_progress_spam_not_errors() {
        assert!(is_throttled_download_log_line(
            "[download]  45.2% of   12.34MiB at  1.00MiB/s ETA 00:05"
        ));
        assert!(is_throttled_download_log_line(
            "[Merger] Merging formats into mkv"
        ));
        assert!(!is_throttled_download_log_line(
            "ERROR: unable to download video"
        ));
    }
}
