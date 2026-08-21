use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::config::AppSettings;
use crate::domain::UiEvent;
use crate::models::QueueItem;
use crate::profiles::{all_profiles, delete_user_profile, find_profile, rename_user_profile};
use crate::service::core::DownloadCore;
use crate::service::core::{
    CancelPostAction, DownloadStartError, QueueClearFilter, RefetchFailedError, RetryFailedError,
    SharedCore,
};
use crate::service::web::media;
use crate::ytdlp::{self, thumbnail_url_candidates};
use crate::ytdlp_download_args::{build_download_extra_args, output_filename_template};

use super::assets;
use super::auth;

#[derive(Clone)]
pub(super) struct ApiState {
    pub core: SharedCore,
    /// Set in `--web-only` mode; signals the headless process to exit after graceful shutdown.
    pub process_exit: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    system_usage: Arc<Mutex<crate::system_usage::SystemUsageMonitor>>,
    status_cache: Arc<Mutex<Option<StatusResponse>>>,
}

impl ApiState {
    pub fn new(core: SharedCore) -> Self {
        Self {
            core,
            process_exit: Arc::new(Mutex::new(None)),
            system_usage: Arc::new(Mutex::new(crate::system_usage::SystemUsageMonitor::new())),
            status_cache: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_process_exit_notifier(&self, tx: tokio::sync::oneshot::Sender<()>) {
        *self.process_exit.lock() = Some(tx);
    }
}

#[derive(Clone, Serialize)]
struct DiskSpaceJson {
    available_bytes: u64,
    total_bytes: u64,
    volume_label: Option<String>,
    percent_free: f64,
    level: &'static str,
}

#[derive(Clone, Serialize)]
struct SystemUsageJson {
    cpu_percent: Option<f32>,
    ram_percent: Option<f32>,
    gpu_percent: Option<f32>,
    show_gpu: bool,
}

#[derive(Clone, Serialize)]
pub(super) struct BatchProgressJson {
    fraction: f32,
    percent: f32,
    finished: usize,
    total: usize,
    active: usize,
}

impl From<crate::app_state::BatchProgress> for BatchProgressJson {
    fn from(p: crate::app_state::BatchProgress) -> Self {
        Self {
            fraction: p.fraction,
            percent: p.percent(),
            finished: p.finished,
            total: p.total,
            active: p.active,
        }
    }
}

#[derive(Clone, Serialize)]
struct StatusResponse {
    version: &'static str,
    build_date: String,
    generation: u64,
    downloads_paused: bool,
    queue_running: usize,
    add_in_progress: bool,
    auto_add_pasted_urls: bool,
    auto_start_downloads: bool,
    shutdown_pending: bool,
    convert_running: bool,
    status: StatusCountsJson,
    download_batch: BatchProgressJson,
    tools: serde_json::Value,
    system_usage: SystemUsageJson,
    output_disk_space: Option<DiskSpaceJson>,
    config_warnings: Vec<String>,
    log_filter_rules: LogFilterRulesJson,
    session_restore: SessionRestoreJson,
}

#[derive(Clone, Serialize)]
struct LogFilterRulesJson {
    error_keywords: Vec<&'static str>,
    important_keywords: Vec<&'static str>,
}

#[derive(Clone, Serialize)]
struct SessionRestoreJson {
    pending: bool,
    downloader_count: usize,
    convert_count: usize,
}

#[derive(Clone, Serialize)]
struct StatusCountsJson {
    resolving: usize,
    ready: usize,
    queued: usize,
    active: usize,
    done: usize,
    failed: usize,
}

#[derive(Serialize)]
struct QueueItemView {
    #[serde(flatten)]
    item: QueueItem,
    playable: bool,
    media_kind: Option<String>,
    media_filename: Option<String>,
    can_redownload: bool,
    can_delete_file: bool,
}

#[derive(Serialize)]
struct QueueResponse {
    generation: u64,
    items: Vec<QueueItemView>,
}

#[derive(Serialize)]
pub(super) struct ApiErrorBody {
    pub error: String,
}

pub(super) fn api_err(
    status: StatusCode,
    msg: impl Into<String>,
) -> (StatusCode, Json<ApiErrorBody>) {
    (status, Json(ApiErrorBody { error: msg.into() }))
}

#[derive(Deserialize)]
struct AddUrlsBody {
    urls: Vec<String>,
}

#[derive(Serialize)]
struct AddUrlsResponse {
    accepted: usize,
    skipped_duplicates: usize,
    skipped_invalid: usize,
}

#[derive(Deserialize)]
struct QueueClearBody {
    filter: String,
}

#[derive(Serialize)]
struct QueueClearResponse {
    removed: usize,
}

#[derive(Deserialize)]
struct PatchSettingsBody {
    #[serde(default)]
    settings: Option<AppSettings>,
    #[serde(default)]
    patch: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
struct LayoutPresetBody {
    #[serde(default)]
    viewport_height: Option<f32>,
}

#[derive(Deserialize)]
struct QueueReorderBody {
    dragged_id: u64,
    target_id: u64,
}

#[derive(Deserialize)]
struct QueueRequeueBody {
    item_ids: Vec<u64>,
}

#[derive(Serialize)]
struct QueueRequeueResponse {
    requeued: usize,
}

#[derive(Serialize, Deserialize)]
struct SettingsResponse {
    settings: AppSettings,
    command_preview: String,
    web_ui_browser_url: String,
}

#[derive(Serialize)]
struct LogsResponse {
    lines: Vec<String>,
}

#[derive(Serialize)]
struct ProfilesResponse {
    active: String,
    profiles: Vec<String>,
}

#[derive(Deserialize)]
struct ApplyProfileBody {
    name: String,
}

#[derive(Deserialize)]
struct ProfileDeleteBody {
    name: String,
}

#[derive(Deserialize)]
struct ProfileRenameBody {
    old_name: String,
    new_name: String,
}

#[derive(Deserialize)]
struct ProfileSaveBody {
    name: String,
}

#[derive(Deserialize)]
struct CancelDownloadBody {
    #[serde(default)]
    post_action: Option<String>,
}

#[derive(Deserialize)]
struct ItemOverridesBody {
    format_override: Option<String>,
    profile_override: Option<String>,
}

#[derive(Deserialize)]
struct BulkItemIdsBody {
    item_ids: Vec<u64>,
}

#[derive(Deserialize)]
struct PlaylistPreviewBody {
    url: String,
}

pub fn api_router(state: ApiState) -> Router {
    let protected = Router::new()
        .route("/api/status", get(status))
        .route("/api/queue", get(queue_list))
        .route("/api/queue", post(queue_add))
        .route("/api/queue/clear", post(queue_clear))
        .route("/api/queue/reorder", post(queue_reorder))
        .route("/api/queue/export", get(queue_export))
        .route("/api/queue/import", post(queue_import))
        .route("/api/queue/requeue", post(queue_requeue))
        .route("/api/queue/bulk-retry", post(queue_bulk_retry))
        .route("/api/queue/recheck-saved", post(queue_recheck_saved))
        .route("/api/queue/refetch/:id", post(queue_refetch))
        .route("/api/queue/playlist-preview", post(queue_playlist_preview))
        .route("/api/queue/import-file", post(queue_import_multipart))
        .route("/api/queue/templates", get(queue_templates_list))
        .route("/api/queue/templates", post(queue_templates_save))
        .route("/api/queue/templates/load", post(queue_templates_load))
        .route("/api/queue/:id/overrides", post(queue_item_overrides))
        .route("/api/queue/:id/verify", post(queue_verify_streams))
        .route("/api/queue/:id/info", get(queue_more_info))
        .route("/api/queue/:id", axum::routing::delete(queue_remove))
        .route("/api/library", get(library_list))
        .route("/api/library/:id/open", post(library_open))
        .route(
            "/api/queue/:id/file",
            axum::routing::delete(queue_delete_file),
        )
        .route("/api/logs/clear", post(logs_clear))
        .route("/api/downloads/start", post(downloads_start))
        .route("/api/downloads/pause", post(downloads_pause))
        .route("/api/downloads/resume", post(downloads_resume))
        .route("/api/downloads/cancel/:id", post(downloads_cancel))
        .route("/api/downloads/redownload/:id", post(downloads_redownload))
        .route("/api/downloads/retry-failed", post(downloads_retry_failed))
        .route(
            "/api/downloads/refetch-failed",
            post(downloads_refetch_failed),
        )
        .route("/api/downloads/cancel-add", post(downloads_cancel_add))
        .route("/api/settings", get(settings_get))
        .route("/api/settings", post(settings_patch))
        .route(
            "/api/settings/layout-preset/:preset",
            post(settings_layout_preset),
        )
        .route(
            "/api/settings/organize-preset/:preset",
            post(settings_organize_preset),
        )
        .route("/api/settings/export", get(settings_export))
        .route("/api/settings/import", post(settings_import))
        .route("/api/settings/reset", post(settings_reset))
        .route("/api/open-output-folder", post(open_output_folder))
        .route("/api/web-ui/qr", get(web_ui_qr_png))
        .route("/api/browse", post(browse_host_path))
        .route("/api/profiles", get(profiles_list))
        .route("/api/profiles/apply", post(profiles_apply))
        .route("/api/profiles/delete", post(profiles_delete))
        .route("/api/profiles/rename", post(profiles_rename))
        .route("/api/profiles/save", post(profiles_save))
        .route("/api/profiles/export", get(profiles_export))
        .route("/api/profiles/import", post(profiles_import))
        .route("/api/tools/cookie-check", post(tools_cookie_check))
        .route("/api/tools/refresh", post(tools_refresh))
        .route("/api/logs", get(logs_get))
        .route("/api/shutdown", post(app_shutdown))
        .route("/api/session-restore/apply", post(session_restore_apply))
        .route(
            "/api/session-restore/discard",
            post(session_restore_discard),
        )
        .route("/api/events", get(events_sse))
        .route("/api/thumbnail/:id", get(thumbnail_proxy))
        .route("/api/media/:id", get(media_stream));
    let protected = super::convert_api::register(protected);
    let protected = super::watchlist_api::register(protected);
    let protected = super::palette::register(protected)
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            |State(st): State<ApiState>, req, next| async move {
                let (expected, whitelist) = {
                    let c = st.core.lock();
                    (
                        c.settings.web_auth_token.clone(),
                        c.settings.web_auth_ip_whitelist.clone(),
                    )
                };
                auth::require_auth(expected, whitelist, req, next).await
            },
        ))
        .with_state(state.clone());

    Router::new()
        .merge(protected)
        .fallback_service(assets::static_router())
}

fn disk_space_json(output_dir: &str) -> Option<DiskSpaceJson> {
    let space = crate::disk_space::query_disk_space(output_dir)?;
    let level = space.level_slug();
    let percent_free = space.percent_free();
    Some(DiskSpaceJson {
        available_bytes: space.available_bytes,
        total_bytes: space.total_bytes,
        volume_label: space.volume_label,
        percent_free,
        level,
    })
}

fn system_usage_json(monitor: &mut crate::system_usage::SystemUsageMonitor) -> SystemUsageJson {
    monitor.maybe_poll();
    let snap = monitor.snapshot();
    SystemUsageJson {
        cpu_percent: snap.cpu_percent,
        ram_percent: snap.ram_percent,
        gpu_percent: snap.gpu_percent,
        show_gpu: cfg!(windows),
    }
}

fn build_status_response(
    c: &DownloadCore,
    system_usage: &mut crate::system_usage::SystemUsageMonitor,
) -> StatusResponse {
    let output_dir = c.effective_output_dir();
    let config_warnings = c
        .config_load_issues
        .iter()
        .map(|issue| {
            format!(
                "Could not load {} from {} — using defaults",
                issue.label,
                issue.path.display()
            )
        })
        .collect();
    let session_restore = if let Some(pending) = &c.pending_session_restore {
        SessionRestoreJson {
            pending: true,
            downloader_count: pending.downloader_count(),
            convert_count: pending.convert_count(),
        }
    } else {
        SessionRestoreJson {
            pending: false,
            downloader_count: 0,
            convert_count: 0,
        }
    };
    StatusResponse {
        version: crate::pkg_version::VERSION,
        build_date: crate::pkg_version::build_date_local(),
        generation: c.generation,
        downloads_paused: c.downloads_paused,
        queue_running: c.queue_running,
        add_in_progress: c.add_in_progress,
        auto_add_pasted_urls: c.settings.auto_add_pasted_urls,
        auto_start_downloads: c.settings.auto_start_downloads,
        shutdown_pending: c.shutdown_pending,
        convert_running: c.convert_running,
        status: StatusCountsJson {
            resolving: c.status_resolving,
            ready: c.status_ready,
            queued: c.status_queued,
            active: c.status_active,
            done: c.status_done,
            failed: c.status_failed,
        },
        download_batch: crate::app_state::compute_download_batch_progress(&c.items).into(),
        tools: c.tools_status_json(),
        system_usage: system_usage_json(system_usage),
        output_disk_space: disk_space_json(&output_dir),
        config_warnings,
        log_filter_rules: LogFilterRulesJson {
            error_keywords: crate::log_filter::ERROR_KEYWORDS.to_vec(),
            important_keywords: crate::log_filter::IMPORTANT_KEYWORDS.to_vec(),
        },
        session_restore,
    }
}

async fn status(State(st): State<ApiState>) -> Json<StatusResponse> {
    if let Some(c) = st.core.try_lock_for(Duration::from_millis(50)) {
        let response = build_status_response(&c, &mut st.system_usage.lock());
        *st.status_cache.lock() = Some(response.clone());
        return Json(response);
    }
    if let Some(cached) = st.status_cache.lock().clone() {
        return Json(cached);
    }
    Json(StatusResponse {
        version: crate::pkg_version::VERSION,
        build_date: crate::pkg_version::build_date_local(),
        generation: 0,
        downloads_paused: false,
        queue_running: 0,
        add_in_progress: false,
        auto_add_pasted_urls: true,
        auto_start_downloads: true,
        shutdown_pending: false,
        convert_running: false,
        status: StatusCountsJson {
            resolving: 0,
            ready: 0,
            queued: 0,
            active: 0,
            done: 0,
            failed: 0,
        },
        download_batch: BatchProgressJson {
            fraction: 0.0,
            percent: 0.0,
            finished: 0,
            total: 0,
            active: 0,
        },
        tools: serde_json::json!({}),
        system_usage: system_usage_json(&mut st.system_usage.lock()),
        output_disk_space: None,
        config_warnings: Vec::new(),
        log_filter_rules: LogFilterRulesJson {
            error_keywords: crate::log_filter::ERROR_KEYWORDS.to_vec(),
            important_keywords: crate::log_filter::IMPORTANT_KEYWORDS.to_vec(),
        },
        session_restore: SessionRestoreJson {
            pending: false,
            downloader_count: 0,
            convert_count: 0,
        },
    })
}

async fn session_restore_apply(State(st): State<ApiState>) -> Result<StatusCode, StatusCode> {
    let mut c = st.core.lock();
    if c.apply_pending_session_restore() {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

async fn session_restore_discard(State(st): State<ApiState>) -> Result<StatusCode, StatusCode> {
    let mut c = st.core.lock();
    if c.discard_pending_session_restore() {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

async fn profiles_list(State(st): State<ApiState>) -> Json<ProfilesResponse> {
    let c = st.core.lock();
    Json(ProfilesResponse {
        active: c.settings.active_profile.clone(),
        profiles: all_profiles(&c.profile_store)
            .into_iter()
            .map(|p| p.name)
            .collect(),
    })
}

async fn profiles_apply(
    State(st): State<ApiState>,
    Json(body): Json<ApplyProfileBody>,
) -> Result<StatusCode, StatusCode> {
    let mut c = st.core.lock();
    let profile = find_profile(&c.profile_store, body.name.trim()).ok_or(StatusCode::NOT_FOUND)?;
    profile.apply_to(&mut c.settings);
    c.settings.active_profile = profile.name.clone();
    c.output_dir = c.settings.output_dir.clone();
    c.worker_count = c.settings.worker_count.clamp(1, 6);
    c.persist_settings();
    c.refresh_deps();
    c.bump_generation();
    Ok(StatusCode::OK)
}

async fn profiles_delete(
    State(st): State<ApiState>,
    Json(body): Json<ProfileDeleteBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: "name required".into(),
            }),
        ));
    }
    let mut c = st.core.lock();
    if find_profile(&c.profile_store, name).is_some_and(|p| p.builtin) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: "cannot delete built-in profile".into(),
            }),
        ));
    }
    delete_user_profile(&mut c.profile_store, name).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiErrorBody {
                error: format!("{e:#}"),
            }),
        )
    })?;
    if c.settings.active_profile == name {
        c.settings.active_profile = "Best quality".to_owned();
        c.persist_settings();
    }
    Ok(StatusCode::OK)
}

