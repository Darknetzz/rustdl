//! Quality watchlist on the shared [`DownloadCore`].

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::app_state;
use crate::models::QueueItem;
use crate::watchlist::{quality_improved, save_watchlist, unix_now_secs, WatchlistEntry};
use crate::ytdlp;

use super::background_spawn;
use super::core::DownloadCore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchlistAddError {
    EmptyUrl,
    Duplicate,
    MissingItem,
}

impl WatchlistAddError {
    pub fn message(self) -> &'static str {
        match self {
            Self::EmptyUrl => "URL is empty.",
            Self::Duplicate => "URL is already on the watchlist.",
            Self::MissingItem => "Queue item not found.",
        }
    }
}

impl DownloadCore {
    pub fn bump_watchlist_generation(&mut self) {
        self.watchlist_generation = self.watchlist_generation.saturating_add(1);
    }

    fn persist_watchlist(&mut self) {
        if let Err(err) = save_watchlist(&self.watchlist) {
            self.append_log(&format!("Failed to save watchlist: {err}"));
        }
        self.bump_watchlist_generation();
    }

    fn watchlist_url_for_item(&self, item: &QueueItem) -> Option<String> {
        app_state::resolve_item_download_url(item)
            .or_else(|| {
                let w = item.webpage_url.trim();
                if w.starts_with("http://") || w.starts_with("https://") {
                    Some(w.to_owned())
                } else {
                    None
                }
            })
            .or_else(|| {
                let s = item.source_line.trim();
                if s.starts_with("http://") || s.starts_with("https://") {
                    Some(s.to_owned())
                } else {
                    None
                }
            })
    }

    fn baseline_resolution_for_item(&self, item: &QueueItem) -> (Option<u32>, Option<u32>) {
        let mut w = item.width;
        let mut h = item.height;
        if let Some(path) = item.local_path.as_deref() {
            if let Some((pw, ph)) =
                ytdlp::probe_video_resolution_with_path(path, &self.settings.ffprobe_path)
            {
                w = Some(w.map_or(pw, |cur| cur.max(pw)));
                h = Some(h.map_or(ph, |cur| cur.max(ph)));
            }
        }
        (w, h)
    }

    pub fn add_watchlist_from_queue_item(
        &mut self,
        item_id: u64,
    ) -> Result<u64, WatchlistAddError> {
        let idx = self
            .item_idx(item_id)
            .ok_or(WatchlistAddError::MissingItem)?;
        let item = self.items[idx].clone();
        let url = self
            .watchlist_url_for_item(&item)
            .filter(|u| !u.trim().is_empty())
            .ok_or(WatchlistAddError::EmptyUrl)?;
        if self.watchlist.has_url(&url) {
            return Err(WatchlistAddError::Duplicate);
        }
        let (baseline_w, baseline_h) = self.baseline_resolution_for_item(&item);
        let title = if item.title.trim().is_empty() {
            url.clone()
        } else {
            item.title.clone()
        };
        let entry = WatchlistEntry {
            url: url.clone(),
            title,
            video_id: item.video_id.clone(),
            baseline_width: baseline_w,
            baseline_height: baseline_h,
            linked_item_id: Some(item_id),
            ..WatchlistEntry::default()
        };
        let entry_id = self.watchlist.add_entry(entry);
        self.persist_watchlist();
        self.append_log(&format!(
            "Watchlist: added \"{}\" (baseline {}).",
            self.watchlist
                .entries
                .iter()
                .find(|e| e.entry_id == entry_id)
                .map(|e| e.title.as_str())
                .unwrap_or(&url),
            crate::watchlist::format_resolution(baseline_w, baseline_h)
        ));
        if self.settings.watchlist_enabled && self.has_yt_dlp {
            self.request_watchlist_probe_now();
        }
        Ok(entry_id)
    }

    pub fn add_watchlist_url(&mut self, url: String) -> Result<u64, WatchlistAddError> {
        let trimmed = url.trim().to_owned();
        if trimmed.is_empty() {
            return Err(WatchlistAddError::EmptyUrl);
        }
        if self.watchlist.has_url(&trimmed) {
            return Err(WatchlistAddError::Duplicate);
        }
        let entry = WatchlistEntry {
            url: trimmed.clone(),
            title: trimmed.clone(),
            ..WatchlistEntry::default()
        };
        let entry_id = self.watchlist.add_entry(entry);
        self.persist_watchlist();
        self.append_log(&format!("Watchlist: added {trimmed}"));
        if self.settings.watchlist_enabled && self.has_yt_dlp {
            self.request_watchlist_probe_now();
        }
        Ok(entry_id)
    }

