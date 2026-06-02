use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
#[cfg(windows)]
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use eframe::egui::{Color32, RichText, TextureHandle};
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;

mod about;
mod av1_panel;
pub(crate) mod background_spawn;
pub(crate) mod core_sync;
mod cards;
pub(crate) mod done_file_index;
mod download_control;
mod eframe_app;
pub(crate) mod events;
mod input_lines;
pub(crate) mod log_panel;
mod queue_cache;
mod queue_persist;
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
    human_bytes_ui, normalize_restored_av1_item, normalize_restored_item, parse_urls_from_text_blob,
};
use crate::app_state::{StatusCounts, TransferTotals};
use crate::app_ui::{
    alert_danger, alert_warning, centered_button_row, content_panel_frame, danger_button,
    draw_status_dot, with_full_width, HEADER_RIGHT_INSET,
    modal_backdrop, secondary_button, status_color, success_button, warning_button,
    ALERT_DANGER_TEXT, ALERT_WARNING_TEXT,
};
use crate::config::{
    default_downloads, export_queue_urls, load_activity_log, load_av1_queue_snapshot,
    load_settings, rustdl_config_dir, save_settings, trim_activity_log,
    AppSettings, Av1QueueSnapshot,
};
use crate::models::Av1QueueItem;
use crate::models::{ItemStatus, QueueItem};
use crate::pkg_version;
use crate::profiles::{find_profile, load_profiles, DownloadProfile, ProfileStore};
use crate::theme::{self, BG_CANVAS, BG_LOG, BORDER_PANEL, TEXT_MUTED};
use crate::ui_icons;
use crate::ytdlp;
use crate::ytdlp_download_args::{
    build_download_extra_args, metadata_extra_args, output_filename_template,
};

const INPUT_SUMMARY_HOLD_SECS: f64 = 2.5;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsTab {
    Shared,
    Downloader,
    Av1,
}

pub(super) fn settings_tab_from_str(s: &str) -> SettingsTab {
    match s.trim().to_ascii_lowercase().as_str() {
        "downloader" => SettingsTab::Downloader,
        "av1" => SettingsTab::Av1,
        _ => SettingsTab::Shared,
    }
}

pub(super) fn settings_tab_to_str(tab: SettingsTab) -> &'static str {
    match tab {
        SettingsTab::Shared => "shared",
        SettingsTab::Downloader => "downloader",
        SettingsTab::Av1 => "av1",
    }
}

#[derive(Clone, Copy)]
enum DownloadPreset {
    BestQuality,
    AudioOnly,
    FastDownload,
    ArchiveMode,
}

pub(crate) use crate::service::CancelPostAction;

pub struct PydlApp {
    pub(crate) shared_core: crate::service::SharedCore,
    pub(crate) core_generation: u64,
    pub(crate) web_server: Option<crate::service::web::WebServerHandle>,
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
    status_counts: StatusCounts,
    item_index_by_id: HashMap<u64, usize>,
    cached_dedupe_keys: HashSet<String>,
    cached_transfer_totals: TransferTotals,
    transfer_totals_dirty: bool,
    last_transfer_totals_at: Option<Instant>,
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
    profile_store: ProfileStore,
    applied_theme: String,
    new_profile_name_buffer: Option<String>,

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
    /// User confirmed quit while work was active; wait for graceful cancellation.
    exit_pending_after_cancel: bool,
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
    av1_mode: bool,
    av1_input_paths: String,
    av1_items: Vec<Av1QueueItem>,
    av1_duration_ms: HashMap<u64, u64>,
    av1_progress_state: HashMap<u64, HashMap<String, String>>,
    av1_media_inflight: HashSet<u64>,
    av1_next_item_id: u64,
    av1_running: bool,
    av1_cancel_flag: Arc<AtomicBool>,
    av1_encoder_choice: Option<crate::av1_transcode::EncoderChoice>,
    av1_encoder_detect_key: String,

    done_file_index: done_file_index::DoneFileIndex,
    /// Suppress repeat log spam when output index hits [`DONE_LOOKUP_MAX_ENTRIES`].
    done_lookup_truncation_logged: bool,

