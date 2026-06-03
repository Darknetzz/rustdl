use std::sync::Once;

use crossbeam_channel::Sender;
use eframe::egui;
use tokio::sync::broadcast;

static UI_CHANNEL_CLOSED_WARN: Once = Once::new();

/// Upper bound on UI work per frame so one burst of download lines cannot freeze the window.
const MAX_UI_EVENTS_PER_FRAME: usize = 128;

/// Delivers UI events to the egui thread and the shared [`DownloadCore`] event loop.
#[derive(Clone)]
pub struct UiEventBus {
    tx: Sender<UiEvent>,
    broadcast: broadcast::Sender<UiEvent>,
}

impl UiEventBus {
    pub fn new(tx: Sender<UiEvent>, broadcast: broadcast::Sender<UiEvent>) -> Self {
        Self { tx, broadcast }
    }

    pub fn publish(&self, event: UiEvent) -> bool {
        let _ = self.broadcast.send(event.clone());
        match self.tx.send(event) {
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
}

/// Publishes an event to the GUI and DownloadCore (see [`UiEventBus::publish`]).
pub(crate) fn try_send_ui(bus: &UiEventBus, event: UiEvent) -> bool {
    bus.publish(event)
}
use crate::models::{ItemStatus, VideoPreview};

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
    /// Activity log line (web SSE subscribers).
    LogLine {
        line: String,
    },
    /// Graceful shutdown finished (web SSE + desktop window close).
    ShutdownRequested,
}

/// yt-dlp progress lines that would flood the log if recorded every event.
fn is_throttled_download_log_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    (l.contains("[download]") && (l.contains('%') || l.contains("frag"))) || l.contains("[merger]")
}

impl PydlApp {
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
                // Queue resolve/progress and download state are applied on DownloadCore
                // (see service/core_events.rs); the GUI syncs from core each frame.
                UiEvent::AddResolved { .. } | UiEvent::AddProgress { .. } | UiEvent::AddDone => {}
                UiEvent::DownloadLine { item_id, line } => {
                    self.maybe_append_download_log(ctx, item_id, &line);
                }
                UiEvent::DownloadDone {
                    item_id,
                    ok,
                    detail,
                } => {
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
                    if !completed && ok {
                        if let Some(idx) = self.item_idx(item_id) {
                            self.set_item_status_at(idx, ItemStatus::Failed);
                            let it = &mut self.items[idx];
                            it.detail = final_detail.clone();
                        }
                        let summary = final_detail.trim();
                        if !summary.is_empty() {
                            self.append_log(&format!(
                                "[item {item_id}] Post-download verification failed: {summary}"
                            ));
                        }
                    }
                    if completed {
                        self.probe_done_item_resolution_if_missing(item_id);
                        // Auto-enqueue to AV1 is applied on DownloadCore (works headless too).
                    }
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
                // AV1 queue state lives on DownloadCore (applied in service::core_events); the GUI
                // mirrors it via sync_core_to_app each frame. Here we only need to keep repainting
                // while a transcode is active so progress updates are visible promptly.
                UiEvent::Av1Line { .. }
                | UiEvent::Av1Duration { .. }
                | UiEvent::Av1MediaProbed { .. }
                | UiEvent::Av1Done { .. }
                | UiEvent::Av1BatchDone => {
                    ctx.request_repaint();
                }
                UiEvent::LogLine { .. } => {}
                UiEvent::ShutdownRequested => {
                    self.exit_allowed = true;
                    self.flush_queue_to_disk();
                    self.flush_av1_queue_to_disk();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
