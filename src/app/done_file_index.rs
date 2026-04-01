use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Max directory entries processed per scan so huge download folders stay responsive.
pub(crate) const DONE_LOOKUP_MAX_ENTRIES: usize = 50_000;

/// Indexes `video_id` → output file path using `[id]` segments in filenames.
pub(crate) struct DoneFileIndex {
    pub(crate) lookup: HashMap<String, PathBuf>,
    cached_output_dir: String,
    cached_dir_mtime: Option<SystemTime>,
    force_refresh: bool,
    pub(crate) scan_truncated: bool,
}

impl DoneFileIndex {
    pub(crate) fn new() -> Self {
        Self {
            lookup: HashMap::new(),
            cached_output_dir: String::new(),
            cached_dir_mtime: None,
            force_refresh: true,
            scan_truncated: false,
        }
    }

    pub(crate) fn force_refresh(&mut self) {
        self.force_refresh = true;
    }

    /// Refreshes the index when the output folder path or its mtime changes.
    pub(crate) fn refresh(&mut self, output_dir: &str) {
        let path = Path::new(output_dir);
        let mtime = if path.is_dir() {
            fs::metadata(path).and_then(|m| m.modified()).ok()
        } else {
            None
        };

        let dirty = self.force_refresh
            || self.cached_output_dir != output_dir
            || self.cached_dir_mtime != mtime;

        if !dirty {
            return;
        }

        self.force_refresh = false;
        self.cached_output_dir = output_dir.to_owned();
        self.cached_dir_mtime = mtime;
        self.lookup.clear();
        self.scan_truncated = false;

        if !path.is_dir() {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        let mut seen = 0usize;
        for entry in entries.flatten() {
            seen += 1;
            if seen > DONE_LOOKUP_MAX_ENTRIES {
                self.scan_truncated = true;
                break;
            }
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let Some(fname) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            for id in bracket_ids_in_filename(fname) {
                self.lookup.insert(id, p.clone());
            }
        }
    }

    pub(crate) fn find(&self, video_id: &str) -> Option<PathBuf> {
        let id = video_id.trim();
        if id.is_empty() {
            return None;
        }
        self.lookup.get(id).cloned()
    }
}

/// Bracketed segments in a download filename stem (`title [id].ext` → id), for indexing output files.
pub(crate) fn bracket_ids_in_filename(file_name: &str) -> Vec<String> {
    let stem = file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(file_name);
    let mut out = Vec::new();
    let mut rest = stem;
    while let Some(i) = rest.find('[') {
        rest = &rest[i + 1..];
        if let Some(j) = rest.find(']') {
            let inner = rest[..j].trim();
            if !inner.is_empty() {
                out.push(inner.to_owned());
            }
            rest = &rest[j + 1..];
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_ids_single() {
        assert_eq!(
            bracket_ids_in_filename("My Video [dQw4w9WgXcQ].mp4"),
            vec!["dQw4w9WgXcQ".to_owned()]
        );
    }

    #[test]
    fn bracket_ids_multiple_in_stem() {
        let ids = bracket_ids_in_filename("a [id1] b [id2].mkv");
        assert!(ids.contains(&"id1".to_owned()));
        assert!(ids.contains(&"id2".to_owned()));
    }

    #[test]
    fn bracket_ids_no_extension_uses_whole_name() {
        assert_eq!(
            bracket_ids_in_filename("clip [xyz]"),
            vec!["xyz".to_owned()]
        );
    }
}