    http_client: reqwest::Client,
    thumb_semaphore: Arc<Semaphore>,
    /// When set, queue JSON is written after this instant (debounced).
    queue_save_deadline: Option<Instant>,
    /// When set, activity log JSON is written after this instant (debounced).
    log_save_deadline: Option<Instant>,
    /// When set, AV1 queue JSON is written after this instant (debounced).
    av1_queue_save_deadline: Option<Instant>,

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
    win_browser_drop_queue: Arc<Mutex<Vec<crate::win_drop_target::WinDropPayload>>>,
    #[cfg(windows)]
    win_browser_drop_target_installed: bool,
    #[cfg(windows)]
    win_browser_drop_target_setup_attempted: bool,
}

impl PydlApp {
    pub fn new(cc: &eframe::CreationContext<'_>, runtime: Arc<Runtime>) -> Self {
        let logo = app_icon::load_logo_texture(&cc.egui_ctx);
        let (rustdl_service, rx) = crate::service::RustdlService::new(runtime.clone());
        let shared_core = rustdl_service.shared_core();
        let tx = shared_core.lock().tx.clone();
        let mut settings = load_settings();
        if settings.web_ui_enabled && settings.web_auth_token.trim().is_empty() {
            settings.web_auth_token = crate::config::generate_web_auth_token();
            let _ = save_settings(&settings);
        }
        theme::apply_ui_theme(&cc.egui_ctx, &settings.theme);
        let profile_store = load_profiles();
        let log_lines = load_activity_log(settings.log_max_chars);
        let mut restored_items = shared_core.lock().items.clone();
        for it in &mut restored_items {
            normalize_restored_item(it);
        }
        let av1_snapshot = if settings.av1_remember_queue {
            load_av1_queue_snapshot()
        } else {
            Av1QueueSnapshot::default()
        };
        let mut restored_av1_items = av1_snapshot.items;
        for it in &mut restored_av1_items {
            normalize_restored_av1_item(it);
        }
        let av1_next_item_id = if restored_av1_items.is_empty() {
            1_000_000
        } else {
            av1_snapshot.next_item_id.max(
                restored_av1_items
                    .iter()
                    .map(|x| x.item_id)
                    .max()
                    .unwrap_or(999_999)
                    + 1,
            )
        };
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
        let av1_mode = settings.last_mode == "av1";
        let settings_tab = settings_tab_from_str(&settings.settings_tab);
        let applied_theme = settings.theme.clone();

        let web_server = crate::service::web::spawn_web_server(
            runtime.clone(),
            shared_core.clone(),
            &settings,
        );

        let mut app = Self {
            shared_core: shared_core.clone(),
            core_generation: shared_core.lock().generation,
            web_server,
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
            status_counts: StatusCounts::default(),
            item_index_by_id: HashMap::new(),
            cached_dedupe_keys: HashSet::new(),
            cached_transfer_totals: TransferTotals::default(),
            transfer_totals_dirty: true,
            last_transfer_totals_at: None,
            has_yt_dlp: false,
            has_ffmpeg: false,
            has_ffprobe: false,
            yt_dlp_version: String::new(),
            ffmpeg_version: String::new(),
            ffprobe_version: String::new(),
            log_lines,
            settings,
            settings_open: false,
            profile_store,
            applied_theme,
            new_profile_name_buffer: None,
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
            settings_tab,
            restored_items_count: 0,
            show_restore_banner: false,
            about_open: false,
            exit_confirm_open: false,
            exit_allowed: false,
            exit_pending_after_cancel: false,
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
            av1_mode,
            av1_input_paths: av1_snapshot.input_paths,
            av1_items: restored_av1_items,
            av1_duration_ms: HashMap::new(),
            av1_progress_state: HashMap::new(),
            av1_media_inflight: HashSet::new(),
            av1_next_item_id,
            av1_running: false,
            av1_cancel_flag: Arc::new(AtomicBool::new(false)),
            av1_encoder_choice: None,
            av1_encoder_detect_key: String::new(),
            done_file_index: done_file_index::DoneFileIndex::new(),
            done_lookup_truncation_logged: false,
            http_client,
            thumb_semaphore,
            queue_save_deadline: None,
            log_save_deadline: None,
            av1_queue_save_deadline: None,
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
        if app.settings.av1_remember_queue && !app.av1_items.is_empty() {
            app.append_log(&format!(
                "AV1: restored {} item(s) from previous session.",
                app.av1_items.len()
            ));
        }
        app.refresh_deps();
        core_sync::push_app_to_core(&app, &app.shared_core);
        if app.settings.web_ui_enabled {
            app.restart_web_server();
        }
        app.queue_av1_restored_assets();
        app.recompute_status();
        app.invalidate_queue_caches();
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
            || self.av1_running
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

    #[allow(dead_code)]
    pub fn apply_ui_smoothness(ctx: &egui::Context) {
        theme::apply_ui_theme(ctx, "dark");
    }

    pub(super) fn reorder_ready_items(&mut self, dragged_id: u64, target_id: u64) {
        if dragged_id == target_id {
            return;
        }
        let Some(dragged_order) = self
            .items
            .iter()
            .find(|it| it.item_id == dragged_id)
            .map(|it| {
                if it.sort_order == 0 {
                    it.item_id
                } else {
                    it.sort_order
                }
            })
        else {
            return;
        };
        let Some(target_order) = self
            .items
            .iter()
            .find(|it| it.item_id == target_id)
            .map(|it| {
                if it.sort_order == 0 {
                    it.item_id
                } else {
                    it.sort_order
                }
            })
        else {
            return;
        };
        if dragged_order < target_order {
            for it in &mut self.items {
                if it.status != ItemStatus::Idle {
                    continue;
                }
                let order = if it.sort_order == 0 {
                    it.item_id
                } else {
                    it.sort_order
                };
                if it.item_id == dragged_id {
                    it.sort_order = target_order;
                } else if order > dragged_order && order <= target_order {
                    it.sort_order = order.saturating_sub(1);
                }
            }
        } else {
            for it in &mut self.items {
                if it.status != ItemStatus::Idle {
                    continue;
                }
                let order = if it.sort_order == 0 {
                    it.item_id
                } else {
                    it.sort_order
                };
                if it.item_id == dragged_id {
                    it.sort_order = target_order;
                } else if order >= target_order && order < dragged_order {
                    it.sort_order = order.saturating_add(1);
                }
            }
        }
        self.schedule_queue_save();
    }

    pub(super) fn sync_theme_if_needed(&mut self, ctx: &egui::Context) {
        if self.applied_theme != self.settings.theme {
            theme::apply_ui_theme(ctx, &self.settings.theme);
            self.applied_theme = self.settings.theme.clone();
        }
    }

    pub(super) fn set_app_mode(&mut self, av1: bool) {
        self.av1_mode = av1;
        self.settings.last_mode = if av1 {
            "av1".to_owned()
        } else {
            "downloader".to_owned()
        };
        self.persist_settings();
    }

    pub(super) fn apply_download_profile(&mut self, profile: &DownloadProfile) {
        profile.apply_to(&mut self.settings);
        self.persist_settings();
        self.append_log(&format!("Applied profile: {}", profile.name));
    }

    pub(super) fn open_config_folder(&mut self) {
        let dir = rustdl_config_dir();
        if let Err(e) = app_actions::open_path(&dir) {
            self.append_log(&format!("Failed to open config folder: {e}"));
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

    pub(super) fn refresh_deps(&mut self) {
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
        self.refresh_av1_encoder_detection();
    }

    pub(super) fn append_log(&mut self, message: &str) {
        let line = crate::time_format::format_log_line(message);
        self.log_lines.push_back(line);
        trim_activity_log(&mut self.log_lines, self.settings.log_max_chars);
        self.schedule_log_save();
    }

    fn poll_done_file_lookup(&mut self) {
        if !self.should_poll_done_lookup() {
            return;
        }
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
        self.settings.settings_tab = settings_tab_to_str(self.settings_tab).to_owned();
        if let Err(err) = save_settings(&self.settings) {
            self.append_log(&format!("Failed to save settings: {err}"));
        }
        if self.settings.av1_remember_queue {
            self.flush_av1_queue_to_disk();
        } else {
            self.clear_av1_queue_persistence();
        }
    }

    pub(super) fn restart_web_server(&mut self) {
        if let Some(mut handle) = self.web_server.take() {
            handle.stop();
        }
        self.web_server = crate::service::web::spawn_web_server(
            self.runtime.clone(),
            self.shared_core.clone(),
            &self.settings,
        );
        if self.settings.web_ui_enabled {
            if self.web_server.is_some() {
                self.append_log(&format!(
                    "Web UI enabled on http://{} (token required)",
                    self.settings.web_bind_address.trim()
                ));
            }
        } else {
            self.append_log("Web UI disabled.");
        }
    }

    fn metadata_extra_args(&self) -> Vec<String> {
        metadata_extra_args(&self.settings)
    }

    pub(super) fn download_extra_args(&self) -> Vec<String> {
        build_download_extra_args(&self.settings)
    }

    pub(super) fn yt_dlp_bin(&self) -> String {
        if self.settings.yt_dlp_path.trim().is_empty() {
            "yt-dlp".to_owned()
        } else {
            self.settings.yt_dlp_path.trim().to_owned()
        }
    }

    pub(super) fn ffmpeg_bin(&self) -> String {
        if self.settings.ffmpeg_path.trim().is_empty() {
            String::new()
        } else {
            self.settings.ffmpeg_path.trim().to_owned()
        }
    }

    fn effective_download_command_preview(&self) -> String {
        let mut parts = vec![self.yt_dlp_bin(), "--newline".to_owned()];
        parts.push("-o".to_owned());
        parts.push(format!(
            "{}/{}",
            self.output_dir,
            output_filename_template(&self.settings)
        ));
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

    pub(super) fn refresh_done_file_lookup(&mut self) {
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

    pub(super) fn find_downloaded_file_for_item(
        &self,
        item: &QueueItem,
    ) -> Option<(PathBuf, std::time::SystemTime)> {
        self.done_file_index.find_path_for_queue_item(item)
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

    pub(super) fn reveal_file_path(&mut self, file_path: &Path) {
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

    pub(super) fn open_item_output_folder(&mut self, item_id: u64) {
        let Some(item) = self.items.iter().find(|x| x.item_id == item_id) else {
            return;
        };
        if let Some((path, _)) = self.find_downloaded_file_for_item(item) {
            if let Some(parent) = path.parent() {
                if let Err(e) = app_actions::open_path(parent) {
                    self.append_log(&format!("Failed to open output folder: {e}"));
                }
                return;
            }
        }
        let dir = Path::new(&self.output_dir);
        if let Err(e) = app_actions::open_path(dir) {
            self.append_log(&format!("Failed to open output folder: {e}"));
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
                if let Some(p) = find_profile(&self.profile_store, "Best quality") {
                    self.apply_download_profile(&p);
                    return;
                }
                self.settings.ffmpeg_extract_audio_mp3 = false;
                self.settings.ffmpeg_remux_mp4 = false;
                self.settings.ffmpeg_faststart = true;
                self.settings.yt_ignore_errors = false;
                self.settings.yt_write_info_json = false;
                self.settings.yt_dlp_extra_args = "--merge-output-format mp4".to_owned();
            }
            DownloadPreset::AudioOnly => {
                if let Some(p) = find_profile(&self.profile_store, "Audio only") {
                    self.apply_download_profile(&p);
                    return;
                }
                self.settings.ffmpeg_extract_audio_mp3 = true;
                self.settings.ffmpeg_remux_mp4 = false;
                self.settings.ffmpeg_faststart = false;
                self.settings.yt_dlp_extra_args = String::new();
            }
            DownloadPreset::FastDownload => {
                if let Some(p) = find_profile(&self.profile_store, "Fast download") {
                    self.apply_download_profile(&p);
                    return;
                }
                self.settings.ffmpeg_extract_audio_mp3 = false;
                self.settings.ffmpeg_remux_mp4 = false;
                self.settings.ffmpeg_faststart = false;
                self.settings.yt_ignore_errors = true;
                self.settings.yt_dlp_extra_args = "--concurrent-fragments 4".to_owned();
            }
            DownloadPreset::ArchiveMode => {
                if let Some(p) = find_profile(&self.profile_store, "Archive mode") {
                    self.apply_download_profile(&p);
                    return;
                }
                self.settings.yt_write_info_json = true;
                self.settings.yt_embed_metadata = true;
                self.settings.yt_write_auto_subs = true;
                self.settings.yt_dlp_extra_args = "--write-description".to_owned();
            }
        }
        self.settings.active_profile = match preset {
            DownloadPreset::BestQuality => "Best quality",
            DownloadPreset::AudioOnly => "Audio only",
            DownloadPreset::FastDownload => "Fast download",
            DownloadPreset::ArchiveMode => "Archive mode",
        }
        .to_owned();
        self.persist_settings();
    }

    fn probe_done_item_resolution_if_missing(&mut self, item_id: u64) {
        let Some(idx) = self.item_idx(item_id) else {
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

    pub(super) fn refresh_input_line_info(&mut self) {
        let lines: Vec<String> = self
            .input_urls
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        self.input_line_info =
            input_lines::analyze_input_lines(&lines, self.dedupe_keys());
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
        let shared = self.shared_core.clone();
        {
            let mut core = shared.lock();
            core_sync::sync_app_to_core(self, &mut core);
            core.queue_urls_for_resolve(lines);
        }
        {
            let core = shared.lock();
            core_sync::sync_core_to_app(&core, self);
        }
        self.refresh_input_line_info();
    }

    fn add_urls(&mut self, now: f64) {
        let lines = self.collect_valid_new_lines();
        self.queue_urls_for_resolve(lines);
        self.clear_input_urls_with_summary_hold(now);
    }

    pub(super) fn import_queue_from_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import queue URLs")
            .add_filter("Text", &["txt"])
            .pick_file()
        else {
            return;
        };
        let content = match fs::read_to_string(&path) {
            Ok(x) => x,
            Err(e) => {
                self.append_log(&format!(
                    "Failed to read queue file {}: {e}",
                    path.to_string_lossy()
                ));
                return;
            }
        };
        let parsed = parse_urls_from_text_blob(&content);
        if parsed.is_empty() {
            self.append_log("Queue import file had no URL entries.");
            return;
        }
        self.append_log(&format!(
            "Importing {} URL(s) from {} into the queue.",
            parsed.len(),
            path.to_string_lossy()
        ));
        self.queue_urls_for_resolve(parsed);
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

    /// Re-run yt-dlp metadata for this row (same URL), replacing the card when resolve completes.
    pub(super) fn retry_metadata_item_id(&mut self, item_id: u64) {
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
        let Some(idx) = self.item_idx(item_id) else {
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
            self.settings.playlist_preview_cap,
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
        let Some(idx) = self.item_idx(item_id) else {
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
            let Some(idx) = self.item_idx(item_id) else {
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
        let idx = self.item_idx(item_id)?;
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
        if taken.is_empty() {
            return;
        }
        if self.av1_mode {
            let paths: Vec<String> = taken
                .into_iter()
                .filter_map(|item| match item {
                    crate::win_drop_target::WinDropPayload::Path(p) => {
                        Some(p.to_string_lossy().to_string())
                    }
                    crate::win_drop_target::WinDropPayload::Url(_) => None,
                })
                .collect();
            if !paths.is_empty() {
                self.extend_av1_input_paths_with_lines(paths);
            }
            return;
        }
        let mut urls = Vec::new();
        for item in taken {
            match item {
                crate::win_drop_target::WinDropPayload::Url(u) => urls.push(u),
                crate::win_drop_target::WinDropPayload::Path(p) => {
                    if let Some(mut from_path) = crate::app_parsing::urls_from_dropped_os_path(&p) {
                        urls.append(&mut from_path);
                    }
                }
            }
        }
        if !urls.is_empty() {
            self.merge_dragged_urls_into_input(urls, ctx);
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

    fn extend_av1_input_paths_with_lines(&mut self, lines: Vec<String>) {
        let lines: Vec<String> = lines
            .into_iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if lines.is_empty() {
            return;
        }
        self.scan_av1_paths_into_queue(&lines);
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

    fn apply_dropped_av1_paths(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let mut paths = Vec::new();
        for df in dropped {
            if let Some(path) = df.path {
                paths.push(path.to_string_lossy().to_string());
            }
        }
        if !paths.is_empty() {
            self.extend_av1_input_paths_with_lines(paths);
        }
    }

    fn exit_work_in_progress(&self) -> bool {
        self.add_in_progress
            || self.status_resolving > 0
            || self.status_queued > 0
            || self.status_active > 0
            || self.queue_running > 0
            || self.av1_running
    }

    fn open_exit_confirm(&mut self) {
        self.exit_confirm_open = true;
    }

    fn confirm_exit(&mut self, ctx: &egui::Context) {
        self.exit_confirm_open = false;
        if self.exit_work_in_progress() {
            self.exit_pending_after_cancel = true;
            self.av1_cancel_flag
                .store(true, std::sync::atomic::Ordering::Relaxed);
            self.cancel_all_active(CancelPostAction::Ready);
            self.append_log("Graceful shutdown requested: cancelling active jobs before exit...");
            return;
        }
        self.exit_allowed = true;
        self.flush_queue_to_disk();
        self.flush_av1_queue_to_disk();
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
        if modal_backdrop(ctx, egui::Id::new("exit_confirm_backdrop")) {
            self.exit_confirm_open = false;
            return;
        }
        let mut exit_confirm_open = self.exit_confirm_open;
        let mut cancel_exit_confirm = false;
        let work_active = self.exit_work_in_progress();
        let mut modal_frame = egui::Frame::window(&ctx.style());
        modal_frame.fill = BG_LOG;
        modal_frame.stroke = egui::Stroke::new(1.0, BORDER_PANEL);
        modal_frame.inner_margin = egui::Margin::same(20.0);
        modal_frame.rounding = egui::Rounding::same(8.0);
        egui::Window::new("Quit rustdl?")
            .open(&mut exit_confirm_open)
            .frame(modal_frame)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.set_max_width(360.0);
                    let draw_alert_body = |ui: &mut egui::Ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(ui_icons::EXIT)
                                    .size(40.0)
                                    .color(if work_active {
                                        ALERT_WARNING_TEXT
                                    } else {
                                        ALERT_DANGER_TEXT
                                    }),
                            );
                            ui.add_space(12.0);
                            if work_active {
                                ui.label(
                                    RichText::new("Downloads or metadata fetches are still running.")
                                        .strong()
                                        .color(ALERT_WARNING_TEXT),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
                                        "Your queue will be saved. Confirm quit to request graceful cancellation of active downloader/AV1 jobs before closing.",
                                    )
                                    .color(ALERT_WARNING_TEXT),
                                );
                            } else {
                                ui.label(
                                    RichText::new("Are you sure you want to close rustdl?")
                                        .strong()
                                        .color(ALERT_DANGER_TEXT),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("Your download queue will be saved.")
                                        .color(ALERT_DANGER_TEXT),
                                );
                            }
                        });
                    };
                    if work_active {
                        alert_warning(ui, draw_alert_body);
                    } else {
                        alert_danger(ui, draw_alert_body);
                    }
                });
                ui.add_space(16.0);
                centered_button_row(ui, "exit_confirm", |ui| {
                    if secondary_button(
                        ui,
                        &format!("{} Cancel", ui_icons::DISMISS),
                        true,
                    )
                    .clicked()
                    {
                        cancel_exit_confirm = true;
                    }
                    ui.add_space(12.0);
                    if danger_button(ui, &format!("{} Quit", ui_icons::EXIT), true).clicked()
                    {
                        self.confirm_exit(ctx);
                    }
                });
            });
        if cancel_exit_confirm {
            exit_confirm_open = false;
        }
        self.exit_confirm_open = exit_confirm_open;
    }
}
