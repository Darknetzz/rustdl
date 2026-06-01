use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::models::QueueItem;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub output_dir: String,
    pub worker_count: usize,
    pub show_thumbnails: bool,
    pub autoscroll_log: bool,
    pub log_max_chars: usize,
    pub yt_dlp_extra_args: String,
    /// Path to a Netscape `cookies.txt`, or a browser name for `--cookies-from-browser` (e.g. `firefox`).
    #[serde(default)]
    pub yt_dlp_cookies: String,
    /// Passed as `--impersonate` (e.g. `chrome`). Helps with some login-gated sites when using cookies.
    #[serde(default)]
    pub yt_dlp_impersonate: String,
    pub ffmpeg_post_args: String,
    pub embed_thumbnail: bool,
    pub yt_dlp_path: String,
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    /// When true, pass `--retries infinite` and `--fragment-retries infinite` to yt-dlp.
    #[serde(default = "default_yt_dlp_unlimited_retries")]
    pub yt_dlp_unlimited_retries: bool,
    /// Used when `yt_dlp_unlimited_retries` is false (yt-dlp default is 10).
    #[serde(default = "default_yt_dlp_retry_count")]
    pub yt_dlp_retry_count: u32,
    pub yt_ignore_errors: bool,
    pub yt_restrict_filenames: bool,
    pub yt_write_info_json: bool,
    pub yt_write_auto_subs: bool,
    pub yt_embed_metadata: bool,
    pub ffmpeg_faststart: bool,
    pub ffmpeg_remux_mp4: bool,
    pub ffmpeg_extract_audio_mp3: bool,
    /// After download, require both a video and an audio stream (ffprobe). Ignored for MP3 extraction.
    #[serde(default = "default_verify_output_video_audio")]
    pub verify_output_video_audio: bool,
    pub compact_cards: bool,
    pub hide_card_subtitle: bool,
    pub auto_add_pasted_urls: bool,
    pub auto_start_downloads: bool,
    pub ui_scale: f32,
    /// List rows instead of horizontal preview cards in the queue.
    #[serde(default)]
    pub card_list_layout: bool,
    /// Show activity log docked under the video queue (vs floating window).
    #[serde(default = "default_logs_docked")]
    pub logs_docked: bool,
    #[serde(default)]
    pub logs_open: bool,
    #[serde(default = "default_log_dock_height")]
    pub log_dock_height: f32,
    /// Activity log timestamps as relative age instead of full local time.
    #[serde(default)]
    pub log_relative_time: bool,
    /// Override ffmpeg path for AV1 converter mode.
    #[serde(default)]
    pub av1_ffmpeg_path: String,
    /// Override ffprobe path for AV1 converter mode.
    #[serde(default)]
    pub av1_ffprobe_path: String,
    /// Recursive folder scan for AV1 input folders.
    #[serde(default = "default_av1_recursive")]
    pub av1_recursive: bool,
    /// Dry-run mode for AV1 conversion planning.
    #[serde(default)]
    pub av1_dry_run: bool,
    /// Delete original input file after successful AV1 conversion.
    #[serde(default)]
    pub av1_delete_original: bool,
    /// Overwrite existing destination file if it exists.
    #[serde(default)]
    pub av1_overwrite: bool,
    /// Re-encode inputs already using AV1 codec.
    #[serde(default)]
    pub av1_reencode_av1: bool,
    /// Default target bitrate (e.g. 1800k). Empty means auto.
    #[serde(default)]
    pub av1_target_bitrate: String,
    /// Maximum output width (maintain aspect ratio).
    #[serde(default = "default_av1_max_width")]
    pub av1_max_width: u32,
    /// Output quality policy.
    #[serde(default)]
    pub av1_size_preset: String,
    /// Require minimum shrink percentage relative to source. Zero disables.
    #[serde(default)]
    pub av1_min_shrink_percent: f32,
}

fn default_logs_docked() -> bool {
    true
}

fn default_log_dock_height() -> f32 {
    180.0
}

fn default_av1_recursive() -> bool {
    true
}

fn default_av1_max_width() -> u32 {
    1920
}

fn default_verify_output_video_audio() -> bool {
    true
}

