//! Download output layout: yt-dlp template composition and post-download path moves.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{AppSettings, DEFAULT_OUTPUT_FILENAME_TEMPLATE};
use crate::models::QueueItem;

pub const FOLDER_FLAT: &str = "flat";
pub const FOLDER_UPLOADER: &str = "uploader";
pub const FOLDER_PLAYLIST: &str = "playlist";
pub const FOLDER_DATE_YM: &str = "date_ym";
pub const FOLDER_CUSTOM: &str = "custom";

pub const FILENAME_TITLE_ID: &str = "title_id";
pub const FILENAME_DATE_TITLE_ID: &str = "date_title_id";
pub const FILENAME_PLAYLIST_INDEX_TITLE_ID: &str = "playlist_index_title_id";
pub const FILENAME_TITLE_ONLY: &str = "title_only";
pub const FILENAME_CUSTOM: &str = "custom";

const SIDECAR_EXTENSIONS: &[&str] = &[
    "info.json", "description", "annotations.xml", "meta.json", "vtt", "srt", "ass", "lrc",
];

/// Default preset template: flat folder + title with id.
pub fn default_preset_template() -> String {
    compose_template_parts(FOLDER_FLAT, FILENAME_TITLE_ID)
}

/// True when the saved template differs from the default preset and should use custom mode.
pub fn template_implies_custom_mode(template: &str) -> bool {
    let t = template.trim();
    if t.is_empty() || t == DEFAULT_OUTPUT_FILENAME_TEMPLATE {
        return false;
    }
    t != default_preset_template()
}

pub fn normalize_organize_folder(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        FOLDER_UPLOADER => FOLDER_UPLOADER.to_owned(),
        FOLDER_PLAYLIST => FOLDER_PLAYLIST.to_owned(),
        FOLDER_DATE_YM => FOLDER_DATE_YM.to_owned(),
        FOLDER_CUSTOM => FOLDER_CUSTOM.to_owned(),
        _ => FOLDER_FLAT.to_owned(),
    }
}

pub fn normalize_organize_filename(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        FILENAME_DATE_TITLE_ID => FILENAME_DATE_TITLE_ID.to_owned(),
        FILENAME_PLAYLIST_INDEX_TITLE_ID => FILENAME_PLAYLIST_INDEX_TITLE_ID.to_owned(),
        FILENAME_TITLE_ONLY => FILENAME_TITLE_ONLY.to_owned(),
        FILENAME_CUSTOM => FILENAME_CUSTOM.to_owned(),
        _ => FILENAME_TITLE_ID.to_owned(),
    }
}

pub fn uses_custom_template(settings: &AppSettings) -> bool {
    settings.download_organize_folder == FOLDER_CUSTOM
        || settings.download_organize_filename == FILENAME_CUSTOM
}

pub fn folder_template_prefix(folder: &str) -> &'static str {
    match folder {
        FOLDER_UPLOADER => "%(uploader)s/",
        FOLDER_PLAYLIST => "%(playlist_title)s/",
        FOLDER_DATE_YM => "%(upload_date>%Y)s/%(upload_date>%m)s/",
        _ => "",
    }
}

pub fn filename_template_suffix(filename: &str) -> &'static str {
    match filename {
        FILENAME_DATE_TITLE_ID => "%(upload_date)s - %(title)s [%(id)s].%(ext)s",
        FILENAME_PLAYLIST_INDEX_TITLE_ID => "%(playlist_index)03d - %(title)s [%(id)s].%(ext)s",
        FILENAME_TITLE_ONLY => "%(title)s.%(ext)s",
        _ => "%(title)s [%(id)s].%(ext)s",
    }
}

pub fn compose_template_parts(folder: &str, filename: &str) -> String {
    if folder == FOLDER_CUSTOM || filename == FILENAME_CUSTOM {
        return String::new();
    }
    format!(
        "{}{}",
        folder_template_prefix(folder),
        filename_template_suffix(filename)
    )
}

/// Build the yt-dlp `-o` template from organize presets or the custom template field.
pub fn compose_output_template(settings: &AppSettings) -> String {
    if uses_custom_template(settings) {
        let template = settings.output_filename_template.trim();
        if template.is_empty() {
            DEFAULT_OUTPUT_FILENAME_TEMPLATE.to_owned()
        } else {
            template.to_owned()
        }
    } else {
        let composed = compose_template_parts(
            &settings.download_organize_folder,
            &settings.download_organize_filename,
        );
        if composed.is_empty() {
            DEFAULT_OUTPUT_FILENAME_TEMPLATE.to_owned()
        } else {
            composed
        }
    }
}

