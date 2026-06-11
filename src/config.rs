use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::models::{ConvertQueueItem, QueueItem};

/// A user data file that failed to parse at startup (surfaced in the GUI / web status).
#[derive(Clone, Debug, Serialize)]
pub struct ConfigLoadIssue {
    pub label: &'static str,
    pub path: PathBuf,
}

static CONFIG_LOAD_ISSUES: OnceLock<Mutex<Vec<ConfigLoadIssue>>> = OnceLock::new();

fn config_issues() -> &'static Mutex<Vec<ConfigLoadIssue>> {
    CONFIG_LOAD_ISSUES.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn record_config_load_failure(path: PathBuf, label: &'static str) {
    quarantine_bad_config_file(&path);
    let display = path.display().to_string();
    if let Ok(mut issues) = config_issues().lock() {
        if !issues.iter().any(|i| i.path == path && i.label == label) {
            issues.push(ConfigLoadIssue { label, path });
        }
    }
    eprintln!(
        "rustdl: failed to load {label} from {display}; using defaults (backup: {display}.bak)"
    );
}

pub fn take_config_load_issues() -> Vec<ConfigLoadIssue> {
    config_issues()
        .lock()
        .map(|mut v| v.drain(..).collect())
        .unwrap_or_default()
}

pub fn peek_config_load_issues() -> Vec<ConfigLoadIssue> {
    config_issues()
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default()
}

fn quarantine_bad_config_file(path: &Path) {
    if !path.is_file() {
        return;
    }
    let bak = PathBuf::from(format!("{}.bak", path.display()));
    let _ = fs::rename(path, bak);
}

