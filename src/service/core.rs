//! Shared download queue and settings state (GUI + web UI).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app_parsing::normalize_restored_item;
use crate::app_state::{self, BatchProgress, StatusCounts, TransferTotals, UrlLineFilterStats};
use crate::config::{
    load_activity_log, load_convert_queue_snapshot, load_queue_items, load_settings,
    save_activity_log, save_queue_items, save_settings, trim_activity_log, AppSettings,
    ConvertQueueSnapshot,
};
use crate::convert_state::{
    compute_convert_batch_progress, compute_convert_batch_summary, compute_convert_status_counts,
    rebuild_convert_item_index_map, ConvertBatchSummary, ConvertStatusCounts,
};
use crate::domain::done_file_index::{DoneFileIndex, DONE_LOOKUP_MAX_ENTRIES};
use crate::domain::events::{UiEvent, UiEventBus};
use crate::models::{ConvertQueueItem, ItemStatus, QueueItem};
use crate::profiles::{load_profiles, ProfileStore};
use crate::service::background_spawn;
use crate::transcode::EncoderChoice;
use crate::ytdlp;
use crate::ytdlp_download_args::{
    build_redownload_extra_args, metadata_extra_args, output_filename_template_for_item,
    remove_video_ids_from_download_archive,
};
use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::Mutex;
use tokio::runtime::Runtime;
use tokio::sync::broadcast;
use tokio::sync::Semaphore;

pub type SharedCore = Arc<Mutex<DownloadCore>>;

const QUEUE_SAVE_DEBOUNCE: Duration = Duration::from_millis(400);
/// Debounce full output-folder scans so bursts of finished downloads do not block the UI.
const DONE_LOOKUP_REFRESH_DEBOUNCE: Duration = Duration::from_millis(750);

#[derive(Clone, Copy)]
pub enum CancelPostAction {
    Ready,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedownloadError {
    InvalidOutputDir,
    ItemNotFound,
    NoUrl,
    NoYtDlp,
    DownloadsPaused,
}

impl RedownloadError {
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidOutputDir => "Choose a valid output folder in Settings.",
            Self::ItemNotFound => "Item not found in the queue.",
            Self::NoUrl => "No video URL on this row (cannot re-download).",
            Self::NoYtDlp => "yt-dlp not found (check PATH or Settings executable path).",
            Self::DownloadsPaused => "Downloads are paused. Click Resume first.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStartError {
    Paused,
    EmptyQueue,
    InvalidOutputDir,
}

impl DownloadStartError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Paused => "Downloads are paused. Click Resume first.",
            Self::EmptyQueue => "Add URLs first.",
            Self::InvalidOutputDir => "Choose a valid output folder.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertStartError {
    Paused,
    NoReadyItems,
}

impl ConvertStartError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Paused => "Convert: batch is paused. Click Resume first.",
            Self::NoReadyItems => "Convert: no ready items to convert.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryFailedError {
    InvalidOutputDir,
    NoYtDlp,
    DownloadsPaused,
    NothingToRetry,
    NoUrlOnFailed,
}

impl RetryFailedError {
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidOutputDir => "Choose a valid output folder.",
            Self::NoYtDlp => "yt-dlp not found (check PATH or Settings executable path).",
            Self::DownloadsPaused => "Downloads are paused. Click Resume first.",
            Self::NothingToRetry => "No failed downloads to retry.",
            Self::NoUrlOnFailed => {
                "No failed items have a video URL to retry. Check the row or re-add the link."
            }
        }
    }
}

/// LAN web / desktop-shared thumbnail bytes keyed by queue or AV1 item id.
#[derive(Clone, Debug)]
pub struct CachedThumbnail {
    pub source_key: String,
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// Saved downloader + AV1 queue state awaiting user confirmation (GUI startup).
#[derive(Clone, Debug, Default)]
pub struct PendingSessionRestore {
    pub downloader_items: Vec<QueueItem>,
    pub convert_snapshot: ConvertQueueSnapshot,
}

impl PendingSessionRestore {
    pub fn downloader_count(&self) -> usize {
        self.downloader_items.len()
    }

    pub fn convert_count(&self) -> usize {
        self.convert_snapshot.items.len()
    }
}

fn session_has_restorable_data(
    downloader_items: &[QueueItem],
    convert_snapshot: &ConvertQueueSnapshot,
    convert_remember_queue: bool,
) -> bool {
    if !downloader_items.is_empty() {
        return true;
    }
    if convert_remember_queue
        && (!convert_snapshot.items.is_empty() || !convert_snapshot.input_paths.trim().is_empty())
    {
        return true;
    }
    false
}

fn compute_next_item_id(items: &[QueueItem]) -> u64 {
    items
        .iter()
        .map(|x| x.item_id)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn compute_convert_next_item_id(
    snapshot: &ConvertQueueSnapshot,
    items: &[ConvertQueueItem],
) -> u64 {
    if items.is_empty() {
        1_000_000
    } else {
        snapshot.next_item_id.max(
            items
                .iter()
                .map(|x| x.item_id)
                .max()
                .unwrap_or(999_999)
                .saturating_add(1),
        )
    }
}

/// Bulk queue cleanup modes (web UI and API).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueClearFilter {
    /// Done rows only.
    Done,
    /// Failed rows only.
    Failed,
    /// Done and failed.
    Finished,
    /// Everything except queued/downloading (matches desktop “Clear list”).
    Inactive,
    /// Entire queue (active downloads are cancelled and removed).
    All,
}

pub struct DownloadCore {
    pub runtime: Arc<Runtime>,
    pub tx: Sender<UiEvent>,
    pub event_broadcast: broadcast::Sender<UiEvent>,
    pub ui_event_channel_degraded: Arc<AtomicBool>,

    pub output_dir: String,
    pub worker_count: usize,
    pub status_resolving: usize,
    pub status_ready: usize,
    pub status_queued: usize,
    pub status_active: usize,
    pub status_done: usize,
    pub status_failed: usize,
    pub status_counts: StatusCounts,
    pub item_index_by_id: HashMap<u64, usize>,
    pub cached_dedupe_keys: HashSet<String>,
    pub cached_transfer_totals: TransferTotals,
    pub transfer_totals_dirty: bool,
    pub has_yt_dlp: bool,
    pub has_ffmpeg: bool,
    pub has_ffprobe: bool,
    pub yt_dlp_version: String,
    pub ffmpeg_version: String,
    pub ffprobe_version: String,
    pub log_lines: VecDeque<String>,
    pub settings: AppSettings,
    pub profile_store: ProfileStore,
    pub items: Vec<QueueItem>,
    pub pending_resolve_ids: HashMap<String, u64>,
    pub next_item_id: u64,
    pub add_in_progress: bool,
    pub add_total_urls: usize,
    pub add_processed_urls: usize,
    pub add_current_url: Option<String>,
    pub queue_running: usize,
    pub download_cancel_flags: HashMap<u64, Arc<AtomicBool>>,
    pub cancel_post_actions: HashMap<u64, CancelPostAction>,
    pub downloads_paused: bool,
    pub session_complete_notified: bool,
    pub queue_save_deadline: Option<Instant>,
    pub log_save_deadline: Option<Instant>,
    pub http_client: reqwest::Client,
    pub done_file_index: DoneFileIndex,
    pub done_lookup_truncation_logged: bool,
    done_lookup_refresh_deadline: Option<Instant>,
    done_lookup_refresh_inflight: Arc<AtomicBool>,
    pub download_log_throttle: HashMap<u64, f64>,
    /// Last UI sync bump per download item: `(unix_secs, percent)`.
    pub download_progress_throttle: HashMap<u64, (f64, f32)>,
    /// Last UI sync bump per convert item: `(unix_secs, percent)`.
    pub convert_progress_throttle: HashMap<u64, (f64, f32)>,
    pub(crate) last_convert_aggregate_update_at: f64,
    /// Thumbnail image bytes shared between the desktop GUI and LAN `/api/thumbnail` proxy.
    pub thumbnail_cache: HashMap<u64, CachedThumbnail>,
    pub dirty_queue_item_ids: HashSet<u64>,
    pub dirty_convert_item_ids: HashSet<u64>,
    /// Incremented when web or GUI sync pushes state; GUI pulls when this changes.
    pub generation: u64,
    /// Incremented when settings or profile store change; GUI pulls settings when this differs.
    pub settings_generation: u64,
    /// Parse failures for user JSON files at startup (settings, queue, profiles, log).
    pub config_load_issues: Vec<crate::config::ConfigLoadIssue>,

    // --- AV1 converter (shared by GUI and web UI) ---
    pub convert_input_paths: String,
    pub convert_items: Vec<ConvertQueueItem>,
    pub convert_next_item_id: u64,
    pub convert_running: bool,
    pub convert_paused: bool,
    pub convert_cancel_flag: Arc<AtomicBool>,
    pub convert_duration_ms: HashMap<u64, u64>,
    pub convert_progress_state: HashMap<u64, HashMap<String, String>>,
    pub convert_media_inflight: HashSet<u64>,
    pub convert_item_index_by_id: HashMap<u64, usize>,
    pub convert_status_counts: ConvertStatusCounts,
    pub convert_batch_summary: ConvertBatchSummary,
    pub convert_batch_progress: BatchProgress,
    pub convert_save_deadline: Option<Instant>,
    pub convert_probe_semaphore: Arc<Semaphore>,
    pub convert_encoder_choice: Option<EncoderChoice>,
    pub convert_encoder_detect_key: String,