/// Static example path for settings UI hints.
pub fn example_output_path(settings: &AppSettings, output_dir: &str) -> String {
    let dir = output_dir.trim();
    let base = if dir.is_empty() { "Downloads" } else { dir };
    let rel = if uses_custom_template(settings) {
        let t = compose_output_template(settings);
        t.replace("%(title)s", "Example Video")
            .replace("%(id)s", "abc123")
            .replace("%(ext)s", "mp4")
            .replace("%(uploader)s", "Example Channel")
            .replace("%(playlist_title)s", "My Playlist")
            .replace("%(playlist_index)03d", "001")
            .replace("%(upload_date)s", "20240115")
            .replace("%(upload_date>%Y)s", "2024")
            .replace("%(upload_date>%m)s", "01")
    } else {
        let mut parts: Vec<String> = Vec::new();
        match settings.download_organize_folder.as_str() {
            FOLDER_UPLOADER => parts.push("Example Channel".to_owned()),
            FOLDER_PLAYLIST => parts.push("My Playlist".to_owned()),
            FOLDER_DATE_YM => {
                parts.push("2024".to_owned());
                parts.push("01".to_owned());
            }
            _ => {}
        }
        let name = match settings.download_organize_filename.as_str() {
            FILENAME_DATE_TITLE_ID => "20240115 - Example Video [abc123].mp4".to_owned(),
            FILENAME_PLAYLIST_INDEX_TITLE_ID => {
                "001 - Example Video [abc123].mp4".to_owned()
            }
            FILENAME_TITLE_ONLY => "Example Video.mp4".to_owned(),
            _ => "Example Video [abc123].mp4".to_owned(),
        };
        parts.push(name);
        parts.join("/")
    };
    format!("{base}/{rel}")
}

/// Sanitize a path segment for use on disk (folder or filename stem).
pub fn sanitize_path_segment(raw: &str) -> String {
    let mut s: String = raw
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s = s.trim_matches('.').trim().to_owned();
    if s.is_empty() {
        "unknown".to_owned()
    } else {
        s
    }
}

fn year_month_from_upload_date(upload_date: Option<&str>) -> Option<(String, String)> {
    let d = upload_date?.trim();
    if d.len() >= 6 && d.chars().all(|c| c.is_ascii_digit()) {
        Some((d[..4].to_owned(), d[4..6].to_owned()))
    } else {
        None
    }
}

fn date_prefix_from_upload_date(upload_date: Option<&str>) -> String {
    upload_date
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(sanitize_path_segment)
        .unwrap_or_else(|| "unknown-date".to_owned())
}

fn build_filename_stem(settings: &AppSettings, item: &QueueItem) -> String {
    let title = sanitize_path_segment(item.title.trim());
    let id = item.video_id.trim();
    let id_part = if id.is_empty() {
        "unknown".to_owned()
    } else {
        id.to_owned()
    };
    match settings.download_organize_filename.as_str() {
        FILENAME_DATE_TITLE_ID => format!(
            "{} - {} [{}]",
            date_prefix_from_upload_date(item.upload_date.as_deref()),
            title,
            id_part
        ),
        FILENAME_PLAYLIST_INDEX_TITLE_ID => {
            let idx = item.playlist_index.unwrap_or(0);
            format!("{:03} - {} [{}]", idx, title, id_part)
        }
        FILENAME_TITLE_ONLY => title,
        _ => format!("{title} [{id_part}]"),
    }
}

fn relative_subdirs(settings: &AppSettings, item: &QueueItem) -> Vec<String> {
    let mut parts = Vec::new();
    match settings.download_organize_folder.as_str() {
        FOLDER_UPLOADER => {
            if let Some(u) = item.uploader.as_deref().filter(|s| !s.trim().is_empty()) {
                parts.push(sanitize_path_segment(u));
            }
        }
        FOLDER_PLAYLIST => {
            if let Some(p) = item
                .playlist_title
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                parts.push(sanitize_path_segment(p));
            }
        }
        FOLDER_DATE_YM => {
            if let Some((y, m)) = year_month_from_upload_date(item.upload_date.as_deref()) {
                parts.push(y);
                parts.push(m);
            } else {
                parts.push("unknown-date".to_owned());
            }
        }
        _ => {}
    }
    parts
}

/// Compute the desired output file path for a queue item (post-download organize).
pub fn target_path_for_item(output_dir: &str, settings: &AppSettings, item: &QueueItem) -> PathBuf {
    let stem = build_filename_stem(settings, item);
    let ext = item
        .local_path
        .as_deref()
        .and_then(|p| Path::new(p).extension().and_then(|e| e.to_str()))
        .unwrap_or("mp4");
    let filename = format!("{stem}.{ext}");
    let mut path = PathBuf::from(output_dir);
    for part in relative_subdirs(settings, item) {
        path.push(part);
    }
    path.push(filename);
    path
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    if let (Ok(ca), Ok(cb)) = (a.canonicalize(), b.canonicalize()) {
        return ca == cb;
    }
    false
}

fn unique_target_path(mut target: PathBuf) -> PathBuf {
    if !target.exists() {
        return target;
    }
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = target
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_owned();
    let ext = target
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    for n in 1..=99 {
        let candidate = parent.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    target.set_file_name(format!("{stem}-dup{ext}"));
    target
}

fn sidecar_paths_for_stem(parent: &Path, stem: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let prefix = format!("{stem}.");
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| {
                        n.starts_with(&prefix)
                            && SIDECAR_EXTENSIONS.iter().any(|ext| n.ends_with(ext))
                    })
        })
        .collect()
}

