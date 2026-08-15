//! On-disk cache for downloader and Converter queue card thumbnails, alongside the
//! source URL / path metadata needed to validate the cache across restarts.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::rustdl_config_dir;

const DOWNLOADER_SUBDIR: &str = "thumbnails/downloader";
const CONVERT_SUBDIR: &str = "thumbnails/convert";

/// Metadata persisted beside each saved thumbnail image.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloaderThumbnailRecord {
    pub source_key: String,
    pub content_type: String,
    pub webpage_url: String,
    pub thumbnail_url: Option<String>,
    pub source_line: String,
    /// Path relative to [`rustdl_config_dir`] (e.g. `thumbnails/downloader/42.img`).
    pub image_path: String,
}

pub fn downloader_thumbnail_dir() -> PathBuf {
    rustdl_config_dir().join(DOWNLOADER_SUBDIR)
}

fn meta_path(base: &Path, item_id: u64) -> PathBuf {
    base.join(format!("{item_id}.json"))
}

fn image_file(base: &Path, item_id: u64) -> PathBuf {
    base.join(format!("{item_id}.img"))
}

fn relative_image_path(item_id: u64) -> String {
    format!("{DOWNLOADER_SUBDIR}/{item_id}.img")
}

pub struct DownloaderThumbnailSave<'a> {
    pub source_key: &'a str,
    pub content_type: &'a str,
    pub webpage_url: &'a str,
    pub thumbnail_url: Option<&'a str>,
    pub source_line: &'a str,
    pub bytes: &'a [u8],
}

pub fn save_downloader_thumbnail(
    item_id: u64,
    save: DownloaderThumbnailSave<'_>,
) -> Result<String> {
    save_downloader_thumbnail_at(&downloader_thumbnail_dir(), item_id, save)
}

pub fn save_downloader_thumbnail_at(
    base: &Path,
    item_id: u64,
    save: DownloaderThumbnailSave<'_>,
) -> Result<String> {
    fs::create_dir_all(base)
        .with_context(|| format!("failed to create thumbnail directory: {}", base.display()))?;
    let rel = relative_image_path(item_id);
    let record = DownloaderThumbnailRecord {
        source_key: save.source_key.to_owned(),
        content_type: save.content_type.to_owned(),
        webpage_url: save.webpage_url.to_owned(),
        thumbnail_url: save.thumbnail_url.map(str::to_owned),
        source_line: save.source_line.to_owned(),
        image_path: rel.clone(),
    };
    fs::write(image_file(base, item_id), save.bytes)
        .with_context(|| format!("failed to write thumbnail image for item {item_id}"))?;
    let raw =
        serde_json::to_string_pretty(&record).context("failed to serialize thumbnail meta")?;
    fs::write(meta_path(base, item_id), raw)
        .with_context(|| format!("failed to write thumbnail meta for item {item_id}"))?;
    Ok(rel)
}

pub fn load_downloader_thumbnail(item_id: u64, source_key: &str) -> Option<(Vec<u8>, String)> {
    load_downloader_thumbnail_at(&downloader_thumbnail_dir(), item_id, source_key)
}

/// Loads a saved downloader thumbnail when the on-disk image exists, ignoring `source_key`.
pub fn load_downloader_thumbnail_any(item_id: u64) -> Option<(Vec<u8>, String)> {
    load_downloader_thumbnail_any_at(&downloader_thumbnail_dir(), item_id)
}

pub fn load_downloader_thumbnail_at(
    base: &Path,
    item_id: u64,
    source_key: &str,
) -> Option<(Vec<u8>, String)> {
    let meta_raw = fs::read_to_string(meta_path(base, item_id)).ok()?;
    let record: DownloaderThumbnailRecord = serde_json::from_str(&meta_raw).ok()?;
    if record.source_key != source_key {
        return None;
    }
    let bytes = fs::read(image_file(base, item_id)).ok()?;
    if bytes.len() < 32 {
        return None;
    }
    Some((bytes, record.content_type))
}