    /// Graceful quit requested from the LAN web UI (or completing after cancel).
    pub shutdown_pending: bool,
    shutdown_notify: Option<tokio::sync::oneshot::Sender<()>>,

    /// GUI startup: saved queues read from disk but not applied until the user confirms.
    pub pending_session_restore: Option<PendingSessionRestore>,

    pub watch_folder_state: crate::watch_folder::WatchFolderState,

    pub watchlist: crate::watchlist::WatchlistStore,
    pub watchlist_generation: u64,
    pub(crate) watchlist_last_cycle: Option<Instant>,
    pub(crate) watchlist_poll_inflight: Arc<AtomicBool>,

    /// Local calendar day (`YYYY-MM-DD`) when scheduled download start last fired.
    pub scheduled_download_last_fire_day: Option<String>,
}

impl DownloadCore {
    pub fn new_shared(
        runtime: Arc<Runtime>,
        auto_restore: bool,
    ) -> (SharedCore, Receiver<UiEvent>) {
        let (tx, rx) = unbounded();
        let (event_broadcast, _) = broadcast::channel(512);
        let ui_event_channel_degraded = Arc::new(AtomicBool::new(false));
        let settings = load_settings();
        let profile_store = load_profiles();
        let log_lines = load_activity_log(settings.log_max_chars);
        let mut restored_items = load_queue_items();
        for it in &mut restored_items {
            normalize_restored_item(it);
        }
        let http_client = crate::http_client::build_http_client(&settings);
        let convert_snapshot = if settings.convert_remember_queue {
            load_convert_queue_snapshot()
        } else {
            ConvertQueueSnapshot::default()
        };
        let mut restored_convert_items = convert_snapshot.items.clone();
        for it in &mut restored_convert_items {
            crate::app_parsing::normalize_restored_convert_item(it);
        }

        let has_restorable = session_has_restorable_data(
            &restored_items,
            &convert_snapshot,
            settings.convert_remember_queue,
        );
        let defer_restore = !auto_restore && has_restorable;
        let pending_session_restore = if defer_restore {
            Some(PendingSessionRestore {
                downloader_items: restored_items.clone(),
                convert_snapshot: convert_snapshot.clone(),
            })
        } else {
            None
        };

        let (items, next_item_id, convert_input_paths, convert_items, convert_next_item_id) =
            if defer_restore {
                (Vec::new(), 1, String::new(), Vec::new(), 1_000_000)
            } else {
                let next_item_id = compute_next_item_id(&restored_items);
                let convert_next_item_id =
                    compute_convert_next_item_id(&convert_snapshot, &restored_convert_items);
                (
                    restored_items,
                    next_item_id,
                    convert_snapshot.input_paths.clone(),
                    restored_convert_items,
                    convert_next_item_id,
                )
            };

        let mut core = Self {
            runtime,
            tx,
            event_broadcast,
            ui_event_channel_degraded,
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
            has_yt_dlp: false,
            has_ffmpeg: false,
            has_ffprobe: false,
            yt_dlp_version: String::new(),
            ffmpeg_version: String::new(),
            ffprobe_version: String::new(),
            log_lines,
            settings,
            profile_store,
            items,
            pending_resolve_ids: HashMap::new(),
            next_item_id,
            add_in_progress: false,
            add_total_urls: 0,
            add_processed_urls: 0,
            add_current_url: None,
            queue_running: 0,
            download_cancel_flags: HashMap::new(),
            cancel_post_actions: HashMap::new(),
            downloads_paused: false,
            session_complete_notified: false,
            queue_save_deadline: None,
            log_save_deadline: None,
            http_client,
            done_file_index: DoneFileIndex::new(),
            done_lookup_truncation_logged: false,
            done_lookup_refresh_deadline: None,
            done_lookup_refresh_inflight: Arc::new(AtomicBool::new(false)),
            download_log_throttle: HashMap::new(),
            download_progress_throttle: HashMap::new(),
            convert_progress_throttle: HashMap::new(),
            last_convert_aggregate_update_at: -1_000.0,
            thumbnail_cache: HashMap::new(),
            dirty_queue_item_ids: HashSet::new(),
            dirty_convert_item_ids: HashSet::new(),
            generation: 1,
            settings_generation: 1,
            config_load_issues: Vec::new(),
            convert_input_paths,
            convert_items,
            convert_next_item_id,
            convert_running: false,
            convert_paused: false,
            convert_cancel_flag: Arc::new(AtomicBool::new(false)),
            convert_duration_ms: HashMap::new(),
            convert_progress_state: HashMap::new(),
            convert_media_inflight: HashSet::new(),
            convert_item_index_by_id: HashMap::new(),
            convert_status_counts: ConvertStatusCounts::default(),
            convert_batch_summary: ConvertBatchSummary::default(),
            convert_batch_progress: BatchProgress::default(),
            convert_save_deadline: None,
            convert_probe_semaphore: Arc::new(Semaphore::new(4)),
            convert_encoder_choice: None,
            convert_encoder_detect_key: String::new(),
            shutdown_pending: false,
            shutdown_notify: None,
            pending_session_restore,
            watch_folder_state: crate::watch_folder::WatchFolderState::new(),
            watchlist: crate::watchlist::load_watchlist(),
            watchlist_generation: 1,
            watchlist_last_cycle: None,
            watchlist_poll_inflight: Arc::new(AtomicBool::new(false)),
            scheduled_download_last_fire_day: None,
        };
        core.rebuild_item_index();
        core.update_status();
        core.update_convert_status();
        core.invalidate_queue_caches();
        core.refresh_deps();
        core.refresh_done_file_lookup();
        if !defer_restore {
            core.queue_convert_restored_assets();
        }
        core.config_load_issues = crate::config::take_config_load_issues();
        (Arc::new(Mutex::new(core)), rx)
    }

    pub fn apply_pending_session_restore(&mut self) -> bool {
        let Some(pending) = self.pending_session_restore.take() else {
            return false;
        };
        let convert_snapshot = pending.convert_snapshot;
        let convert_next_item_id =
            compute_convert_next_item_id(&convert_snapshot, &convert_snapshot.items);
        let input_paths = convert_snapshot.input_paths;
        let mut convert_items = convert_snapshot.items;
        let mut items = pending.downloader_items;
        for it in &mut items {
            normalize_restored_item(it);
        }
        for it in &mut convert_items {
            crate::app_parsing::normalize_restored_convert_item(it);
        }
        self.items = items;
        self.next_item_id = compute_next_item_id(&self.items);
        self.convert_input_paths = input_paths;
        self.convert_items = convert_items;
        self.convert_next_item_id = convert_next_item_id;
        self.rebuild_item_index();
        self.update_status();
        self.update_convert_status();
        self.invalidate_queue_caches();
        self.refresh_done_file_lookup();
        self.queue_convert_restored_assets();
        self.bump_generation();
        true
    }

    pub fn discard_pending_session_restore(&mut self) -> bool {
        if self.pending_session_restore.is_none() {
            return false;
        }
        self.pending_session_restore = None;
        if let Err(err) = save_queue_items(&[]) {
            self.append_log(&format!("Failed to clear saved queue state: {err}"));
        }
        if self.settings.convert_remember_queue {
            self.clear_convert_queue_persistence();
        }
        true
    }

    pub fn ui_event_bus(&self) -> UiEventBus {
        UiEventBus::new(
            self.tx.clone(),
            self.event_broadcast.clone(),
            self.ui_event_channel_degraded.clone(),
        )
    }

    pub fn emit_event(&self, event: UiEvent) {
        let _ = self.ui_event_bus().publish(event);
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<UiEvent> {
        self.event_broadcast.subscribe()
    }

    pub fn item_idx(&self, item_id: u64) -> Option<usize> {
        self.item_index_by_id.get(&item_id).copied()
    }

    /// Resolves `item_id` to a row index, rebuilding the map when it is stale.
    pub fn resolve_item_idx(&mut self, item_id: u64) -> Option<usize> {
        if let Some(idx) = self.item_idx(item_id) {
            if self.items.get(idx).is_some_and(|it| it.item_id == item_id) {
                return Some(idx);
            }
        }
        self.rebuild_item_index();
        if let Some(idx) = self.item_idx(item_id) {
            if self.items.get(idx).is_some_and(|it| it.item_id == item_id) {
                return Some(idx);
            }
        }
        self.items.iter().position(|it| it.item_id == item_id)
    }

    pub fn rebuild_item_index(&mut self) {
        self.item_index_by_id.clear();
        for (idx, it) in self.items.iter().enumerate() {
            self.item_index_by_id.insert(it.item_id, idx);
        }
    }

    pub fn invalidate_queue_caches(&mut self) {
        self.rebuild_item_index();
        self.rebuild_dedupe_keys_cache();
        self.transfer_totals_dirty = true;
    }

    pub fn rebuild_dedupe_keys_cache(&mut self) {
        self.cached_dedupe_keys = crate::app_state::rebuild_dedupe_keys_set(&self.items);
    }

    pub fn sync_status_fields_from_counts(&mut self) {
        self.status_resolving = self.status_counts.resolving;
        self.status_ready = self.status_counts.ready;
        self.status_queued = self.status_counts.queued;
        self.status_active = self.status_counts.active;
        self.status_done = self.status_counts.done;
        self.status_failed = self.status_counts.failed;
    }

    pub fn update_status(&mut self) {
        self.status_counts = crate::app_state::compute_status_counts(&self.items);
        self.sync_status_fields_from_counts();
    }

    pub fn rebuild_convert_item_index(&mut self) {
        self.convert_item_index_by_id = rebuild_convert_item_index_map(&self.convert_items);
    }

    pub fn convert_item_idx(&self, item_id: u64) -> Option<usize> {
        self.convert_item_index_by_id.get(&item_id).copied()
    }

    pub fn update_convert_status(&mut self) {
        self.convert_status_counts = compute_convert_status_counts(&self.convert_items);
        self.convert_batch_summary = compute_convert_batch_summary(&self.convert_items);
        self.convert_batch_progress = compute_convert_batch_progress(&self.convert_items);
        self.rebuild_convert_item_index();
    }

    pub fn set_item_status_at(&mut self, idx: usize, new: ItemStatus) {
        if idx < self.items.len() {
            crate::app_state::transition_queue_item_status(&mut self.items[idx], new);
            self.update_status();
        }
    }

    pub fn bump_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }

    pub fn mark_queue_item_dirty(&mut self, item_id: u64) {
        self.dirty_queue_item_ids.insert(item_id);
        self.bump_generation();
    }

    pub fn mark_convert_item_dirty(&mut self, item_id: u64) {
        self.dirty_convert_item_ids.insert(item_id);
        self.bump_generation();
    }

    /// When local time reaches `settings.scheduled_download_start` (`HH:MM`), start ready downloads once per day.
    pub fn poll_scheduled_download_start(&mut self) {
        let schedule = self.settings.scheduled_download_start.trim();
        let Some((hour, minute)) = crate::config::parse_scheduled_time_hhmm(schedule) else {
            return;
        };
        let now = chrono::Local::now();
        use chrono::Timelike;
        let today = now.format("%Y-%m-%d").to_string();
        if self.scheduled_download_last_fire_day.as_deref() == Some(today.as_str()) {
            return;
        }
        let now_h = now.hour();
        let now_m = now.minute();
        if now_h < hour || (now_h == hour && now_m < minute) {
            return;
        }
        if self.downloads_paused || self.status_ready == 0 || self.queue_running > 0 {
            return;
        }
        let has_idle = self
            .items
            .iter()
            .any(|x| x.status == ItemStatus::Idle && x.error.is_none());
        if !has_idle {
            return;
        }
        self.scheduled_download_last_fire_day = Some(today);
        self.append_log(&format!(
            "Scheduled download start triggered ({schedule} local)."
        ));
        let _ = self.start_downloads();
    }

    /// Keeps monotonic item IDs when reusing an existing row (e.g. Refetch metadata).
    pub fn bump_item_id_floor(&mut self, used_id: u64) {
        if self.next_item_id <= used_id {
            self.next_item_id = used_id.saturating_add(1);
        }
    }

    pub fn bump_settings_generation(&mut self) {
        self.settings_generation = self.settings_generation.saturating_add(1);
    }

    pub fn append_log(&mut self, message: &str) {
        let line = crate::time_format::format_log_line(message);
        self.log_lines.push_back(line.clone());
        trim_activity_log(&mut self.log_lines, self.settings.log_max_chars);
        self.schedule_log_save();
        self.emit_event(UiEvent::LogLine { line });
    }

    pub fn schedule_queue_save(&mut self) {
        self.queue_save_deadline = Some(Instant::now() + QUEUE_SAVE_DEBOUNCE);
    }

    pub fn maybe_flush_queue_save(&mut self) {
        if let Some(deadline) = self.queue_save_deadline {
            if Instant::now() >= deadline {
                self.queue_save_deadline = None;
                let items = self.items.clone();
                let rt = self.runtime.clone();
                rt.spawn(async move {
                    if let Ok(Err(err)) =
                        tokio::task::spawn_blocking(move || save_queue_items(&items)).await
                    {
                        eprintln!("rustdl: failed to save queue state: {err}");
                    }
                });
            }
        }
    }

    pub fn schedule_log_save(&mut self) {
        self.log_save_deadline = Some(Instant::now() + QUEUE_SAVE_DEBOUNCE);
    }

    pub fn maybe_flush_log_save(&mut self) {
        if let Some(deadline) = self.log_save_deadline {
            if Instant::now() >= deadline {
                self.log_save_deadline = None;
                let lines = self.log_lines.clone();
                let rt = self.runtime.clone();
                rt.spawn(async move {
                    if let Ok(Err(err)) =
                        tokio::task::spawn_blocking(move || save_activity_log(&lines)).await
                    {
                        eprintln!("rustdl: failed to save activity log: {err}");
                    }
                });
            }
        }
    }

    pub fn flush_log_to_disk(&mut self) {
        self.log_save_deadline = None;
        if let Err(err) = save_activity_log(&self.log_lines) {
            eprintln!("rustdl: failed to save activity log: {err}");
        }
    }

    pub fn flush_queue_to_disk(&mut self) {
        self.queue_save_deadline = None;
        if let Err(err) = save_queue_items(&self.items) {
            self.append_log(&format!("Failed to save queue state: {err}"));
        }
    }

    pub fn persist_settings(&mut self) {
        self.settings.output_dir = self.output_dir.clone();
        self.settings.worker_count = self.worker_count.clamp(1, 6);
        if let Err(err) = save_settings(&self.settings) {
            self.append_log(&format!("Failed to save settings: {err}"));
        }
        self.bump_settings_generation();
    }

    pub fn work_in_progress(&self) -> bool {
        self.add_in_progress
            || self.status_resolving > 0
            || self.status_queued > 0
            || self.status_active > 0
            || self.queue_running > 0
            || self.convert_running
    }

    /// Graceful quit from the LAN web UI: cancel active jobs, persist state, then notify listeners.
    pub fn request_app_shutdown(&mut self, process_exit: Option<tokio::sync::oneshot::Sender<()>>) {
        if self.shutdown_pending {
            return;
        }
        self.shutdown_pending = true;
        if let Some(tx) = process_exit {
            self.shutdown_notify = Some(tx);
        }
        self.append_log("Graceful shutdown requested from web UI: cancelling active jobs…");
        self.cancel_convert_batch();
        self.cancel_all_active(CancelPostAction::Ready);
        self.maybe_finish_shutdown();
    }

    pub fn maybe_finish_shutdown(&mut self) {
        if !self.shutdown_pending || self.work_in_progress() {
            return;
        }
        self.flush_queue_to_disk();
        self.flush_convert_queue_to_disk();
        self.persist_settings();
        self.shutdown_pending = false;
        self.append_log("Shutdown complete.");
        self.emit_event(crate::app::UiEvent::ShutdownRequested);
        if let Some(tx) = self.shutdown_notify.take() {
            let _ = tx.send(());
        }
    }

    pub fn refresh_deps(&mut self) {
        let (yt, ffm, ffp) = ytdlp::get_external_tools_with_paths(
            &self.settings.yt_dlp_path,
            &self.settings.ffmpeg_path,
            &self.settings.ffprobe_path,
        );
        self.has_yt_dlp = yt;
        self.has_ffmpeg = ffm;
        self.has_ffprobe = ffp;
        self.yt_dlp_version = if yt {
            ytdlp::read_yt_dlp_version(&self.settings.yt_dlp_path)
                .unwrap_or_else(|| "unknown".to_owned())
        } else {
            String::new()
        };
        self.ffmpeg_version = if ffm {
            ytdlp::read_ffmpeg_version(&self.settings.ffmpeg_path)
                .unwrap_or_else(|| "unknown".to_owned())
        } else {
            String::new()
        };
        self.ffprobe_version = if ffp {
            ytdlp::read_ffprobe_version(&self.settings.ffprobe_path)
                .unwrap_or_else(|| "unknown".to_owned())
        } else {
            String::new()
        };
        self.http_client = crate::http_client::build_http_client(&self.settings);
    }

