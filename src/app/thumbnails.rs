use std::path::{Path, PathBuf};

use eframe::egui;
use image::imageops::FilterType;

use crate::app::done_file_index::resolve_path_under_output;
use crate::convert_state::convert_source_path_missing;
use crate::models::ItemStatus;

use super::queue_cache::{THUMBNAIL_DECODE_MAX_WIDTH, THUMBNAIL_QUEUE_SOFT_CAP};
use super::{background_spawn, events::try_send_ui, PydlApp, UiEvent};

pub(crate) fn decode_thumbnail_image(bytes: Vec<u8>) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if img.width() > THUMBNAIL_DECODE_MAX_WIDTH {
        img.resize(
            THUMBNAIL_DECODE_MAX_WIDTH,
            THUMBNAIL_DECODE_MAX_WIDTH,
            FilterType::Triangle,
        )
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let raw = rgba.into_raw();
    Some(egui::ColorImage::from_rgba_unmultiplied(size, &raw))
}

impl PydlApp {
    fn thumbnails_allowed_for_queue(&self, item_id: u64) -> bool {
        if !self.settings.show_thumbnails {
            return false;
        }
        let Some(idx) = self.item_idx(item_id) else {
            return false;
        };
        let status = self.items[idx].status;
        // Finished rows still need previews (ffmpeg frame grab / saved disk cache).
        if matches!(status, ItemStatus::Done | ItemStatus::Failed) {
            return true;
        }
        if self.items.len() <= THUMBNAIL_QUEUE_SOFT_CAP {
            return true;
        }
        matches!(
            status,
            ItemStatus::Idle | ItemStatus::Queued | ItemStatus::Downloading | ItemStatus::Resolving
        )
    }

    fn resolve_queue_local_media_path(&self, item: &crate::models::QueueItem) -> Option<PathBuf> {
        if !matches!(item.status, ItemStatus::Done | ItemStatus::Failed) {
            return None;
        }
        item.local_path
            .as_ref()
            .and_then(|rel| resolve_path_under_output(&self.output_dir, rel))
            .or_else(|| {
                self.find_downloaded_file_for_item(item)
                    .map(|(path, _)| path)
            })
            .filter(|path| queue_local_thumbnail_supported(path))
    }

    /// Loads missing downloader card textures (disk cache, remote URLs, ffmpeg frame grab).
    pub(super) fn ensure_downloader_thumbnails(&mut self) {
        if !self.settings.show_thumbnails {
            return;
        }
        let mut pending: Vec<(u8, u64)> = self
            .items
            .iter()
            .filter(|it| {
                !self.textures.contains_key(&it.item_id)
                    && !self.thumbnail_inflight.contains(&it.item_id)
                    && !self.thumbnail_attempted.contains(&it.item_id)
                    && self.thumbnails_allowed_for_queue(it.item_id)
            })
            .map(|it| {
                let pri = match it.status {
                    ItemStatus::Done | ItemStatus::Failed => 0,
                    ItemStatus::Downloading | ItemStatus::Queued | ItemStatus::Resolving => 1,
                    _ => 2,
                };
                (pri, it.item_id)
            })
            .collect();
        pending.sort_by_key(|(p, id)| (*p, *id));
        for (_, item_id) in pending.into_iter().take(THUMBNAIL_QUEUE_SOFT_CAP) {
            self.queue_thumbnail_load(item_id);
        }
    }