pub fn load_downloader_thumbnail_any_at(base: &Path, item_id: u64) -> Option<(Vec<u8>, String)> {
    let meta_raw = fs::read_to_string(meta_path(base, item_id)).ok()?;
    let record: DownloaderThumbnailRecord = serde_json::from_str(&meta_raw).ok()?;
    let bytes = fs::read(image_file(base, item_id)).ok()?;
    if bytes.len() < 32 {
        return None;
    }
    Some((bytes, record.content_type))
}

pub fn delete_downloader_thumbnail(item_id: u64) {
    delete_downloader_thumbnail_at(&downloader_thumbnail_dir(), item_id);
}

pub fn delete_downloader_thumbnail_at(base: &Path, item_id: u64) {
    let _ = fs::remove_file(meta_path(base, item_id));
    let _ = fs::remove_file(image_file(base, item_id));
}

/// Removes on-disk thumbnails for queue rows that no longer exist.
pub fn prune_downloader_thumbnails(active_item_ids: &HashSet<u64>) {
    prune_downloader_thumbnails_at(&downloader_thumbnail_dir(), active_item_ids);
}

pub fn prune_downloader_thumbnails_at(base: &Path, active_item_ids: &HashSet<u64>) {
    prune_thumbnails_at(base, active_item_ids, delete_downloader_thumbnail_at);
}

fn prune_thumbnails_at(base: &Path, active_item_ids: &HashSet<u64>, delete: impl Fn(&Path, u64)) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".json")) else {
            continue;
        };
        let Ok(item_id) = stem.parse::<u64>() else {
            continue;
        };
        if !active_item_ids.contains(&item_id) {
            delete(base, item_id);
        }
    }
}

/// Metadata persisted beside each saved Converter queue thumbnail image.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConvertThumbnailRecord {
    pub source_key: String,
    pub content_type: String,
    /// Source file path this thumbnail was extracted from (the Converter's analog of a
    /// downloader item's URL), kept for cache-freshness checks and debugging.
    pub source_path: String,
    /// Path relative to [`rustdl_config_dir`] (e.g. `thumbnails/convert/1000005.img`).
    pub image_path: String,
}

pub fn convert_thumbnail_dir() -> PathBuf {
    rustdl_config_dir().join(CONVERT_SUBDIR)
}

fn convert_meta_path(base: &Path, item_id: u64) -> PathBuf {
    base.join(format!("{item_id}.json"))
}

fn convert_image_file(base: &Path, item_id: u64) -> PathBuf {
    base.join(format!("{item_id}.img"))
}

fn convert_relative_image_path(item_id: u64) -> String {
    format!("{CONVERT_SUBDIR}/{item_id}.img")
}

pub struct ConvertThumbnailSave<'a> {
    pub source_key: &'a str,
    pub content_type: &'a str,
    pub source_path: &'a str,
    pub bytes: &'a [u8],
}

pub fn save_convert_thumbnail(item_id: u64, save: ConvertThumbnailSave<'_>) -> Result<String> {
    save_convert_thumbnail_at(&convert_thumbnail_dir(), item_id, save)
}

pub fn save_convert_thumbnail_at(
    base: &Path,
    item_id: u64,
    save: ConvertThumbnailSave<'_>,
) -> Result<String> {
    fs::create_dir_all(base)
        .with_context(|| format!("failed to create thumbnail directory: {}", base.display()))?;
    let rel = convert_relative_image_path(item_id);
    let record = ConvertThumbnailRecord {
        source_key: save.source_key.to_owned(),
        content_type: save.content_type.to_owned(),
        source_path: save.source_path.to_owned(),
        image_path: rel.clone(),
    };
    fs::write(convert_image_file(base, item_id), save.bytes)
        .with_context(|| format!("failed to write thumbnail image for item {item_id}"))?;
    let raw =
        serde_json::to_string_pretty(&record).context("failed to serialize thumbnail meta")?;
    fs::write(convert_meta_path(base, item_id), raw)
        .with_context(|| format!("failed to write thumbnail meta for item {item_id}"))?;
    Ok(rel)
}

pub fn load_convert_thumbnail(item_id: u64, source_key: &str) -> Option<(Vec<u8>, String)> {
    load_convert_thumbnail_at(&convert_thumbnail_dir(), item_id, source_key)
}