pub(crate) fn load_json_file<T: DeserializeOwned + Default>(
    path: PathBuf,
    label: &'static str,
) -> T {
    let raw = match fs::read_to_string(&path) {
        Ok(v) => v,
        Err(_) => return T::default(),
    };
    match serde_json::from_str::<T>(&raw) {
        Ok(v) => v,
        Err(_) => {
            record_config_load_failure(path, label);
            T::default()
        }
    }
}

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
    /// After a successful download, add the output file to the video converter queue.
    #[serde(default, alias = "enqueue_downloads_to_av1")]
    pub enqueue_downloads_to_convert: bool,
    pub ui_scale: f32,
    /// List rows instead of horizontal preview cards in the queue.
    #[serde(default)]
    pub card_list_layout: bool,
    /// Downloader output folder / profile / search section expanded in the main panel.
    #[serde(default = "default_downloader_options_expanded")]
    pub downloader_options_expanded: bool,
    /// Show video queue docked in the main window (vs separate window).
    #[serde(default = "default_videos_docked")]
    pub videos_docked: bool,
    /// Floating video window visible when `videos_docked` is false.
    #[serde(default = "default_videos_open")]
    pub videos_open: bool,
    #[serde(default = "default_video_float_width")]
    pub video_float_width: f32,
    #[serde(default = "default_video_float_height")]
    pub video_float_height: f32,
    /// Height of the docked video queue panel in the main window (resizable divider).
    #[serde(default = "default_videos_dock_height")]
    pub videos_dock_height: f32,
    /// Show activity log docked under the video queue (vs floating window).
    #[serde(default = "default_logs_docked")]
    pub logs_docked: bool,
    #[serde(default = "default_logs_open")]
    pub logs_open: bool,
    #[serde(default = "default_log_dock_height")]
    pub log_dock_height: f32,
    /// Pinned footer height when the video queue is undocked (strip + optional docked log).
    #[serde(default = "default_undocked_footer_height")]
    pub undocked_footer_height: f32,
    #[serde(default = "default_log_float_width")]
    pub log_float_width: f32,
    #[serde(default = "default_log_float_height")]
    pub log_float_height: f32,
    /// Activity log timestamps as relative age instead of full local time.
    #[serde(default)]
    pub log_relative_time: bool,
    /// Recursive folder scan for converter input folders.
    #[serde(default = "default_convert_recursive", alias = "av1_recursive")]
    pub convert_recursive: bool,
    /// Dry-run mode for conversion planning.
    #[serde(default, alias = "av1_dry_run")]
    pub convert_dry_run: bool,
    /// Start the convert batch automatically after new paths are scanned into the queue.
    #[serde(default, alias = "av1_auto_start_on_add")]
    pub convert_auto_start_on_add: bool,
    /// Delete original input file after successful conversion.
    #[serde(default, alias = "av1_delete_original")]
    pub convert_delete_original: bool,
    /// Rename encoded output back to the source filename after successful conversion.
    #[serde(default, alias = "av1_rename_original")]
    pub convert_rename_original: bool,
    /// Overwrite existing destination file if it exists.
    #[serde(default, alias = "av1_overwrite")]
    pub convert_overwrite: bool,
    /// Re-encode inputs already using the target codec.
    #[serde(default, alias = "av1_reencode_av1")]
    pub convert_reencode_target: bool,
    /// Write outputs using a recommended container for the target codec.
    #[serde(
        default = "default_convert_use_recommended_container",
        alias = "av1_use_recommended_container"
    )]
    pub convert_use_recommended_container: bool,
    /// Target video codec: `av1`, `hevc`, or `h264`.
    #[serde(default = "default_convert_target_codec", alias = "av1_target_codec")]
    pub convert_target_codec: String,
    /// Default target bitrate (e.g. 1800k). Empty means auto.
    #[serde(default, alias = "av1_target_bitrate")]
    pub convert_target_bitrate: String,
    /// Maximum output width (maintain aspect ratio).
    #[serde(default = "default_convert_max_width", alias = "av1_max_width")]
    pub convert_max_width: u32,
    /// Output quality policy.
    #[serde(default, alias = "av1_size_preset")]
    pub convert_size_preset: String,
    /// Require minimum shrink percentage relative to source. Zero disables.
    #[serde(default, alias = "av1_min_shrink_percent")]
    pub convert_min_shrink_percent: f32,
    /// Keep converter queue items across app restarts until manually cleared.
    #[serde(
        default = "default_convert_remember_queue",
        alias = "av1_remember_queue"
    )]
    pub convert_remember_queue: bool,
    /// Last top-level mode: `downloader` or `convert`.
    #[serde(default = "default_last_mode")]
    pub last_mode: String,
    /// Last settings tab: `shared`, `downloader`, or `convert`.
    #[serde(default = "default_settings_tab")]
    pub settings_tab: String,
    /// UI theme: `dark`, `light`, or `system`.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// yt-dlp output filename template (`-o`).
    #[serde(default = "default_output_filename_template")]
    pub output_filename_template: String,
    /// Subfolder layout: `flat`, `uploader`, `playlist`, `date_ym`, or `custom`.
    #[serde(default = "default_download_organize_folder")]
    pub download_organize_folder: String,
    /// Filename style: `title_id`, `date_title_id`, `playlist_index_title_id`, `title_only`, or `custom`.
    #[serde(default = "default_download_organize_filename")]
    pub download_organize_filename: String,
    /// After download, move files into the organize layout when presets are active.
    #[serde(default)]
    pub post_download_organize: bool,
    /// Quality preset: `best`, `1080p`, `720p`, `audio`, or `custom`.
    #[serde(default = "default_quality_preset")]
    pub quality_preset: String,
    /// Custom `-f` string when `quality_preset` is `custom`.
    #[serde(default)]
    pub quality_format_custom: String,
    /// Merge container: `default`, `mp4`, `mkv`, or `webm`.
    #[serde(default = "default_merge_container")]
    pub merge_container: String,
    /// Show first-run setup hint banner.
    #[serde(default = "default_show_first_run_hint")]
    pub show_first_run_hint: bool,
    /// Path to yt-dlp download archive file (`--download-archive`).
    #[serde(default)]
    pub yt_download_archive: String,
    /// Proxy URL for yt-dlp (`--proxy`).
    #[serde(default)]
    pub yt_proxy: String,
    /// Max download rate for yt-dlp (`--limit-rate`, e.g. `50K`, `4M`).
    #[serde(default)]
    pub yt_limit_rate: String,
    /// Remove SponsorBlock segments (`--sponsorblock-remove`).
    #[serde(default)]
    pub yt_sponsorblock_remove: bool,
    /// Mark SponsorBlock categories (`--sponsorblock-mark`, comma-separated).
    #[serde(default)]
    pub yt_sponsorblock_mark: String,
    /// Max playlist entries to preview when resolving URLs.
    #[serde(default = "default_playlist_preview_cap")]
    pub playlist_preview_cap: usize,
    /// Active named download profile (built-in or user-defined).
    #[serde(default = "default_active_profile")]
    pub active_profile: String,
    /// Force ffmpeg encoder for converter mode; empty = auto-detect.
    #[serde(default, alias = "av1_encoder_override")]
    pub convert_encoder_override: String,
    /// Enable LAN web UI (HTTP API + built-in pages).
    #[serde(default)]
    pub web_ui_enabled: bool,
    /// Bind address for web UI, e.g. `0.0.0.0:8765`.
    #[serde(default = "default_web_bind_address")]
    pub web_bind_address: String,
    /// Bearer / `X-Rustdl-Token` value required for API access.
    #[serde(default)]
    pub web_auth_token: String,
    /// GitHub personal access token for in-app update checks (required when the repo is private).
    #[serde(default)]
    pub github_token: String,
    /// Last queue search filter text (Downloader queue panel / web UI).
    #[serde(default)]
    pub queue_search: String,
    /// Activity log filter: `all`, `important`, or `errors`.
    #[serde(default = "default_log_filter")]
    pub log_filter: String,
    /// Session queue restore: `ask`, `always`, or `never`.
    #[serde(default = "default_session_restore_preference")]
    pub session_restore_preference: String,
    /// Max content column width in pixels; `0` = use full panel width.
    #[serde(default)]
    pub max_content_width: f32,
    /// Downloader mode panel tint / accent (`#rrggbb`); empty = default blue.
    #[serde(default, alias = "mode_downloader_bg")]
    pub mode_downloader_color: String,
    /// Video Converter mode panel tint / accent (`#rrggbb`); empty = default purple.
    #[serde(default, alias = "mode_convert_bg", alias = "mode_av1_color")]
    pub mode_convert_color: String,
}