    pub(super) fn queue_thumbnail_load(&mut self, item_id: u64) {
        if !self.thumbnails_allowed_for_queue(item_id) {
            return;
        }
        if self.textures.contains_key(&item_id) || self.thumbnail_inflight.contains(&item_id) {
            return;
        }
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        let item = self.items[idx].clone();
        let urls = crate::ytdlp::thumbnail_url_candidates(&item);
        let local_media = self.resolve_queue_local_media_path(&item);
        if urls.is_empty() && local_media.is_none() {
            return;
        }
        self.thumbnail_inflight.insert(item_id);
        let source_key = crate::service::core::DownloadCore::queue_thumbnail_source_key(&item);
        let bus = self.ui_bus.clone();
        let rt = self.runtime.clone();
        let client = self.http_client.clone();
        let sem = self.thumb_semaphore.clone();
        let shared_core = self.shared_core.clone();
        let has_ffmpeg = self.has_ffmpeg;
        let ffmpeg_path = self.settings.ffmpeg_path.clone();
        rt.spawn(async move {
            let permit = sem.acquire_owned().await;
            let Ok(_permit) = permit else {
                try_send_ui(
                    &bus,
                    UiEvent::ThumbnailFetched {
                        item_id,
                        image: None,
                    },
                );
                return;
            };
            let bytes = shared_core
                .lock()
                .cached_thumbnail_bytes(item_id, &source_key)
                .map(|(b, _)| b);
            let bytes = if bytes.is_some() {
                bytes
            } else {
                fetch_queue_thumbnail_bytes(
                    &client,
                    &urls,
                    local_media.as_deref(),
                    has_ffmpeg,
                    &ffmpeg_path,
                )
                .await
                .map(|(b, content_type)| {
                    shared_core.lock().cache_thumbnail_bytes(
                        item_id,
                        source_key.clone(),
                        b.clone(),
                        content_type,
                    );
                    b
                })
            };
            let image = match bytes {
                None => None,
                Some(b) => tokio::task::spawn_blocking(move || decode_thumbnail_image(b))
                    .await
                    .ok()
                    .flatten(),
            };
            try_send_ui(&bus, UiEvent::ThumbnailFetched { item_id, image });
        });
    }

    pub(super) fn queue_convert_local_thumbnail(
        &mut self,
        item_id: u64,
        file_path: PathBuf,
        ffmpeg_path: String,
    ) {
        if !self.settings.show_thumbnails {
            return;
        }
        if self.textures.contains_key(&item_id) || self.thumbnail_inflight.contains(&item_id) {
            return;
        }
        self.thumbnail_inflight.insert(item_id);
        background_spawn::spawn_convert_local_thumbnail(
            &self.runtime,
            &self.ui_bus,
            &self.shared_core,
            item_id,
            file_path,
            ffmpeg_path,
        );
    }

    /// Loads local-video thumbnails (egui textures) for any mirrored AV1 rows that lack one.
    /// Convert queue state itself is owned by `DownloadCore`; only the textures are GUI-local.
    pub(super) fn ensure_convert_thumbnails(&mut self) {
        if !self.settings.show_thumbnails || !self.has_ffmpeg || self.convert_items.is_empty() {
            return;
        }
        let ffmpeg_path = self.settings.ffmpeg_path.clone();
        let pending: Vec<(u64, PathBuf)> = self
            .convert_items
            .iter()
            .filter(|it| {
                !self.textures.contains_key(&it.item_id)
                    && !self.thumbnail_inflight.contains(&it.item_id)
            })
            .map(|it| (it.item_id, PathBuf::from(&it.source_path)))
            .collect();
        for (item_id, path) in pending {
            if convert_source_path_missing(path.to_string_lossy().as_ref()) {
                self.thumbnail_attempted.insert(item_id);
                continue;
            }
            self.queue_convert_local_thumbnail(item_id, path, ffmpeg_path.clone());
        }
    }
}

fn queue_local_thumbnail_supported(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("mp4" | "webm" | "mkv" | "mov" | "m4v" | "avi")
    )
}

async fn fetch_queue_thumbnail_bytes(
    client: &reqwest::Client,
    urls: &[String],
    local_media: Option<&Path>,
    has_ffmpeg: bool,
    ffmpeg_path: &str,
) -> Option<(Vec<u8>, String)> {
    if let Some(path) = local_media {
        if has_ffmpeg {
            let ffmpeg_path = ffmpeg_path.to_owned();
            let path = path.to_path_buf();
            if let Some(png) = tokio::task::spawn_blocking(move || {
                crate::transcode::extract_thumbnail_png_bytes(&path, &ffmpeg_path)
            })
            .await
            .ok()
            .flatten()
            {
                return Some((png, "image/png".to_owned()));
            }
        }
    }
    for url in urls {
        if let Some(found) = crate::ytdlp::fetch_thumbnail_bytes(client, url).await {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::decode_thumbnail_image;

    #[test]
    fn decode_thumbnail_accepts_tiny_png() {
        let png = include_bytes!("../../assets/rustdl-icon.png");
        assert!(decode_thumbnail_image(png.to_vec()).is_some());
    }
}
