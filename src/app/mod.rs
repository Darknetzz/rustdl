use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(windows)]
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui;
use eframe::egui::{Color32, RichText, TextureHandle};
use egui_material_icons::icons;
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;
use url::Url;

mod about;
mod background_spawn;
mod cards;
mod done_file_index;
mod events;
mod input_lines;
mod log_panel;
mod settings_panel;
mod thumbnails;
mod update_check;

pub(crate) use done_file_index::DONE_LOOKUP_MAX_ENTRIES;
pub(crate) use events::UiEvent;
pub(crate) use input_lines::{InputLineInfo, InputLineKind};
pub(crate) use log_panel::LogFilter;
pub(crate) use log_panel::{
    attach_paste_context_menu, draw_input_line_summary, draw_precheck_status, log_line_color,
    LOG_COLOR_ERROR, LOG_COLOR_WARN,
};

use crate::app_actions;
use crate::app_icon;
use crate::app_parsing::{
    human_bytes_ui, normalize_restored_item, parse_urls_from_text_blob, split_cli_like,
};
use crate::app_state::{self};
use crate::app_ui::{
    danger_button, draw_status_dot, secondary_button, status_color, success_button,
    warning_button,
};
use crate::config::{
    activity_log_file_path, default_downloads, export_queue_urls, load_activity_log,
    load_queue_items, load_settings, rustdl_config_dir, save_activity_log, save_queue_items,
    save_settings, trim_activity_log, AppSettings,
};
use crate::theme::{self, BG_CANVAS, BORDER_PANEL, TEXT_MUTED};
use crate::models::{ItemStatus, QueueItem};
use crate::pkg_version;
use crate::ui_icons;
use crate::ytdlp;

const ICON_ADD: &str = icons::ICON_ADD;
const ICON_CLEAR: &str = icons::ICON_CLOSE;
const ICON_REMOVE: &str = icons::ICON_DELETE;
const ICON_DOWNLOAD: &str = icons::ICON_DOWNLOAD;
const ICON_OK: &str = "✔";
const ICON_MISSING: &str = "✖";
const INPUT_SUMMARY_HOLD_SECS: f64 = 2.5;
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Executables,
    Download,
    Postprocess,
}

#[derive(Clone, Copy)]
enum DownloadPreset {
    BestQuality,
    AudioOnly,
    FastDownload,
    ArchiveMode,
}

#[derive(Clone, Copy)]
pub(crate) enum CancelPostAction {
    Ready,
    Remove,
}

pub struct PydlApp {
    runtime: Arc<Runtime>,
    tx: Sender<UiEvent>,
    rx: Receiver<UiEvent>,

    input_urls: String,
    output_dir: String,
    worker_count: usize,
    status_resolving: usize,
    status_ready: usize,
    status_queued: usize,
    status_active: usize,
    status_done: usize,
    status_failed: usize,
    has_yt_dlp: bool,
    has_ffmpeg: bool,
    has_ffprobe: bool,
    yt_dlp_version: String,
    ffmpeg_version: String,
    ffprobe_version: String,
    /// Ring buffer of log lines (avoids scanning a huge string every frame).
    log_lines: VecDeque<String>,
    settings: AppSettings,
    settings_open: bool,
    settings_dirty: bool,

    items: Vec<QueueItem>,
    pending_resolve_ids: HashMap<String, u64>,
    textures: HashMap<u64, TextureHandle>,
    /// Same artwork as the window icon, shown next to the title.
    logo: TextureHandle,
    thumbnail_attempted: HashSet<u64>,
    thumbnail_inflight: HashSet<u64>,
    next_item_id: u64,
    add_in_progress: bool,
    add_total_urls: usize,
    add_processed_urls: usize,
    add_current_url: Option<String>,
    queue_running: usize,
    download_cancel_flags: HashMap<u64, Arc<AtomicBool>>,
    cancel_post_actions: HashMap<u64, CancelPostAction>,
    log_filter: LogFilter,
    input_line_info: Vec<InputLineInfo>,
    input_line_info_hold: Vec<InputLineInfo>,
    input_line_info_hold_until: Option<f64>,
    /// `input_urls` at end of previous frame (for paste / newline heuristics).
    input_urls_snapshot: String,
    auto_add_after: Option<f64>,
    settings_tab: SettingsTab,
    restored_items_count: usize,
    show_restore_banner: bool,
    about_open: bool,
    exit_confirm_open: bool,
    /// After the user confirms quit, allow the next viewport close through.
    exit_allowed: bool,
    /// When set, only the matching queue section is shown (click download summary).
    queue_group_focus: Option<&'static str>,
    /// Scroll target set when focusing a queue group from the summary row.
    scroll_to_queue_group: Option<&'static str>,
    queue_search: String,
    selected_item_ids: HashSet<u64>,
    downloads_paused: bool,
    /// Avoid repeating desktop notifications for the same idle spell.
    session_complete_notified: bool,
    update_check_in_progress: bool,
    update_latest_version: Option<String>,
    update_release_url: Option<String>,
    update_has_update: bool,
    update_status_text: String,

    done_file_index: done_file_index::DoneFileIndex,
    /// Suppress repeat log spam when output index hits [`DONE_LOOKUP_MAX_ENTRIES`].
    done_lookup_truncation_logged: bool,

    http_client: reqwest::Client,
    thumb_semaphore: Arc<Semaphore>,
    /// When set, queue JSON is written after this instant (debounced).
    queue_save_deadline: Option<Instant>,
    /// When set, activity log JSON is written after this instant (debounced).
    log_save_deadline: Option<Instant>,

    /// Last egui time we appended a throttled noisy download line per item (see `events.rs`).
    download_log_throttle: HashMap<u64, f64>,
    /// Context-menu Paste: clipboard injected at start of next frame (see `attach_paste_context_menu`).
    deferred_menu_paste_urls: Option<String>,
    deferred_menu_paste_output_dir: Option<String>,
    /// Decoded thumbnails waiting for `load_texture` (bounded per frame).
    pending_thumbnail_uploads: VecDeque<(u64, egui::ColorImage)>,
    /// Rate-limits output-folder scans for the done-file index (hot path is every frame).
    last_done_lookup_poll: Option<Instant>,
    #[cfg(windows)]
    win_browser_drop_queue: Arc<Mutex<Vec<String>>>,
    #[cfg(windows)]
    win_browser_drop_target_installed: bool,
    #[cfg(windows)]
    win_browser_drop_target_setup_attempted: bool,
}

impl PydlApp {
    pub fn new(cc: &eframe::CreationContext<'_>, runtime: Arc<Runtime>) -> Self {
        let logo = app_icon::load_logo_texture(&cc.egui_ctx);
        let (tx, rx) = unbounded();
        let settings = load_settings();
        let log_lines = load_activity_log(settings.log_max_chars);
        let mut restored_items = load_queue_items();
        for it in &mut restored_items {
            normalize_restored_item(it);
        }
        let next_item_id = restored_items
            .iter()
            .map(|x| x.item_id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(15))
            .user_agent(format!("rustdl/{}", pkg_version::VERSION))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let thumb_semaphore = Arc::new(Semaphore::new(8));

        let mut app = Self {
            runtime,
            tx,
            rx,
            input_urls: String::new(),
            output_dir: settings.output_dir.clone(),
            worker_count: settings.worker_count.clamp(1, 6),
            status_resolving: 0,
            status_ready: 0,
            status_queued: 0,
            status_active: 0,
            status_done: 0,
            status_failed: 0,
            has_yt_dlp: false,
            has_ffmpeg: false,
            has_ffprobe: false,
            yt_dlp_version: String::new(),
            ffmpeg_version: String::new(),
            ffprobe_version: String::new(),
            log_lines,
            settings,
            settings_open: false,
            settings_dirty: false,
            items: restored_items,
            pending_resolve_ids: HashMap::new(),
            textures: HashMap::new(),
            logo,
            thumbnail_attempted: HashSet::new(),
            thumbnail_inflight: HashSet::new(),
            next_item_id,
            add_in_progress: false,
            add_total_urls: 0,
            add_processed_urls: 0,
            add_current_url: None,
            queue_running: 0,
            download_cancel_flags: HashMap::new(),
            cancel_post_actions: HashMap::new(),
            log_filter: LogFilter::All,
            input_line_info: Vec::new(),
            input_line_info_hold: Vec::new(),
            input_line_info_hold_until: None,
            input_urls_snapshot: String::new(),
            auto_add_after: None,
            settings_tab: SettingsTab::General,
            restored_items_count: 0,
            show_restore_banner: false,
            about_open: false,
            exit_confirm_open: false,
            exit_allowed: false,
            queue_group_focus: None,
            scroll_to_queue_group: None,
            queue_search: String::new(),
            selected_item_ids: HashSet::new(),
            downloads_paused: false,
            session_complete_notified: false,
            update_check_in_progress: false,
            update_latest_version: None,
            update_release_url: None,
            update_has_update: false,
            update_status_text: String::new(),
            done_file_index: done_file_index::DoneFileIndex::new(),
            done_lookup_truncation_logged: false,
            http_client,
            thumb_semaphore,
            queue_save_deadline: None,
            log_save_deadline: None,
            download_log_throttle: HashMap::new(),
            pending_thumbnail_uploads: VecDeque::new(),
            last_done_lookup_poll: None,
            deferred_menu_paste_urls: None,
            deferred_menu_paste_output_dir: None,
            #[cfg(windows)]
            win_browser_drop_queue: Arc::new(Mutex::new(Vec::new())),
            #[cfg(windows)]
            win_browser_drop_target_installed: false,
            #[cfg(windows)]
            win_browser_drop_target_setup_attempted: false,
        };
        app.restored_items_count = app.items.len();
        app.show_restore_banner = app.restored_items_count > 0;
        app.append_log(&format!(
            "--- Session started {} ---",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        ));
        app.refresh_deps();
        app.update_status();
        app.refresh_input_line_info();
        app
    }

