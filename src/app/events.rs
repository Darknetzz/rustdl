use std::sync::Once;

use crossbeam_channel::Sender;
use eframe::egui;

use crate::app_parsing::parse_speed_eta;

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
}

/// yt-dlp progress lines that would flood the log if recorded every event.
fn is_throttled_download_log_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    (l.contains("[download]") && (l.contains('%') || l.contains("frag")))
        || l.contains("[merger]")
}

impl PydlApp {
    pub(super) fn process_events(&mut self, ctx: &egui::Context) {
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
                    if let Some(pos) = self.items.iter().position(|x| x.item_id == iid) {
                        self.items.remove(pos);
                    } else {
                        continue;
                    }
                    let deduped = ytdlp::dedupe_previews(&self.dedupe_keys(), &rows);
                    if deduped.is_empty() {
                        self.append_log(&format!("No new videos found for: {source_line}"));
                        // Keep a visible card: otherwise the resolving row vanishes and it looks broken.
                        let iid = self.next_item_id;
                        self.next_item_id += 1;
                        if rows.is_empty() {
                            self.items.insert(
                                0,
                                QueueItem {
                                    item_id: iid,
                                    source_line: source_line.clone(),
                                    title: source_line.clone(),
                                    error: Some("No preview returned for this URL.".to_owned()),
                                    status: ItemStatus::Idle,
                                    ..Default::default()
                                },
                            );
                        } else {
                            let mut it = QueueItem::from_preview(iid, rows[0].clone());
                            it.error = Some(
                                "This video is already in the list (same as a finished or pending item)."
                                    .to_owned(),
                            );
                            self.items.insert(0, it);
                        }
                    } else {
                        for pv in deduped {
                            let iid = self.next_item_id;
                            self.next_item_id += 1;
                            self.items
                                .insert(0, QueueItem::from_preview(iid, pv.clone()));
                            if self.settings.show_thumbnails {
                                if let Some(url) = pv.thumbnail_url.clone() {
                                    self.queue_thumbnail_load(iid, url);
                                }
                            }
                        }
                    }
                    self.update_status();
                    self.refresh_input_line_info();
                    self.schedule_queue_save();
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
                    if let Some(it) = self.items.iter_mut().find(|x| x.item_id == item_id) {
                        it.status = ItemStatus::Downloading;
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
                    self.update_status();
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
                                if let Some(it) =
                                    self.items.iter_mut().find(|x| x.item_id == item_id)
                                {
                                    it.status = ItemStatus::Idle;
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
                        self.update_status();
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
                    if let Some(it) = self.items.iter_mut().find(|x| x.item_id == item_id) {
                        it.status = if completed {
                            ItemStatus::Done
                        } else {
                            ItemStatus::Failed
                        };
                        it.percent = if completed { 100.0 } else { it.percent };
                        it.detail = final_detail;
                        if completed {
                            it.eta_text = "0s".to_owned();
                        }
                    }
                    if completed {
                        self.probe_done_item_resolution_if_missing(item_id);
                    }
                    self.download_cancel_flags.remove(&item_id);
                    self.queue_running = self.queue_running.saturating_sub(1);
                    self.update_status();
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
                    self.pending_thumbnail_uploads
                        .push_back((item_id, image));
                }
            }
        }
        self.drain_pending_thumbnail_uploads(ctx);
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
        assert!(is_throttled_download_log_line("[Merger] Merging formats into mkv"));
        assert!(!is_throttled_download_log_line(
            "ERROR: unable to download video"
        ));
    }
}
