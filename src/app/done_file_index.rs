use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::models::QueueItem;
use crate::ytdlp;

/// Max directory entries processed per scan so huge download folders stay responsive.
pub(crate) const DONE_LOOKUP_MAX_ENTRIES: usize = 50_000;

/// Subfolder depth when indexing the output directory (playlist/uploader templates).
const DONE_LOOKUP_MAX_DEPTH: u32 = 8;

/// Indexes `video_id` → output file path and last-modified time using `[id]` segments in filenames.
pub(crate) struct DoneFileIndex {
    pub(crate) lookup: HashMap<String, (PathBuf, SystemTime)>,
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
        let mut seen = 0usize;
        self.scan_output_tree(path, &mut seen, 0);
    }

    fn scan_output_tree(&mut self, dir: &Path, seen: &mut usize, depth: u32) {
        if depth > DONE_LOOKUP_MAX_DEPTH {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            *seen += 1;
            if *seen > DONE_LOOKUP_MAX_ENTRIES {
                self.scan_truncated = true;
                return;
            }
            let p = entry.path();
            if p.is_dir() {
                self.scan_output_tree(&p, seen, depth + 1);
                if self.scan_truncated {
                    return;
                }
                continue;
            }
            if !p.is_file() || is_temporary_download_name(&p) {
                continue;
            }
            let Some(fname) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let mtime = fs::metadata(&p)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            for id in filename_index_ids(fname) {
                self.lookup.insert(id, (p.clone(), mtime));
            }
        }
    }

    pub(crate) fn find(&self, video_id: &str) -> Option<(PathBuf, SystemTime)> {
        let id = video_id.trim();
        if id.is_empty() {
            return None;
        }
        self.lookup.get(id).cloned()
    }

    /// Match a queue row to an on-disk download using id fields, URLs, title hints, etc.
    pub(crate) fn find_path_for_queue_item(
        &self,
        output_dir: &str,
        item: &QueueItem,
    ) -> Option<(PathBuf, SystemTime)> {
        if let Some(ref saved) = item.local_path {
            if let Some(path) = resolve_path_under_output(output_dir, saved) {
                let mtime = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                return Some((path, mtime));
            }
        }
        self.find_path_in_index(item)
    }

    /// Index-only lookup (video id, title brackets, title hint).
    pub(crate) fn find_path_in_index(&self, item: &QueueItem) -> Option<(PathBuf, SystemTime)> {
        let try_id = |id: &str| -> Option<(PathBuf, SystemTime)> {
            let id = id.trim();
            if id.is_empty() {
                None
            } else {
                self.find(id)
            }
        };
        if let Some(hit) = try_id(&item.video_id) {
            return Some(hit);
        }
        if let Some(vid) = ytdlp::youtube_video_id_from_item(item) {
            if let Some(hit) = try_id(&vid) {
                return Some(hit);
            }
        }
        for id in title_bracket_ids(&item.title) {
            if let Some(hit) = try_id(&id) {
                return Some(hit);
            }
        }
        find_unique_by_title_hint(self, &item.title)
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
                push_unique(&mut out, inner);
            }
            rest = &rest[j + 1..];
        } else {
            break;
        }
    }
    out
}

/// Ids extracted from a filename for the done-file lookup table.
pub(crate) fn filename_index_ids(file_name: &str) -> Vec<String> {
    let stem = file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(file_name);
    let mut out = bracket_ids_in_filename(file_name);
    if ytdlp::is_plausible_youtube_video_id(stem) {
        push_unique(&mut out, stem);
    }
    if let Some(last) = stem.rsplit(" - ").next() {
        let last = last.trim();
        if ytdlp::is_plausible_youtube_video_id(last) {
            push_unique(&mut out, last);
        }
    }
    out
}

fn title_bracket_ids(title: &str) -> Vec<String> {
    bracket_ids_in_filename(title)
}

fn push_unique(out: &mut Vec<String>, id: &str) {
    let id = id.trim();
    if id.is_empty() || out.iter().any(|x| x == id) {
        return;
    }
    out.push(id.to_owned());
}

