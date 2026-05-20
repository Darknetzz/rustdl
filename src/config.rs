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
        }
    }
}

fn config_path() -> PathBuf {
    if let Some(cfg) = dirs::config_dir() {
        return cfg.join("rustdl").join("rustdl_config.json");
    }
    PathBuf::from("rustdl_config.json")
}

fn queue_path() -> PathBuf {
    if let Some(cfg) = dirs::config_dir() {
        return cfg.join("rustdl").join("rustdl_queue.json");
    }
    PathBuf::from("rustdl_queue.json")
}

fn activity_log_path() -> PathBuf {
    if let Some(cfg) = dirs::config_dir() {
        return cfg.join("rustdl").join("rustdl_activity_log.json");
    }
    PathBuf::from("rustdl_activity_log.json")
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
