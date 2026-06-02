use std::collections::HashSet;

use crate::service::core::{DownloadCore, SharedCore};
use super::PydlApp;

fn sync_queue_fields(app: &PydlApp, core: &mut DownloadCore) {
    core.items = app.items.clone();
    core.rebuild_item_index();
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
    core.status_resolving = app.status_resolving;
    core.status_ready = app.status_ready;
    core.status_queued = app.status_queued;
    core.status_active = app.status_active;
    core.status_done = app.status_done;
    core.status_failed = app.status_failed;
    core.status_counts = app.status_counts;
    core.cached_dedupe_keys = app.cached_dedupe_keys.clone();
    core.cached_transfer_totals = app.cached_transfer_totals.clone();
    core.transfer_totals_dirty = app.transfer_totals_dirty;
    core.download_log_throttle = app.download_log_throttle.clone();
    core.bump_generation();
}

/// Copies app state into the shared core. Queue rows are only pushed when the desktop UI
/// marked them dirty (or the core queue is still empty) so web API changes are not overwritten.
pub fn sync_app_to_core(app: &mut PydlApp, core: &mut DownloadCore) {
    core.output_dir = app.output_dir.clone();
    core.worker_count = app.worker_count;
    core.has_yt_dlp = app.has_yt_dlp;
    core.has_ffmpeg = app.has_ffmpeg;
    core.has_ffprobe = app.has_ffprobe;
    core.yt_dlp_version = app.yt_dlp_version.clone();
    core.ffmpeg_version = app.ffmpeg_version.clone();
    core.ffprobe_version = app.ffprobe_version.clone();
    core.log_lines = app.log_lines.clone();
    core.settings = app.settings.clone();
    core.profile_store = app.profile_store.clone();
    core.downloads_paused = app.downloads_paused;
    core.session_complete_notified = app.session_complete_notified;

    if app.queue_dirty || core.items.is_empty() {
        sync_queue_fields(app, core);
        app.queue_dirty = false;
    }
}

pub fn sync_core_to_app(core: &DownloadCore, app: &mut PydlApp) {
    let previous_item_ids: HashSet<u64> = app.items.iter().map(|it| it.item_id).collect();
    app.output_dir = core.output_dir.clone();
    app.worker_count = core.worker_count;
    app.status_resolving = core.status_resolving;
    app.status_ready = core.status_ready;
    app.status_queued = core.status_queued;
    app.status_active = core.status_active;
    app.status_done = core.status_done;
    app.status_failed = core.status_failed;
    app.status_counts = core.status_counts;
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
    app.rebuild_item_index();
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
    app.queue_dirty = false;
    let new_thumbnails: Vec<(u64, String)> = if app.settings.show_thumbnails {
        app.items
            .iter()
            .filter(|it| !previous_item_ids.contains(&it.item_id))
            .filter_map(|it| {
                it.thumbnail_url
                    .clone()
                    .map(|url| (it.item_id, url))
            })
            .collect()
    } else {
        Vec::new()
    };
    for (item_id, url) in new_thumbnails {
        app.queue_thumbnail_load(item_id, url);
    }
}

pub fn push_app_to_core(app: &mut PydlApp, shared: &SharedCore) {
    let mut core = shared.lock();
    sync_app_to_core(app, &mut core);
}
