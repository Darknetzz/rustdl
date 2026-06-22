use std::collections::HashSet;

use super::PydlApp;
use crate::service::core::{DownloadCore, SharedCore};

fn sync_queue_fields(app: &mut PydlApp, core: &mut DownloadCore) {
    app.recompute_status();
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
    // Activity log is owned by DownloadCore; the GUI mirrors it via sync_core_to_app.
    if app.settings_dirty {
        core.settings = app.settings.clone();
        core.profile_store = app.profile_store.clone();
        core.bump_settings_generation();
        app.synced_settings_generation = core.settings_generation;
        app.settings_dirty = false;
    }
    core.downloads_paused = app.downloads_paused;
    core.convert_paused = app.convert_paused;
    core.session_complete_notified = app.session_complete_notified;
    // The AV1 input textarea is GUI-editable; mirror it like output_dir. The rest of the AV1
    // queue state is owned by the core and flows back via sync_core_to_app.
    core.convert_input_paths = app.convert_input_paths.clone();

    if app.queue_dirty || core.items.is_empty() {
        sync_queue_fields(app, core);
        app.queue_dirty = false;
    }
}

fn sync_shared_fields_from_core(core: &DownloadCore, app: &mut PydlApp) {
    app.output_dir = core.output_dir.clone();
    app.worker_count = core.worker_count;
    app.has_yt_dlp = core.has_yt_dlp;
    app.has_ffmpeg = core.has_ffmpeg;
    app.has_ffprobe = core.has_ffprobe;
    app.yt_dlp_version = core.yt_dlp_version.clone();
    app.ffmpeg_version = core.ffmpeg_version.clone();
    app.ffprobe_version = core.ffprobe_version.clone();

    // Incremental log sync: append only new lines instead of cloning the full deque.
    if core.log_lines.len() < app.synced_log_len {
        app.log_lines.clear();
        app.synced_log_len = 0;
    }
    if app.synced_log_len < core.log_lines.len() {
        for line in core.log_lines.iter().skip(app.synced_log_len) {
            app.log_lines.push_back(line.clone());
        }
        app.synced_log_len = core.log_lines.len();
    }

    if core.settings_generation != app.synced_settings_generation {
        app.settings = core.settings.clone();
        app.profile_store = core.profile_store.clone();
        app.synced_settings_generation = core.settings_generation;
    }

    app.downloads_paused = core.downloads_paused;
    app.convert_paused = core.convert_paused;
    app.session_complete_notified = core.session_complete_notified;
    app.convert_save_deadline = core.convert_save_deadline;
}

fn queue_item_mirror_changed(
    app_it: &crate::models::QueueItem,
    core_it: &crate::models::QueueItem,
) -> bool {
    app_it.status != core_it.status
        || (app_it.percent - core_it.percent).abs() > f32::EPSILON
        || app_it.detail != core_it.detail
        || app_it.local_path != core_it.local_path
        || app_it.thumbnail_path != core_it.thumbnail_path
        || app_it.error != core_it.error
        || app_it.title != core_it.title
        || app_it.speed_text != core_it.speed_text
        || app_it.eta_text != core_it.eta_text
        || app_it.size_text != core_it.size_text
}

fn sync_queue_from_core(core: &DownloadCore, app: &mut PydlApp, previous_item_ids: &HashSet<u64>) {
    let app_ids: HashSet<u64> = app.items.iter().map(|it| it.item_id).collect();
    let core_ids: HashSet<u64> = core.items.iter().map(|it| it.item_id).collect();
    let dirty_only = !core.dirty_queue_item_ids.is_empty()
        && app_ids == core_ids
        && app.items.len() == core.items.len();
    if dirty_only {
        let core_by_id: std::collections::HashMap<u64, &crate::models::QueueItem> =
            core.items.iter().map(|it| (it.item_id, it)).collect();
        for app_it in app.items.iter_mut() {
            if core.dirty_queue_item_ids.contains(&app_it.item_id) {
                if let Some(core_it) = core_by_id.get(&app_it.item_id) {
                    *app_it = (*core_it).clone();
                }
            }
        }
    } else if app_ids == core_ids && app.items.len() == core.items.len() {
        let core_by_id: std::collections::HashMap<u64, &crate::models::QueueItem> =
            core.items.iter().map(|it| (it.item_id, it)).collect();
        for app_it in app.items.iter_mut() {
            if let Some(core_it) = core_by_id.get(&app_it.item_id) {
                if queue_item_mirror_changed(app_it, core_it) {
                    *app_it = (*core_it).clone();
                }
            }
        }
    } else {
        app.items = core.items.clone();
    }
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
    app.download_log_throttle = core.download_log_throttle.clone();
    app.queue_dirty = false;

    if app.settings.show_thumbnails {
        let new_item_ids: Vec<u64> = app
            .items
            .iter()
            .filter(|it| !previous_item_ids.contains(&it.item_id))
            .map(|it| it.item_id)
            .collect();
        for item_id in new_item_ids {
            app.queue_thumbnail_load(item_id);
        }
        app.ensure_downloader_thumbnails();
    }
}

fn convert_item_mirror_changed(
    app_it: &crate::models::ConvertQueueItem,
    core_it: &crate::models::ConvertQueueItem,
) -> bool {
    app_it.status != core_it.status
        || (app_it.percent - core_it.percent).abs() > f32::EPSILON
        || app_it.detail != core_it.detail
        || app_it.source_path != core_it.source_path
        || app_it.output_path != core_it.output_path
        || app_it.video_codec != core_it.video_codec
        || app_it.width != core_it.width
        || app_it.height != core_it.height
        || app_it.fps != core_it.fps
        || app_it.bitrate_bps != core_it.bitrate_bps
        || app_it.input_bytes != core_it.input_bytes
        || app_it.output_bytes != core_it.output_bytes
        || app_it.source_missing != core_it.source_missing
        || app_it.size_limit_kind_override != core_it.size_limit_kind_override
        || app_it.size_limit_value_override != core_it.size_limit_value_override
        || app_it.size_limit_violation_override != core_it.size_limit_violation_override
}