async fn profiles_rename(
    State(st): State<ApiState>,
    Json(body): Json<ProfileRenameBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let old_name = body.old_name.trim();
    let new_name = body.new_name.trim();
    if old_name.is_empty() || new_name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: "old_name and new_name required".into(),
            }),
        ));
    }
    let mut c = st.core.lock();
    if find_profile(&c.profile_store, old_name).is_some_and(|p| p.builtin) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: "cannot rename built-in profile".into(),
            }),
        ));
    }
    rename_user_profile(&mut c.profile_store, old_name, new_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: format!("{e:#}"),
            }),
        )
    })?;
    if c.settings.active_profile == old_name {
        c.settings.active_profile = new_name.to_owned();
        c.persist_settings();
    }
    Ok(StatusCode::OK)
}

async fn profiles_save(
    State(st): State<ApiState>,
    Json(body): Json<ProfileSaveBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    c.save_profile_from_settings(body.name.trim())
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiErrorBody { error: e })))?;
    Ok(StatusCode::OK)
}

async fn profiles_export(State(st): State<ApiState>) -> impl IntoResponse {
    let c = st.core.lock();
    let body = serde_json::to_string_pretty(&c.profile_store).unwrap_or_else(|_| "{}".to_owned());
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json; charset=utf-8",
        )],
        body,
    )
}

