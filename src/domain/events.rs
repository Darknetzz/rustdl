use std::sync::Once;

use crossbeam_channel::Sender;
use eframe::egui;
use tokio::sync::broadcast;

use crate::models::VideoPreview;

static UI_CHANNEL_CLOSED_WARN: Once = Once::new();

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
pub fn try_send_ui(bus: &UiEventBus, event: UiEvent) -> bool {
    bus.publish(event)
}

#[derive(Clone)]
pub enum UiEvent {
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
        download_browser_url: Option<String>,
        download_api_url: Option<String>,
        has_update: bool,
        message: String,
    },
    UpdateDownloadDone {
        ok: bool,
        pending_path: Option<std::path::PathBuf>,
        message: String,
    },
    ThumbnailFetched {
        item_id: u64,
        /// Decoded on a worker thread; GPU upload is deferred (see `pending_thumbnail_uploads`).
        image: Option<egui::ColorImage>,
    },
    ConvertLine {
        item_id: u64,
        line: String,
    },
    ConvertDuration {
        item_id: u64,
        duration_ms: u64,
    },
    ConvertMediaProbed {
        item_id: u64,
        media: crate::transcode::ConvertInputMedia,
    },
    ConvertDone {
        item_id: u64,
        ok: bool,
        detail: String,
        final_output_path: Option<String>,
    },
    ConvertBatchDone,
    /// Activity log line (web SSE subscribers).
    LogLine {
        line: String,
    },
    /// Graceful shutdown finished (web SSE + desktop window close).
    ShutdownRequested,
    /// Flat playlist preview finished (`ytdlp::flat_playlist_preview`).
    PlaylistPreviewDone {
        source_url: String,
        title: Option<String>,
        urls: Vec<String>,
        error: Option<String>,
    },
}

/// yt-dlp progress lines that would flood the log if recorded every event.
pub fn is_throttled_download_log_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with(crate::ytdlp::PROGRESS_PREFIX) {
        return true;
    }
    let l = trimmed.to_ascii_lowercase();
    (l.contains("[download]") && (l.contains('%') || l.contains("frag"))) || l.contains("[merger]")
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
            "progress:98.4%|236700648|NA|240516070.39999998"
        ));
        assert!(is_throttled_download_log_line(
            "[Merger] Merging formats into mkv"
        ));
        assert!(!is_throttled_download_log_line(
            "ERROR: unable to download video"
        ));
        assert!(!is_throttled_download_log_line(
            "[FixupM3u8] Fixing MPEG-TS in MP4 container"
        ));
    }
}
