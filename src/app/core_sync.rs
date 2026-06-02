use crate::service::core::{DownloadCore, SharedCore};
use super::PydlApp;

pub fn sync_app_to_core(app: &PydlApp, core: &mut DownloadCore) {
    core.output_dir = app.output_dir.clone();
    core.worker_count = app.worker_count;
    core.status_resolving = app.status_resolving;
    core.status_ready = app.status_ready;
    core.status_queued = app.status_queued;
    core.status_active = app.status_active;
    core.status_done = app.status_done;
    core.status_failed = app.status_failed;
    core.status_counts = app.status_counts;
    core.item_index_by_id = app.item_index_by_id.clone();
    core.cached_dedupe_keys = app.cached_dedupe_keys.clone();
    core.cached_transfer_totals = app.cached_transfer_totals.clone();
    core.transfer_totals_dirty = app.transfer_totals_dirty;
    core.has_yt_dlp = app.has_yt_dlp;
    core.has_ffmpeg = app.has_ffmpeg;
    core.has_ffprobe = app.has_ffprobe;
    core.yt_dlp_version = app.yt_dlp_version.clone();
    core.ffmpeg_version = app.ffmpeg_version.clone();
    core.ffprobe_version = app.ffprobe_version.clone();
    core.log_lines = app.log_lines.clone();
    core.settings = app.settings.clone();
    core.profile_store = app.profile_store.clone();
    core.items = app.items.clone();
    core.pending_resolve_ids = app.pending_resolve_ids.clone();
    core.next_item_id = app.next_item_id;
    core.add_in_progress = app.add_in_progress;
    core.add_total_urls = app.add_total_urls;
    core.add_processed_urls = app.add_processed_urls;
    core.add_current_url = app.add_current_url.clone();
    core.queue_running = app.queue_running;
    core.download_cancel_flags = app.download_cancel_flags.clone();
    core.cancel_post_actions = app
        .cancel_post_actions
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    core.downloads_paused = app.downloads_paused;
    core.session_complete_notified = app.session_complete_notified;
    core.download_log_throttle = app.download_log_throttle.clone();
    core.bump_generation();
}

pub fn sync_core_to_app(core: &DownloadCore, app: &mut PydlApp) {
    app.output_dir = core.output_dir.clone();
    app.worker_count = core.worker_count;
    app.status_resolving = core.status_resolving;
    app.status_ready = core.status_ready;
    app.status_queued = core.status_queued;
    app.status_active = core.status_active;
    app.status_done = core.status_done;
    app.status_failed = core.status_failed;
    app.status_counts = core.status_counts;
    app.item_index_by_id = core.item_index_by_id.clone();
    app.cached_dedupe_keys = core.cached_dedupe_keys.clone();
    app.cached_transfer_totals = core.cached_transfer_totals.clone();
    app.transfer_totals_dirty = core.transfer_totals_dirty;
    app.has_yt_dlp = core.has_yt_dlp;
    app.has_ffmpeg = core.has_ffmpeg;
    app.has_ffprobe = core.has_ffprobe;
    app.yt_dlp_version = core.yt_dlp_version.clone();
    app.ffmpeg_version = core.ffmpeg_version.clone();
    app.ffprobe_version = core.ffprobe_version.clone();
    app.log_lines = core.log_lines.clone();
    app.settings = core.settings.clone();
    app.profile_store = core.profile_store.clone();
    app.items = core.items.clone();
    app.pending_resolve_ids = core.pending_resolve_ids.clone();
    app.next_item_id = core.next_item_id;
    app.add_in_progress = core.add_in_progress;
    app.add_total_urls = core.add_total_urls;
    app.add_processed_urls = core.add_processed_urls;
    app.add_current_url = core.add_current_url.clone();
    app.queue_running = core.queue_running;
    app.download_cancel_flags = core.download_cancel_flags.clone();
    app.cancel_post_actions = core
        .cancel_post_actions
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    app.downloads_paused = core.downloads_paused;
    app.session_complete_notified = core.session_complete_notified;
    app.core_generation = core.generation;
}

pub fn push_app_to_core(app: &PydlApp, shared: &SharedCore) {
    let mut core = shared.lock();
    sync_app_to_core(app, &mut core);
}