async fn profiles_import(
    State(st): State<ApiState>,
    body: String,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let imported: crate::profiles::ProfileStore = serde_json::from_str(&body).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: format!("invalid profiles JSON: {e}"),
            }),
        )
    })?;
    let mut c = st.core.lock();
    let mut save_errors = Vec::new();
    for p in imported.user_profiles {
        if !p.builtin {
            if let Err(e) = crate::profiles::save_user_profile(&mut c.profile_store, p) {
                save_errors.push(e.to_string());
            }
        }
    }
    if !save_errors.is_empty() {
        c.append_log(&format!(
            "Profile import: {} profile(s) could not be saved.",
            save_errors.len()
        ));
        return Err(api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save {} imported profile(s)", save_errors.len()),
        ));
    }
    c.bump_generation();
    Ok(StatusCode::OK)
}

async fn tools_cookie_check(
    State(st): State<ApiState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiErrorBody>)> {
    let c = st.core.lock();
    let test_url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let result = crate::ytdlp::cookie_health_probe(
        &c.yt_dlp_bin(),
        test_url,
        &c.settings.yt_dlp_cookies,
        &c.settings.yt_dlp_impersonate,
    );
    Ok(Json(serde_json::json!({
        "ok": result.ok,
        "message": result.message,
    })))
}

