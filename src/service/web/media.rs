//! Stream completed downloads to the LAN web UI (HTTP Range / seeking).

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::header;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio_util::io::ReaderStream;

use crate::app::done_file_index::DoneFileIndex;
use crate::models::{ItemStatus, QueueItem};
use crate::service::core::DownloadCore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Audio,
}

pub fn media_kind_for_path(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "webm" | "mkv" | "mov" | "m4v" | "avi" => Some(MediaKind::Video),
        "mp3" | "m4a" | "opus" | "ogg" | "flac" | "wav" | "aac" => Some(MediaKind::Audio),
        _ => None,
    }
}

pub fn mime_for_path(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "opus" => "audio/opus",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "aac" => "audio/aac",
        _ => "application/octet-stream",
    }
}

pub fn item_media_playable(core: &DownloadCore, item: &QueueItem) -> bool {
    if !matches!(item.status, ItemStatus::Done | ItemStatus::Failed) {
        return false;
    }
    resolve_item_media_path(core, item)
        .ok()
        .and_then(|p| media_kind_for_path(&p))
        .is_some()
}

pub fn item_media_filename(core: &DownloadCore, item: &QueueItem) -> Option<String> {
    let path = resolve_item_media_path(core, item).ok()?;
    path.file_name().and_then(|n| n.to_str()).map(str::to_owned)
}

pub fn resolve_item_media_path(
    core: &DownloadCore,
    item: &QueueItem,
) -> Result<PathBuf, StatusCode> {
    let (path, _) = core
        .done_file_index
        .find_path_for_queue_item(&core.output_dir, item)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(path)
}

pub fn resolve_item_media_path_from_index(
    output_dir: &str,
    index: &DoneFileIndex,
    item: &QueueItem,
) -> Result<PathBuf, StatusCode> {
    index
        .find_path_for_queue_item(output_dir, item)
        .map(|(path, _)| path)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn stream_media_path(path: &Path, headers: &HeaderMap) -> Result<Response, StatusCode> {
    if media_kind_for_path(path).is_none() {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let total = meta.len();
    if total == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    let mime = mime_for_path(path);
    let mut file = File::open(path).await.map_err(|_| StatusCode::NOT_FOUND)?;

    let (start, end) = if let Some(range) = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_range(v, total))
    {
        range
    } else {
        (0, total.saturating_sub(1))
    };

    let len = end.saturating_sub(start).saturating_add(1);
    file.seek(SeekFrom::Start(start))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let stream = ReaderStream::new(file.take(len));
    let body = Body::from_stream(stream);

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, max-age=60");

    if start == 0 && end + 1 >= total {
        builder = builder
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, total.to_string());
    } else {
        builder = builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{end}/{total}"),
            )
            .header(header::CONTENT_LENGTH, len.to_string());
    }

    builder
        .body(body)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Inclusive byte range (`start`, `end`) for file length `total`.
fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    if let Some((start, end)) = spec.split_once('-') {
        if start.is_empty() {
            let suffix: u64 = end.parse().ok()?;
            if suffix == 0 {
                return None;
            }
            let start = total.saturating_sub(suffix);
            return Some((start, total.saturating_sub(1)));
        }
        let start: u64 = start.parse().ok()?;
        let end = if end.is_empty() {
            total.saturating_sub(1)
        } else {
            end.parse().ok()?
        };
        if start > end || end >= total {
            return None;
        }
        return Some((start, end));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_suffix() {
        let (s, e) = parse_range("bytes=-500", 1000).unwrap();
        assert_eq!(s, 500);
        assert_eq!(e, 999);
    }

    #[test]
    fn parse_range_open_end() {
        let (s, e) = parse_range("bytes=0-", 100).unwrap();
        assert_eq!(s, 0);
        assert_eq!(e, 99);
    }
}
