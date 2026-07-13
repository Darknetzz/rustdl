//! URL quality watchlist — re-probe sources and detect when higher resolution appears.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::rustdl_config_dir;
use crate::ytdlp;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchlistEntry {
    pub entry_id: u64,
    pub url: String,
    pub title: String,
    pub video_id: String,
    /// Resolution of the copy we already have (or last known download).
    pub baseline_height: Option<u32>,
    pub baseline_width: Option<u32>,
    pub last_probe_height: Option<u32>,
    pub last_probe_width: Option<u32>,
    pub last_probe_at: Option<u64>,
    pub last_probe_error: Option<String>,
    /// Set when a probe finds higher max resolution than baseline.
    pub improved_pending: bool,
    pub linked_item_id: Option<u64>,
    pub paused: bool,
    pub note: String,
}

impl Default for WatchlistEntry {
    fn default() -> Self {
        Self {
            entry_id: 0,
            url: String::new(),
            title: String::new(),
            video_id: String::new(),
            baseline_height: None,
            baseline_width: None,
            last_probe_height: None,
            last_probe_width: None,
            last_probe_at: None,
            last_probe_error: None,
            improved_pending: false,
            linked_item_id: None,
            paused: false,
            note: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WatchlistStore {
    pub entries: Vec<WatchlistEntry>,
    pub next_entry_id: u64,
}

impl WatchlistStore {
    pub fn entry_idx(&self, entry_id: u64) -> Option<usize> {
        self.entries.iter().position(|e| e.entry_id == entry_id)
    }

    pub fn has_url(&self, url: &str) -> bool {
        let key = ytdlp::normalize_url_for_dedupe(url);
        if key.is_empty() {
            return false;
        }
        self.entries
            .iter()
            .any(|e| ytdlp::normalize_url_for_dedupe(&e.url) == key)
    }

    pub fn add_entry(&mut self, mut entry: WatchlistEntry) -> u64 {
        let id = self.next_entry_id.max(1);
        self.next_entry_id = id.saturating_add(1);
        entry.entry_id = id;
        self.entries.push(entry);
        id
    }

    pub fn remove_entry(&mut self, entry_id: u64) -> bool {
        let Some(idx) = self.entry_idx(entry_id) else {
            return false;
        };
        self.entries.remove(idx);
        true
    }
}

pub fn watchlist_file_path() -> PathBuf {
    rustdl_config_dir().join("rustdl_watchlist.json")
}

pub fn load_watchlist() -> WatchlistStore {
    let path = watchlist_file_path();
    if !path.exists() {
        return WatchlistStore::default();
    }
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => WatchlistStore::default(),
    }
}

pub fn save_watchlist(store: &WatchlistStore) -> Result<()> {
    let path = watchlist_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(store).context("serialize watchlist")?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &raw).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("rename {}", path.display()))?;
    Ok(())
}

pub fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// True when `probe_height` exceeds `baseline_height` by at least `min_height_delta` pixels.
pub fn quality_improved(
    baseline_height: Option<u32>,
    probe_height: Option<u32>,
    min_height_delta: u32,
) -> bool {
    let Some(probe) = probe_height else {
        return false;
    };
    let Some(base) = baseline_height else {
        return probe > 0;
    };
    let delta = min_height_delta.max(1);
    probe >= base.saturating_add(delta)
}

pub fn format_resolution(width: Option<u32>, height: Option<u32>) -> String {
    match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => format!("{w}×{h}"),
        (_, Some(h)) if h > 0 => format!("{h}p"),
        (Some(w), _) if w > 0 => format!("{w}w"),
        _ => "—".to_owned(),
    }
}

pub fn format_resolution_height(height: Option<u32>) -> String {
    height
        .map(|h| format!("{h}p"))
        .unwrap_or_else(|| "—".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_improved_requires_delta() {
        assert!(!quality_improved(Some(720), Some(720), 1));
        assert!(quality_improved(Some(360), Some(720), 1));
        assert!(!quality_improved(Some(360), Some(400), 100));
        assert!(quality_improved(None, Some(1080), 1));
        assert!(!quality_improved(Some(720), None, 1));
    }
}