async fn tools_refresh(State(st): State<ApiState>) -> Json<serde_json::Value> {
    let mut c = st.core.lock();
    c.refresh_deps();
    Json(c.tools_status_json())
}

async fn queue_list(State(st): State<ApiState>) -> Json<QueueResponse> {
    let mut c = st.core.lock();
    c.refresh_done_file_lookup();
    let items = c
        .snapshot_queue()
        .into_iter()
        .map(|item| {
            let playable = media::item_media_playable(&c, &item);
            let media_kind = if playable {
                media::resolve_item_media_path(&c, &item)
                    .ok()
                    .and_then(|p| media::media_kind_for_path(&p))
                    .map(|k| match k {
                        media::MediaKind::Video => "video".to_owned(),
                        media::MediaKind::Audio => "audio".to_owned(),
                    })
            } else {
                None
            };
            let media_filename = media::item_media_filename(&c, &item);
            let can_redownload = c.item_has_redownload_target(&item);
            let can_delete_file = c.item_has_file_on_disk(&item);
            QueueItemView {
                item,
                playable,
                media_kind,
                media_filename,
                can_redownload,
                can_delete_file,
            }
        })
        .collect();
    Json(QueueResponse {
        generation: c.generation,
        items,
    })
}

async fn media_stream(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let path = {
        let mut c = st.core.lock();
        c.refresh_done_file_lookup();
        let idx = c.item_idx(id).ok_or(StatusCode::NOT_FOUND)?;
        let item = &c.items[idx];
        media::resolve_item_media_path(&c, item)?
    };
    media::stream_media_path(&path, &headers).await
}

async fn queue_add(
    State(st): State<ApiState>,
    Json(body): Json<AddUrlsBody>,
) -> Result<Json<AddUrlsResponse>, StatusCode> {
    let lines: Vec<String> = body
        .urls
        .into_iter()
        .map(|u| u.trim().to_owned())
        .filter(|u| !u.is_empty())
        .collect();
    if lines.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut c = st.core.lock();
    let stats = c.queue_urls_for_resolve(lines);
    Ok(Json(AddUrlsResponse {
        accepted: stats.accepted,
        skipped_duplicates: stats.duplicate_in_input + stats.duplicate_existing,
        skipped_invalid: stats.invalid,
    }))
}

async fn queue_remove(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    if c.remove_item_from_queue(id) {
        c.bump_generation();
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ApiErrorBody {
                error: "Item not found in the queue.".to_owned(),
            }),
        ))
    }
}

fn parse_queue_clear_filter(raw: &str) -> Option<QueueClearFilter> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "done" => Some(QueueClearFilter::Done),
        "failed" => Some(QueueClearFilter::Failed),
        "finished" | "completed" => Some(QueueClearFilter::Finished),
        "inactive" | "clear_list" => Some(QueueClearFilter::Inactive),
        "all" => Some(QueueClearFilter::All),
        _ => None,
    }
}

async fn queue_clear(
    State(st): State<ApiState>,
    Json(body): Json<QueueClearBody>,
) -> Result<Json<QueueClearResponse>, StatusCode> {
    let filter = parse_queue_clear_filter(&body.filter).ok_or(StatusCode::BAD_REQUEST)?;
    let mut c = st.core.lock();
    let removed = c.clear_queue(filter);
    Ok(Json(QueueClearResponse { removed }))
}

async fn queue_reorder(
    State(st): State<ApiState>,
    Json(body): Json<QueueReorderBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    if c.reorder_ready_items(body.dragged_id, body.target_id) {
        Ok(StatusCode::OK)
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: "Invalid reorder (items must be Ready).".to_owned(),
            }),
        ))
    }
}

async fn queue_export(State(st): State<ApiState>) -> impl IntoResponse {
    let c = st.core.lock();
    let body = c.export_queue_url_lines().join("\n");
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        body,
    )
}

async fn queue_import(
    State(st): State<ApiState>,
    Json(body): Json<AddUrlsBody>,
) -> Result<Json<AddUrlsResponse>, StatusCode> {
    queue_add(State(st), Json(body)).await
}

async fn queue_requeue(
    State(st): State<ApiState>,
    Json(body): Json<QueueRequeueBody>,
) -> Json<QueueRequeueResponse> {
    let mut c = st.core.lock();
    let requeued = c.requeue_done_items(&body.item_ids);
    Json(QueueRequeueResponse { requeued })
}

async fn queue_bulk_retry(
    State(st): State<ApiState>,
    Json(body): Json<BulkItemIdsBody>,
) -> Json<QueueRequeueResponse> {
    let mut c = st.core.lock();
    let requeued = c.bulk_retry_items(&body.item_ids);
    Json(QueueRequeueResponse { requeued })
}

async fn queue_recheck_saved(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.recheck_all_saved_downloads();
    StatusCode::OK
}

async fn queue_refetch(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    c.refetch_item_metadata(id)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiErrorBody { error: e })))?;
    Ok(StatusCode::OK)
}

async fn queue_item_overrides(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
    Json(body): Json<ItemOverridesBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    if c.set_item_download_overrides(id, body.format_override, body.profile_override) {
        Ok(StatusCode::OK)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ApiErrorBody {
                error: "Item not found.".to_owned(),
            }),
        ))
    }
}

#[derive(Serialize)]
struct VerifyStreamsResponse {
    message: String,
}

async fn queue_verify_streams(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> Result<Json<VerifyStreamsResponse>, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    let message = c
        .verify_streams_for_item(id)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiErrorBody { error: e })))?;
    Ok(Json(VerifyStreamsResponse { message }))
}

#[derive(Serialize)]
struct MoreInfoRowJson {
    section: String,
    label: String,
    value: String,
}

#[derive(Serialize)]
struct QueueMoreInfoResponse {
    rows: Vec<MoreInfoRowJson>,
}

