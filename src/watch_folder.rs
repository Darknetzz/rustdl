//! Poll-based folder watchers for auto-enqueue (downloader) and auto-scan (converter).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct WatchFolderState {
    seen_downloader: HashSet<PathBuf>,
    seen_convert: HashSet<PathBuf>,
    last_poll: Option<SystemTime>,
}

impl Default for WatchFolderState {
    fn default() -> Self {
        Self {
            seen_downloader: HashSet::new(),
            seen_convert: HashSet::new(),
            last_poll: None,
        }
    }
}

impl WatchFolderState {
    pub fn new() -> Self {
        Self::default()
    }

    fn should_poll(&mut self) -> bool {
        let now = SystemTime::now();
        let due = self
            .last_poll
            .map(|t| now.duration_since(t).unwrap_or(Duration::ZERO) >= POLL_INTERVAL)
            .unwrap_or(true);
        if due {
            self.last_poll = Some(now);
        }
        due
    }

    pub fn poll_downloader_folder(&mut self, folder: &Path) -> Vec<String> {
        if !self.should_poll() {
            return vec![];
        }
        collect_new_url_files(folder, &mut self.seen_downloader)
    }

    pub fn poll_convert_folder(&mut self, folder: &Path) -> Vec<String> {
        if !self.should_poll() {
            return vec![];
        }
        collect_new_media_files(folder, &mut self.seen_convert)
    }
}

fn collect_new_url_files(folder: &Path, seen: &mut HashSet<PathBuf>) -> Vec<String> {
    if !folder.is_dir() {
        return vec![];
    }
    let mut urls = Vec::new();
    let Ok(entries) = fs::read_dir(folder) else {
        return urls;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext != "url" && ext != "txt" {
            continue;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&path) {
            for line in raw.lines() {
                let line = line.trim();
                if line.starts_with("http://") || line.starts_with("https://") {
                    urls.push(line.to_owned());
                }
            }
        }
    }
    urls
}

fn collect_new_media_files(folder: &Path, seen: &mut HashSet<PathBuf>) -> Vec<String> {
    if !folder.is_dir() {
        return vec![];
    }
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir(folder) else {
        return paths;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if !crate::transcode::is_video_path(&path) {
            continue;
        }
        if seen.insert(path.clone()) {
            paths.push(path.to_string_lossy().into_owned());
        }
    }
    paths
}