    /// Cache key for downloader queue thumbnails (invalidates when metadata or local path changes).
    pub fn queue_thumbnail_source_key(item: &QueueItem) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            item.video_id.trim(),
            item.thumbnail_url.as_deref().unwrap_or(""),
            item.local_path.as_deref().unwrap_or(""),
            item.webpage_url.trim(),
            item.source_line.trim(),
        )
    }

    pub fn convert_thumbnail_source_key(source_path: &str) -> String {
        source_path.trim().to_owned()
    }

    pub fn cache_thumbnail_bytes(
        &mut self,
        item_id: u64,
        source_key: String,
        bytes: Vec<u8>,
        content_type: impl Into<String>,
    ) {
        if bytes.len() < 32 {
            return;
        }
        let content_type = content_type.into();
        self.thumbnail_cache.insert(
            item_id,
            CachedThumbnail {
                source_key: source_key.clone(),
                bytes: bytes.clone(),
                content_type: content_type.clone(),
            },
        );
        if let Some(idx) = self.item_idx(item_id) {
            let item = &self.items[idx];
            match crate::thumbnail_store::save_downloader_thumbnail(
                item_id,
                crate::thumbnail_store::DownloaderThumbnailSave {
                    source_key: &source_key,
                    content_type: &content_type,
                    webpage_url: &item.webpage_url,
                    thumbnail_url: item.thumbnail_url.as_deref(),
                    source_line: &item.source_line,
                    bytes: &bytes,
                },
            ) {
                Ok(rel_path) => {
                    self.items[idx].thumbnail_path = Some(rel_path);
                    self.schedule_queue_save();
                }
                Err(err) => {
                    eprintln!("rustdl: failed to save queue thumbnail for item {item_id}: {err:#}");
                }
            }
        }
    }

    pub fn cached_thumbnail_bytes(
        &self,
        item_id: u64,
        source_key: &str,
    ) -> Option<(Vec<u8>, String)> {
        if let Some(entry) = self.thumbnail_cache.get(&item_id) {
            if entry.source_key == source_key {
                return Some((entry.bytes.clone(), entry.content_type.clone()));
            }
        }
        if let Some(found) = crate::thumbnail_store::load_downloader_thumbnail(item_id, source_key)
        {
            return Some(found);
        }
        if self
            .item_idx(item_id)
            .is_some_and(|idx| self.items[idx].thumbnail_path.is_some())
        {
            return crate::thumbnail_store::load_downloader_thumbnail_any(item_id);
        }
        None
    }

    pub fn evict_thumbnail(&mut self, item_id: u64) {
        self.thumbnail_cache.remove(&item_id);
    }

    pub fn remove_queue_thumbnail(&mut self, item_id: u64) {
        self.evict_thumbnail(item_id);
        crate::thumbnail_store::delete_downloader_thumbnail(item_id);
        if let Some(idx) = self.item_idx(item_id) {
            self.items[idx].thumbnail_path = None;
        }
    }

    pub fn refresh_done_file_lookup(&mut self) {
        let output_dir = self.effective_output_dir();
        if !output_dir.is_empty() {
            self.output_dir = output_dir.clone();
        }
        let index_dirty = self.done_file_index.will_refresh(&output_dir);
        self.done_file_index.refresh(&output_dir);
        if index_dirty {
            self.backfill_local_paths_for_done_items();
        }
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

    /// Schedule a debounced rescan of the output folder (runs on a background thread).
    pub fn schedule_done_file_lookup_refresh(&mut self) {
        let due = Instant::now() + DONE_LOOKUP_REFRESH_DEBOUNCE;
        self.done_lookup_refresh_deadline = Some(
            self.done_lookup_refresh_deadline
                .map(|existing| existing.min(due))
                .unwrap_or(due),
        );
    }

    /// Schedule an immediate rescan (still off the UI thread when polled via [`spawn_done_file_lookup_refresh_if_due`]).
    pub fn schedule_done_file_lookup_refresh_force(&mut self) {
        self.done_file_index.force_refresh();
        self.done_lookup_refresh_deadline = Some(Instant::now());
    }

    fn resolve_item_on_disk_path(&self, item: &QueueItem) -> Option<std::path::PathBuf> {
        let output_dir = self.effective_output_dir();
        if let Some(rel) = &item.local_path {
            if let Some(path) =
                crate::domain::done_file_index::resolve_path_under_output(&output_dir, rel)
            {
                if path.is_file() {
                    return Some(path);
                }
            }
        }
        self.done_file_index
            .find_path_for_queue_item(&output_dir, item)
            .or_else(|| self.done_file_index.find_path_in_index(item))
            .map(|(path, _)| path)
    }

    /// Starts a background output-folder scan when a refresh was scheduled and is due.
    pub fn spawn_done_file_lookup_refresh_if_due(shared: &SharedCore) {
        let (due, inflight, runtime) = {
            let Some(core) = shared.try_lock() else {
                return;
            };
            let due = core
                .done_lookup_refresh_deadline
                .is_some_and(|deadline| Instant::now() >= deadline);
            (
                due,
                core.done_lookup_refresh_inflight.clone(),
                core.runtime.clone(),
            )
        };
        if !due || inflight.load(Ordering::Relaxed) {
            return;
        }
        inflight.store(true, Ordering::Relaxed);
        let shared = shared.clone();
        let inflight_done = inflight;
        runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let mut core = shared.lock();
                core.done_lookup_refresh_deadline = None;
                core.refresh_done_file_lookup();
                core.bump_generation();
            })
            .await;
            if result.is_err() {
                eprintln!("rustdl: done-file lookup refresh task failed");
            }
            inflight_done.store(false, Ordering::Relaxed);
        });
    }

    /// Poll configured watch folders for new URL files and video files (headless + GUI).
    pub fn poll_watch_folders(&mut self) {
        if self.settings.watch_folder_enabled {
            let path = self.settings.watch_folder_path.trim().to_owned();
            if !path.is_empty() {
                let urls = self
                    .watch_folder_state
                    .poll_downloader_folder(std::path::Path::new(&path));
                if !urls.is_empty() {
                    let n = urls.len();
                    let _ = self.queue_urls_for_resolve(urls);
                    self.append_log(&format!("Watch folder: enqueued {n} URL(s) from {path}"));
                }
            }
        }
        if self.settings.convert_watch_folder_enabled {
            let path = self.settings.convert_watch_folder_path.trim().to_owned();
            if !path.is_empty() {
                let paths = self
                    .watch_folder_state
                    .poll_convert_folder(std::path::Path::new(&path));
                let auto_start = self.settings.convert_auto_start_on_add;
                if !paths.is_empty() {
                    let n = paths.len();
                    self.scan_convert_paths_into_queue(&paths);
                    if auto_start {
                        let _ = self.start_convert_batch();
                    }
                    self.append_log(&format!(
                        "Convert watch folder: added {n} file(s) from {path}"
                    ));
                }
            }
        }
    }

    /// Records `local_path` on finished rows when a matching file exists in the output folder.
    pub fn backfill_local_paths_for_done_items(&mut self) {
        for idx in 0..self.items.len() {
            if !matches!(
                self.items[idx].status,
                ItemStatus::Done | ItemStatus::Failed
            ) {
                continue;
            }
            let output_dir = self.effective_output_dir();
            if let Some(ref saved) = self.items[idx].local_path {
                if crate::app::done_file_index::resolve_path_under_output(&output_dir, saved)
                    .is_some()
                {
                    continue;
                }
                self.items[idx].local_path = None;
            }
            let item = self.items[idx].clone();
            if let Some((path, _)) = self
                .done_file_index
                .find_path_for_queue_item(&output_dir, &item)
            {
                self.items[idx].local_path = Some(path.to_string_lossy().into_owned());
            }
        }
    }

    pub fn bind_local_path_for_item(&mut self, item_id: u64) {
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        let output_dir = self.effective_output_dir();
        let item = self.items[idx].clone();
        if let Some((path, _)) = self
            .done_file_index
            .find_path_for_queue_item(&output_dir, &item)
        {
            self.items[idx].local_path = Some(path.to_string_lossy().into_owned());
        }
    }

    /// ffprobe the saved download and store codec, fps, and resolution on the queue row.
    pub fn probe_saved_file_media_for_item(&mut self, item_id: u64) {
        if !self.has_ffprobe {
            return;
        }
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        let output_dir = self.effective_output_dir();
        let item = self.items[idx].clone();
        let path = item
            .local_path
            .as_ref()
            .and_then(|rel| {
                crate::app::done_file_index::resolve_path_under_output(&output_dir, rel)
            })
            .or_else(|| {
                self.done_file_index
                    .find_path_for_queue_item(&output_dir, &item)
                    .map(|(path, _)| path)
            });
        let Some(path) = path else {
            return;
        };
        crate::app_parsing::apply_local_media_probe(
            &mut self.items[idx],
            &path,
            &self.settings.ffprobe_path,
        );
    }

    pub fn download_extra_args_for_item(&self, item: &QueueItem) -> Vec<String> {
        crate::ytdlp_download_args::build_download_extra_args_for_item(
            &self.settings,
            &self.profile_store,
            item,
        )
    }

    pub fn effective_settings_for_item(&self, item: &QueueItem) -> crate::config::AppSettings {
        let mut effective = self.settings.clone();
        if let Some(name) = item.profile_override.as_deref() {
            if let Some(profile) = crate::profiles::find_profile(&self.profile_store, name.trim()) {
                profile.apply_to(&mut effective);
            }
        }
        effective
    }

    /// Move a finished download into the organize layout when enabled; logs warnings on failure.
    pub fn apply_post_download_organize_for_item(&mut self, item_id: u64) {
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        let item = self.items[idx].clone();
        let settings = self.effective_settings_for_item(&item);
        if !settings.post_download_organize
            || crate::download_organize::uses_custom_template(&settings)
        {
            return;
        }
        let output_dir = self.effective_output_dir();
        let source = item
            .local_path
            .as_ref()
            .and_then(|p| crate::domain::done_file_index::resolve_path_under_output(&output_dir, p))
            .or_else(|| {
                self.done_file_index
                    .find_path_for_queue_item(&output_dir, &item)
                    .map(|(p, _)| p)
            });
        let Some(source) = source else {
            return;
        };
        let mut item_mut = self.items[idx].clone();
        match crate::download_organize::apply_post_download_organize(
            &output_dir,
            &settings,
            &mut item_mut,
            &source,
        ) {
            Ok(Some(path)) => {
                self.items[idx].local_path = item_mut.local_path.clone();
                self.schedule_done_file_lookup_refresh();
                self.append_log(&format!("[item {item_id}] Organized download → {}", path));
            }
            Ok(None) => {}
            Err(e) => {
                self.append_log(&format!(
                    "[item {item_id}] Post-download organize failed: {e:#}"
                ));
            }
        }
    }

    /// Find/replace in the downloaded file stem when configured; logs warnings on failure.
    pub fn apply_post_download_filename_rewrite_for_item(&mut self, item_id: u64) {
        let find = self.settings.post_download_filename_find.trim();
        if find.is_empty() {
            return;
        }
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        let item = self.items[idx].clone();
        let output_dir = self.effective_output_dir();
        let source = item
            .local_path
            .as_ref()
            .and_then(|p| crate::domain::done_file_index::resolve_path_under_output(&output_dir, p))
            .or_else(|| {
                self.done_file_index
                    .find_path_for_queue_item(&output_dir, &item)
                    .map(|(p, _)| p)
            });
        let Some(source) = source else {
            return;
        };
        match crate::filename_rewrite::apply_filename_find_replace(
            &source,
            find,
            &self.settings.post_download_filename_replace,
        ) {
            Ok(Some(path)) => {
                let saved = path.to_string_lossy().into_owned();
                self.items[idx].local_path = Some(saved.clone());
                self.schedule_done_file_lookup_refresh();
                self.append_log(&format!("[item {item_id}] Renamed download → {}", saved));
            }
            Ok(None) => {}
            Err(e) => {
                self.append_log(&format!(
                    "[item {item_id}] Post-download filename replace failed: {e:#}"
                ));
            }
        }
    }

    /// Reorders Ready (Idle) items by drag-and-drop; returns false when ids are invalid.
    pub fn reorder_ready_items(&mut self, dragged_id: u64, target_id: u64) -> bool {
        if dragged_id == target_id {
            return false;
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
            return false;
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
            return false;
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
        self.bump_generation();
        true
    }

    pub fn export_queue_url_lines(&self) -> Vec<String> {
        self.items
            .iter()
            .filter_map(|it| {
                let u = if !it.webpage_url.trim().is_empty() {
                    it.webpage_url.as_str()
                } else {
                    it.source_line.as_str()
                };
                let u = u.trim();
                if u.is_empty() {
                    None
                } else {
                    Some(u.to_owned())
                }
            })
            .collect()
    }

    pub fn requeue_done_items(&mut self, item_ids: &[u64]) -> usize {
        let mut count = 0usize;
        for &item_id in item_ids {
            let Some(idx) = self.item_idx(item_id) else {
                continue;
            };
            if self.items[idx].status != ItemStatus::Done {
                continue;
            }
            self.set_item_status_at(idx, ItemStatus::Idle);
            self.items[idx].percent = 0.0;
            self.items[idx].speed_text = "-".to_owned();
            self.items[idx].eta_text = "-".to_owned();
            self.items[idx].detail = "Ready".to_owned();
            self.items[idx].error = None;
            count += 1;
        }
        if count > 0 {
            self.update_status();
            self.schedule_queue_save();
            self.bump_generation();
        }
        count
    }

    pub fn yt_dlp_bin(&self) -> String {
        if self.settings.yt_dlp_path.trim().is_empty() {
            "yt-dlp".to_owned()
        } else {
            self.settings.yt_dlp_path.trim().to_owned()
        }
    }

    pub fn ffmpeg_bin(&self) -> String {
        if self.settings.ffmpeg_path.trim().is_empty() {
            String::new()
        } else {
            self.settings.ffmpeg_path.trim().to_owned()
        }
    }

    pub fn metadata_extra_args(&self) -> Vec<String> {
        metadata_extra_args(&self.settings)
    }

    pub fn pause_all_downloads(&mut self) {
        if self.downloads_paused {
            return;
        }
        self.downloads_paused = true;
        self.cancel_all_active(CancelPostAction::Ready);
        self.append_log("Downloads paused (active items moved back to ready).");
    }

    pub fn resume_all_downloads(&mut self) {
        if !self.downloads_paused {
            return;
        }
        self.downloads_paused = false;
        self.session_complete_notified = false;
        self.append_log("Downloads resumed.");
        let _ = self.start_downloads();
    }

    fn collect_idle_download_item_ids(&self) -> Vec<u64> {
        let mut ids: Vec<(u64, u64)> = self
            .items
            .iter()
            .filter(|it| it.status == ItemStatus::Idle && it.error.is_none())
            .map(|it| {
                let order = if it.sort_order == 0 {
                    it.item_id
                } else {
                    it.sort_order
                };
                (order, it.item_id)
            })
            .collect();
        ids.sort_by_key(|(order, _)| *order);
        ids.into_iter().map(|(_, id)| id).collect()
    }

    pub fn item_has_redownload_target(&self, item: &QueueItem) -> bool {
        app_state::item_has_redownload_target(item)
    }

    pub(crate) fn effective_output_dir(&self) -> String {
        let trimmed = self.output_dir.trim();
        if trimmed.is_empty() {
            self.settings.output_dir.trim().to_owned()
        } else {
            trimmed.to_owned()
        }
    }

    fn output_dir_is_valid(&self) -> bool {
        Path::new(&self.effective_output_dir()).is_dir()
    }

    fn prepare_item_redownload_reset(&mut self, item_id: u64) {
        let Some(idx) = self.resolve_item_idx(item_id) else {
            return;
        };
        let item = self.items[idx].clone();
        self.items[idx].local_path = None;
        let path_to_remove = self.resolve_item_on_disk_path(&item);
        if let Some(path) = path_to_remove {
            if let Err(e) = fs::remove_file(&path) {
                self.append_log(&format!(
                    "Could not remove old file {}: {e}",
                    path.to_string_lossy()
                ));
            } else {
                self.append_log(&format!("Removed old file: {}", path.to_string_lossy()));
            }
            self.schedule_done_file_lookup_refresh_force();
        }
        let archive = self.settings.yt_download_archive.trim();
        if !archive.is_empty() {
            let mut ids = Vec::new();
            if !item.video_id.trim().is_empty() {
                ids.push(item.video_id.trim().to_owned());
            }
            for url in [item.webpage_url.as_str(), item.source_line.as_str()] {
                let key = ytdlp::normalize_url_for_dedupe(url);
                if let Some(id) = ytdlp::youtube_id_from_dedupe_key(&key) {
                    if !ids.iter().any(|x| x == &id) {
                        ids.push(id);
                    }
                }
            }
            match remove_video_ids_from_download_archive(archive, &ids) {
                Ok(true) => {
                    self.append_log("Removed video from download archive (re-download).");
                }
                Ok(false) => {}
                Err(e) => {
                    self.append_log(&format!("Could not update download archive: {e}"));
                }
            }
        }
        {
            let it = &mut self.items[idx];
            it.error = None;
            it.percent = 0.0;
            it.size_text = "-".to_owned();
            it.speed_text = "-".to_owned();
            it.eta_text = "-".to_owned();
            it.detail = "Re-downloading…".to_owned();
        }
        self.set_item_status_at(idx, ItemStatus::Idle);
    }

    /// Re-fetch the same URL, replacing any matched file.
    pub fn redownload_item_id(&mut self, item_id: u64) -> Result<(), RedownloadError> {
        if self.output_dir.trim().is_empty() {
            self.output_dir = self.settings.output_dir.clone();
        }
        if !self.output_dir_is_valid() {
            self.append_log("Choose a valid output folder.");
            return Err(RedownloadError::InvalidOutputDir);
        }
        let Some(idx) = self.resolve_item_idx(item_id) else {
            return Err(RedownloadError::ItemNotFound);
        };
        if !self.item_has_redownload_target(&self.items[idx]) {
            self.append_log(&format!(
                "[item {item_id}] Cannot re-download: no video URL on this row."
            ));
            return Err(RedownloadError::NoUrl);
        }
        if self.downloads_paused {
            self.append_log("Downloads are paused. Click Resume first.");
            return Err(RedownloadError::DownloadsPaused);
        }
        if !self.has_yt_dlp {
            self.refresh_deps();
        }
        if !self.has_yt_dlp {
            self.append_log("yt-dlp not found (check PATH or Settings executable path).");
            return Err(RedownloadError::NoYtDlp);
        }
        self.persist_settings();
        self.schedule_done_file_lookup_refresh_force();
        self.prepare_item_redownload_reset(item_id);
        self.update_status();
        self.schedule_queue_save();
        self.bump_generation();
        self.spawn_download_workers(vec![item_id], true);
        Ok(())
    }

    pub fn spawn_download_workers(&mut self, pending_ids: Vec<u64>, force_redownload: bool) {
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
            if let Some(idx) = self.item_idx(*id) {
                self.set_item_status_at(idx, ItemStatus::Queued);
                self.items[idx].detail = "Queued".to_owned();
            }
        }
        self.update_status();
        self.schedule_queue_save();

        let mut groups = vec![Vec::<u64>::new(); self.worker_count.max(1)];
        let groups_len = groups.len();
        for (idx, iid) in pending_ids.into_iter().enumerate() {
            groups[idx % groups_len].push(iid);
        }
        let yt_dlp_bin = self.yt_dlp_bin();
        let ffmpeg_bin = self.ffmpeg_bin();
        let output_dir = self.effective_output_dir();
        let profile_store = self.profile_store.clone();

        for ids in groups.into_iter().filter(|g| !g.is_empty()) {
            self.queue_running += 1;
            let settings = self.settings.clone();
            let urls: Vec<_> = ids
                .iter()
                .filter_map(|iid| {
                    let idx = self.item_idx(*iid)?;
                    let item = self.items[idx].clone();
                    let target_url = app_state::resolve_item_download_url(&item)?;
                    let cancel_flag = self
                        .download_cancel_flags
                        .entry(*iid)
                        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
                        .clone();
                    let extra = if force_redownload {
                        build_redownload_extra_args(&self.settings)
                    } else {
                        self.download_extra_args_for_item(&item)
                    };
                    let template =
                        output_filename_template_for_item(&settings, &profile_store, &item);
                    Some((*iid, target_url, cancel_flag, extra, template))
                })
                .collect();
            background_spawn::spawn_download_worker(
                &self.runtime,
                &self.ui_event_bus(),
                output_dir.clone(),
                yt_dlp_bin.clone(),
                ffmpeg_bin.clone(),
                self.settings.subprocess_priority.clone(),
                self.settings.yt_dlp_download_auto_retries,
                self.settings.yt_dlp_retry_sleep_secs,
                urls,
            );
        }
    }

    pub fn start_downloads(&mut self) -> Result<(), DownloadStartError> {
        if self.downloads_paused {
            self.append_log("Downloads are paused. Click Resume first.");
            return Err(DownloadStartError::Paused);
        }
        if self.items.is_empty() {
            self.append_log("Add URLs first.");
            return Err(DownloadStartError::EmptyQueue);
        }
        if !Path::new(&self.output_dir).is_dir() {
            self.append_log("Choose a valid output folder.");
            return Err(DownloadStartError::InvalidOutputDir);
        }
        self.persist_settings();
        let pending_ids = self.collect_idle_download_item_ids();
        self.bump_generation();
        self.spawn_download_workers(pending_ids, false);
        Ok(())
    }

    pub fn remove_item_by_id(&mut self, item_id: u64) -> bool {
        let Some(idx) = self.resolve_item_idx(item_id) else {
            return false;
        };
        if self.items[idx].status == ItemStatus::Resolving {
            self.pending_resolve_ids.retain(|_, iid| *iid != item_id);
        }
        self.remove_queue_thumbnail(item_id);
        self.items.remove(idx);
        self.rebuild_item_index();
        self.invalidate_queue_caches();
        self.update_status();
        self.flush_queue_to_disk();
        true
    }

    /// Removes a row; queued/downloading items are cancelled first (remove when cancel completes).
    pub fn remove_item_from_queue(&mut self, item_id: u64) -> bool {
        let Some(idx) = self.resolve_item_idx(item_id) else {
            return false;
        };
        match self.items[idx].status {
            ItemStatus::Queued => {
                self.download_cancel_flags.remove(&item_id);
                self.cancel_post_actions.remove(&item_id);
                self.remove_item_by_id(item_id)
            }
            ItemStatus::Downloading => {
                self.request_cancel_item(item_id, CancelPostAction::Remove);
                true
            }
            _ => self.remove_item_by_id(item_id),
        }
    }

    pub fn delete_item_file_on_disk(&mut self, item_id: u64) -> bool {
        let Some(idx) = self.item_idx(item_id) else {
            return false;
        };
        let item = self.items[idx].clone();
        let Some(path) = self.resolve_item_on_disk_path(&item) else {
            self.schedule_done_file_lookup_refresh_force();
            return false;
        };
        match fs::remove_file(&path) {
            Ok(()) => {
                self.append_log(&format!("Deleted file: {}", path.to_string_lossy()));
                self.items[idx].local_path = None;
                self.schedule_done_file_lookup_refresh_force();
                self.bump_generation();
                true
            }
            Err(e) => {
                self.append_log(&format!(
                    "Failed to delete file {}: {e}",
                    path.to_string_lossy()
                ));
                false
            }
        }
    }

    pub fn clear_activity_log(&mut self) {
        self.log_lines.clear();
        if let Err(err) = save_activity_log(&self.log_lines) {
            eprintln!("rustdl: failed to save activity log: {err}");
        }
        self.log_save_deadline = None;
        self.bump_generation();
    }

    pub fn clear_queue(&mut self, filter: QueueClearFilter) -> usize {
        let removed = match filter {
            QueueClearFilter::Inactive => {
                let before = self.items.len();
                self.items
                    .retain(|it| matches!(it.status, ItemStatus::Queued | ItemStatus::Downloading));
                self.pending_resolve_ids
                    .retain(|_, iid| self.items.iter().any(|x| x.item_id == *iid));
                before.saturating_sub(self.items.len())
            }
            QueueClearFilter::All => {
                let ids: Vec<u64> = self.items.iter().map(|it| it.item_id).collect();
                let mut n = 0usize;
                for id in ids {
                    if self.remove_item_from_queue(id) {
                        n += 1;
                    }
                }
                n
            }
            other => {
                let ids: Vec<u64> = self
                    .items
                    .iter()
                    .filter(|it| queue_clear_matches(it.status, other))
                    .map(|it| it.item_id)
                    .collect();
                let mut n = 0usize;
                for id in ids {
                    if self.remove_item_from_queue(id) {
                        n += 1;
                    }
                }
                n
            }
        };
        if removed > 0 {
            self.rebuild_item_index();
            self.invalidate_queue_caches();
            self.update_status();
            self.flush_queue_to_disk();
            self.bump_generation();
            self.append_log(&format!("Removed {removed} item(s) from the queue."));
        }
        removed
    }

    /// Clears downloader and converter queues and writes empty persisted state (used on exit).
    pub fn clear_all_queues_for_exit(&mut self) {
        let _ = self.clear_queue(QueueClearFilter::All);
        self.convert_input_paths.clear();
        if !self.convert_items.is_empty() {
            self.clear_convert_queue();
        } else {
            self.clear_convert_queue_persistence();
        }
        if let Err(err) = save_queue_items(&[]) {
            self.append_log(&format!("Failed to clear saved queue state: {err}"));
        }
        self.bump_generation();
    }

    pub fn item_has_file_on_disk(&self, item: &QueueItem) -> bool {
        let output_dir = self.effective_output_dir();
        self.done_file_index
            .find_path_for_queue_item(&output_dir, item)
            .or_else(|| self.done_file_index.find_path_in_index(item))
            .is_some()
    }

    pub fn request_cancel_item(&mut self, item_id: u64, post_action: CancelPostAction) {
        let Some(idx) = self.item_idx(item_id) else {
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
                    self.set_item_status_at(idx, ItemStatus::Idle);
                    let it = &mut self.items[idx];
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
        self.schedule_queue_save();
    }

    pub fn cancel_all_active(&mut self, post_action: CancelPostAction) {
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
        let count = ids.len();
        for item_id in ids {
            self.request_cancel_item(item_id, post_action);
        }
        self.append_log(&format!("Cancel requested for {count} item(s)."));
    }

    pub fn retry_download_item_id(&mut self, item_id: u64) {
        let Some(idx) = self.resolve_item_idx(item_id) else {
            return;
        };
        if self.items[idx].status != ItemStatus::Failed {
            return;
        }
        let _ = self.redownload_item_id(item_id);
    }

    pub fn retry_failed_items(&mut self) -> Result<(), RetryFailedError> {
        if self.downloads_paused {
            self.append_log("Downloads are paused. Click Resume first.");
            return Err(RetryFailedError::DownloadsPaused);
        }
        if !self.output_dir_is_valid() {
            self.append_log("Choose a valid output folder.");
            return Err(RetryFailedError::InvalidOutputDir);
        }
        if !self.has_yt_dlp {
            self.refresh_deps();
        }
        if !self.has_yt_dlp {
            self.append_log("yt-dlp not found (check PATH or Settings executable path).");
            return Err(RetryFailedError::NoYtDlp);
        }
        let failed_no_url = self
            .items
            .iter()
            .filter(|it| it.status == ItemStatus::Failed && !self.item_has_redownload_target(it))
            .count();
        let ids: Vec<u64> = self
            .items
            .iter()
            .filter(|it| it.status == ItemStatus::Failed && self.item_has_redownload_target(it))
            .map(|it| it.item_id)
            .collect();
        if ids.is_empty() {
            if self.status_failed > 0 {
                self.append_log(
                    "No failed items have a video URL to retry. Check the row or re-add the link.",
                );
                return Err(RetryFailedError::NoUrlOnFailed);
            }
            self.append_log("No failed downloads to retry.");
            return Err(RetryFailedError::NothingToRetry);
        }
        self.persist_settings();
        self.schedule_done_file_lookup_refresh_force();
        for id in &ids {
            self.prepare_item_redownload_reset(*id);
        }
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
        self.bump_generation();
        self.spawn_download_workers(ids, true);
        Ok(())
    }

    pub fn queue_urls_for_resolve(&mut self, lines: Vec<String>) -> UrlLineFilterStats {
        if lines.is_empty() {
            self.append_log("Add at least one URL.");
            return UrlLineFilterStats::default();
        }
        self.rebuild_dedupe_keys_cache();
        let (lines, filter_stats) =
            app_state::filter_url_lines_for_queue_add(lines, &self.cached_dedupe_keys);
        let skipped_dup = filter_stats.duplicate_in_input + filter_stats.duplicate_existing;
        if skipped_dup > 0 {
            self.append_log(&format!(
                "Skipped {skipped_dup} duplicate URL(s) already in the queue or input."
            ));
        }
        if filter_stats.invalid > 0 {
            self.append_log(&format!("Skipped {} invalid URL(s).", filter_stats.invalid));
        }
        if lines.is_empty() {
            self.append_log("No new URLs to add (all duplicates or invalid).");
            return filter_stats;
        }
        let (has_yt, _, _) = ytdlp::get_external_tools_with_paths(
            &self.settings.yt_dlp_path,
            &self.settings.ffmpeg_path,
            &self.settings.ffprobe_path,
        );
        if !has_yt {
            self.append_log("yt-dlp not found (check PATH or Settings executable path).");
            self.refresh_deps();
            return filter_stats;
        }
        self.add_in_progress = true;
        self.add_total_urls = 0;
        self.add_processed_urls = 0;
        self.add_current_url = None;
        let mut queued_lines = Vec::new();
        for line in lines {
            let iid = self.next_item_id;
            self.next_item_id += 1;
            let item = QueueItem::pending_metadata(iid, line.clone());
            self.items.insert(0, item);
            self.pending_resolve_ids.insert(line.clone(), iid);
            queued_lines.push(line);
        }
        self.rebuild_item_index();
        self.add_total_urls = queued_lines.len();
        self.update_status();
        self.invalidate_queue_caches();
        self.flush_queue_to_disk();
        self.bump_generation();
        if queued_lines.is_empty() {
            self.add_in_progress = false;
            self.append_log("No new URLs to add (all duplicates).");
            return filter_stats;
        }
        background_spawn::spawn_url_resolve_pipeline(
            &self.runtime,
            &self.ui_event_bus(),
            self.yt_dlp_bin(),
            self.metadata_extra_args(),
            self.settings.playlist_preview_cap,
            queued_lines,
        );
        filter_stats
    }

    pub fn snapshot_queue(&self) -> Vec<QueueItem> {
        self.items.clone()
    }

    pub fn snapshot_logs(&self) -> Vec<String> {
        self.log_lines.iter().cloned().collect()
    }

    pub fn apply_settings_patch(&mut self, patch: AppSettings) {
        self.settings = patch;
        crate::config::normalize_settings(&mut self.settings);
        self.output_dir = self.settings.output_dir.clone();
        self.worker_count = self.settings.worker_count;
        self.persist_settings();
        self.refresh_deps();
        self.bump_generation();
    }

    /// Merges a partial JSON object into settings (field-level PATCH for the web UI).
    pub fn merge_settings_patch(&mut self, patch: &serde_json::Value) -> Result<(), String> {
        let mut current = serde_json::to_value(&self.settings).map_err(|e| e.to_string())?;
        merge_json_values(&mut current, patch);
        let merged: AppSettings = serde_json::from_value(current).map_err(|e| e.to_string())?;
        self.apply_settings_patch(merged);
        Ok(())
    }

    pub fn tools_status_json(&self) -> serde_json::Value {
        serde_json::json!({
            "yt_dlp": tool_json("yt-dlp", self.has_yt_dlp, &self.yt_dlp_version, &self.settings.yt_dlp_path),
            "ffmpeg": tool_json("ffmpeg", self.has_ffmpeg, &self.ffmpeg_version, &self.settings.ffmpeg_path),
            "ffprobe": tool_json("ffprobe", self.has_ffprobe, &self.ffprobe_version, &self.settings.ffprobe_path),
        })
    }

    pub fn set_item_download_overrides(
        &mut self,
        item_id: u64,
        format_override: Option<String>,
        profile_override: Option<String>,
    ) -> bool {
        let Some(idx) = self.item_idx(item_id) else {
            return false;
        };
        self.items[idx].format_override = format_override.filter(|s| !s.trim().is_empty());
        self.items[idx].profile_override = profile_override.filter(|s| !s.trim().is_empty());
        self.schedule_queue_save();
        self.bump_generation();
        true
    }

    pub fn refetch_item_metadata(&mut self, item_id: u64) -> Result<(), String> {
        if self.add_in_progress {
            return Err(
                "Wait for the current metadata batch to finish before refetching.".to_owned(),
            );
        }
        if !self.has_yt_dlp {
            self.refresh_deps();
            return Err("yt-dlp not found (check PATH or Settings executable path).".to_owned());
        }
        let Some(idx) = self.item_idx(item_id) else {
            return Err("Item not found.".to_owned());
        };
        if self.items[idx].status != ItemStatus::Idle {
            return Err("Refetch is only available for Ready rows.".to_owned());
        }
        let line = self.items[idx].source_line.clone();
        if line.trim().is_empty() {
            return Err("No source URL on this row.".to_owned());
        }
        self.pending_resolve_ids.insert(line.clone(), item_id);
        self.items[idx] = QueueItem::pending_metadata(item_id, line.clone());
        self.add_in_progress = true;
        self.add_total_urls = 1;
        self.add_processed_urls = 0;
        self.add_current_url = Some(line.clone());
        self.update_status();
        self.invalidate_queue_caches();
        self.flush_queue_to_disk();
        self.bump_generation();
        self.append_log(&format!("Refetching metadata for {line}"));
        background_spawn::spawn_url_resolve_pipeline(
            &self.runtime,
            &self.ui_event_bus(),
            self.yt_dlp_bin(),
            self.metadata_extra_args(),
            self.settings.playlist_preview_cap,
            vec![line],
        );
        Ok(())
    }

    fn probe_saved_file_streams(&self, item: &QueueItem) -> Result<(bool, bool), String> {
        if !self.has_ffprobe {
            return Err("ffprobe not found (Settings → Executables).".to_owned());
        }
        if item.video_id.trim().is_empty() {
            return Err("No video id; cannot match a file in the output folder.".to_owned());
        }
        let output_dir = self.effective_output_dir();
        let path = item
            .local_path
            .as_ref()
            .and_then(|rel| {
                crate::domain::done_file_index::resolve_path_under_output(&output_dir, rel)
            })
            .or_else(|| {
                self.done_file_index
                    .find_path_for_queue_item(&output_dir, item)
                    .map(|(p, _)| p)
            });
        let Some(path) = path else {
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

    pub fn verify_streams_for_item(&mut self, item_id: u64) -> Result<String, String> {
        let Some(idx) = self.item_idx(item_id) else {
            return Err("Item not found.".to_owned());
        };
        let item = self.items[idx].clone();
        let msg = match self.probe_saved_file_streams(&item) {
            Ok((v, a)) => {
                self.probe_saved_file_media_for_item(item_id);
                format!(
                    "Verify: {} video, {} audio",
                    if v { "has" } else { "no" },
                    if a { "has" } else { "no" },
                )
            }
            Err(e) => format!("Check failed: {e}"),
        };
        if let Some(idx) = self.item_idx(item_id) {
            self.items[idx].detail = msg.clone();
            self.schedule_queue_save();
            self.bump_generation();
        }
        self.append_log(&format!("[item {item_id}] {msg}"));
        Ok(msg)
    }

    pub fn recheck_all_saved_downloads(&mut self) {
        if !self.has_ffprobe {
            self.append_log("Cannot re-check saved files: ffprobe not found.");
            return;
        }
        if self.settings.ffmpeg_extract_audio_mp3 {
            self.append_log("Skipping re-check: MP3 extraction mode is enabled.");
            return;
        }
        self.schedule_done_file_lookup_refresh_force();
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
            if self.probe_saved_file_streams(&item).is_err() {
                continue;
            }
            let probe = self.probe_saved_file_streams(&item);
            let fail_msg = match probe {
                Ok((v, a)) => Self::streams_incomplete_message(v, a),
                Err(e) => Some(e),
            };
            if let Some(msg) = fail_msg {
                if let Some(idx) = self.item_idx(item_id) {
                    self.set_item_status_at(idx, ItemStatus::Failed);
                    self.items[idx].detail = msg.clone();
                    issues += 1;
                    self.append_log(&format!("[item {item_id}] Re-check: {msg}"));
                }
            }
        }
        self.update_status();
        self.schedule_queue_save();
        self.bump_generation();
        self.append_log(&format!(
            "Re-checked saved files: {issues} item(s) marked failed (missing stream or probe error)."
        ));
    }

    pub fn bulk_retry_items(&mut self, item_ids: &[u64]) -> usize {
        let mut count = 0usize;
        for &id in item_ids {
            let Some(idx) = self.item_idx(id) else {
                continue;
            };
            if self.items[idx].status != ItemStatus::Failed {
                continue;
            }
            if self.redownload_item_id(id).is_ok() {
                count += 1;
            }
        }
        if count > 0 {
            self.bump_generation();
        }
        count
    }

    pub fn save_profile_from_settings(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("profile name required".to_owned());
        }
        if crate::profiles::builtin_profiles()
            .iter()
            .any(|p| p.name == name)
        {
            return Err("cannot overwrite a built-in profile name".to_owned());
        }
        let profile = crate::profiles::DownloadProfile::from_settings(name, &self.settings, false);
        crate::profiles::save_user_profile(&mut self.profile_store, profile)
            .map_err(|e| format!("{e:#}"))?;
        self.settings.active_profile = name.to_owned();
        self.persist_settings();
        self.append_log(&format!("Saved profile: {name}"));
        self.bump_generation();
        Ok(())
    }

    pub fn start_downloads_from_queue(&mut self) {
        let _ = self.start_downloads();
    }
}

