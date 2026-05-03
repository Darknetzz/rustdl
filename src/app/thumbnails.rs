use eframe::egui;

use super::{events::try_send_ui, PydlApp, UiEvent};

fn decode_thumbnail_image(bytes: Vec<u8>) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let raw = rgba.into_raw();
    Some(egui::ColorImage::from_rgba_unmultiplied(size, &raw))
}

impl PydlApp {
    pub(super) fn queue_thumbnail_load(&mut self, item_id: u64, url: String) {
        if self.textures.contains_key(&item_id) || self.thumbnail_inflight.contains(&item_id) {
            return;
        }
        self.thumbnail_inflight.insert(item_id);
        let tx = self.tx.clone();
        let rt = self.runtime.clone();
        let client = self.http_client.clone();
        let sem = self.thumb_semaphore.clone();
        rt.spawn(async move {
            let permit = sem.acquire_owned().await;
            let Ok(_permit) = permit else {
                try_send_ui(
                    &tx,
                    UiEvent::ThumbnailFetched {
                        item_id,
                        image: None,
                    },
                );
                return;
            };
            let bytes = match client.get(&url).send().await {
                Ok(resp) => resp.bytes().await.ok().map(|b| b.to_vec()),
                Err(_) => None,
            };
            let image = match bytes {
                None => None,
                Some(b) => tokio::task::spawn_blocking(move || decode_thumbnail_image(b))
                    .await
                    .ok()
                    .flatten(),
            };
            try_send_ui(&tx, UiEvent::ThumbnailFetched { item_id, image });
        });
    }
}