fn default_web_bind_address() -> String {
    "0.0.0.0:8765".to_owned()
}

fn default_log_filter() -> String {
    "all".to_owned()
}

fn default_session_restore_preference() -> String {
    "ask".to_owned()
}

pub fn session_restore_auto_load(preference: &str) -> bool {
    preference.trim().eq_ignore_ascii_case("always")
}

pub fn session_restore_discard_on_startup(preference: &str) -> bool {
    preference.trim().eq_ignore_ascii_case("never")
}

/// GitHub token for release API calls: settings field, then `RUSTDL_GITHUB_TOKEN`, then `GITHUB_TOKEN`.
pub fn resolve_github_token(settings: &AppSettings) -> Option<String> {
    let from_settings = settings.github_token.trim();
    if !from_settings.is_empty() {
        return Some(from_settings.to_owned());
    }
    for var in ["RUSTDL_GITHUB_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(value) = std::env::var(var) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

/// Generates a random token when enabling the web UI for the first time.
pub fn generate_web_auth_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub const DEFAULT_OUTPUT_FILENAME_TEMPLATE: &str = "%(title)s [%(id)s].%(ext)s";
pub const DEFAULT_PLAYLIST_PREVIEW_CAP: usize = 20;

fn default_last_mode() -> String {
    "downloader".to_owned()
}

fn default_settings_tab() -> String {
    "shared".to_owned()
}

fn default_theme() -> String {
    "dark".to_owned()
}

fn default_output_filename_template() -> String {
    DEFAULT_OUTPUT_FILENAME_TEMPLATE.to_owned()
}

fn default_download_organize_folder() -> String {
    crate::download_organize::FOLDER_FLAT.to_owned()
}

fn default_download_organize_filename() -> String {
    crate::download_organize::FILENAME_TITLE_ID.to_owned()
}

fn default_quality_preset() -> String {
    "best".to_owned()
}

fn default_merge_container() -> String {
    "default".to_owned()
}

fn default_show_first_run_hint() -> bool {
    true
}

fn default_playlist_preview_cap() -> usize {
    DEFAULT_PLAYLIST_PREVIEW_CAP
}

fn default_active_profile() -> String {
    "Best quality".to_owned()
}

fn default_convert_remember_queue() -> bool {
    true
}

fn default_convert_target_codec() -> String {
    "av1".to_owned()
}

fn default_videos_docked() -> bool {
    true
}

fn default_videos_open() -> bool {
    true
}

fn default_video_float_width() -> f32 {
    920.0
}

fn default_video_float_height() -> f32 {
    640.0
}

fn default_videos_dock_height() -> f32 {
    360.0
}

fn default_logs_docked() -> bool {
    true
}

fn default_logs_open() -> bool {
    true
}

fn default_undocked_footer_height() -> f32 {
    380.0
}

fn default_log_dock_height() -> f32 {
    180.0
}

fn default_log_float_width() -> f32 {
    640.0
}

fn default_log_float_height() -> f32 {
    440.0
}

fn default_convert_recursive() -> bool {
    true
}

fn default_convert_max_width() -> u32 {
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

fn default_downloader_options_expanded() -> bool {
    true
}

fn default_convert_use_recommended_container() -> bool {
    true
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
            enqueue_downloads_to_convert: false,
            ui_scale: 1.08,
            card_list_layout: false,
            downloader_options_expanded: true,
            videos_docked: true,
            videos_open: true,
            video_float_width: default_video_float_width(),
            video_float_height: default_video_float_height(),
            videos_dock_height: default_videos_dock_height(),
            logs_docked: true,
            logs_open: true,
            log_dock_height: 180.0,
            undocked_footer_height: default_undocked_footer_height(),
            log_float_width: default_log_float_width(),
            log_float_height: default_log_float_height(),
            log_relative_time: false,
            convert_recursive: true,
            convert_dry_run: false,
            convert_auto_start_on_add: false,
            convert_delete_original: false,
            convert_rename_original: false,
            convert_overwrite: false,
            convert_reencode_target: false,
            convert_use_recommended_container: true,
            convert_target_codec: default_convert_target_codec(),
            convert_target_bitrate: String::new(),
            convert_max_width: 1920,
            convert_size_preset: "balanced".to_owned(),
            convert_min_shrink_percent: 0.0,
            convert_remember_queue: true,
            last_mode: default_last_mode(),
            settings_tab: default_settings_tab(),
            theme: default_theme(),
            output_filename_template: default_output_filename_template(),
            download_organize_folder: default_download_organize_folder(),
            download_organize_filename: default_download_organize_filename(),
            post_download_organize: false,
            quality_preset: default_quality_preset(),
            quality_format_custom: String::new(),
            merge_container: default_merge_container(),
            show_first_run_hint: default_show_first_run_hint(),
            yt_download_archive: String::new(),
            yt_proxy: String::new(),
            yt_limit_rate: String::new(),
            yt_sponsorblock_remove: false,
            yt_sponsorblock_mark: String::new(),
            playlist_preview_cap: default_playlist_preview_cap(),
            active_profile: default_active_profile(),
            convert_encoder_override: String::new(),
            web_ui_enabled: false,
            web_bind_address: default_web_bind_address(),
            web_auth_token: String::new(),
            github_token: String::new(),
            queue_search: String::new(),
            log_filter: default_log_filter(),
            session_restore_preference: default_session_restore_preference(),
            max_content_width: 0.0,
            mode_downloader_color: String::new(),
            mode_convert_color: String::new(),
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
    lines.iter().map(|s| s.len().saturating_add(1)).sum()
}

/// Trims oldest lines so the in-memory / on-disk log respects [`AppSettings::log_max_chars`].
pub fn trim_activity_log(lines: &mut VecDeque<String>, max_chars: usize) {
    let max_chars = max_chars.clamp(2_000, 200_000);
    while !lines.is_empty()
        && (lines.len() > MAX_ACTIVITY_LOG_LINES || activity_log_char_count(lines) > max_chars)
    {
        lines.pop_front();
    }
}

pub fn load_activity_log(max_chars: usize) -> VecDeque<String> {
    let path = activity_log_path();
    let mut lines: VecDeque<String> = load_json_file(path, "activity log");
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
    let mut cfg: AppSettings = load_json_file(path.clone(), "settings");
    if cfg.output_dir.trim().is_empty() || !PathBuf::from(&cfg.output_dir).is_dir() {
        cfg.output_dir = default_downloads().to_string_lossy().to_string();
    }
    cfg.worker_count = cfg.worker_count.clamp(1, 6);
    cfg.log_max_chars = cfg.log_max_chars.clamp(2_000, 200_000);
    cfg.ui_scale = cfg.ui_scale.clamp(0.85, 1.5);
    cfg.yt_dlp_retry_count = cfg.yt_dlp_retry_count.clamp(1, 999);
    cfg.log_dock_height = cfg.log_dock_height.clamp(80.0, 480.0);
    cfg.undocked_footer_height = cfg.undocked_footer_height.clamp(100.0, 600.0);
    cfg.log_float_width = cfg.log_float_width.clamp(400.0, 2400.0);
    cfg.log_float_height = cfg.log_float_height.clamp(260.0, 1600.0);
    cfg.videos_dock_height = cfg.videos_dock_height.clamp(180.0, 800.0);
    cfg.video_float_width = cfg.video_float_width.clamp(480.0, 2400.0);
    cfg.video_float_height = cfg.video_float_height.clamp(320.0, 1600.0);
    cfg.convert_max_width = cfg.convert_max_width.clamp(320, 7680);
    cfg.convert_min_shrink_percent = cfg.convert_min_shrink_percent.clamp(0.0, 95.0);
    let preset = cfg.convert_size_preset.trim().to_ascii_lowercase();
    if !matches!(preset.as_str(), "light" | "balanced" | "aggressive") {
        cfg.convert_size_preset = "balanced".to_owned();
    } else {
        cfg.convert_size_preset = preset;
    }
    cfg.convert_target_codec =
        crate::transcode::normalize_target_codec(&cfg.convert_target_codec).to_owned();
    let mode = cfg.last_mode.trim().to_ascii_lowercase();
    cfg.last_mode = if mode == "convert" || mode == "av1" {
        "convert".to_owned()
    } else {
        "downloader".to_owned()
    };
    let tab = cfg.settings_tab.trim().to_ascii_lowercase();
    cfg.settings_tab = match tab.as_str() {
        "downloader" => "downloader".to_owned(),
        "convert" | "av1" => "convert".to_owned(),
        "web" | "web_ui" => "web".to_owned(),
        _ => "shared".to_owned(),
    };
    let theme = cfg.theme.trim().to_ascii_lowercase();
    cfg.theme = match theme.as_str() {
        "light" => "light".to_owned(),
        "system" => "system".to_owned(),
        _ => "dark".to_owned(),
    };
    if cfg.output_filename_template.trim().is_empty() {
        cfg.output_filename_template = default_output_filename_template();
    }
    cfg.download_organize_folder =
        crate::download_organize::normalize_organize_folder(&cfg.download_organize_folder);
    cfg.download_organize_filename =
        crate::download_organize::normalize_organize_filename(&cfg.download_organize_filename);
    if crate::download_organize::template_implies_custom_mode(&cfg.output_filename_template) {
        cfg.download_organize_folder = crate::download_organize::FOLDER_CUSTOM.to_owned();
        cfg.download_organize_filename = crate::download_organize::FILENAME_CUSTOM.to_owned();
    }
    let qp = cfg.quality_preset.trim().to_ascii_lowercase();
    cfg.quality_preset = match qp.as_str() {
        "1080p" => "1080p".to_owned(),
        "720p" => "720p".to_owned(),
        "audio" => "audio".to_owned(),
        "custom" => "custom".to_owned(),
        _ => "best".to_owned(),
    };
    let mc = cfg.merge_container.trim().to_ascii_lowercase();
    cfg.merge_container = match mc.as_str() {
        "mp4" => "mp4".to_owned(),
        "mkv" => "mkv".to_owned(),
        "webm" => "webm".to_owned(),
        _ => "default".to_owned(),
    };
    cfg.playlist_preview_cap = cfg.playlist_preview_cap.clamp(1, 500);
    if cfg.active_profile.trim().is_empty() {
        cfg.active_profile = default_active_profile();
    }
    cfg
}

pub fn profiles_file_path() -> PathBuf {
    rustdl_config_dir().join("rustdl_profiles.json")
}

pub(crate) fn profiles_path() -> PathBuf {
    if dirs::config_dir().is_some() {
        profiles_file_path()
    } else {
        PathBuf::from("rustdl_profiles.json")
    }
}

pub fn export_settings_json(settings: &AppSettings, path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).ok();
        }
    }
    let raw = serde_json::to_string_pretty(settings).context("failed to serialize settings")?;
    fs::write(path, raw).with_context(|| format!("failed to write {}", path.to_string_lossy()))?;
    Ok(())
}