    /// Keeps the window repainting during background work; egui is event-driven and would otherwise
    /// feel frozen until the next mouse/keyboard input.
    fn request_repaint_if_background_busy(&self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let input_summary_hold_active = self.input_line_info.is_empty()
            && self
                .input_line_info_hold_until
                .is_some_and(|until| now < until);
        let busy = self.add_in_progress
            || self.status_resolving > 0
            || self.status_active > 0
            || self.queue_running > 0
            || self.update_check_in_progress
            || !self.thumbnail_inflight.is_empty()
            || !self.pending_thumbnail_uploads.is_empty()
            || self.auto_add_after.is_some()
            || self.queue_save_deadline.is_some()
            || self.log_save_deadline.is_some()
            || input_summary_hold_active;
        if busy {
            // Cap idle repaint rate during heavy background work to reduce full UI passes.
            ctx.request_repaint_after(Duration::from_secs_f64(1.0 / 30.0));
        }
    }

    pub fn apply_ui_smoothness(ctx: &egui::Context) {
        ctx.set_visuals(theme::dark_visuals());
        ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(9.0, 7.0);
            style.spacing.button_padding = egui::vec2(14.0, 8.0);
            let r = egui::Rounding::same(6.0);
            style.visuals.widgets.noninteractive.rounding = r;
            style.visuals.widgets.inactive.rounding = r;
            style.visuals.widgets.hovered.rounding = r;
            style.visuals.widgets.active.rounding = r;
            style.visuals.window_rounding = egui::Rounding::same(10.0);
        });
    }

    const QUEUE_SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

    fn schedule_queue_save(&mut self) {
        self.queue_save_deadline = Some(Instant::now() + Self::QUEUE_SAVE_DEBOUNCE);
    }

    fn maybe_flush_queue_save(&mut self) {
        if let Some(deadline) = self.queue_save_deadline {
            if Instant::now() >= deadline {
                self.queue_save_deadline = None;
                self.flush_queue_to_disk();
            }
        }
    }

    fn flush_queue_to_disk(&mut self) {
        self.queue_save_deadline = None;
        if let Err(err) = save_queue_items(&self.items) {
            self.append_log(&format!("Failed to save queue state: {err}"));
        }
    }

    fn schedule_log_save(&mut self) {
        self.log_save_deadline = Some(Instant::now() + Self::QUEUE_SAVE_DEBOUNCE);
    }

    fn maybe_flush_log_save(&mut self) {
        if let Some(deadline) = self.log_save_deadline {
            if Instant::now() >= deadline {
                self.log_save_deadline = None;
                self.flush_log_to_disk();
            }
        }
    }

    fn flush_log_to_disk(&mut self) {
        self.log_save_deadline = None;
        if let Err(err) = save_activity_log(&self.log_lines) {
            eprintln!("rustdl: failed to save activity log: {err}");
        }
    }

    pub(super) fn clear_activity_log(&mut self) {
        self.log_lines.clear();
        self.flush_log_to_disk();
    }

    pub(super) fn open_config_folder(&mut self) {
        let dir = rustdl_config_dir();
        if let Err(e) = app_actions::open_path(&dir) {
            self.append_log(&format!("Failed to open config folder: {e}"));
        }
    }

    pub(super) fn open_activity_log_file(&mut self) {
        let path = activity_log_file_path();
        if !path.exists() {
            let _ = save_activity_log(&self.log_lines);
        }
        if let Err(e) = app_actions::open_path(&path) {
            self.append_log(&format!("Failed to open activity log: {e}"));
        }
    }

    fn toggle_logs_panel(&mut self) {
        self.settings.logs_open = !self.settings.logs_open;
        self.persist_settings();
    }

    fn focus_queue_group(&mut self, group: &'static str) {
        self.queue_group_focus = Some(group);
        self.scroll_to_queue_group = Some(group);
    }

    pub(super) fn item_matches_search(&self, item: &QueueItem) -> bool {
        let q = self.queue_search.trim().to_ascii_lowercase();
        if q.is_empty() {
            return true;
        }
        item.title.to_ascii_lowercase().contains(&q)
            || item.source_line.to_ascii_lowercase().contains(&q)
            || item.webpage_url.to_ascii_lowercase().contains(&q)
            || item
                .uploader
                .as_ref()
                .is_some_and(|u| u.to_ascii_lowercase().contains(&q))
            || item.video_id.to_ascii_lowercase().contains(&q)
    }

    fn pause_all_downloads(&mut self) {
        if self.downloads_paused {
            return;
        }
        self.downloads_paused = true;
        self.cancel_all_active(CancelPostAction::Ready);
        self.append_log("Downloads paused (active items moved back to ready).");
    }

    fn resume_all_downloads(&mut self) {
        if !self.downloads_paused {
            return;
        }
        self.downloads_paused = false;
        self.session_complete_notified = false;
        self.append_log("Downloads resumed.");
        self.start_downloads();
    }

    fn export_queue_to_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export queue URLs")
            .set_file_name("rustdl-queue.txt")
            .add_filter("Text", &["txt"])
            .save_file()
        else {
            return;
        };
        match export_queue_urls(&self.items, &path) {
            Ok(()) => self.append_log(&format!(
                "Exported {} URL(s) to {}",
                self.items.len(),
                path.to_string_lossy()
            )),
            Err(e) => self.append_log(&format!("Export failed: {e}")),
        }
    }

    fn remove_selected_items(&mut self) {
        let ids: Vec<u64> = self.selected_item_ids.iter().copied().collect();
        if ids.is_empty() {
            self.append_log("No items selected.");
            return;
        }
        for id in ids {
            let _ = self.remove_item_by_id(id);
        }
        self.selected_item_ids.clear();
        self.update_status();
        self.refresh_input_line_info();
        self.schedule_queue_save();
    }

    fn retry_selected_failed(&mut self) {
        for id in self.selected_item_ids.iter().copied().collect::<Vec<_>>() {
            if self
                .items
                .iter()
                .any(|x| x.item_id == id && x.status == ItemStatus::Failed)
            {
                self.retry_download_item_id(id);
            }
        }
    }

    fn maybe_notify_session_complete(&mut self) {
        if self.session_complete_notified
            || self.queue_running > 0
            || self.add_in_progress
            || self.status_resolving > 0
            || self.status_active > 0
            || self.status_queued > 0
        {
            return;
        }
        let has_items = !self.items.is_empty();
        let all_terminal = self.items.iter().all(|it| {
            matches!(
                it.status,
                ItemStatus::Done | ItemStatus::Failed | ItemStatus::Idle
            )
        });
        if !has_items || !all_terminal {
            return;
        }
        self.session_complete_notified = true;
        let summary = format!(
            "rustdl: {} done, {} failed",
            self.status_done, self.status_failed
        );
        if let Err(e) = notify_rust::Notification::new()
            .summary("rustdl")
            .body(&summary)
            .show()
        {
            self.append_log(&format!("Notification failed: {e}"));
        }
    }

    fn refresh_deps(&mut self) {
        let (yt, ffm, ffp) = ytdlp::get_external_tools_with_paths(
            &self.settings.yt_dlp_path,
            &self.settings.ffmpeg_path,
            &self.settings.ffprobe_path,
        );
        self.has_yt_dlp = yt;
        self.has_ffmpeg = ffm;
        self.has_ffprobe = ffp;
        self.yt_dlp_version = if yt {
            ytdlp::read_yt_dlp_version(&self.settings.yt_dlp_path).unwrap_or_default()
        } else {
            String::new()
        };
        self.ffmpeg_version = if ffm {
            ytdlp::read_ffmpeg_version(&self.settings.ffmpeg_path).unwrap_or_default()
        } else {
            String::new()
        };
        self.ffprobe_version = if ffp {
            ytdlp::read_ffprobe_version(&self.settings.ffprobe_path).unwrap_or_default()
        } else {
            String::new()
        };
    }

    fn append_log(&mut self, message: &str) {
        let line = crate::time_format::format_log_line(message);
        self.log_lines.push_back(line);
        trim_activity_log(&mut self.log_lines, self.settings.log_max_chars);
        self.schedule_log_save();
    }

    fn poll_done_file_lookup(&mut self) {
        const INTERVAL: Duration = Duration::from_millis(400);
        let now = Instant::now();
        let should_poll = match self.last_done_lookup_poll {
            None => true,
            Some(t) => now.saturating_duration_since(t) >= INTERVAL,
        };
        if should_poll {
            self.refresh_done_file_lookup();
            self.last_done_lookup_poll = Some(now);
        }
    }

    pub(super) fn persist_settings(&mut self) {
        self.settings.output_dir = self.output_dir.clone();
        self.settings.worker_count = self.worker_count.clamp(1, 6);
        if let Err(err) = save_settings(&self.settings) {
            self.append_log(&format!("Failed to save settings: {err}"));
        }
    }

    fn ytdlp_cookie_args(&self) -> Vec<String> {
        ytdlp::cookie_args_from_setting(&self.settings.yt_dlp_cookies)
    }

    fn ytdlp_impersonate_args(&self) -> Vec<String> {
        ytdlp::impersonate_args_from_setting(&self.settings.yt_dlp_impersonate)
    }

    /// Cookies and impersonation flags for `yt-dlp -J` when resolving URLs.
    fn metadata_extra_args(&self) -> Vec<String> {
        let mut args = self.ytdlp_impersonate_args();
        args.extend(self.ytdlp_cookie_args());
        args
    }

    fn download_extra_args(&self) -> Vec<String> {
        // Retry flags first; cookies; user "Extra args" follow and can override (yt-dlp: last wins).
        let mut args = Vec::new();
        if self.settings.yt_dlp_unlimited_retries {
            args.push("--retries".to_owned());
            args.push("infinite".to_owned());
            args.push("--fragment-retries".to_owned());
            args.push("infinite".to_owned());
        } else {
            let n = self.settings.yt_dlp_retry_count.to_string();
            args.push("--retries".to_owned());
            args.push(n.clone());
            args.push("--fragment-retries".to_owned());
            args.push(n);
        }
        args.extend(self.ytdlp_impersonate_args());
        args.extend(self.ytdlp_cookie_args());
        args.extend(split_cli_like(&self.settings.yt_dlp_extra_args));
        if self.settings.yt_ignore_errors {
            args.push("--ignore-errors".to_owned());
        }
        if self.settings.yt_restrict_filenames {
            args.push("--restrict-filenames".to_owned());
        }
        if self.settings.yt_write_info_json {
            args.push("--write-info-json".to_owned());
        }
        if self.settings.yt_write_auto_subs {
            args.push("--write-auto-subs".to_owned());
        }
        if self.settings.embed_thumbnail {
            args.push("--embed-thumbnail".to_owned());
        }
        if self.settings.yt_embed_metadata {
            args.push("--embed-metadata".to_owned());
        }
        if self.settings.ffmpeg_extract_audio_mp3 {
            args.push("--extract-audio".to_owned());
            args.push("--audio-format".to_owned());
            args.push("mp3".to_owned());
        } else if self.settings.ffmpeg_remux_mp4 {
            args.push("--remux-video".to_owned());
            args.push("mp4".to_owned());
        }
        let mut post_args = self.settings.ffmpeg_post_args.trim().to_owned();
        if self.settings.ffmpeg_faststart {
            if !post_args.is_empty() {
                post_args.push(' ');
            }
            post_args.push_str("-movflags +faststart");
        }
        if !post_args.trim().is_empty() {
            args.push("--postprocessor-args".to_owned());
            args.push(post_args);
        }
        args
    }

    fn yt_dlp_bin(&self) -> String {
        if self.settings.yt_dlp_path.trim().is_empty() {
            "yt-dlp".to_owned()
        } else {
            self.settings.yt_dlp_path.trim().to_owned()
        }
    }

    fn ffmpeg_bin(&self) -> String {
        if self.settings.ffmpeg_path.trim().is_empty() {
            String::new()
        } else {
            self.settings.ffmpeg_path.trim().to_owned()
        }
    }

    fn effective_download_command_preview(&self) -> String {
        let mut parts = vec![self.yt_dlp_bin(), "--newline".to_owned()];
        let ffmpeg = self.ffmpeg_bin();
        if !ffmpeg.is_empty() {
            parts.push("--ffmpeg-location".to_owned());
            parts.push(ffmpeg);
        }
        parts.extend(self.download_extra_args());
        parts.push("<url>".to_owned());
        parts.join(" ")
    }

    fn open_output_folder(&mut self) {
        if !Path::new(&self.output_dir).is_dir() {
            self.append_log("Output folder does not exist.");
            return;
        }
        if let Err(e) = app_actions::open_str_path(&self.output_dir) {
            self.append_log(&format!("Failed to open output folder: {e}"));
        }
    }

    fn refresh_done_file_lookup(&mut self) {
        self.done_file_index.refresh(&self.output_dir);
        if self.done_file_index.scan_truncated {
            if !self.done_lookup_truncation_logged {
                self.done_lookup_truncation_logged = true;
                self.append_log(&format!(
                    "Output folder listing truncated after {} entries; some files may not appear in Open/Reveal until you reduce folder size or move downloads.",
                    DONE_LOOKUP_MAX_ENTRIES
                ));
            }
        } else {
            self.done_lookup_truncation_logged = false;
        }
    }

    fn find_downloaded_file_for_item(
        &self,
        item: &QueueItem,
    ) -> Option<(PathBuf, std::time::SystemTime)> {
        self.done_file_index.find(&item.video_id)
    }

    fn open_file_path(&mut self, file_path: &Path) {
        if let Err(e) = app_actions::open_path(file_path) {
            self.append_log(&format!("Failed to open downloaded file: {e}"));
        }
    }

    fn delete_file_path(&mut self, file_path: &Path) {
        match fs::remove_file(file_path) {
            Ok(_) => {
                self.append_log(&format!("Deleted file: {}", file_path.to_string_lossy()));
                self.done_file_index.force_refresh();
                self.refresh_done_file_lookup();
            }
            Err(e) => self.append_log(&format!(
                "Failed to delete file {}: {e}",
                file_path.to_string_lossy()
            )),
        }
    }

    fn reveal_file_path(&mut self, file_path: &Path) {
        let target = if cfg!(target_os = "windows") {
            file_path.to_path_buf()
        } else if let Some(parent) = file_path.parent() {
            parent.to_path_buf()
        } else {
            file_path.to_path_buf()
        };
        if let Err(e) = app_actions::open_path(&target) {
            self.append_log(&format!("Failed to reveal file in folder: {e}"));
        }
    }

    fn start_update_check(&mut self) {
        if self.update_check_in_progress {
            return;
        }
        self.update_check_in_progress = true;
        self.update_status_text = "Checking for updates...".to_owned();
        background_spawn::spawn_update_check(&self.runtime, &self.tx, self.http_client.clone());
    }

    fn open_release_url(&mut self) {
        let Some(url) = self.update_release_url.clone() else {
            self.append_log("No release URL available.");
            return;
        };
        if let Err(e) = app_actions::open_browser(&url) {
            self.append_log(&format!("Failed to open release page: {e}"));
        }
    }

    fn apply_preset(&mut self, preset: DownloadPreset) {
        match preset {
            DownloadPreset::BestQuality => {
                self.settings.ffmpeg_extract_audio_mp3 = false;
                self.settings.ffmpeg_remux_mp4 = false;
                self.settings.ffmpeg_faststart = true;
                self.settings.yt_ignore_errors = false;
                self.settings.yt_write_info_json = false;
                self.settings.yt_dlp_extra_args = "--merge-output-format mp4".to_owned();
            }
            DownloadPreset::AudioOnly => {
                self.settings.ffmpeg_extract_audio_mp3 = true;
                self.settings.ffmpeg_remux_mp4 = false;
                self.settings.ffmpeg_faststart = false;
                self.settings.yt_dlp_extra_args = String::new();
            }
            DownloadPreset::FastDownload => {
                self.settings.ffmpeg_extract_audio_mp3 = false;
                self.settings.ffmpeg_remux_mp4 = false;
                self.settings.ffmpeg_faststart = false;
                self.settings.yt_ignore_errors = true;
                self.settings.yt_dlp_extra_args = "--concurrent-fragments 4".to_owned();
            }
            DownloadPreset::ArchiveMode => {
                self.settings.yt_write_info_json = true;
                self.settings.yt_embed_metadata = true;
                self.settings.yt_write_auto_subs = true;
                self.settings.yt_dlp_extra_args = "--write-description".to_owned();
            }
        }
        self.settings_dirty = true;
        self.persist_settings();
    }

    /// Queues every **failed** row that has a download URL (same as per-card **Retry download**).
    fn retry_failed_items(&mut self) {
        if !Path::new(&self.output_dir).is_dir() {
            self.append_log("Choose a valid output folder.");
            return;
        }
        let (has_yt, _, _) = ytdlp::get_external_tools_with_paths(
            &self.settings.yt_dlp_path,
            &self.settings.ffmpeg_path,
            &self.settings.ffprobe_path,
        );
        if !has_yt {
            self.append_log("yt-dlp not found (check PATH or Settings executable path).");
            self.refresh_deps();
            return;
        }
        let failed_no_url = self
            .items
            .iter()
            .filter(|it| {
                it.status == ItemStatus::Failed && !self.item_has_redownload_target(it)
            })
            .count();
        let ids: Vec<u64> = self
            .items
            .iter()
            .filter(|it| {
                it.status == ItemStatus::Failed && self.item_has_redownload_target(it)
            })
            .map(|it| it.item_id)
            .collect();
        if ids.is_empty() {
            if self.status_failed > 0 {
                self.append_log(
                    "No failed items have a video URL to retry. Check the row or re-add the link.",
                );
            } else {
                self.append_log("No failed downloads to retry.");
            }
            return;
        }
        self.persist_settings();
        self.refresh_done_file_lookup();
        for id in &ids {
            self.prepare_item_redownload_reset(*id);
        }
        self.refresh_done_file_lookup();
        self.update_status();
        self.schedule_queue_save();
        self.append_log(&format!(
            "Retrying {} failed download(s).{}",
            ids.len(),
            if failed_no_url > 0 {
                format!(" Skipped {failed_no_url} without a URL.")
            } else {
                String::new()
            }
        ));
        self.spawn_download_workers(ids);
    }

    fn maybe_auto_start_downloads(&mut self) {
        if self.downloads_paused || !self.settings.auto_start_downloads {
            return;
        }
        let has_idle_items = self
            .items
            .iter()
            .any(|x| x.status == ItemStatus::Idle && x.error.is_none());
        if has_idle_items {
            self.start_downloads();
        }
    }

    fn probe_done_item_resolution_if_missing(&mut self, item_id: u64) {
        let Some(idx) = self.items.iter().position(|x| x.item_id == item_id) else {
            return;
        };
        if self.items[idx].width.is_some() && self.items[idx].height.is_some() {
            return;
        }
        if self.items[idx].status != ItemStatus::Done {
            return;
        }
        let Some((path, _)) = self.find_downloaded_file_for_item(&self.items[idx]) else {
            return;
        };
        let path_str = path.to_string_lossy().to_string();
        if let Some((w, h)) =
            ytdlp::probe_video_resolution_with_path(&path_str, &self.settings.ffprobe_path)
        {
            self.items[idx].width = Some(w);
            self.items[idx].height = Some(h);
        }
    }

    fn update_status(&mut self) {
        let counts = app_state::compute_status_counts(&self.items);
        self.status_resolving = counts.resolving;
        self.status_ready = counts.ready;
        self.status_queued = counts.queued;
        self.status_active = counts.active;
        self.status_done = counts.done;
        self.status_failed = counts.failed;
    }

    fn dedupe_keys(&self) -> HashSet<String> {
        let mut keys = HashSet::new();
        for it in &self.items {
            if it.status == ItemStatus::Resolving {
                continue;
            }
            keys.insert(ytdlp::normalize_url_for_dedupe(&it.source_line));
            if !it.webpage_url.is_empty() {
                keys.insert(ytdlp::normalize_url_for_dedupe(&it.webpage_url));
            }
            if !it.video_id.is_empty() {
                keys.insert(format!("vid:{}", it.video_id));
            }
        }
        keys.into_iter().filter(|k| !k.is_empty()).collect()
    }

    fn refresh_input_line_info(&mut self) {
        let lines: Vec<String> = self
            .input_urls
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let existing = self.dedupe_keys();
        self.input_line_info = input_lines::analyze_input_lines(&lines, &existing);
    }

    fn hold_current_input_line_summary(&mut self, now: f64) {
        if self.input_line_info.is_empty() {
            self.input_line_info_hold.clear();
            self.input_line_info_hold_until = None;
            return;
        }
        self.input_line_info_hold = self.input_line_info.clone();
        self.input_line_info_hold_until = Some(now + INPUT_SUMMARY_HOLD_SECS);
    }

    fn clear_input_urls_with_summary_hold(&mut self, now: f64) {
        self.hold_current_input_line_summary(now);
        self.input_urls.clear();
        self.refresh_input_line_info();
    }

    fn collect_valid_new_lines(&self) -> Vec<String> {
        self.input_line_info
            .iter()
            .filter(|x| x.kind == InputLineKind::Valid)
            .map(|x| x.line.clone())
            .collect()
    }

    fn queue_urls_for_resolve(&mut self, lines: Vec<String>) {
        if lines.is_empty() {
            self.append_log("Add at least one URL.");
            return;
        }
        let (has_yt, _, _) = ytdlp::get_external_tools_with_paths(
            &self.settings.yt_dlp_path,
            &self.settings.ffmpeg_path,
            &self.settings.ffprobe_path,
        );
        if !has_yt {
            self.append_log("yt-dlp not found (check PATH or Settings executable path).");
            self.refresh_deps();
            return;
        }
        self.add_in_progress = true;
        self.add_total_urls = 0;
        self.add_processed_urls = 0;
        self.add_current_url = None;
        let mut queued_lines = Vec::new();
        for line in lines {
            let iid = self.next_item_id;
            self.next_item_id += 1;
            self.items
                .insert(0, QueueItem::pending_metadata(iid, line.clone()));
            self.pending_resolve_ids.insert(line.clone(), iid);
            queued_lines.push(line);
        }
        self.add_total_urls = queued_lines.len();
        self.update_status();
        self.schedule_queue_save();
        if queued_lines.is_empty() {
            self.add_in_progress = false;
            self.append_log("No new URLs to add (all duplicates).");
            return;
        }
        let yt_dlp_bin = self.yt_dlp_bin();
        let metadata_args = self.metadata_extra_args();
        background_spawn::spawn_url_resolve_pipeline(
            &self.runtime,
            &self.tx,
            yt_dlp_bin,
            metadata_args,
            queued_lines,
        );
    }

    fn add_urls(&mut self, now: f64) {
        let lines = self.collect_valid_new_lines();
        self.queue_urls_for_resolve(lines);
        self.clear_input_urls_with_summary_hold(now);
    }

    fn import_urls_from_file(&mut self) {
        let Some(path) = app_actions::pick_url_input_file() else {
            return;
        };
        let content = match fs::read_to_string(&path) {
            Ok(x) => x,
            Err(e) => {
                self.append_log(&format!(
                    "Failed to read URL file {}: {e}",
                    path.to_string_lossy()
                ));
                return;
            }
        };
        let parsed = parse_urls_from_text_blob(&content);
        if parsed.is_empty() {
            self.append_log("Selected file had no URL entries.");
            return;
        }
        let n = parsed.len();
        self.extend_input_urls_with_lines(parsed, Some(0.0));
        self.append_log(&format!(
            "Imported {} URL candidate(s) from {}.",
            n,
            path.to_string_lossy()
        ));
    }

    fn collect_idle_download_item_ids(&self) -> Vec<u64> {
        self.items
            .iter()
            .filter(|it| it.status == ItemStatus::Idle && it.error.is_none())
            .map(|it| it.item_id)
            .collect()
    }

    fn spawn_download_workers(&mut self, pending_ids: Vec<u64>) {
        if self.downloads_paused {
            self.append_log("Downloads are paused. Click Resume to continue.");
            return;
        }
        if pending_ids.is_empty() {
            self.append_log("Nothing to download.");
            return;
        }
        self.session_complete_notified = false;

        for id in &pending_ids {
            if let Some(it) = self.items.iter_mut().find(|x| x.item_id == *id) {
                it.status = ItemStatus::Queued;
                it.detail = "Queued".to_owned();
            }
        }
        self.update_status();
        self.schedule_queue_save();

        let mut groups = vec![Vec::<u64>::new(); self.worker_count.max(1)];
        let groups_len = groups.len();
        for (idx, iid) in pending_ids.into_iter().enumerate() {
            groups[idx % groups_len].push(iid);
        }
        let download_args = self.download_extra_args();
        let yt_dlp_bin = self.yt_dlp_bin();
        let ffmpeg_bin = self.ffmpeg_bin();

        for ids in groups.into_iter().filter(|g| !g.is_empty()) {
            self.queue_running += 1;
            let urls = ids
                .iter()
                .filter_map(|iid| {
                    self.items.iter().find(|x| x.item_id == *iid).map(|x| {
                        let cancel_flag = self
                            .download_cancel_flags
                            .entry(*iid)
                            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
                            .clone();
                        (
                            *iid,
                            x.webpage_url.clone(),
                            x.source_line.clone(),
                            cancel_flag,
                        )
                    })
                })
                .collect::<Vec<_>>();
            background_spawn::spawn_download_worker(
                &self.runtime,
                &self.tx,
                self.output_dir.clone(),
                download_args.clone(),
                yt_dlp_bin.clone(),
                ffmpeg_bin.clone(),
                urls,
            );
        }
    }

    fn start_downloads(&mut self) {
        if self.downloads_paused {
            self.append_log("Downloads are paused. Click Resume first.");
            return;
        }
        if self.items.is_empty() {
            self.append_log("Add URLs first.");
            return;
        }
        if !Path::new(&self.output_dir).is_dir() {
            self.append_log("Choose a valid output folder.");
            return;
        }
        self.persist_settings();
        let pending_ids = self.collect_idle_download_item_ids();
        self.spawn_download_workers(pending_ids);
    }

    fn remove_item_by_id(&mut self, item_id: u64) -> bool {
        let Some(idx) = self.items.iter().position(|x| x.item_id == item_id) else {
            return false;
        };
        if self.items[idx].status == ItemStatus::Resolving {
            self.pending_resolve_ids.retain(|_, iid| *iid != item_id);
        }
        self.items.remove(idx);
        self.textures.remove(&item_id);
        self.thumbnail_attempted.remove(&item_id);
        self.thumbnail_inflight.remove(&item_id);
        self.download_cancel_flags.remove(&item_id);
        self.cancel_post_actions.remove(&item_id);
        true
    }

    fn request_cancel_item(&mut self, item_id: u64, post_action: CancelPostAction) {
        let Some(idx) = self.items.iter().position(|x| x.item_id == item_id) else {
            return;
        };
        match self.items[idx].status {
            ItemStatus::Queued => {
                self.cancel_post_actions.remove(&item_id);
                if matches!(post_action, CancelPostAction::Remove) {
                    let _ = self.remove_item_by_id(item_id);
                    self.append_log(&format!(
                        "[item {item_id}] Cancelled and removed from queue."
                    ));
                } else {
                    let it = &mut self.items[idx];
                    it.status = ItemStatus::Idle;
                    it.percent = 0.0;
                    it.speed_text = "-".to_owned();
                    it.eta_text = "-".to_owned();
                    it.detail = "Cancelled (ready)".to_owned();
                    self.append_log(&format!(
                        "[item {item_id}] Cancelled and moved back to ready."
                    ));
                }
                self.download_cancel_flags.remove(&item_id);
            }
            ItemStatus::Downloading => {
                if let Some(flag) = self.download_cancel_flags.get(&item_id) {
                    flag.store(true, Ordering::Relaxed);
                } else {
                    self.download_cancel_flags
                        .insert(item_id, Arc::new(AtomicBool::new(true)));
                }
                self.cancel_post_actions.insert(item_id, post_action);
                self.items[idx].detail = match post_action {
                    CancelPostAction::Ready => "Cancelling… will return to ready".to_owned(),
                    CancelPostAction::Remove => "Cancelling… will remove row".to_owned(),
                };
                self.append_log(&format!("[item {item_id}] Cancel requested."));
            }
            _ => return,
        }
        self.update_status();
        self.refresh_input_line_info();
        self.schedule_queue_save();
    }

    fn cancel_all_active(&mut self, post_action: CancelPostAction) {
        let ids: Vec<u64> = self
            .items
            .iter()
            .filter(|it| matches!(it.status, ItemStatus::Queued | ItemStatus::Downloading))
            .map(|it| it.item_id)
            .collect();
        if ids.is_empty() {
            self.append_log("No active queued/downloading items to cancel.");
            return;
        }
        for item_id in &ids {
            self.request_cancel_item(*item_id, post_action);
        }
        self.append_log(&format!("Cancel requested for {} item(s).", ids.len()));
    }

    fn item_has_redownload_target(&self, item: &QueueItem) -> bool {
        let u = item.webpage_url.trim();
        let s = item.source_line.trim();
        !u.is_empty() || (!s.is_empty() && Url::parse(s).is_ok())
    }

    /// Removes a matched output file (if any) and clears progress so the row can be queued again.
    fn prepare_item_redownload_reset(&mut self, item_id: u64) {
        let Some(idx) = self.items.iter().position(|x| x.item_id == item_id) else {
            return;
        };
        if let Some((path, _)) = self.find_downloaded_file_for_item(&self.items[idx]) {
            if let Err(e) = fs::remove_file(&path) {
                self.append_log(&format!(
                    "Could not remove old file {}: {e}",
                    path.to_string_lossy()
                ));
            } else {
                self.append_log(&format!("Removed old file: {}", path.to_string_lossy()));
            }
            self.done_file_index.force_refresh();
            self.refresh_done_file_lookup();
        }
        {
            let it = &mut self.items[idx];
            it.error = None;
            it.status = ItemStatus::Idle;
            it.percent = 0.0;
            it.size_text = "-".to_owned();
            it.speed_text = "-".to_owned();
            it.eta_text = "-".to_owned();
            it.detail = "Re-downloading…".to_owned();
        }
    }

    /// Deletes the matched output file if present, resets the row to idle, and starts a download for this id only.
    fn redownload_item_id(&mut self, item_id: u64) {
        if !Path::new(&self.output_dir).is_dir() {
            self.append_log("Choose a valid output folder.");
            return;
        }
        let Some(idx) = self.items.iter().position(|x| x.item_id == item_id) else {
            return;
        };
        if !self.item_has_redownload_target(&self.items[idx]) {
            self.append_log(&format!(
                "[item {item_id}] Cannot re-download: no video URL on this row."
            ));
            return;
        }
        self.persist_settings();
        self.refresh_done_file_lookup();
        self.prepare_item_redownload_reset(item_id);
        self.refresh_done_file_lookup();
        self.update_status();
        self.schedule_queue_save();
        self.spawn_download_workers(vec![item_id]);
    }

    /// Same as [`Self::redownload_item_id`] but only allowed from a failed download row (clear UX label).
    fn retry_download_item_id(&mut self, item_id: u64) {
        let Some(idx) = self.items.iter().position(|x| x.item_id == item_id) else {
            return;
        };
        if self.items[idx].status != ItemStatus::Failed {
            return;
        }
        self.redownload_item_id(item_id);
    }

    /// Re-run yt-dlp metadata for this row (same URL), replacing the card when resolve completes.
    fn retry_metadata_item_id(&mut self, item_id: u64) {
        if self.add_in_progress {
            self.append_log(
                "Wait for the current \"Add URLs\" / metadata batch to finish before retrying.",
            );
            return;
        }
        let (has_yt, _, _) = ytdlp::get_external_tools_with_paths(
            &self.settings.yt_dlp_path,
            &self.settings.ffmpeg_path,
            &self.settings.ffprobe_path,
        );
        if !has_yt {
            self.append_log("yt-dlp not found (check PATH or Settings executable path).");
            self.refresh_deps();
            return;
        }
        let Some(idx) = self.items.iter().position(|x| x.item_id == item_id) else {
            return;
        };
        if self.items[idx].status != ItemStatus::Idle {
            return;
        }
        let line = self.items[idx].source_line.clone();
        if line.trim().is_empty() {
            self.append_log(&format!(
                "[item {item_id}] Cannot retry: no source URL on this row."
            ));
            return;
        }
        self.persist_settings();
        self.pending_resolve_ids.insert(line.clone(), item_id);
        self.items[idx] = QueueItem::pending_metadata(item_id, line.clone());
        self.add_in_progress = true;
        self.add_total_urls = 1;
        self.add_processed_urls = 0;
        self.add_current_url = Some(line.clone());
        self.update_status();
        self.schedule_queue_save();
        let yt_dlp_bin = self.yt_dlp_bin();
        let metadata_args = self.metadata_extra_args();
        background_spawn::spawn_url_resolve_pipeline(
            &self.runtime,
            &self.tx,
            yt_dlp_bin,
            metadata_args,
            vec![line],
        );
    }

    fn streams_incomplete_message(has_video: bool, has_audio: bool) -> Option<String> {
        if !has_video && !has_audio {
            Some("File has neither video nor audio streams according to ffprobe.".to_owned())
        } else if !has_video {
            Some(
                "Download has audio only (no video stream). Try yt-dlp -f \"bv*+ba/b\" with ffmpeg merge, or check available formats (-F)."
                    .to_owned(),
            )
        } else if !has_audio {
            Some(
                "Download has video but no audio stream. Try a different format or merge (bestvideo+bestaudio)."
                    .to_owned(),
            )
        } else {
            None
        }
    }

    /// Probes the saved file for this queue row. Err = could not probe; Ok((v, a)) = stream presence.
    fn probe_saved_file_streams(&self, item: &QueueItem) -> Result<(bool, bool), String> {
        if !self.has_ffprobe {
            return Err("ffprobe not found (Settings → Executables).".to_owned());
        }
        if item.video_id.trim().is_empty() {
            return Err("No video id; cannot match a file in the output folder.".to_owned());
        }
        let Some((path, _)) = self.find_downloaded_file_for_item(item) else {
            return Err("No matching file in the output folder.".to_owned());
        };
        let path_str = path.to_string_lossy().to_string();
        ytdlp::probe_video_audio_stream_presence(&path_str, &self.settings.ffprobe_path).ok_or_else(
            || {
                "ffprobe failed or could not parse output. Check the file and ffprobe path."
                    .to_owned()
            },
        )
    }

    /// User-triggered check: updates detail and log; does not change status.
    fn check_streams_for_item_id(&mut self, item_id: u64) {
        let Some(idx) = self.items.iter().position(|x| x.item_id == item_id) else {
            return;
        };
        let item = self.items[idx].clone();
        let msg = match self.probe_saved_file_streams(&item) {
            Ok((v, a)) => {
                let summary = format!(
                    "Streams: {} video, {} audio",
                    if v { "has" } else { "no" },
                    if a { "has" } else { "no" },
                );
                if v {
                    let path_str = self
                        .find_downloaded_file_for_item(&item)
                        .map(|(p, _)| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !path_str.is_empty() {
                        if let Some((w, h)) = ytdlp::probe_video_resolution_with_path(
                            &path_str,
                            &self.settings.ffprobe_path,
                        ) {
                            self.items[idx].width = Some(w);
                            self.items[idx].height = Some(h);
                        }
                    }
                }
                summary
            }
            Err(e) => format!("Check failed: {e}"),
        };
        self.items[idx].detail = msg.clone();
        self.append_log(&format!("[item {item_id}] {msg}"));
        self.schedule_queue_save();
    }

    /// Re-scan saved files for done/failed rows and mark failed when video or audio is missing (skipped in MP3 extraction mode).
    fn recheck_all_saved_downloads(&mut self) {
        if !self.has_ffprobe {
            self.append_log("Cannot re-check saved files: ffprobe not found.");
            return;
        }
        if self.settings.ffmpeg_extract_audio_mp3 {
            self.append_log("Skipping re-check: MP3 extraction mode is enabled.");
            return;
        }
        self.refresh_done_file_lookup();
        let ids: Vec<u64> = self
            .items
            .iter()
            .filter(|it| matches!(it.status, ItemStatus::Done | ItemStatus::Failed))
            .map(|it| it.item_id)
            .collect();
        let mut issues = 0usize;
        for item_id in ids {
            let Some(idx) = self.items.iter().position(|x| x.item_id == item_id) else {
                continue;
            };
            let item = self.items[idx].clone();
            if item.video_id.trim().is_empty() {
                continue;
            }
            if self.find_downloaded_file_for_item(&item).is_none() {
                continue;
            }
            let probe = self.probe_saved_file_streams(&item);
            let fail_msg = match probe {
                Ok((v, a)) => Self::streams_incomplete_message(v, a),
                Err(e) => Some(e),
            };
            if let Some(msg) = fail_msg {
                self.items[idx].status = ItemStatus::Failed;
                self.items[idx].detail = msg.clone();
                issues += 1;
                self.append_log(&format!("[item {item_id}] Re-check: {msg}"));
            }
        }
        self.update_status();
        self.schedule_queue_save();
        self.append_log(&format!(
            "Re-checked saved files: {issues} item(s) marked failed (missing stream or probe error)."
        ));
    }

    fn verify_done_item_has_video_and_audio(&self, item_id: u64) -> Option<String> {
        if !self.settings.verify_output_video_audio
            || self.settings.ffmpeg_extract_audio_mp3
            || !self.has_ffprobe
        {
            return None;
        }
        let idx = self.items.iter().position(|x| x.item_id == item_id)?;
        let item = &self.items[idx];
        if item.video_id.trim().is_empty() {
            return None;
        }
        let (_path, _) = self.find_downloaded_file_for_item(item)?;
        let (has_video, has_audio) = match self.probe_saved_file_streams(item) {
            Ok(x) => x,
            Err(msg) => return Some(msg),
        };
        Self::streams_incomplete_message(has_video, has_audio)
    }

    #[cfg(windows)]
    fn maybe_install_win_browser_drop_target(&mut self, frame: &eframe::Frame) {
        if self.win_browser_drop_target_installed || self.win_browser_drop_target_setup_attempted {
            return;
        }
        let Some(hwnd) = crate::win_icon::hwnd_from_frame(frame) else {
            return;
        };
        self.win_browser_drop_target_setup_attempted = true;
        match crate::win_drop_target::install_once(hwnd, self.win_browser_drop_queue.clone()) {
            Ok(()) => {
                self.win_browser_drop_target_installed = true;
            }
            Err(_) => {
                self.append_log(
                    "Could not register URL drag-and-drop (RegisterDragDrop failed). Browser drops may not work until restart.",
                );
            }
        }
    }

    #[cfg(windows)]
    fn drain_win_browser_url_drops(&mut self, ctx: &egui::Context) {
        let taken = match self.win_browser_drop_queue.lock() {
            Ok(mut g) => std::mem::take(&mut *g),
            Err(e) => std::mem::take(&mut *e.into_inner()),
        };
        if !taken.is_empty() {
            self.merge_dragged_urls_into_input(taken, ctx);
        }
    }

    fn merge_dragged_urls_into_input(&mut self, urls: Vec<String>, ctx: &egui::Context) {
        let deadline = ctx.input(|i| i.time + 0.7);
        self.extend_input_urls_with_lines(urls, Some(deadline));
    }

    /// Appends trimmed non-empty lines to the URL field, refreshes validation, and sets [`Self::auto_add_after`] when requested and settings allow.
    fn extend_input_urls_with_lines(&mut self, lines: Vec<String>, auto_add_deadline: Option<f64>) {
        let lines: Vec<String> = lines
            .into_iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if lines.is_empty() {
            return;
        }
        if !self.input_urls.trim().is_empty() && !self.input_urls.ends_with('\n') {
            self.input_urls.push('\n');
        }
        self.input_urls.push_str(&lines.join("\n"));
        if !self.input_urls.ends_with('\n') {
            self.input_urls.push('\n');
        }
        self.refresh_input_line_info();
        self.auto_add_after = if self.settings.auto_add_pasted_urls {
            auto_add_deadline
        } else {
            None
        };
    }

    fn apply_dropped_shortcut_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let mut urls = Vec::new();
        for df in dropped {
            if let Some(path) = df.path {
                if let Some(mut u) = crate::app_parsing::urls_from_dropped_os_path(&path) {
                    urls.append(&mut u);
                }
            }
        }
        if !urls.is_empty() {
            self.merge_dragged_urls_into_input(urls, ctx);
        }
    }

    fn exit_work_in_progress(&self) -> bool {
        self.add_in_progress
            || self.status_resolving > 0
            || self.status_queued > 0
            || self.status_active > 0
            || self.queue_running > 0
    }

    fn open_exit_confirm(&mut self) {
        self.exit_confirm_open = true;
    }

    fn confirm_exit(&mut self, ctx: &egui::Context) {
        self.exit_confirm_open = false;
        self.exit_allowed = true;
        self.flush_queue_to_disk();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_viewport_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.exit_allowed {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        self.open_exit_confirm();
    }

    fn draw_exit_confirm_dialog(&mut self, ctx: &egui::Context) {
        if !self.exit_confirm_open {
            return;
        }
        egui::Window::new("Quit rustdl?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if self.exit_work_in_progress() {
                    ui.label("Downloads or metadata fetches are still running.");
                    ui.label(
                        "Your queue will be saved. Active yt-dlp jobs may continue until they finish or you stop them.",
                    );
                } else {
                    ui.label("Quit rustdl?");
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if secondary_button(ui, "Cancel", true).clicked() {
                        self.exit_confirm_open = false;
                    }
                    if danger_button(ui, "Quit", true).clicked() {
                        self.confirm_exit(ctx);
                    }
                });
            });
    }
}
impl eframe::App for PydlApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        #[cfg(windows)]
        crate::win_icon::apply_native_window_icons(frame, &app_icon::window_icon());
        #[cfg(not(windows))]
        let _ = frame;
        ctx.set_zoom_factor(self.settings.ui_scale.clamp(0.85, 1.5));
        if let Some(text) = self.deferred_menu_paste_urls.take() {
            ctx.input_mut(|inp| inp.events.push(egui::Event::Paste(text)));
        }
        if let Some(text) = self.deferred_menu_paste_output_dir.take() {
            ctx.input_mut(|inp| inp.events.push(egui::Event::Paste(text)));
        }
        self.maybe_flush_queue_save();
        self.maybe_flush_log_save();
        self.process_events(ctx);
        #[cfg(windows)]
        {
            self.maybe_install_win_browser_drop_target(frame);
            self.drain_win_browser_url_drops(ctx);
        }
        self.apply_dropped_shortcut_files(ctx);
        self.handle_viewport_close_request(ctx);
        self.poll_done_file_lookup();
        if let Some(deadline) = self.auto_add_after {
            let now = ctx.input(|i| i.time);
            if !self.add_in_progress && now >= deadline {
                let valid = self.collect_valid_new_lines();
                if !valid.is_empty() {
                    self.queue_urls_for_resolve(valid);
                    self.clear_input_urls_with_summary_hold(now);
                } else {
                    self.refresh_input_line_info();
                    if input_lines::is_only_duplicate_lines(&self.input_line_info) {
                        self.clear_input_urls_with_summary_hold(now);
                    }
                }
                self.auto_add_after = None;
            }
        }
        let trigger_add = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
        let trigger_download = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::D));

        egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let sz = egui::vec2(40.0, 40.0);
                    let img = ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(self.logo.id(), sz))
                            .sense(egui::Sense::click()),
                    );
                    let title = ui.add(
                        egui::Label::new(RichText::new("rustdl").heading())
                            .sense(egui::Sense::click()),
                    );
                    let header = img
                        .union(title)
                        .on_hover_text("About rustdl — click to open");
                    if header.clicked() {
                        self.about_open = true;
                    }
                });
                ui.label(
                    "Add URLs to load previews; start downloads to see progress on each card.",
                );
                if self.show_restore_banner && self.restored_items_count > 0 {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            LOG_COLOR_WARN,
                            format!(
                                "Restored {} item(s) from previous session.",
                                self.restored_items_count
                            ),
                        );
                        if warning_button(
                            ui,
                            &format!("{} Dismiss", ui_icons::DISMISS),
                            true,
                        )
                        .clicked()
                        {
                            self.show_restore_banner = false;
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if secondary_button(
                        ui,
                        &format!("{} Settings", ui_icons::SETTINGS),
                        true,
                    )
                    .clicked()
                    {
                        self.settings_open = true;
                    }
                    if secondary_button(ui, &format!("{} Logs", ui_icons::LOGS), true).clicked() {
                        self.toggle_logs_panel();
                    }
                    if danger_button(ui, &format!("{} Exit", ui_icons::EXIT), true).clicked() {
                        self.open_exit_confirm();
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    draw_precheck_status(ui, "yt-dlp", self.has_yt_dlp, &self.yt_dlp_version);
                    ui.separator();
                    draw_precheck_status(ui, "ffmpeg", self.has_ffmpeg, &self.ffmpeg_version);
                    ui.separator();
                    draw_precheck_status(ui, "ffprobe", self.has_ffprobe, &self.ffprobe_version);
                });
                if !self.has_yt_dlp || !self.has_ffmpeg || !self.has_ffprobe {
                    ui.colored_label(
                        LOG_COLOR_WARN,
                        "Setup hint: configure missing tools in Settings -> Executables.",
                    );
                }
                #[cfg(not(windows))]
                ui.label(
                    RichText::new(
                        "Tip: browser drag-and-drop for URLs is supported on Windows only; paste URLs or drop .url/.txt files on other platforms.",
                    )
                    .small()
                    .color(TEXT_MUTED),
                );
                ui.separator();

                ui.label("URLs (one per line)");
                let prev_url_snapshot = self.input_urls_snapshot.clone();
                let url_edit = ui.add_sized(
                    [ui.available_width(), 120.0],
                    egui::TextEdit::multiline(&mut self.input_urls)
                        .hint_text(
                            "https://... — paste, drag from browser, or drop .url / .webloc / list (.txt, .m3u)",
                        ),
                );
                attach_paste_context_menu(&url_edit, &mut self.deferred_menu_paste_urls);
                if url_edit.changed() {
                    let paste_event = ctx.input(|i| {
                        i.events
                            .iter()
                            .any(|e| matches!(e, egui::Event::Paste(_)))
                    });
                    input_lines::append_newline_after_pasted_valid_url(
                        &mut self.input_urls,
                        &prev_url_snapshot,
                        paste_event,
                        url_edit.has_focus(),
                    );
                    self.refresh_input_line_info();
                    self.input_line_info_hold_until = None;
                    if self.settings.auto_add_pasted_urls {
                        self.auto_add_after = Some(ctx.input(|i| i.time + 0.7));
                    } else {
                        self.auto_add_after = None;
                    }
                }
                let now = ctx.input(|i| i.time);
                let summary_lines = if self.input_line_info.is_empty()
                    && self
                        .input_line_info_hold_until
                        .is_some_and(|until| now < until)
                {
                    &self.input_line_info_hold
                } else {
                    &self.input_line_info
                };
                draw_input_line_summary(ui, summary_lines);
                log_panel::draw_input_line_preview(ui, summary_lines);

                ui.horizontal_wrapped(|ui| {
                    if success_button(ui, &format!("{ICON_ADD} Add URLs"), !self.add_in_progress)
                        .clicked()
                    {
                        self.add_urls(ctx.input(|i| i.time));
                    }
                    if secondary_button(
                        ui,
                        &format!(
                            "{} Import file (.txt/.csv)",
                            ui_icons::IMPORT_FILE
                        ),
                        true,
                    )
                    .clicked()
                    {
                        self.import_urls_from_file();
                    }
                    if self.add_in_progress {
                        ui.spinner();
                        let mut msg = format!(
                            "Adding URLs ({}/{})",
                            self.add_processed_urls, self.add_total_urls
                        );
                        if let Some(current) = &self.add_current_url {
                            if !current.is_empty() {
                                let short = current.chars().take(56).collect::<String>();
                                let suffix = if current.chars().count() > 56 {
                                    "..."
                                } else {
                                    ""
                                };
                                msg.push_str(&format!(" - fetching metadata for {short}{suffix}"));
                            }
                        }
                        ui.label(RichText::new(msg).small().color(Color32::LIGHT_BLUE));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let mut parts: Vec<(&str, usize, Color32)> = Vec::new();
                    if self.status_resolving > 0 {
                        parts.push((
                            "resolving",
                            self.status_resolving,
                            status_color(ItemStatus::Resolving),
                        ));
                    }
                    if self.status_ready > 0 {
                        parts.push((
                            "ready",
                            self.status_ready,
                            status_color(ItemStatus::Idle),
                        ));
                    }
                    if self.status_queued > 0 {
                        parts.push((
                            "queued",
                            self.status_queued,
                            status_color(ItemStatus::Queued),
                        ));
                    }
                    if self.status_active > 0 {
                        parts.push((
                            "active",
                            self.status_active,
                            status_color(ItemStatus::Downloading),
                        ));
                    }
                    if self.status_done > 0 {
                        parts.push(("done", self.status_done, status_color(ItemStatus::Done)));
                    }
                    if self.status_failed > 0 {
                        parts.push((
                            "failed",
                            self.status_failed,
                            status_color(ItemStatus::Failed),
                        ));
                    }
                    if !parts.is_empty() {
                        ui.label(RichText::new("Downloads:").color(TEXT_MUTED));
                        if self.queue_group_focus.is_some()
                            && ui.small_button("Show all").clicked()
                        {
                            self.queue_group_focus = None;
                        }
                        for (idx, (name, count, color)) in parts.iter().enumerate() {
                            let suffix = if idx + 1 == parts.len() { "" } else { "," };
                            let group = match *name {
                                "ready" => "Ready",
                                "queued" | "active" => "Active",
                                "done" => "Done",
                                "failed" => "Issues",
                                "resolving" => "Resolving",
                                _ => "Active",
                            };
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                draw_status_dot(ui, *color);
                                let label = format!("{count} {name}{suffix}");
                                let r = ui.add(
                                    egui::Label::new(RichText::new(label).color(*color))
                                        .sense(egui::Sense::click()),
                                );
                                if r.clicked() {
                                    self.focus_queue_group(group);
                                }
                                r.on_hover_text(format!("Show {group} items"));
                            });
                        }
                    }
                });
                let total_finished = self.status_done + self.status_failed;
                let total_known =
                    self.status_ready + self.status_queued + self.status_active + total_finished;
                if total_known > 0 {
                    let session_busy =
                        self.status_active > 0 || self.queue_running > 0 || self.add_in_progress;
                    let pb = egui::ProgressBar::new(total_finished as f32 / total_known as f32)
                        .animate(session_busy)
                        .text(format!(
                            "Session progress: {}/{} done ({} failed)",
                            total_finished, total_known, self.status_failed
                        ));
                    let pb_resp = ui.add(pb);
                    if pb_resp.clicked() {
                        self.focus_queue_group("Done");
                    }
                    if self.status_ready == 0
                        && self.status_queued == 0
                        && self.status_active == 0
                        && self.status_resolving == 0
                        && total_finished > 0
                    {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                status_color(ItemStatus::Done),
                                "All downloads finished for this session.",
                            );
                            if secondary_button(
                                ui,
                                &format!("{} Open output folder", ui_icons::OPEN_FOLDER),
                                true,
                            )
                            .clicked()
                            {
                                self.open_output_folder();
                            }
                        });
                    }
                    let totals = self.transfer_totals();
                    if totals.with_known_total > 0 && totals.known_total_bytes > 0 {
                        let pct = (totals.downloaded_bytes as f64
                            / totals.known_total_bytes as f64
                            * 100.0)
                            .clamp(0.0, 100.0);
                        ui.label(
                            RichText::new(format!(
                                "Transfer: {} / {} ({pct:.1}%)",
                                human_bytes_ui(totals.downloaded_bytes),
                                human_bytes_ui(totals.known_total_bytes),
                            ))
                            .small()
                            .color(Color32::GRAY),
                        );
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Output folder");
                    let output_dir_edit = ui.text_edit_singleline(&mut self.output_dir);
                    attach_paste_context_menu(
                        &output_dir_edit,
                        &mut self.deferred_menu_paste_output_dir,
                    );
                    if output_dir_edit.changed() {
                        self.persist_settings();
                        self.last_done_lookup_poll = None;
                    }
                    if secondary_button(
                        ui,
                        &format!("{} Use Downloads", ui_icons::USE_DOWNLOADS),
                        true,
                    )
                    .clicked()
                    {
                        self.output_dir = default_downloads().to_string_lossy().to_string();
                        self.persist_settings();
                        self.last_done_lookup_poll = None;
                    }
                    if secondary_button(
                        ui,
                        &format!("{} Open output folder", ui_icons::OPEN_FOLDER),
                        true,
                    )
                    .clicked()
                    {
                        self.open_output_folder();
                    }
                });

                ui.separator();
                let has_idle_items = self
                    .items
                    .iter()
                    .any(|x| x.status == ItemStatus::Idle && x.error.is_none());

                // Queue actions + primary download control live above the scroll so they stay
                // reachable when the window is short (CentralPanel does not scroll as a whole).
                ui.label(RichText::new("Queue").small().color(TEXT_MUTED));
                ui.horizontal(|ui| {
                    ui.label("Search");
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut self.queue_search)
                            .hint_text("Title, URL, uploader…")
                            .desired_width(200.0),
                    );
                    if search.changed() {
                        self.queue_group_focus = None;
                    }
                    if !self.queue_search.is_empty() && ui.small_button("Clear").clicked() {
                        self.queue_search.clear();
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    if self.downloads_paused {
                        if success_button(ui, &format!("{} Resume downloads", ICON_DOWNLOAD), true)
                            .clicked()
                        {
                            self.resume_all_downloads();
                        }
                    } else if warning_button(
                        ui,
                        &format!("{} Pause downloads", ui_icons::CANCEL_TO_READY),
                        self.status_queued > 0 || self.status_active > 0,
                    )
                    .clicked()
                    {
                        self.pause_all_downloads();
                    }
                    if secondary_button(
                        ui,
                        &format!("{} Export URLs", ui_icons::IMPORT_FILE),
                        !self.items.is_empty(),
                    )
                    .clicked()
                    {
                        self.export_queue_to_file();
                    }
                    if !self.selected_item_ids.is_empty() {
                        if danger_button(
                            ui,
                            &format!(
                                "{} Remove selected ({})",
                                ICON_REMOVE,
                                self.selected_item_ids.len()
                            ),
                            true,
                        )
                        .clicked()
                        {
                            self.remove_selected_items();
                        }
                        if self.status_failed > 0
                            && warning_button(
                                ui,
                                &format!("{} Retry selected", ui_icons::RETRY),
                                true,
                            )
                            .clicked()
                        {
                            self.retry_selected_failed();
                        }
                    }
                    if danger_button(ui, &format!("{ICON_CLEAR} Clear list"), true).clicked() {
                        self.items.retain(|x| {
                            matches!(x.status, ItemStatus::Queued | ItemStatus::Downloading)
                        });
                        self.pending_resolve_ids
                            .retain(|_, iid| self.items.iter().any(|x| x.item_id == *iid));
                        self.update_status();
                        self.refresh_input_line_info();
                        self.schedule_queue_save();
                    }
                    if self.status_failed > 0
                        && warning_button(
                            ui,
                            &format!("{} Retry all failed", ui_icons::RETRY),
                            true,
                        )
                        .on_hover_text(
                            "Retry every failed download that still has a URL (same as each card's Retry download).",
                        )
                        .clicked()
                    {
                        self.retry_failed_items();
                    }
                    if (self.status_queued > 0 || self.status_active > 0)
                        && warning_button(
                            ui,
                            &format!("{} Cancel all -> Ready", ui_icons::CANCEL_TO_READY),
                            true,
                        )
                        .clicked()
                    {
                        self.cancel_all_active(CancelPostAction::Ready);
                    }
                    if (self.status_queued > 0 || self.status_active > 0)
                        && danger_button(
                            ui,
                            &format!("{} Cancel all -> Remove", ui_icons::CANCEL_TO_REMOVE),
                            true,
                        )
                        .clicked()
                    {
                        self.cancel_all_active(CancelPostAction::Remove);
                    }
                    let recheck = ui
                        .add_enabled(
                            self.has_ffprobe && !self.settings.ffmpeg_extract_audio_mp3,
                            egui::Button::new(
                                RichText::new(format!(
                                    "{} Re-check saved files",
                                    ui_icons::RECHECK
                                ))
                                .color(Color32::from_rgb(40, 24, 0)),
                            )
                            .fill(Color32::from_rgb(255, 167, 38))
                            .stroke(egui::Stroke::new(
                                1.0,
                                Color32::from_rgb(214, 120, 20),
                            )),
                        )
                        .on_hover_text(
                            "Run ffprobe on each finished download on disk; mark rows failed if video or audio is missing.",
                        )
                        .on_disabled_hover_text(
                            "Requires ffprobe. Disabled while MP3 extraction is enabled.",
                        );
                    if recheck.clicked() {
                        self.recheck_all_saved_downloads();
                    }
                });
                if has_idle_items
                    && success_button(ui, &format!("{ICON_DOWNLOAD} Start downloads"), true)
                        .clicked()
                {
                    self.start_downloads();
                }
                if trigger_add && !self.add_in_progress {
                    self.add_urls(ctx.input(|i| i.time));
                }
                if trigger_download && has_idle_items {
                    self.start_downloads();
                }

                // Dedicated scroll region with a finite height so the card grid always scrolls.
                // Activity log lives in a separate window ("Logs" button).
                const RESERVE_BOTTOM_PX: f32 = 20.0;
                let min_card_viewport = if self.settings.compact_cards {
                    260.0
                } else {
                    335.0
                };
                let video_scroll_h =
                    (ui.available_height() - RESERVE_BOTTOM_PX).max(min_card_viewport);

                egui::Frame::dark_canvas(ui.style())
                    .fill(BG_CANVAS)
                    .stroke(egui::Stroke::new(1.0, BORDER_PANEL))
                    .inner_margin(egui::Margin::same(10.0))
                    .rounding(egui::Rounding::same(8.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Videos").strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if secondary_button(
                                    ui,
                                    if self.settings.logs_docked {
                                        "Undock log"
                                    } else {
                                        "Dock log"
                                    },
                                    true,
                                )
                                .clicked()
                                {
                                    self.settings.logs_docked = !self.settings.logs_docked;
                                    self.persist_settings();
                                }
                            });
                        });
                        let dock_log =
                            self.settings.logs_open && self.settings.logs_docked;
                        let log_h = if dock_log {
                            self.settings.log_dock_height.clamp(80.0, 480.0)
                        } else {
                            0.0
                        };
                        let cards_h = if dock_log {
                            (video_scroll_h - log_h - 12.0).max(120.0)
                        } else {
                            video_scroll_h
                        };
                        egui::ScrollArea::vertical()
                            .id_salt("rustdl_videos_scroll")
                            .auto_shrink([false, false])
                            .max_height(cards_h)
                            .animated(true)
                            .drag_to_scroll(true)
                            .show(ui, |ui| {
                                if self.items.is_empty() {
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(32.0);
                                        ui.label(
                                            RichText::new("Nothing here yet").color(TEXT_MUTED),
                                        );
                                        ui.label(
                                            RichText::new(
                                                "Paste URL(s) and click Add URLs to fetch previews.",
                                            )
                                            .small(),
                                        );
                                    });
                                } else {
                                    self.draw_grouped_cards(ui);
                                }
                            });
                        if dock_log {
                            ui.add_space(6.0);
                            ui.label(RichText::new("Activity log").small().strong());
                            ui.add(
                                egui::Slider::new(&mut self.settings.log_dock_height, 80.0..=480.0)
                                    .text("Log height"),
                            );
                            self.draw_activity_log_panel(ui);
                        }
                    });
        });

        let was_settings_open = self.settings_open;
        self.draw_settings_window(ctx);
        self.draw_about_window(ctx);
        if self.settings.logs_open && !self.settings.logs_docked {
            self.draw_logs_window(ctx);
        }
        self.maybe_notify_session_complete();
        if was_settings_open && !self.settings_open && self.settings_dirty {
            self.settings.worker_count = self.worker_count.clamp(1, 6);
            self.settings.output_dir = self.output_dir.clone();
            self.persist_settings();
            trim_activity_log(&mut self.log_lines, self.settings.log_max_chars);
            self.flush_log_to_disk();
            self.refresh_deps();
            self.settings_dirty = false;
        }

        self.input_urls_snapshot = self.input_urls.clone();
        self.draw_exit_confirm_dialog(ctx);
        self.request_repaint_if_background_busy(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_queue_to_disk();
        self.flush_log_to_disk();
        let _ = save_settings(&self.settings);
    }
}
