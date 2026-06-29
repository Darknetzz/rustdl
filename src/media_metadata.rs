//! Formatting and display rows for downloader queue item metadata.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};

use crate::models::QueueItem;
use crate::time_format::{format_absolute_local, format_relative_ago};

/// Rows for the per-item **More info** popup: `(section title, label, value)`.
pub struct InfoRow {
    pub section: &'static str,
    pub label: &'static str,
    pub value: String,
}

pub fn format_yyyymmdd(date: &str) -> Option<String> {
    let d = date.trim();
    if d.len() != 8 || !d.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year = d[..4].parse().ok()?;
    let month = d[4..6].parse().ok()?;
    let day = d[6..8].parse().ok()?;
    let nd = NaiveDate::from_ymd_opt(year, month, day)?;
    Some(nd.format("%Y-%m-%d").to_string())
}

pub fn format_upload_moment(upload_date: Option<&str>, upload_timestamp: Option<i64>) -> Option<String> {
    if let Some(ts) = upload_timestamp.filter(|t| *t > 0) {
        let dt = Utc.timestamp_opt(ts, 0).single()?;
        let local: DateTime<Local> = dt.into();
        let abs = local.format("%Y-%m-%d %H:%M").to_string();
        let ago = format_relative_ago(SystemTime::UNIX_EPOCH + Duration::from_secs(ts as u64));
        return Some(format!("{abs} ({ago})"));
    }
    upload_date
        .and_then(|d| format_yyyymmdd(d))
        .map(|d| d)
}

pub fn format_container_time(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "—".to_owned();
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        let local: DateTime<Local> = dt.into();
        return local.format("%Y-%m-%d %H:%M:%S").to_string();
    }
    trimmed.to_owned()
}

pub fn format_unix_secs(secs: u64) -> String {
    let t = UNIX_EPOCH + Duration::from_secs(secs);
    format!("{} ({})", format_absolute_local(t), format_relative_ago(t))
}

pub fn format_duration_secs(sec: i64) -> String {
    if sec < 0 {
        return "—".to_owned();
    }
    let h = sec / 3600;
    let m = (sec % 3600) / 60;
    let s = sec % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn format_view_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub fn queue_item_more_info_rows(item: &QueueItem) -> Vec<InfoRow> {
    let mut rows = Vec::new();
    let push = |rows: &mut Vec<InfoRow>, section: &'static str, label: &'static str, value: String| {
        if value.is_empty() || value == "—" {
            return;
        }
        rows.push(InfoRow {
            section,
            label,
            value,
        });
    };

    push(
        &mut rows,
        "Source",
        "Title",
        if item.title.is_empty() {
            "(no title)".to_owned()
        } else {
            item.title.clone()
        },
    );
    if !item.video_id.is_empty() {
        push(&mut rows, "Source", "Video ID", item.video_id.clone());
    }
    if let Some(u) = item.uploader.as_deref().filter(|s| !s.is_empty()) {
        push(&mut rows, "Source", "Uploader", u.to_owned());
    }
    if let Some(d) = item.duration {
        push(
            &mut rows,
            "Source",
            "Duration",
            format_duration_secs(d),
        );
    }
    if let Some(v) = format_upload_moment(item.upload_date.as_deref(), item.upload_timestamp) {
        push(&mut rows, "Source", "Published", v);
    } else if let Some(d) = item
        .upload_date
        .as_deref()
        .and_then(format_yyyymmdd)
    {
        push(&mut rows, "Source", "Upload date", d);
    }
    if let Some(rd) = item.release_date.as_deref().and_then(|d| format_yyyymmdd(d)) {
        push(&mut rows, "Source", "Release date", rd);
    }
    if let Some(n) = item.view_count {
        push(&mut rows, "Source", "Views", format_view_count(n));
    }
    if let Some(ls) = item.live_status.as_deref().filter(|s| !s.is_empty()) {
        push(&mut rows, "Source", "Live", ls.to_owned());
    }
    if let Some((w, h)) = item.width.zip(item.height).filter(|(w, h)| *w > 0 && *h > 0) {
        push(
            &mut rows,
            "Source",
            "Max resolution (probe)",
            format!("{w}×{h}"),
        );
    }
    let url = if !item.webpage_url.trim().is_empty() {
        item.webpage_url.clone()
    } else {
        item.source_line.clone()
    };
    if url.starts_with("http://") || url.starts_with("https://") {
        push(&mut rows, "Source", "URL", url);
    }
    if let Some(pt) = item.playlist_title.as_deref().filter(|s| !s.is_empty()) {
        let mut pl = pt.to_owned();
        if let Some(ix) = item.playlist_index {
            pl.push_str(&format!(" (#{ix})"));
        }
        push(&mut rows, "Source", "Playlist", pl);
    }

    let has_file = item.file_format.is_some()
        || item.file_creation_time.is_some()
        || item.file_encoder.is_some()
        || item.audio_codec.is_some()
        || item.file_bitrate_bps.is_some()
        || item.file_saved_mtime.is_some()
        || !item.video_codec.is_empty()
        || item.local_path.is_some();
    if has_file {
        if let Some(p) = item.local_path.as_deref().filter(|s| !s.is_empty()) {
            push(&mut rows, "Downloaded file", "Path", p.to_owned());
        }
        if let Some(fmt) = item.file_format.as_deref() {
            push(&mut rows, "Downloaded file", "Container", fmt.to_owned());
        }
        if !item.video_codec.is_empty() {
            push(
                &mut rows,
                "Downloaded file",
                "Video codec",
                item.video_codec.to_uppercase(),
            );
        }
        if let Some(a) = item.audio_codec.as_deref().filter(|s| !s.is_empty()) {
            push(
                &mut rows,
                "Downloaded file",
                "Audio codec",
                a.to_uppercase(),
            );
        }
        if let Some(fps) = item.fps.filter(|f| *f > 0.0) {
            push(
                &mut rows,
                "Downloaded file",
                "Frame rate",
                format!("{fps:.2} fps"),
            );
        }
        if let Some(bps) = item.file_bitrate_bps {
            push(
                &mut rows,
                "Downloaded file",
                "Bitrate",
                format_bitrate(bps),
            );
        }
        if let Some(ct) = item.file_creation_time.as_deref() {
            push(
                &mut rows,
                "Downloaded file",
                "Container date",
                format_container_time(ct),
            );
        }
        if let Some(enc) = item.file_encoder.as_deref().filter(|s| !s.is_empty()) {
            push(&mut rows, "Downloaded file", "Encoder tag", enc.to_owned());
        }
        if let Some(ts) = item.file_saved_mtime {
            push(
                &mut rows,
                "Downloaded file",
                "Saved on disk",
                format_unix_secs(ts),
            );
        }
        if let Some(ts) = item.completed_at {
            push(
                &mut rows,
                "Downloaded file",
                "Marked done",
                format_unix_secs(ts),
            );
        }
    }

    rows
}

fn format_bitrate(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.2} Mbps", bps as f64 / 1_000_000.0)
    } else if bps >= 1_000 {
        format!("{:.0} kbps", bps as f64 / 1_000.0)
    } else {
        format!("{bps} bps")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_yyyymmdd_parses() {
        assert_eq!(format_yyyymmdd("20240315").as_deref(), Some("2024-03-15"));
        assert!(format_yyyymmdd("bad").is_none());
    }

    #[test]
    fn upload_moment_prefers_timestamp() {
        let s = format_upload_moment(Some("20240101"), Some(1_704_067_200));
        assert!(s.is_some());
        assert!(s.unwrap().contains("2024"));
    }
}