fn sync_convert_from_core(
    core: &DownloadCore,
    app: &mut PydlApp,
    previous_convert_ids: &HashSet<u64>,
) {
    if app.convert_input_paths != core.convert_input_paths {
        app.convert_input_paths = core.convert_input_paths.clone();
    }
    if core.convert_running && !app.convert_running {
        app.convert_batch_complete_notified = false;
    }
    app.convert_running = core.convert_running;
    app.convert_paused = core.convert_paused;
    app.convert_media_inflight = core.convert_media_inflight.clone();
    app.convert_encoder_choice = core.convert_encoder_choice.clone();
    app.convert_encoder_detect_key = core.convert_encoder_detect_key.clone();
    if app.convert_encoder_choice.is_some() {
        app.convert_encoder_detection_inflight = false;
    }
    app.convert_status_counts = core.convert_status_counts;
    app.convert_batch_summary = core.convert_batch_summary;
    app.convert_batch_progress = core.convert_batch_progress;

    let app_ids: HashSet<u64> = app.convert_items.iter().map(|it| it.item_id).collect();
    let core_ids: HashSet<u64> = core.convert_items.iter().map(|it| it.item_id).collect();
    let dirty_only = !core.dirty_convert_item_ids.is_empty()
        && app_ids == core_ids
        && app.convert_items.len() == core.convert_items.len();
    if dirty_only {
        let core_by_id: std::collections::HashMap<u64, &crate::models::ConvertQueueItem> = core
            .convert_items
            .iter()
            .map(|it| (it.item_id, it))
            .collect();
        for app_it in app.convert_items.iter_mut() {
            if core.dirty_convert_item_ids.contains(&app_it.item_id) {
                if let Some(core_it) = core_by_id.get(&app_it.item_id) {
                    *app_it = (*core_it).clone();
                }
            }
        }
    } else if app_ids == core_ids && app.convert_items.len() == core.convert_items.len() {
        let core_by_id: std::collections::HashMap<u64, &crate::models::ConvertQueueItem> = core
            .convert_items
            .iter()
            .map(|it| (it.item_id, it))
            .collect();
        for app_it in app.convert_items.iter_mut() {
            if let Some(core_it) = core_by_id.get(&app_it.item_id) {
                if convert_item_mirror_changed(app_it, core_it) {
                    *app_it = (*core_it).clone();
                }
            }
        }
    } else {
        app.convert_items = core.convert_items.clone();
    }
    app.rebuild_convert_item_index();

    if app.settings.show_thumbnails {
        let new_item_ids: Vec<u64> = app
            .convert_items
            .iter()
            .filter(|it| !previous_convert_ids.contains(&it.item_id))
            .map(|it| it.item_id)
            .collect();
        for item_id in new_item_ids {
            if let Some(idx) = app.convert_item_idx(item_id) {
                let path = app.convert_items[idx].source_path.clone();
                if !app.convert_items[idx].source_missing {
                    app.queue_convert_local_thumbnail(
                        item_id,
                        std::path::PathBuf::from(path),
                        app.settings.ffmpeg_path.clone(),
                    );
                }
            }
        }
        app.ensure_convert_thumbnails();
    }
}

pub fn sync_core_to_app(core: &mut DownloadCore, app: &mut PydlApp) {
    let previous_item_ids: HashSet<u64> = app.items.iter().map(|it| it.item_id).collect();
    let previous_convert_ids: HashSet<u64> =
        app.convert_items.iter().map(|it| it.item_id).collect();
    sync_shared_fields_from_core(core, app);
    app.done_file_index = core.done_file_index.clone();
    app.done_lookup_truncation_logged = core.done_lookup_truncation_logged;
    if core.generation != app.core_generation {
        sync_queue_from_core(core, app, &previous_item_ids);
        sync_convert_from_core(core, app, &previous_convert_ids);
        core.dirty_queue_item_ids.clear();
        core.dirty_convert_item_ids.clear();
        app.core_generation = core.generation;
    }
}

pub fn push_app_to_core(app: &mut PydlApp, shared: &SharedCore) {
    let mut core = shared.lock();
    sync_app_to_core(app, &mut core);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ItemStatus, QueueItem};
    use std::sync::Arc;
    use tokio::runtime::Runtime;

    #[test]
    fn queue_sync_runs_when_generation_changes() {
        let runtime = Arc::new(Runtime::new().expect("runtime"));
        let (shared, _rx) = DownloadCore::new_shared(runtime, true);
        let before = shared.lock().generation;
        {
            let mut core = shared.lock();
            core.items.push(QueueItem {
                item_id: 7,
                status: ItemStatus::Queued,
                ..Default::default()
            });
            core.update_status();
            core.bump_generation();
        }
        let after = shared.lock().generation;
        assert!(after > before);
    }

    #[test]
    fn sync_app_to_core_pushes_dirty_queue() {
        let runtime = Arc::new(Runtime::new().expect("runtime"));
        let (shared, _rx) = DownloadCore::new_shared(runtime, true);
        {
            let mut core = shared.lock();
            core.items.clear();
            core.rebuild_item_index();
            core.update_status();
        }

        let mut mirror = shared.lock();
        mirror.items.push(QueueItem {
            item_id: 99,
            status: ItemStatus::Idle,
            ..Default::default()
        });
        mirror.update_status();
        mirror.bump_generation();
        assert_eq!(mirror.items.len(), 1);
    }
}
