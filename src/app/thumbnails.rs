use std::path::PathBuf;

use eframe::egui;
use image::imageops::FilterType;

use super::queue_cache::{THUMBNAIL_DECODE_MAX_WIDTH, THUMBNAIL_QUEUE_SOFT_CAP};
use super::{background_spawn, events::try_send_ui, PydlApp, UiEvent};

fn decode_thumbnail_image(bytes: Vec<u8>) -> Option<egui::ColorImage> {
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
        let bus = self.ui_bus.clone();
        let rt = self.runtime.clone();
        let client = self.http_client.clone();
        let sem = self.thumb_semaphore.clone();
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
            let fetch_url = crate::ytdlp::normalize_thumbnail_url(&url);
            let mut req = client.get(&fetch_url);
            if crate::ytdlp::thumbnail_request_needs_referer(&fetch_url) {
                req = req.header("Referer", "https://www.youtube.com/");
            }
            let bytes = match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    resp.bytes().await.ok().map(|b| b.to_vec())
                }
                _ => None,
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

    pub(super) fn queue_av1_local_thumbnail(
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
        background_spawn::spawn_av1_local_thumbnail(
            &self.runtime,
            &self.ui_bus,
            item_id,
            file_path,
            ffmpeg_path,
        );
    }

    /// Loads local-video thumbnails (egui textures) for any mirrored AV1 rows that lack one.
    /// AV1 queue state itself is owned by `DownloadCore`; only the textures are GUI-local.
    pub(super) fn ensure_av1_thumbnails(&mut self) {
        if !self.settings.show_thumbnails || !self.has_ffmpeg || self.av1_items.is_empty() {
            return;
        }
        let ffmpeg_path = self.settings.ffmpeg_path.clone();
        let pending: Vec<(u64, PathBuf)> = self
            .av1_items
            .iter()
            .filter(|it| {
                !self.textures.contains_key(&it.item_id)
                    && !self.thumbnail_inflight.contains(&it.item_id)
            })
            .map(|it| (it.item_id, PathBuf::from(&it.source_path)))
            .collect();
        for (item_id, path) in pending {
            self.queue_av1_local_thumbnail(item_id, path, ffmpeg_path.clone());
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