    pub fn remove_watchlist_entry(&mut self, entry_id: u64) -> bool {
        if !self.watchlist.remove_entry(entry_id) {
            return false;
        }
        self.persist_watchlist();
        true
    }

    pub fn set_watchlist_entry_paused(&mut self, entry_id: u64, paused: bool) -> bool {
        let Some(idx) = self.watchlist.entry_idx(entry_id) else {
            return false;
        };
        self.watchlist.entries[idx].paused = paused;
        self.persist_watchlist();
        true
    }

    pub fn enqueue_watchlist_entry(&mut self, entry_id: u64) -> bool {
        let Some(entry) = self
            .watchlist
            .entries
            .iter()
            .find(|e| e.entry_id == entry_id)
            .cloned()
        else {
            return false;
        };
        let url = entry.url.clone();
        self.queue_urls_for_resolve(vec![url]);
        if let Some(idx) = self.watchlist.entry_idx(entry_id) {
            self.watchlist.entries[idx].improved_pending = false;
            self.persist_watchlist();
        }
        let _ = entry;
        true
    }

    pub fn request_watchlist_probe_now(&mut self) {
        self.watchlist_last_cycle = None;
    }

    pub fn maybe_schedule_watchlist_poll(&mut self, shared: &super::core::SharedCore) {
        if !self.settings.watchlist_enabled || !self.has_yt_dlp {
            return;
        }
        if self.watchlist_poll_inflight.load(Ordering::Relaxed) {
            return;
        }
        if !self.watchlist.entries.iter().any(|e| !e.paused) {
            return;
        }
        let interval = Duration::from_secs(self.settings.watchlist_poll_hours.max(1) as u64 * 3600);
        if let Some(last) = self.watchlist_last_cycle {
            if Instant::now().saturating_duration_since(last) < interval {
                return;
            }
        }
        self.watchlist_last_cycle = Some(Instant::now());
        background_spawn::spawn_watchlist_poll_cycle(shared.clone());
    }

    pub(crate) fn apply_watchlist_probe_result(
        &mut self,
        entry_id: u64,
        probe: ytdlp::MaxResolutionProbe,
    ) {
        let Some(idx) = self.watchlist.entry_idx(entry_id) else {
            return;
        };
        let title = self.watchlist.entries[idx].title.clone();
        let url = self.watchlist.entries[idx].url.clone();
        let baseline_height = self.watchlist.entries[idx].baseline_height;
        let baseline_width = self.watchlist.entries[idx].baseline_width;
        let entry = &mut self.watchlist.entries[idx];
        entry.last_probe_at = Some(unix_now_secs());
        entry.last_probe_error = probe.error.clone();
        if let Some(err) = &probe.error {
            entry.improved_pending = false;
            self.append_log(&format!("Watchlist probe failed for \"{title}\": {err}"));
            self.persist_watchlist();
            return;
        }
        if !entry.title.trim().is_empty() && entry.title == entry.url {
            entry.title = probe.title.clone();
        }
        if entry.video_id.is_empty() {
            entry.video_id = probe.video_id.clone();
        }
        entry.last_probe_width = probe.width;
        entry.last_probe_height = probe.height;
        if baseline_height.is_none() && baseline_width.is_none() {
            entry.baseline_width = probe.width;
            entry.baseline_height = probe.height;
            entry.improved_pending = false;
            let title = entry.title.clone();
            self.append_log(&format!(
                "Watchlist: baseline for \"{title}\" set to {}.",
                crate::watchlist::format_resolution(probe.width, probe.height)
            ));
            self.persist_watchlist();
            return;
        }
        let improved = quality_improved(
            entry.baseline_height,
            probe.height,
            self.settings.watchlist_min_height_delta,
        );
        let auto_enqueue = self.settings.watchlist_auto_enqueue;
        entry.improved_pending = improved && !auto_enqueue;
        let log_title = entry.title.clone();
        if improved && auto_enqueue {
            entry.improved_pending = false;
        }
        if improved {
            let base = crate::watchlist::format_resolution_height(baseline_height);
            let now = crate::watchlist::format_resolution_height(probe.height);
            self.append_log(&format!(
                "Watchlist: better quality for \"{log_title}\" ({base} → {now})."
            ));
            if auto_enqueue {
                self.queue_urls_for_resolve(vec![url]);
                self.append_log(&format!(
                    "Watchlist: enqueued \"{log_title}\" for download."
                ));
            }
        }
        self.persist_watchlist();
    }
}