/// Move `source` to the organize layout; updates item `local_path` on success.
pub fn apply_post_download_organize(
    output_dir: &str,
    settings: &AppSettings,
    item: &mut QueueItem,
    source: &Path,
) -> Result<Option<String>> {
    if !settings.post_download_organize || uses_custom_template(settings) {
        return Ok(None);
    }
    let target = target_path_for_item(output_dir, settings, item);
    if paths_equal(source, &target) {
        return Ok(None);
    }
    let target = unique_target_path(target);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("create organize folder {}", parent.to_string_lossy())
        })?;
    }
    fs::rename(source, &target).with_context(|| {
        format!(
            "move {} -> {}",
            source.display(),
            target.display()
        )
    })?;
    let old_parent = source.parent().unwrap_or(Path::new(output_dir));
    let old_stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let new_stem = target
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let new_parent = target.parent().unwrap_or(Path::new(output_dir));
    for sidecar in sidecar_paths_for_stem(old_parent, old_stem) {
        let fname = sidecar
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let suffix = fname.strip_prefix(&format!("{old_stem}.")).unwrap_or(fname);
        let dest = new_parent.join(format!("{new_stem}.{suffix}"));
        if sidecar != dest {
            if dest.exists() {
                let _ = fs::remove_file(&dest);
            }
            let _ = fs::rename(&sidecar, &dest);
        }
    }
    let saved = target.to_string_lossy().into_owned();
    item.local_path = Some(saved.clone());
    Ok(Some(saved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppSettings;

    fn base_settings() -> AppSettings {
        AppSettings::default()
    }

    #[test]
    fn compose_flat_title_id() {
        let s = base_settings();
        assert_eq!(
            compose_output_template(&s),
            "%(title)s [%(id)s].%(ext)s"
        );
    }

    #[test]
    fn compose_uploader_folder() {
        let mut s = base_settings();
        s.download_organize_folder = FOLDER_UPLOADER.to_owned();
        assert_eq!(
            compose_output_template(&s),
            "%(uploader)s/%(title)s [%(id)s].%(ext)s"
        );
    }

    #[test]
    fn compose_playlist_index_filename() {
        let mut s = base_settings();
        s.download_organize_folder = FOLDER_PLAYLIST.to_owned();
        s.download_organize_filename = FILENAME_PLAYLIST_INDEX_TITLE_ID.to_owned();
        assert_eq!(
            compose_output_template(&s),
            "%(playlist_title)s/%(playlist_index)03d - %(title)s [%(id)s].%(ext)s"
        );
    }

    #[test]
    fn compose_date_ym_folder() {
        let mut s = base_settings();
        s.download_organize_folder = FOLDER_DATE_YM.to_owned();
        assert_eq!(
            compose_output_template(&s),
            "%(upload_date>%Y)s/%(upload_date>%m)s/%(title)s [%(id)s].%(ext)s"
        );
    }

    #[test]
    fn custom_mode_uses_output_template() {
        let mut s = base_settings();
        s.download_organize_folder = FOLDER_CUSTOM.to_owned();
        s.output_filename_template = "%(channel)s/%(title)s.%(ext)s".to_owned();
        assert_eq!(
            compose_output_template(&s),
            "%(channel)s/%(title)s.%(ext)s"
        );
    }

    #[test]
    fn template_implies_custom_detects_non_default() {
        assert!(!template_implies_custom_mode(
            DEFAULT_OUTPUT_FILENAME_TEMPLATE
        ));
        assert!(template_implies_custom_mode(
            "%(uploader)s/%(title)s.%(ext)s"
        ));
    }

    #[test]
    fn sanitize_path_segment_strips_invalid() {
        assert_eq!(sanitize_path_segment("foo/bar:baz"), "foo_bar_baz");
        assert_eq!(sanitize_path_segment("  ..  "), "unknown");
    }

    #[test]
    fn target_path_uploader_and_title_id() {
        let mut s = base_settings();
        s.download_organize_folder = FOLDER_UPLOADER.to_owned();
        let item = QueueItem {
            title: "My Video".to_owned(),
            video_id: "vid1".to_owned(),
            uploader: Some("Cool Channel".to_owned()),
            local_path: Some("My Video [vid1].mp4".to_owned()),
            ..Default::default()
        };
        let path = target_path_for_item("/out", &s, &item);
        assert!(path.to_string_lossy().contains("Cool Channel"));
        assert!(path.to_string_lossy().ends_with("My Video [vid1].mp4"));
    }

    #[test]
    fn target_path_playlist_fallback_flat_when_no_title() {
        let mut s = base_settings();
        s.download_organize_folder = FOLDER_PLAYLIST.to_owned();
        let item = QueueItem {
            title: "T".to_owned(),
            video_id: "x".to_owned(),
            local_path: Some("T [x].webm".to_owned()),
            ..Default::default()
        };
        let path = target_path_for_item("/out", &s, &item);
        assert_eq!(path, PathBuf::from("/out/T [x].webm"));
    }
}
