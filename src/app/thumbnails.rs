use super::{events::try_send_ui, PydlApp, UiEvent};

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
                        bytes: None,
                    },
                );
                return;
            };
            let bytes = match client.get(&url).send().await {
                Ok(resp) => resp.bytes().await.ok().map(|b| b.to_vec()),
                Err(_) => None,
            };
            try_send_ui(&tx, UiEvent::ThumbnailFetched { item_id, bytes });
        });
    }
}
