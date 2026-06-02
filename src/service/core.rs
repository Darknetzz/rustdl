//! Shared download queue and settings state (GUI + web UI).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::Mutex;
use tokio::runtime::Runtime;
use tokio::sync::broadcast;
use crate::app::background_spawn;
use crate::app::done_file_index::{DoneFileIndex, DONE_LOOKUP_MAX_ENTRIES};
use crate::app_parsing::normalize_restored_item;
use crate::app_state::{StatusCounts, TransferTotals};
use crate::config::{
    load_activity_log, load_queue_items, load_settings, save_queue_items, save_settings,
    trim_activity_log, AppSettings,
};
use crate::models::{ItemStatus, QueueItem};
use crate::profiles::{load_profiles, ProfileStore};
use crate::app::events::{try_send_ui, UiEvent};
use crate::ytdlp;
use crate::ytdlp_download_args::{build_download_extra_args, metadata_extra_args, output_filename_template};

pub type SharedCore = Arc<Mutex<DownloadCore>>;

const QUEUE_SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

#[derive(Clone, Copy)]
pub enum CancelPostAction {
    Ready,
    Remove,
}

pub struct DownloadCore {
    pub runtime: Arc<Runtime>,
    pub tx: Sender<UiEvent>,
    pub event_broadcast: broadcast::Sender<UiEvent>,

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
    pub download_log_throttle: HashMap<u64, f64>,
    /// Incremented when web or GUI sync pushes state; GUI pulls when this changes.
    pub generation: u64,
}

impl DownloadCore {
    pub fn new_shared(runtime: Arc<Runtime>) -> (SharedCore, Receiver<UiEvent>) {
        let (tx, rx) = unbounded();
        let (event_broadcast, _) = broadcast::channel(512);
        let settings = load_settings();
        let profile_store = load_profiles();
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
            .user_agent(format!("rustdl/{}", crate::pkg_version::VERSION))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let mut core = Self {
            runtime,
            tx,
            event_broadcast,
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
            items: restored_items,
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
            download_log_throttle: HashMap::new(),
            generation: 1,
        };
        core.rebuild_item_index();
        core.sync_status_fields_from_counts();
        core.refresh_deps();
        core.refresh_done_file_lookup();
        (Arc::new(Mutex::new(core)), rx)
    }

    pub fn emit_event(&self, event: UiEvent) {
        let _ = self.event_broadcast.send(event.clone());
        let _ = try_send_ui(&self.tx, event);
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<UiEvent> {
        self.event_broadcast.subscribe()
    }

    pub fn item_idx(&self, item_id: u64) -> Option<usize> {
        self.item_index_by_id.get(&item_id).copied()
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

    pub fn set_item_status_at(&mut self, idx: usize, new: ItemStatus) {
        if idx < self.items.len() {
            self.items[idx].status = new;
            self.update_status();
        }
    }

    pub fn bump_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
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

    pub fn schedule_log_save(&mut self) {
        self.log_save_deadline = Some(Instant::now() + QUEUE_SAVE_DEBOUNCE);
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

    pub fn refresh_done_file_lookup(&mut self) {
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

    pub fn download_extra_args(&self) -> Vec<String> {
        build_download_extra_args(&self.settings)
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
        self.start_downloads();
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

    pub fn spawn_download_workers(&mut self, pending_ids: Vec<u64>) {
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
        let download_args = self.download_extra_args();
        let yt_dlp_bin = self.yt_dlp_bin();
        let ffmpeg_bin = self.ffmpeg_bin();
        let output_template = output_filename_template(&self.settings);

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
                output_template.clone(),
                download_args.clone(),
                yt_dlp_bin.clone(),
                ffmpeg_bin.clone(),
                urls,
            );
        }
    }

    pub fn start_downloads(&mut self) {
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
        self.bump_generation();
        self.spawn_download_workers(pending_ids);
    }

    pub fn remove_item_by_id(&mut self, item_id: u64) -> bool {
        let Some(idx) = self.item_idx(item_id) else {
            return false;
        };
        if self.items[idx].status == ItemStatus::Resolving {
            self.pending_resolve_ids.retain(|_, iid| *iid != item_id);
        }
        self.items.remove(idx);
        self.rebuild_item_index();
        self.invalidate_queue_caches();
        self.update_status();
        self.flush_queue_to_disk();
        true
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
        for item_id in ids {
            self.request_cancel_item(item_id, post_action);
        }
    }

    pub fn queue_urls_for_resolve(&mut self, lines: Vec<String>) {
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
            return;
        }
        background_spawn::spawn_url_resolve_pipeline(
            &self.runtime,
            &self.tx,
            self.yt_dlp_bin(),
            self.metadata_extra_args(),
            self.settings.playlist_preview_cap,
            queued_lines,
        );
    }

    pub fn snapshot_queue(&self) -> Vec<QueueItem> {
        self.items.clone()
    }

    pub fn snapshot_logs(&self) -> Vec<String> {
        self.log_lines.iter().cloned().collect()
    }

    pub fn apply_settings_patch(&mut self, patch: AppSettings) {
        self.settings = patch;
        self.output_dir = self.settings.output_dir.clone();
        self.worker_count = self.settings.worker_count.clamp(1, 6);
        self.persist_settings();
        self.refresh_deps();
        self.bump_generation();
    }

    pub fn tools_status_json(&self) -> serde_json::Value {
        serde_json::json!({
            "yt_dlp": tool_json("yt-dlp", self.has_yt_dlp, &self.yt_dlp_version, &self.settings.yt_dlp_path),
            "ffmpeg": tool_json("ffmpeg", self.has_ffmpeg, &self.ffmpeg_version, &self.settings.ffmpeg_path),
            "ffprobe": tool_json("ffprobe", self.has_ffprobe, &self.ffprobe_version, &self.settings.ffprobe_path),
        })
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