fn default_yt_dlp_unlimited_retries() -> bool {
    true
}

fn default_yt_dlp_retry_count() -> u32 {
    10
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            output_dir: default_downloads().to_string_lossy().to_string(),
            worker_count: 3,
            show_thumbnails: true,
            autoscroll_log: true,
            log_max_chars: 28_000,
            yt_dlp_extra_args: String::new(),
            yt_dlp_cookies: String::new(),
            yt_dlp_impersonate: String::new(),
            ffmpeg_post_args: String::new(),
            embed_thumbnail: false,
            yt_dlp_path: String::new(),
            ffmpeg_path: String::new(),
            ffprobe_path: String::new(),
            yt_dlp_unlimited_retries: true,
            yt_dlp_retry_count: 10,
            yt_ignore_errors: false,
            yt_restrict_filenames: false,
            yt_write_info_json: false,
            yt_write_auto_subs: false,
            yt_embed_metadata: false,
            ffmpeg_faststart: true,
            ffmpeg_remux_mp4: false,
            ffmpeg_extract_audio_mp3: false,
            verify_output_video_audio: true,
            compact_cards: false,
            hide_card_subtitle: false,
            auto_add_pasted_urls: true,
            auto_start_downloads: true,
            ui_scale: 1.08,
            card_list_layout: false,
            logs_docked: true,
            logs_open: false,
            log_dock_height: 180.0,
            log_relative_time: false,
            av1_ffmpeg_path: String::new(),
            av1_ffprobe_path: String::new(),
            av1_recursive: true,
            av1_dry_run: false,
            av1_delete_original: false,
            av1_overwrite: false,
            av1_reencode_av1: false,
            av1_target_bitrate: String::new(),
            av1_max_width: 1920,
            av1_size_preset: "balanced".to_owned(),
            av1_min_shrink_percent: 0.0,
        }
    }
}

/// `%APPDATA%/rustdl` or `./rustdl` fallback.
pub fn rustdl_config_dir() -> PathBuf {
    if let Some(cfg) = dirs::config_dir() {
        cfg.join("rustdl")
    } else {
        PathBuf::from("rustdl")
    }
}

pub fn config_file_path() -> PathBuf {
    rustdl_config_dir().join("rustdl_config.json")
}

pub fn queue_file_path() -> PathBuf {
    rustdl_config_dir().join("rustdl_queue.json")
}

pub fn activity_log_file_path() -> PathBuf {
    rustdl_config_dir().join("rustdl_activity_log.json")
}

fn config_path() -> PathBuf {
    if dirs::config_dir().is_some() {
        config_file_path()
    } else {
        PathBuf::from("rustdl_config.json")
    }
}

fn queue_path() -> PathBuf {
    if dirs::config_dir().is_some() {
        queue_file_path()
    } else {
        PathBuf::from("rustdl_queue.json")
    }
}

fn activity_log_path() -> PathBuf {
    if dirs::config_dir().is_some() {
        activity_log_file_path()
    } else {
        PathBuf::from("rustdl_activity_log.json")
    }
}

const MAX_ACTIVITY_LOG_LINES: usize = 4_000;

fn activity_log_char_count(lines: &VecDeque<String>) -> usize {
    lines
        .iter()
        .map(|s| s.len().saturating_add(1))
        .sum()
}

/// Trims oldest lines so the in-memory / on-disk log respects [`AppSettings::log_max_chars`].
pub fn trim_activity_log(lines: &mut VecDeque<String>, max_chars: usize) {
    let max_chars = max_chars.clamp(2_000, 200_000);
    while !lines.is_empty()
        && (lines.len() > MAX_ACTIVITY_LOG_LINES
            || activity_log_char_count(lines) > max_chars)
    {
        lines.pop_front();
    }
}

pub fn load_activity_log(max_chars: usize) -> VecDeque<String> {
    let path = activity_log_path();
    let raw = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return VecDeque::new(),
    };
    let mut lines: VecDeque<String> = serde_json::from_str(&raw).unwrap_or_default();
    trim_activity_log(&mut lines, max_chars);
    lines
}