fn title_hint_fragment(title: &str) -> String {
    let before_bracket = title.split('[').next().unwrap_or(title).trim();
    let mut s: String = before_bracket
        .chars()
        .take(48)
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    s = s.trim().to_ascii_lowercase();
    s
}

fn is_temporary_download_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".part")
        || lower.ends_with(".ytdl")
        || lower.ends_with(".temp")
        || lower.ends_with(".tmp")
}

/// Resolve a stored or relative path to a file under `output_dir`.
pub(crate) fn resolve_path_under_output(output_dir: &str, raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let root = Path::new(output_dir);
    let direct = PathBuf::from(raw);
    let candidates = {
        #[allow(unused_mut)]
        let mut v = vec![direct.clone(), root.join(&direct)];
        #[cfg(windows)]
        {
            let norm = raw.replace('/', "\\");
            if norm != raw {
                v.push(PathBuf::from(&norm));
                v.push(root.join(&norm));
            }
        }
        v
    };
    for path in candidates {
        if path.is_file() && path_is_under_output_dir(output_dir, &path) {
            return Some(path.canonicalize().unwrap_or(path));
        }
    }
    None
}

/// True when `file` is the same as or nested under `output_dir` (handles SMB and Windows casing).
pub(crate) fn path_is_under_output_dir(output_dir: &str, file: &Path) -> bool {
    let root = Path::new(output_dir);
    if file.starts_with(root) {
        return true;
    }
    if let (Ok(root_canon), Ok(file_canon)) = (root.canonicalize(), file.canonicalize()) {
        if file_canon.starts_with(&root_canon) {
            return true;
        }
    }
    #[cfg(windows)]
    {
        let norm = |p: &Path| {
            p.to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase()
        };
        let r = norm(root);
        let f = norm(file);
        !r.is_empty() && (f == r || f.starts_with(&format!("{r}\\")))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn find_unique_by_title_hint(
    index: &DoneFileIndex,
    title: &str,
) -> Option<(PathBuf, SystemTime)> {
    let hint = title_hint_fragment(title);
    if hint.len() < 6 {
        return None;
    }
    let mut hit: Option<(PathBuf, SystemTime)> = None;
    for (path, mtime) in index.lookup.values() {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.to_ascii_lowercase().contains(&hint) {
            if hit.is_some() {
                return None;
            }
            hit = Some((path.clone(), *mtime));
        }
    }
    hit
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

    #[test]
    fn filename_index_ids_includes_bare_youtube_stem() {
        let ids = filename_index_ids("dQw4w9WgXcQ.mp4");
        assert!(ids.iter().any(|id| id == "dQw4w9WgXcQ"));
    }

    #[test]
    fn path_is_under_output_dir_accepts_nested_file() {
        let dir = std::env::temp_dir().join("rustdl_done_index_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("clip.mp4");
        std::fs::write(&file, b"x").unwrap();
        assert!(super::path_is_under_output_dir(
            &dir.to_string_lossy(),
            &file
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_path_for_queue_item_matches_title_bracket_id() {
        let mut index = DoneFileIndex::new();
        index.lookup.insert(
            "dQw4w9WgXcQ".to_owned(),
            (
                PathBuf::from("/tmp/My Song [dQw4w9WgXcQ].mp4"),
                SystemTime::UNIX_EPOCH,
            ),
        );
        let item = QueueItem {
            video_id: String::new(),
            title: "My Song [dQw4w9WgXcQ]".to_owned(),
            ..QueueItem::default()
        };
        assert!(index
            .find_path_for_queue_item("/tmp", &item)
            .is_some());
    }

    #[test]
    fn resolve_path_under_output_joins_relative_destination() {
        let dir = std::env::temp_dir().join("rustdl_resolve_path_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("clip.mp4");
        std::fs::write(&file, b"x").unwrap();
        let dir_s = dir.to_string_lossy().to_string();
        assert_eq!(
            super::resolve_path_under_output(&dir_s, "clip.mp4"),
            Some(file.canonicalize().unwrap_or(file.clone()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