pub fn load_convert_thumbnail_at(
    base: &Path,
    item_id: u64,
    source_key: &str,
) -> Option<(Vec<u8>, String)> {
    let meta_raw = fs::read_to_string(convert_meta_path(base, item_id)).ok()?;
    let record: ConvertThumbnailRecord = serde_json::from_str(&meta_raw).ok()?;
    if record.source_key != source_key {
        return None;
    }
    let bytes = fs::read(convert_image_file(base, item_id)).ok()?;
    if bytes.len() < 32 {
        return None;
    }
    Some((bytes, record.content_type))
}

pub fn delete_convert_thumbnail(item_id: u64) {
    delete_convert_thumbnail_at(&convert_thumbnail_dir(), item_id);
}

pub fn delete_convert_thumbnail_at(base: &Path, item_id: u64) {
    let _ = fs::remove_file(convert_meta_path(base, item_id));
    let _ = fs::remove_file(convert_image_file(base, item_id));
}

/// Removes on-disk Converter thumbnails for queue rows that no longer exist.
pub fn prune_convert_thumbnails(active_item_ids: &HashSet<u64>) {
    prune_convert_thumbnails_at(&convert_thumbnail_dir(), active_item_ids);
}

pub fn prune_convert_thumbnails_at(base: &Path, active_item_ids: &HashSet<u64>) {
    prune_thumbnails_at(base, active_item_ids, delete_convert_thumbnail_at);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn save_load_and_prune_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rel = save_downloader_thumbnail_at(
            dir.path(),
            42,
            DownloaderThumbnailSave {
                source_key: "key-a",
                content_type: "image/png",
                webpage_url: "https://example.com/watch?v=abc",
                thumbnail_url: Some("https://example.com/thumb.jpg"),
                source_line: "https://example.com/watch?v=abc",
                bytes: &[0u8; 64],
            },
        )
        .expect("save");
        assert_eq!(rel, "thumbnails/downloader/42.img");
        let loaded = load_downloader_thumbnail_at(dir.path(), 42, "key-a").expect("load");
        assert_eq!(loaded.0.len(), 64);
        assert_eq!(loaded.1, "image/png");
        assert!(load_downloader_thumbnail_at(dir.path(), 42, "other").is_none());

        let any = load_downloader_thumbnail_any_at(dir.path(), 42).expect("any");
        assert_eq!(any.0.len(), 64);

        let mut active = HashSet::new();
        active.insert(42);
        prune_downloader_thumbnails_at(dir.path(), &active);
        assert!(load_downloader_thumbnail_at(dir.path(), 42, "key-a").is_some());

        active.clear();
        prune_downloader_thumbnails_at(dir.path(), &active);
        assert!(load_downloader_thumbnail_at(dir.path(), 42, "key-a").is_none());
    }

    #[test]
    fn convert_save_load_and_prune_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rel = save_convert_thumbnail_at(
            dir.path(),
            1_000_005,
            ConvertThumbnailSave {
                source_key: "D:\\Videos\\movie.mkv",
                content_type: "image/png",
                source_path: "D:\\Videos\\movie.mkv",
                bytes: &[0u8; 64],
            },
        )
        .expect("save");
        assert_eq!(rel, "thumbnails/convert/1000005.img");
        let loaded = load_convert_thumbnail_at(dir.path(), 1_000_005, "D:\\Videos\\movie.mkv")
            .expect("load");
        assert_eq!(loaded.0.len(), 64);
        assert_eq!(loaded.1, "image/png");
        assert!(load_convert_thumbnail_at(dir.path(), 1_000_005, "other").is_none());

        let mut active = HashSet::new();
        active.insert(1_000_005);
        prune_convert_thumbnails_at(dir.path(), &active);
        assert!(
            load_convert_thumbnail_at(dir.path(), 1_000_005, "D:\\Videos\\movie.mkv").is_some()
        );

        active.clear();
        prune_convert_thumbnails_at(dir.path(), &active);
        assert!(
            load_convert_thumbnail_at(dir.path(), 1_000_005, "D:\\Videos\\movie.mkv").is_none()
        );
    }
}