pub fn import_settings_json(path: &std::path::Path) -> Result<AppSettings> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.to_string_lossy()))?;
    let mut cfg = serde_json::from_str::<AppSettings>(&raw).context("invalid settings JSON")?;
    if cfg.output_dir.trim().is_empty() || !PathBuf::from(&cfg.output_dir).is_dir() {
        cfg.output_dir = default_downloads().to_string_lossy().to_string();
    }
    Ok(cfg)
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
    load_json_file(queue_path(), "download queue")
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
    let active: std::collections::HashSet<u64> = items.iter().map(|it| it.item_id).collect();
    crate::thumbnail_store::prune_downloader_thumbnails(&active);
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConvertQueueSnapshot {
    pub input_paths: String,
    pub next_item_id: u64,
    pub items: Vec<ConvertQueueItem>,
}

pub fn convert_queue_file_path() -> PathBuf {
    rustdl_config_dir().join("rustdl_convert_queue.json")
}

fn legacy_av1_queue_file_path() -> PathBuf {
    rustdl_config_dir().join("rustdl_av1_queue.json")
}

fn convert_queue_path() -> PathBuf {
    if dirs::config_dir().is_some() {
        convert_queue_file_path()
    } else {
        PathBuf::from("rustdl_convert_queue.json")
    }
}