pub fn save_activity_log(lines: &VecDeque<String>) -> Result<()> {
    let path = activity_log_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create activity log directory: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    let payload: Vec<&str> = lines.iter().map(String::as_str).collect();
    let raw = serde_json::to_string_pretty(&payload).context("failed to serialize activity log")?;
    fs::write(&path, raw).with_context(|| {
        format!(
            "failed to write activity log file: {}",
            path.to_string_lossy()
        )
    })?;
    Ok(())
}

pub fn default_downloads() -> PathBuf {
    if let Some(d) = dirs::download_dir() {
        return d;
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn load_settings() -> AppSettings {
    let path = config_path();
    let raw = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return AppSettings::default(),
    };
    let mut cfg = serde_json::from_str::<AppSettings>(&raw).unwrap_or_default();
    if cfg.output_dir.trim().is_empty() || !PathBuf::from(&cfg.output_dir).is_dir() {
        cfg.output_dir = default_downloads().to_string_lossy().to_string();
    }
    cfg.worker_count = cfg.worker_count.clamp(1, 6);
    cfg.log_max_chars = cfg.log_max_chars.clamp(2_000, 200_000);
    cfg.ui_scale = cfg.ui_scale.clamp(0.85, 1.5);
    cfg.yt_dlp_retry_count = cfg.yt_dlp_retry_count.clamp(1, 999);
    cfg.log_dock_height = cfg.log_dock_height.clamp(80.0, 480.0);
    cfg.av1_max_width = cfg.av1_max_width.clamp(320, 7680);
    cfg.av1_min_shrink_percent = cfg.av1_min_shrink_percent.clamp(0.0, 95.0);
    let preset = cfg.av1_size_preset.trim().to_ascii_lowercase();
    if !matches!(preset.as_str(), "light" | "balanced" | "aggressive") {
        cfg.av1_size_preset = "balanced".to_owned();
    } else {
        cfg.av1_size_preset = preset;
    }
    cfg
}

pub fn save_settings(settings: &AppSettings) -> Result<()> {
    let cfg_path = config_path();
    if let Some(parent) = cfg_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create config directory: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    let raw = serde_json::to_string_pretty(settings).context("failed to serialize settings")?;
    fs::write(&cfg_path, raw).with_context(|| {
        format!(
            "failed to write settings file: {}",
            cfg_path.to_string_lossy()
        )
    })?;
    Ok(())
}

pub fn load_queue_items() -> Vec<QueueItem> {
    let path = queue_path();
    let raw = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    serde_json::from_str::<Vec<QueueItem>>(&raw).unwrap_or_default()
}

/// Writes one URL per line (source line or webpage URL).
pub fn export_queue_urls(items: &[QueueItem], path: &std::path::Path) -> Result<()> {
    use std::io::Write;
    let mut lines = Vec::new();
    for it in items {
        let u = if !it.webpage_url.trim().is_empty() {
            it.webpage_url.as_str()
        } else {
            it.source_line.as_str()
        };
        if !u.trim().is_empty() {
            lines.push(u.trim().to_owned());
        }
    }
    let raw = lines.join("\n");
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).ok();
        }
    }
    let mut f = fs::File::create(path).context("failed to create export file")?;
    f.write_all(raw.as_bytes())
        .context("failed to write export file")?;
    Ok(())
}

pub fn save_queue_items(items: &[QueueItem]) -> Result<()> {
    let path = queue_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create queue directory: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    let raw = serde_json::to_string_pretty(items).context("failed to serialize queue items")?;
    fs::write(&path, raw)
        .with_context(|| format!("failed to write queue file: {}", path.to_string_lossy()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_round_trips_json() {
        let s = AppSettings::default();
        let raw = serde_json::to_string(&s).expect("serialize");
        let back: AppSettings = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back.worker_count, s.worker_count);
        assert_eq!(back.yt_dlp_unlimited_retries, s.yt_dlp_unlimited_retries);
    }

    #[test]
    fn settings_partial_json_uses_defaults() {
        let raw = r#"{"worker_count":2}"#;
        let cfg: AppSettings = serde_json::from_str(raw).expect("deserialize");
        assert_eq!(cfg.worker_count, 2);
        assert!(cfg.yt_dlp_unlimited_retries);
    }
}
