use std::path::PathBuf;

use eframe::egui;
use image::imageops::FilterType;

use crate::convert_state::convert_source_path_missing;

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
        if self.items.len() <= THUMBNAIL_QUEUE_SOFT_CAP {
            return true;
        }
        self.item_idx(item_id).is_some_and(|idx| {
            matches!(
                self.items[idx].status,
                crate::models::ItemStatus::Idle
                    | crate::models::ItemStatus::Queued
                    | crate::models::ItemStatus::Downloading
                    | crate::models::ItemStatus::Resolving
            )
        })
    }

    pub(super) fn queue_thumbnail_load(&mut self, item_id: u64, url: String) {
        if !self.thumbnails_allowed_for_queue(item_id) {
            return;
        }
        if self.textures.contains_key(&item_id) || self.thumbnail_inflight.contains(&item_id) {
            return;
        }
        self.thumbnail_inflight.insert(item_id);
        let source_key = self.item_idx(item_id).map(|idx| {
            crate::service::core::DownloadCore::queue_thumbnail_source_key(&self.items[idx])
        });
        let bus = self.ui_bus.clone();
        let rt = self.runtime.clone();
        let client = self.http_client.clone();
        let sem = self.thumb_semaphore.clone();
        let shared_core = self.shared_core.clone();
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
            let bytes = if let Some(key) = source_key.as_ref() {
                shared_core
                    .lock()
                    .cached_thumbnail_bytes(item_id, key)
                    .map(|(b, _)| b)
            } else {
                None
            };
            let bytes = if bytes.is_some() {
                bytes
            } else {
                crate::ytdlp::fetch_thumbnail_bytes(&client, &url)
                    .await
                    .map(|(b, content_type)| {
                        if let Some(key) = source_key.as_ref() {
                            shared_core.lock().cache_thumbnail_bytes(
                                item_id,
                                key.clone(),
                                b.clone(),
                                content_type,
                            );
                        }
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

#[cfg(test)]
mod tests {
    use super::decode_thumbnail_image;

    #[test]
    fn decode_thumbnail_accepts_tiny_png() {
        let png = include_bytes!("../../assets/rustdl-icon.png");
        assert!(decode_thumbnail_image(png.to_vec()).is_some());
    }
}