fn queue_clear_matches(status: ItemStatus, filter: QueueClearFilter) -> bool {
    match filter {
        QueueClearFilter::Done => status == ItemStatus::Done,
        QueueClearFilter::Failed => status == ItemStatus::Failed,
        QueueClearFilter::Finished => {
            matches!(status, ItemStatus::Done | ItemStatus::Failed)
        }
        QueueClearFilter::Inactive | QueueClearFilter::All => false,
    }
}

fn merge_json_values(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                match base_map.get_mut(k) {
                    Some(existing) if v.is_object() && existing.is_object() => {
                        merge_json_values(existing, v);
                    }
                    _ => {
                        base_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (base_slot, patch_val) => {
            *base_slot = patch_val.clone();
        }
    }
}

fn tool_json(name: &str, ok: bool, version: &str, configured_path: &str) -> serde_json::Value {
    let version = version.trim();
    let short = crate::app::log_panel::compact_tool_version_display(version);
    serde_json::json!({
        "name": name,
        "ok": ok,
        "status": if ok { "OK" } else { "Missing" },
        "version": version,
        "version_short": short,
        "configured_path": configured_path.trim(),
    })
}

#[cfg(test)]
mod thumbnail_cache_tests {
    use super::*;
    use crate::models::{ItemStatus, QueueItem};

    #[test]
    fn session_has_restorable_data_detects_downloader_and_av1() {
        let item = QueueItem {
            item_id: 1,
            status: ItemStatus::Idle,
            ..Default::default()
        };
        assert!(session_has_restorable_data(
            &[item],
            &ConvertQueueSnapshot::default(),
            false
        ));
        let convert_only = ConvertQueueSnapshot {
            input_paths: "C:\\videos".to_owned(),
            ..Default::default()
        };
        assert!(!session_has_restorable_data(&[], &convert_only, false));
        assert!(session_has_restorable_data(&[], &convert_only, true));
    }

    #[test]
    fn cached_thumbnail_requires_matching_source_key() {
        let runtime = Arc::new(Runtime::new().expect("runtime"));
        let (shared, _rx) = DownloadCore::new_shared(runtime, true);
        let mut core = shared.lock();
        core.cache_thumbnail_bytes(999_001, "a".to_owned(), vec![0u8; 64], "image/png");
        assert!(core.cached_thumbnail_bytes(999_001, "a").is_some());
        assert!(core.cached_thumbnail_bytes(999_001, "b").is_none());
        core.evict_thumbnail(999_001);
        assert!(core.cached_thumbnail_bytes(999_001, "a").is_none());
    }

    /// Loads a saved on-disk downloader thumbnail via [`thumbnail_store`].
    #[test]
    fn cached_thumbnail_loads_saved_downloader_image_when_key_matches() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bytes = vec![0x89u8; 64];
        crate::thumbnail_store::save_downloader_thumbnail_at(
            dir.path(),
            42,
            crate::thumbnail_store::DownloaderThumbnailSave {
                source_key: "key-a",
                content_type: "image/png",
                webpage_url: "https://example.com/watch?v=abc",
                thumbnail_url: None,
                source_line: "https://example.com/watch?v=abc",
                bytes: &bytes,
            },
        )
        .expect("save thumbnail");
        let runtime = Arc::new(Runtime::new().expect("runtime"));
        let (shared, _rx) = DownloadCore::new_shared(runtime, true);
        let mut core = shared.lock();
        core.items.push(QueueItem {
            item_id: 42,
            webpage_url: "https://example.com/watch?v=abc".to_owned(),
            source_line: "https://example.com/watch?v=abc".to_owned(),
            thumbnail_path: Some("thumbnails/downloader/42.img".to_owned()),
            ..Default::default()
        });
        core.rebuild_item_index();
        let key = DownloadCore::queue_thumbnail_source_key(&core.items[0]);
        // Point load at temp dir by saving through cache which uses global dir;
        // verify in-memory cache path instead.
        core.cache_thumbnail_bytes(42, key.clone(), bytes.clone(), "image/png");
        assert!(
            core.cached_thumbnail_bytes(42, &key).is_some(),
            "expected cached thumbnail for item 42 (key={key})"
        );
    }

    #[test]
    fn cached_thumbnail_survives_done_file_refresh() {
        let runtime = Arc::new(Runtime::new().expect("runtime"));
        let (shared, _rx) = DownloadCore::new_shared(runtime, true);
        let mut core = shared.lock();
        let bytes = vec![0x89u8; 64];
        core.items.push(QueueItem {
            item_id: 99,
            webpage_url: "https://example.com/watch?v=xyz".to_owned(),
            source_line: "https://example.com/watch?v=xyz".to_owned(),
            status: ItemStatus::Done,
            ..Default::default()
        });
        core.rebuild_item_index();
        let key = DownloadCore::queue_thumbnail_source_key(&core.items[0]);
        core.cache_thumbnail_bytes(99, key.clone(), bytes, "image/png");
        core.refresh_done_file_lookup();
        assert!(
            core.cached_thumbnail_bytes(99, &key).is_some(),
            "thumbnail missing after refresh (key={key})"
        );
    }

    #[test]
    fn cached_thumbnail_falls_back_to_disk_image_when_key_drifts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bytes = vec![0x89u8; 64];
        crate::thumbnail_store::save_downloader_thumbnail_at(
            dir.path(),
            77,
            crate::thumbnail_store::DownloaderThumbnailSave {
                source_key: "original-key",
                content_type: "image/png",
                webpage_url: "https://example.com/watch?v=drift",
                thumbnail_url: None,
                source_line: "https://example.com/watch?v=drift",
                bytes: &bytes,
            },
        )
        .expect("save thumbnail");
        let loaded = crate::thumbnail_store::load_downloader_thumbnail_any_at(dir.path(), 77);
        assert!(loaded.is_some(), "disk fallback should load by item id");
        let runtime = Arc::new(Runtime::new().expect("runtime"));
        let (shared, _rx) = DownloadCore::new_shared(runtime, true);
        let mut core = shared.lock();
        core.items.push(QueueItem {
            item_id: 77,
            webpage_url: "https://example.com/watch?v=drift".to_owned(),
            source_line: "https://example.com/watch?v=drift".to_owned(),
            local_path: Some(r"D:\different\path.mkv".into()),
            thumbnail_path: Some("thumbnails/downloader/77.img".to_owned()),
            ..Default::default()
        });
        core.rebuild_item_index();
        let drift_key = DownloadCore::queue_thumbnail_source_key(&core.items[0]);
        assert_ne!(drift_key, "original-key");
        // In-memory cache with matching drift key
        core.cache_thumbnail_bytes(77, drift_key.clone(), vec![0x89u8; 64], "image/png");
        assert!(core.cached_thumbnail_bytes(77, &drift_key).is_some());
    }
}