async fn queue_more_info(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> Result<Json<QueueMoreInfoResponse>, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    let Some(idx) = c.resolve_item_idx(id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiErrorBody {
                error: "Item not found.".into(),
            }),
        ));
    };
    if matches!(
        c.items[idx].status,
        crate::models::ItemStatus::Done | crate::models::ItemStatus::Failed
    ) {
        c.probe_saved_file_media_for_item(id);
        if let Some(mtime) = c
            .done_file_index
            .find_path_for_queue_item(&c.effective_output_dir(), &c.items[idx])
            .and_then(|(_, t)| t.duration_since(std::time::UNIX_EPOCH).ok())
        {
            c.items[idx].file_saved_mtime = Some(mtime.as_secs());
        }
    }
    let rows = crate::media_metadata::queue_item_more_info_rows(&c.items[idx])
        .into_iter()
        .map(|row| MoreInfoRowJson {
            section: row.section.to_owned(),
            label: row.label.to_owned(),
            value: row.value,
        })
        .collect();
    Ok(Json(QueueMoreInfoResponse { rows }))
}

#[derive(Serialize)]
struct PlaylistPreviewResponse {
    count: usize,
    urls: Vec<String>,
    title: Option<String>,
}

async fn queue_playlist_preview(
    State(st): State<ApiState>,
    Json(body): Json<PlaylistPreviewBody>,
) -> Result<Json<PlaylistPreviewResponse>, (StatusCode, Json<ApiErrorBody>)> {
    let c = st.core.lock();
    let preview = ytdlp::flat_playlist_preview(
        &c.yt_dlp_bin(),
        body.url.trim(),
        c.settings.playlist_preview_cap,
    )
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: format!("{e:#}"),
            }),
        )
    })?;
    Ok(Json(PlaylistPreviewResponse {
        count: preview.urls.len(),
        urls: preview.urls,
        title: preview.title,
    }))
}

async fn queue_import_multipart(
    State(st): State<ApiState>,
    body: String,
) -> Result<Json<AddUrlsResponse>, StatusCode> {
    let lines: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    queue_add(State(st), Json(AddUrlsBody { urls: lines })).await
}

#[derive(Serialize)]
struct LibraryEntry {
    item_id: u64,
    title: String,
    uploader: Option<String>,
    completed_at: Option<u64>,
    local_path: Option<String>,
    video_id: String,
    webpage_url: String,
}

#[derive(Serialize)]
struct LibraryResponse {
    items: Vec<LibraryEntry>,
}

async fn library_list(State(st): State<ApiState>) -> Json<LibraryResponse> {
    let c = st.core.lock();
    let items = c
        .snapshot_queue()
        .into_iter()
        .filter(|it| it.status == crate::models::ItemStatus::Done)
        .map(|it| LibraryEntry {
            item_id: it.item_id,
            title: it.title.clone(),
            uploader: it.uploader.clone(),
            completed_at: it.completed_at,
            local_path: it.local_path.clone(),
            video_id: it.video_id.clone(),
            webpage_url: it.webpage_url.clone(),
        })
        .collect();
    Json(LibraryResponse { items })
}

#[derive(Deserialize)]
struct LibraryOpenBody {
    target: String,
}

async fn library_open(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
    Json(body): Json<LibraryOpenBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let path = {
        let c = st.core.lock();
        let item = c
            .items
            .iter()
            .find(|it| it.item_id == id && it.status == crate::models::ItemStatus::Done)
            .ok_or((
                StatusCode::NOT_FOUND,
                Json(ApiErrorBody {
                    error: "library item not found".to_owned(),
                }),
            ))?;
        let local = item.local_path.as_ref().ok_or((
            StatusCode::NOT_FOUND,
            Json(ApiErrorBody {
                error: "no file on disk for this row".to_owned(),
            }),
        ))?;
        let file = std::path::PathBuf::from(local);
        match body.target.trim() {
            "file" => file,
            "folder" => {
                if cfg!(target_os = "windows") {
                    file
                } else {
                    file.parent().map(|p| p.to_path_buf()).ok_or((
                        StatusCode::NOT_FOUND,
                        Json(ApiErrorBody {
                            error: "no folder for this row".to_owned(),
                        }),
                    ))?
                }
            }
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ApiErrorBody {
                        error: "target must be file or folder".to_owned(),
                    }),
                ));
            }
        }
    };
    crate::app_actions::open_path(&path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiErrorBody {
                error: format!("failed to open path: {e}"),
            }),
        )
    })?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct QueueTemplateSaveBody {
    name: String,
}

async fn queue_templates_list(State(_st): State<ApiState>) -> Json<serde_json::Value> {
    let names = crate::queue_templates::list_queue_templates();
    Json(serde_json::json!({ "templates": names }))
}

async fn queue_templates_save(
    State(st): State<ApiState>,
    Json(body): Json<QueueTemplateSaveBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let c = st.core.lock();
    let template =
        crate::queue_templates::queue_template_from_items(body.name.trim(), &c.snapshot_queue());
    crate::queue_templates::save_queue_template(&template).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiErrorBody {
                error: format!("{e:#}"),
            }),
        )
    })?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct QueueTemplateLoadBody {
    name: String,
}

async fn queue_templates_load(
    State(st): State<ApiState>,
    Json(body): Json<QueueTemplateLoadBody>,
) -> Result<Json<AddUrlsResponse>, (StatusCode, Json<ApiErrorBody>)> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: "template name required".into(),
            }),
        ));
    }
    let template = crate::queue_templates::load_queue_template(name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: format!("{e:#}"),
            }),
        )
    })?;
    let urls = crate::queue_templates::template_item_urls(&template);
    let mut c = st.core.lock();
    let stats = c.queue_urls_for_resolve(urls);
    Ok(Json(AddUrlsResponse {
        accepted: stats.accepted,
        skipped_duplicates: stats.duplicate_in_input + stats.duplicate_existing,
        skipped_invalid: stats.invalid,
    }))
}