fn legacy_av1_queue_path() -> PathBuf {
    if dirs::config_dir().is_some() {
        legacy_av1_queue_file_path()
    } else {
        PathBuf::from("rustdl_av1_queue.json")
    }
}

pub fn load_convert_queue_snapshot() -> ConvertQueueSnapshot {
    let path = convert_queue_path();
    if path.is_file() {
        return load_json_file(path, "converter queue");
    }
    load_json_file(legacy_av1_queue_path(), "converter queue (legacy)")
}

pub fn save_convert_queue_snapshot(snapshot: &ConvertQueueSnapshot) -> Result<()> {
    let path = convert_queue_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create converter queue directory: {}",
                parent.to_string_lossy()
            )
        })?;
    }
    let raw = serde_json::to_string_pretty(snapshot)
        .context("failed to serialize converter queue snapshot")?;
    fs::write(&path, raw).with_context(|| {
        format!(
            "failed to write converter queue file: {}",
            path.to_string_lossy()
        )
    })?;
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

    #[test]
    fn load_json_file_returns_default_on_invalid_json() {
        let dir = std::env::temp_dir().join(format!("rustdl_cfg_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("bad.json");
        fs::write(&path, "{not json").expect("write");
        let v: AppSettings = load_json_file(path.clone(), "settings");
        assert_eq!(v.worker_count, AppSettings::default().worker_count);
        assert!(dir.join("bad.json.bak").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