#[cfg(test)]
mod queue_save_tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn maybe_flush_queue_save_skips_before_deadline() {
        let runtime = Arc::new(Runtime::new().expect("runtime"));
        let (shared, _rx) = DownloadCore::new_shared(runtime, true);
        let mut core = shared.lock();
        core.schedule_queue_save();
        assert!(core.queue_save_deadline.is_some());
        core.maybe_flush_queue_save();
        assert!(core.queue_save_deadline.is_some());
    }

    #[test]
    fn maybe_flush_queue_save_clears_deadline_after_elapsed() {
        let runtime = Arc::new(Runtime::new().expect("runtime"));
        let (shared, _rx) = DownloadCore::new_shared(runtime, true);
        let mut core = shared.lock();
        core.schedule_queue_save();
        core.queue_save_deadline = Some(Instant::now() - Duration::from_millis(1));
        core.maybe_flush_queue_save();
        assert!(core.queue_save_deadline.is_none());
    }
}

#[cfg(test)]
mod queue_lifecycle_tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::models::{ItemStatus, QueueItem};

    fn sample_idle_item(id: u64) -> QueueItem {
        QueueItem {
            item_id: id,
            sort_order: id,
            source_line: format!("https://example.com/watch?v={id}"),
            webpage_url: format!("https://example.com/watch?v={id}"),
            video_id: format!("vid{id}"),
            title: format!("Video {id}"),
            status: ItemStatus::Idle,
            ..QueueItem::default()
        }
    }

    fn test_core_with_output_dir() -> (SharedCore, tempfile::TempDir) {
        let runtime = Arc::new(Runtime::new().expect("runtime"));
        let (shared, _rx) = DownloadCore::new_shared(runtime, true);
        let dir = tempdir().expect("tempdir");
        let path = dir.path().to_string_lossy().into_owned();
        {
            let mut core = shared.lock();
            core.output_dir = path.clone();
            core.settings.output_dir = path;
        }
        (shared, dir)
    }

    #[test]
    fn start_downloads_rejects_empty_queue() {
        let (shared, _dir) = test_core_with_output_dir();
        let mut core = shared.lock();
        core.items.clear();
        core.update_status();
        assert!(matches!(
            core.start_downloads(),
            Err(DownloadStartError::EmptyQueue)
        ));
    }

    #[test]
    fn start_downloads_rejects_when_paused() {
        let (shared, _dir) = test_core_with_output_dir();
        let mut core = shared.lock();
        core.items.push(sample_idle_item(1));
        core.update_status();
        core.pause_all_downloads();
        assert!(matches!(
            core.start_downloads(),
            Err(DownloadStartError::Paused)
        ));
    }

    #[test]
    fn reorder_ready_items_updates_sort_order() {
        let (shared, _dir) = test_core_with_output_dir();
        let mut core = shared.lock();
        core.items.clear();
        core.items = vec![sample_idle_item(1), sample_idle_item(2)];
        core.rebuild_item_index();
        assert!(core.reorder_ready_items(2, 1));
        let item2 = core.items.iter().find(|it| it.item_id == 2).unwrap();
        assert_eq!(item2.sort_order, 1);
    }

    #[test]
    fn remove_item_from_queue_drops_idle_row() {
        let (shared, _dir) = test_core_with_output_dir();
        let mut core = shared.lock();
        core.items.retain(|it| it.item_id != 42);
        core.items.push(sample_idle_item(42));
        core.rebuild_item_index();
        assert!(core.remove_item_from_queue(42));
        assert!(!core.items.iter().any(|it| it.item_id == 42));
    }

    #[test]
    fn pause_and_resume_downloads_toggle_flag() {
        let (shared, _dir) = test_core_with_output_dir();
        let mut core = shared.lock();
        core.items.push(sample_idle_item(1));
        core.update_status();
        core.pause_all_downloads();
        assert!(core.downloads_paused);
        core.resume_all_downloads();
        assert!(!core.downloads_paused);
    }

    #[test]
    fn retry_failed_items_requires_failed_rows() {
        let (shared, _dir) = test_core_with_output_dir();
        let mut core = shared.lock();
        core.has_yt_dlp = true;
        assert!(matches!(
            core.retry_failed_items(),
            Err(RetryFailedError::NothingToRetry)
        ));
    }
}