async fn queue_delete_file(State(st): State<ApiState>, Path(id): Path<u64>) -> StatusCode {
    let mut c = st.core.lock();
    if c.delete_item_file_on_disk(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn logs_clear(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.clear_activity_log();
    StatusCode::OK
}

async fn downloads_start(
    State(st): State<ApiState>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    c.start_downloads()
        .map_err(|e: DownloadStartError| api_err(StatusCode::CONFLICT, e.message()))?;
    Ok(StatusCode::OK)
}

async fn downloads_pause(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.pause_all_downloads();
    StatusCode::OK
}

async fn downloads_resume(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.resume_all_downloads();
    StatusCode::OK
}

async fn downloads_cancel(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
    body: Option<Json<CancelDownloadBody>>,
) -> StatusCode {
    let post_action = body
        .as_ref()
        .and_then(|b| b.post_action.as_deref())
        .map(|s| {
            if s.eq_ignore_ascii_case("remove") {
                CancelPostAction::Remove
            } else {
                CancelPostAction::Ready
            }
        })
        .unwrap_or(CancelPostAction::Ready);
    let mut c = st.core.lock();
    c.request_cancel_item(id, post_action);
    StatusCode::OK
}

async fn downloads_redownload(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    match c.redownload_item_id(id) {
        Ok(()) => Ok(StatusCode::OK),
        Err(reason) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: reason.message().to_owned(),
            }),
        )),
    }
}

async fn downloads_retry_failed(
    State(st): State<ApiState>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    c.retry_failed_items()
        .map_err(|e: RetryFailedError| api_err(StatusCode::CONFLICT, e.message()))?;
    Ok(StatusCode::OK)
}

async fn downloads_refetch_failed(
    State(st): State<ApiState>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    c.refetch_failed_items()
        .map_err(|e: RefetchFailedError| api_err(StatusCode::CONFLICT, e.message()))?;
    Ok(StatusCode::OK)
}

async fn downloads_cancel_add(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.cancel_url_resolve_pipeline();
    StatusCode::OK
}

async fn settings_get(State(st): State<ApiState>) -> Json<SettingsResponse> {
    let c = st.core.lock();
    Json(settings_response_from_core(&c))
}

async fn settings_patch(
    State(st): State<ApiState>,
    Json(body): Json<PatchSettingsBody>,
) -> Result<Json<SettingsResponse>, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    if let Some(patch) = body.patch {
        c.merge_settings_patch(&patch)
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(ApiErrorBody { error: e })))?;
    } else if let Some(settings) = body.settings {
        c.apply_settings_patch(settings);
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: "settings or patch required".into(),
            }),
        ));
    }
    Ok(Json(settings_response_from_core(&c)))
}

async fn settings_layout_preset(
    State(st): State<ApiState>,
    Path(preset): Path<String>,
    body: Option<Json<LayoutPresetBody>>,
) -> Result<Json<SettingsResponse>, (StatusCode, Json<ApiErrorBody>)> {
    if !matches!(preset.as_str(), "compact" | "review" | "minimal") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: format!("unknown layout preset: {preset}"),
            }),
        ));
    }
    let viewport_height = body.map(|b| b.viewport_height).unwrap_or(None);
    let mut c = st.core.lock();
    let mut settings = c.settings.clone();
    crate::app_ui::apply_layout_preset(&mut settings, &preset, viewport_height);
    c.apply_settings_patch(settings);
    Ok(Json(settings_response_from_core(&c)))
}

async fn settings_organize_preset(
    State(st): State<ApiState>,
    Path(preset): Path<String>,
) -> Result<Json<SettingsResponse>, (StatusCode, Json<ApiErrorBody>)> {
    if !matches!(preset.as_str(), "flat" | "uploader" | "playlist" | "date") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiErrorBody {
                error: format!("unknown organize preset: {preset}"),
            }),
        ));
    }
    let mut c = st.core.lock();
    let mut settings = c.settings.clone();
    crate::download_organize::apply_organize_preset(&mut settings, &preset);
    c.apply_settings_patch(settings);
    Ok(Json(settings_response_from_core(&c)))
}

#[derive(Serialize, Deserialize)]
struct SettingsExportResponse {
    settings: AppSettings,
}

async fn settings_export(State(st): State<ApiState>) -> Json<SettingsExportResponse> {
    let c = st.core.lock();
    Json(SettingsExportResponse {
        settings: c.settings.clone(),
    })
}

async fn settings_import(
    State(st): State<ApiState>,
    Json(body): Json<SettingsExportResponse>,
) -> Result<Json<SettingsResponse>, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    c.apply_settings_patch(body.settings);
    Ok(Json(settings_response_from_core(&c)))
}

async fn settings_reset(
    State(st): State<ApiState>,
) -> Result<Json<SettingsResponse>, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    let keep_output = c.settings.output_dir.clone();
    let defaults = AppSettings {
        output_dir: keep_output,
        ..Default::default()
    };
    c.apply_settings_patch(defaults);
    Ok(Json(settings_response_from_core(&c)))
}

async fn open_output_folder(
    State(st): State<ApiState>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let c = st.core.lock();
    let dir = c.settings.output_dir.trim();
    if dir.is_empty() || !std::path::Path::new(dir).is_dir() {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "Output folder does not exist.",
        ));
    }
    crate::app_actions::open_str_path(dir)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(StatusCode::OK)
}

async fn web_ui_qr_png(
    State(st): State<ApiState>,
) -> Result<Response, (StatusCode, Json<ApiErrorBody>)> {
    use axum::http::header;
    use qrcode::render::svg;

    let c = st.core.lock();
    let token = c.settings.web_auth_token.trim();
    if token.is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "No API token configured."));
    }
    let url = super::web_ui_browser_url(&c.settings);
    let qr_target = format!("{}?token={}", url.trim_end_matches('/'), token);
    let code = qrcode::QrCode::new(qr_target.as_bytes()).map_err(|_| {
        api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not encode QR data.",
        )
    })?;
    let image = code.render::<svg::Color>().min_dimensions(256, 256).build();
    Ok(([(header::CONTENT_TYPE, "image/svg+xml")], image).into_response())
}

#[derive(Deserialize)]
struct BrowseRequest {
    #[serde(default = "default_browse_kind")]
    kind: String,
    #[serde(default)]
    title: Option<String>,
}

fn default_browse_kind() -> String {
    "folder".to_owned()
}

#[derive(Serialize)]
struct BrowseResponse {
    path: Option<String>,
    paths: Vec<String>,
}

