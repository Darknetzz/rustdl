use eframe::egui;

pub use crate::domain::events::{try_send_ui, UiEvent};

use super::PydlApp;

/// Upper bound on UI work per frame so one burst of download lines cannot freeze the window.
const MAX_UI_EVENTS_PER_FRAME: usize = 128;

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
                UiEvent::DownloadLine { .. } => {
                    ctx.request_repaint();
                }
                UiEvent::DownloadDone {
                    item_id,
                    ok,
                    detail: _,
                } => {
                    if ok {
                        self.probe_done_item_resolution_if_missing(item_id);
                        if self.settings.show_thumbnails && !self.textures.contains_key(&item_id) {
                            self.thumbnail_attempted.remove(&item_id);
                            self.queue_thumbnail_load(item_id);
                        }
                    }
                    self.mark_transfer_totals_dirty();
                }
                UiEvent::UpdateCheckDone {
                    latest_version,
                    release_url,
                    download_browser_url,
                    download_api_url,
                    has_update,
                    message,
                } => {
                    self.update_check_in_progress = false;
                    self.update_latest_version = latest_version;
                    self.update_release_url = release_url;
                    self.update_download_asset = download_browser_url.map(|browser_download_url| {
                        crate::app::update_check::PlatformReleaseAsset {
                            browser_download_url,
                            api_url: download_api_url.unwrap_or_default(),
                        }
                    });
                    self.update_has_update = has_update;
                    self.update_status_text = if message.starts_with("GitHub returned")
                        || message.starts_with("No published")
                    {
                        format!("Update check failed: {message}")
                    } else {
                        message
                    };
                    if !has_update {
                        self.update_pending_path = None;
                    }
                }
                UiEvent::UpdateDownloadDone {
                    ok,
                    pending_path,
                    message,
                } => {
                    self.update_download_in_progress = false;
                    if ok {
                        self.update_pending_path = pending_path;
                    }
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
                // Convert queue state lives on DownloadCore (applied in service::core_events); the GUI
                // mirrors it via sync_core_to_app each frame. Repaint on meaningful progress only.
                UiEvent::ConvertLine { line, .. } => {
                    if line.starts_with("progress=")
                        || line.starts_with("starting with ")
                        || line.starts_with("skip_reason=")
                        || line.starts_with("dry-run:")
                    {
                        ctx.request_repaint();
                    }
                }
                UiEvent::ConvertDuration { .. }
                | UiEvent::ConvertMediaProbed { .. }
                | UiEvent::ConvertDone { .. }
                | UiEvent::ConvertBatchDone => {
                    ctx.request_repaint();
                }
                UiEvent::LogLine { .. } => {
                    ctx.request_repaint();
                }
                UiEvent::ShutdownRequested => {
                    self.exit_allowed = true;
                    self.flush_queue_to_disk();
                    self.flush_convert_queue_to_disk();
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
}