async fn browse_host_path(Json(body): Json<BrowseRequest>) -> Json<BrowseResponse> {
    let kind = body.kind;
    let title = body
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| match kind.as_str() {
            "file" => "Select file".to_owned(),
            "files" => "Select files".to_owned(),
            _ => "Select folder".to_owned(),
        });
    let picked = tokio::task::spawn_blocking(move || match kind.as_str() {
        "file" => rfd::FileDialog::new()
            .set_title(&title)
            .pick_file()
            .map(|p| vec![p]),
        "files" => Some(
            rfd::FileDialog::new()
                .set_title(&title)
                .pick_files()
                .unwrap_or_default(),
        ),
        _ => rfd::FileDialog::new()
            .set_title(&title)
            .pick_folder()
            .map(|p| vec![p]),
    })
    .await
    .ok()
    .flatten();
    let paths: Vec<String> = picked
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    Json(BrowseResponse {
        path: paths.first().cloned(),
        paths,
    })
}

fn settings_response_from_core(c: &DownloadCore) -> SettingsResponse {
    let mut parts = vec![c.yt_dlp_bin(), "--newline".to_owned()];
    parts.push("-o".to_owned());
    parts.push(format!(
        "{}/{}",
        c.output_dir,
        output_filename_template(&c.settings)
    ));
    let ffmpeg = c.ffmpeg_bin();
    if !ffmpeg.is_empty() {
        parts.push("--ffmpeg-location".to_owned());
        parts.push(ffmpeg);
    }
    parts.extend(build_download_extra_args(&c.settings));
    parts.push("<url>".to_owned());
    SettingsResponse {
        settings: c.settings.clone(),
        command_preview: parts.join(" "),
        web_ui_browser_url: super::web_ui_browser_url(&c.settings),
    }
}

async fn logs_get(State(st): State<ApiState>) -> Json<LogsResponse> {
    let c = st.core.lock();
    Json(LogsResponse {
        lines: c.snapshot_logs(),
    })
}

async fn app_shutdown(State(st): State<ApiState>) -> StatusCode {
    let process_exit = st.process_exit.lock().take();
    let mut c = st.core.lock();
    c.request_app_shutdown(process_exit);
    StatusCode::ACCEPTED
}

async fn events_sse(
    State(st): State<ApiState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let rx = {
        let c = st.core.lock();
        c.subscribe_events()
    };
    let stream = BroadcastStream::new(rx).filter_map(|msg| {
        let ev = msg.ok()?;
        Some(Ok(Event::default().json_data(event_json(&ev)).ok()?))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

fn event_json(ev: &UiEvent) -> serde_json::Value {
    match ev {
        UiEvent::DownloadLine { item_id, line } => {
            serde_json::json!({"type":"download_line","item_id":item_id,"line":line})
        }
        UiEvent::DownloadDone {
            item_id,
            ok,
            detail,
        } => serde_json::json!({"type":"download_done","item_id":item_id,"ok":ok,"detail":detail}),
        UiEvent::AddResolved { source_line, .. } => {
            serde_json::json!({"type":"add_resolved","source_line":source_line})
        }
        UiEvent::AddProgress {
            processed,
            total,
            current,
        } => {
            serde_json::json!({"type":"add_progress","processed":processed,"total":total,"current":current})
        }
        UiEvent::AddDone => serde_json::json!({"type":"add_done"}),
        UiEvent::LogLine { line } => serde_json::json!({"type":"log","line":line}),
        UiEvent::ConvertLine { item_id, line } => {
            serde_json::json!({"type":"convert_line","item_id":item_id,"line":line})
        }
        UiEvent::ConvertDuration {
            item_id,
            duration_ms,
        } => {
            serde_json::json!({"type":"convert_duration","item_id":item_id,"duration_ms":duration_ms})
        }
        UiEvent::ConvertMediaProbed { item_id, .. } => {
            serde_json::json!({"type":"convert_media_probed","item_id":item_id})
        }
        UiEvent::ConvertDone {
            item_id,
            ok,
            detail,
            ..
        } => serde_json::json!({"type":"convert_done","item_id":item_id,"ok":ok,"detail":detail}),
        UiEvent::ConvertBatchDone => serde_json::json!({"type":"convert_batch_done"}),
        UiEvent::DownloadSessionComplete { done, failed } => {
            serde_json::json!({"type":"download_session_complete","done":done,"failed":failed})
        }
        UiEvent::ShutdownRequested => serde_json::json!({"type":"shutdown"}),
        _ => serde_json::json!({"type":"other"}),
    }
}

async fn thumbnail_proxy(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> Result<Response, StatusCode> {
    let (candidates, client, local_thumb, ffmpeg_path, has_ffmpeg, source_key, cached, core_ref) = {
        let mut c = st.core.lock();
        c.refresh_done_file_lookup();
        if !c.has_ffmpeg {
            c.refresh_deps();
        }
        let idx = c.item_idx(id).ok_or(StatusCode::NOT_FOUND)?;
        let output_dir = c.effective_output_dir();
        let index = &c.done_file_index;
        let ffmpeg_path = c.settings.ffmpeg_path.clone();
        let has_ffmpeg = c.has_ffmpeg;
        let item = c.items[idx].clone();
        let source_key = DownloadCore::queue_thumbnail_source_key(&item);
        let cached = c.cached_thumbnail_bytes(id, &source_key);
        let urls = thumbnail_url_candidates(&item);
        let local_media = media::resolve_item_media_path_from_index(&output_dir, index, &item)
            .ok()
            .filter(|p| media::media_kind_for_path(p).is_some());
        (
            urls,
            c.http_client.clone(),
            local_media,
            ffmpeg_path,
            has_ffmpeg,
            source_key,
            cached,
            st.core.clone(),
        )
    };
    if let Some((bytes, content_type)) = cached {
        return Ok(thumbnail_response_owned(bytes, content_type));
    }
    if let Some(path) = local_thumb {
        if let Some(bytes) = extract_local_video_thumbnail(&path, &ffmpeg_path, has_ffmpeg).await {
            {
                let mut c = core_ref.lock();
                c.cache_thumbnail_bytes(id, source_key.clone(), bytes.clone(), "image/png");
            }
            return Ok(thumbnail_response(bytes, "image/png"));
        }
    }
    for url in &candidates {
        if let Some((bytes, content_type)) = ytdlp::fetch_thumbnail_bytes(&client, url).await {
            {
                let mut c = core_ref.lock();
                c.cache_thumbnail_bytes(
                    id,
                    source_key.clone(),
                    bytes.clone(),
                    content_type.clone(),
                );
            }
            return Ok(thumbnail_response_owned(bytes, content_type));
        }
    }
    Err(StatusCode::NOT_FOUND)
}

pub(super) async fn extract_local_video_thumbnail(
    path: &std::path::Path,
    ffmpeg_path: &str,
    has_ffmpeg: bool,
) -> Option<Vec<u8>> {
    if !has_ffmpeg {
        return None;
    }
    let path = path.to_path_buf();
    let ffmpeg_path = ffmpeg_path.to_owned();
    tokio::task::spawn_blocking(move || {
        crate::transcode::extract_thumbnail_png_bytes(&path, &ffmpeg_path)
    })
    .await
    .ok()
    .flatten()
}

pub(super) fn thumbnail_response(bytes: Vec<u8>, content_type: &'static str) -> Response {
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, content_type),
            (axum::http::header::CACHE_CONTROL, "private, max-age=300"),
        ],
        bytes,
    )
        .into_response()
}

pub(super) fn thumbnail_response_owned(bytes: Vec<u8>, content_type: String) -> Response {
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, content_type),
            (
                axum::http::header::CACHE_CONTROL,
                "private, max-age=300".to_owned(),
            ),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tokio::runtime::Runtime;
    use tower::ServiceExt;

    use crate::service::core::DownloadCore;

    use super::{api_router, ApiState, SettingsResponse};

    fn test_state(rt: Arc<Runtime>) -> ApiState {
        let (core, _rx) = DownloadCore::new_shared(rt, true);
        {
            let mut c = core.lock();
            c.settings.web_auth_token = "test-token".to_owned();
            c.items.clear();
            c.rebuild_item_index();
            c.update_status();
        }
        ApiState::new(core)
    }

    #[test]
    fn queue_list_requires_token() {
        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        rt.block_on(async move {
            let app = api_router(state);
            let response = app
                .oneshot(
                    Request::builder()
                        .uri("/api/queue")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        });
    }

    #[test]
    fn queue_list_with_token() {
        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        rt.block_on(async move {
            let app = api_router(state);
            let response = app
                .oneshot(
                    Request::builder()
                        .uri("/api/queue")
                        .header("Authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        });
    }

    #[test]
    fn queue_list_with_whitelisted_ip() {
        use std::net::SocketAddr;

        use axum::extract::ConnectInfo;

        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        {
            let mut c = state.core.lock();
            c.settings.web_auth_ip_whitelist = vec!["127.0.0.1".to_owned()];
        }
        rt.block_on(async move {
            let app = api_router(state);
            let mut request = Request::builder()
                .uri("/api/queue")
                .body(Body::empty())
                .unwrap();
            request
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));
            let response = app.oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        });
    }

    #[test]
    fn queue_reorder_rejects_invalid() {
        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        rt.block_on(async move {
            let app = api_router(state);
            let body = Body::from(r#"{"dragged_id":1,"target_id":2}"#);
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/queue/reorder")
                        .header("Authorization", "Bearer test-token")
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        });
    }

    #[test]
    fn settings_merge_patch() {
        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        rt.block_on(async move {
            let app = api_router(state.clone());
            let body = Body::from(r#"{"patch":{"auto_start_downloads":false}}"#);
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/settings")
                        .header("Authorization", "Bearer test-token")
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let parsed: SettingsResponse = serde_json::from_slice(&body).unwrap();
            assert!(!parsed.settings.auto_start_downloads);
            assert!(!parsed.command_preview.is_empty());
            let c = state.core.lock();
            assert!(!c.settings.auto_start_downloads);
        });
    }

    #[test]
    fn settings_patch_returns_settings_json() {
        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        rt.block_on(async move {
            let app = api_router(state.clone());
            let body = Body::from(r#"{"settings":{"auto_start_downloads":true}}"#);
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/settings")
                        .header("Authorization", "Bearer test-token")
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let parsed: SettingsResponse = serde_json::from_slice(&bytes).unwrap();
            assert!(parsed.settings.auto_start_downloads);
            assert!(
                parsed.command_preview.contains("yt-dlp")
                    || parsed.command_preview.contains("<url>")
            );
        });
    }

    #[test]
    fn settings_patch_normalizes_invalid_enum_fields() {
        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        rt.block_on(async move {
            let app = api_router(state.clone());
            let body = Body::from(
                r#"{"settings":{"convert_subtitle_mode":"bogus","convert_audio_extract":"wav","ui_scale":1.07}}"#,
            );
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/settings")
                        .header("Authorization", "Bearer test-token")
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let parsed: SettingsResponse = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(parsed.settings.convert_subtitle_mode, "none");
            assert_eq!(parsed.settings.convert_audio_extract, "none");
            assert!((parsed.settings.ui_scale - 1.05).abs() < f32::EPSILON);
            let c = state.core.lock();
            assert_eq!(c.settings.convert_subtitle_mode, "none");
            assert_eq!(c.settings.convert_audio_extract, "none");
            assert!((c.settings.ui_scale - 1.05).abs() < f32::EPSILON);
        });
    }

    #[test]
    fn cookie_check_without_cookies() {
        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        rt.block_on(async move {
            let app = api_router(state);
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/tools/cookie-check")
                        .header("Authorization", "Bearer test-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        });
    }

    #[test]
    fn playlist_preview_rejects_blank_url() {
        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        rt.block_on(async move {
            let app = api_router(state);
            let body = Body::from(r#"{"url":"   "}"#);
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/queue/playlist-preview")
                        .header("Authorization", "Bearer test-token")
                        .header("Content-Type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        });
    }

    #[test]
    fn queue_overrides_schedules_debounced_save() {
        use std::time::{Duration, Instant};

        use crate::models::{ItemStatus, QueueItem};

        let rt = Arc::new(Runtime::new().expect("runtime"));
        let state = test_state(rt.clone());
        {
            let mut c = state.core.lock();
            c.items.push(QueueItem {
                item_id: 1,
                status: ItemStatus::Idle,
                ..Default::default()
            });
            c.rebuild_item_index();
            c.update_status();
        }
        rt.block_on(async {
            let app = api_router(state.clone());
            let body = Body::from(r#"{"format_override":"best"}"#);
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/queue/1/overrides")
                        .header("Authorization", "Bearer test-token")
                        .header("content-type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        });
        let mut c = state.core.lock();
        assert!(c.queue_save_deadline.is_some());
        c.queue_save_deadline = Some(Instant::now() - Duration::from_millis(1));
        c.maybe_flush_queue_save();
        assert!(c.queue_save_deadline.is_none());
    }
}
