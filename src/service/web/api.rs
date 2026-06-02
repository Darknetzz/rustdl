use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::app::UiEvent;
use crate::config::AppSettings;
use crate::models::QueueItem;
use crate::profiles::{all_profiles, find_profile};
use crate::service::core::{CancelPostAction, QueueClearFilter, SharedCore};
use crate::service::web::media;
use crate::ytdlp::{self, thumbnail_url_candidates};
use crate::ytdlp_download_args::{build_download_extra_args, output_filename_template};

use super::assets;
use super::auth;

#[derive(Clone)]
pub struct ApiState {
    pub core: SharedCore,
}

fn expected_token(st: &ApiState) -> String {
    st.core.lock().settings.web_auth_token.clone()
}

#[derive(Serialize)]
struct StatusResponse {
    version: &'static str,
    downloads_paused: bool,
    queue_running: usize,
    add_in_progress: bool,
    auto_add_pasted_urls: bool,
    auto_start_downloads: bool,
    status: StatusCountsJson,
    tools: serde_json::Value,
}

#[derive(Serialize)]
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
    items: Vec<QueueItemView>,
}

#[derive(Serialize)]
struct ApiErrorBody {
    error: String,
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
    settings: AppSettings,
}

#[derive(Serialize)]
struct SettingsResponse {
    settings: AppSettings,
    command_preview: String,
}

#[derive(Serialize)]
struct LogsResponse {
    lines: Vec<String>,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
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

pub fn api_router(state: ApiState) -> Router {
    let protected = Router::new()
        .route("/api/status", get(status))
        .route("/api/queue", get(queue_list))
        .route("/api/queue", post(queue_add))
        .route("/api/queue/{id}", axum::routing::delete(queue_remove))
        .route("/api/queue/clear", post(queue_clear))
        .route("/api/queue/{id}/file", axum::routing::delete(queue_delete_file))
        .route("/api/logs/clear", post(logs_clear))
        .route("/api/downloads/start", post(downloads_start))
        .route("/api/downloads/pause", post(downloads_pause))
        .route("/api/downloads/resume", post(downloads_resume))
        .route("/api/downloads/cancel/{id}", post(downloads_cancel))
        .route("/api/downloads/redownload/{id}", post(downloads_redownload))
        .route("/api/settings", get(settings_get))
        .route("/api/settings", post(settings_patch))
        .route("/api/profiles", get(profiles_list))
        .route("/api/profiles/apply", post(profiles_apply))
        .route("/api/tools/refresh", post(tools_refresh))
        .route("/api/logs", get(logs_get))
        .route("/api/events", get(events_sse))
        .route("/api/thumbnail/{id}", get(thumbnail_proxy))
        .route("/api/media/{id}", get(media_stream))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            |State(st): State<ApiState>, req, next| async move {
                let expected = expected_token(&st);
                auth::require_token(expected, req, next).await
            },
        ))
        .with_state(state.clone());

    Router::new()
        .merge(protected)
        .fallback_service(assets::static_router())
}

async fn status(State(st): State<ApiState>) -> Json<StatusResponse> {
    let c = st.core.lock();
    Json(StatusResponse {
        version: crate::pkg_version::VERSION,
        downloads_paused: c.downloads_paused,
        queue_running: c.queue_running,
        add_in_progress: c.add_in_progress,
        auto_add_pasted_urls: c.settings.auto_add_pasted_urls,
        auto_start_downloads: c.settings.auto_start_downloads,
        status: StatusCountsJson {
            resolving: c.status_resolving,
            ready: c.status_ready,
            queued: c.status_queued,
            active: c.status_active,
            done: c.status_done,
            failed: c.status_failed,
        },
        tools: c.tools_status_json(),
    })
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
    let profile = find_profile(&c.profile_store, body.name.trim())
        .ok_or(StatusCode::NOT_FOUND)?;
    profile.apply_to(&mut c.settings);
    c.settings.active_profile = profile.name.clone();
    c.output_dir = c.settings.output_dir.clone();
    c.worker_count = c.settings.worker_count.clamp(1, 6);
    c.persist_settings();
    c.refresh_deps();
    c.bump_generation();
    Ok(StatusCode::OK)
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
    Json(QueueResponse { items })
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

async fn queue_delete_file(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> StatusCode {
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

async fn downloads_start(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.start_downloads();
    StatusCode::OK
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
) -> StatusCode {
    let mut c = st.core.lock();
    c.request_cancel_item(id, CancelPostAction::Ready);
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

async fn settings_get(State(st): State<ApiState>) -> Json<SettingsResponse> {
    let c = st.core.lock();
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
    Json(SettingsResponse {
        settings: c.settings.clone(),
        command_preview: parts.join(" "),
    })
}

async fn settings_patch(
    State(st): State<ApiState>,
    Json(body): Json<PatchSettingsBody>,
) -> StatusCode {
    let mut c = st.core.lock();
    c.apply_settings_patch(body.settings);
    StatusCode::OK
}

async fn logs_get(State(st): State<ApiState>) -> Json<LogsResponse> {
    let c = st.core.lock();
    Json(LogsResponse {
        lines: c.snapshot_logs(),
    })
}

async fn events_sse(
    State(st): State<ApiState>,
    Query(q): Query<TokenQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let expected = expected_token(&st);
    let ok = auth::token_matches(&expected, q.token.as_deref())
        || headers
            .get(auth::AUTH_HEADER)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| auth::token_matches(&expected, Some(t)));
    if !ok {
        return Err(StatusCode::UNAUTHORIZED);
    }
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
        } => serde_json::json!({"type":"add_progress","processed":processed,"total":total,"current":current}),
        UiEvent::AddDone => serde_json::json!({"type":"add_done"}),
        UiEvent::LogLine { line } => serde_json::json!({"type":"log","line":line}),
        _ => serde_json::json!({"type":"other"}),
    }
}

async fn thumbnail_proxy(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, StatusCode> {
    let (candidates, client, local_thumb, ffmpeg_path, has_ffmpeg) = {
        let mut c = st.core.lock();
        c.refresh_done_file_lookup();
        if !c.has_ffmpeg {
            c.refresh_deps();
        }
        let idx = c.item_idx(id).ok_or(StatusCode::NOT_FOUND)?;
        let output_dir = c.output_dir.clone();
        let index = &c.done_file_index;
        let ffmpeg_path = c.settings.ffmpeg_path.clone();
        let has_ffmpeg = c.has_ffmpeg;
        let item = c.items[idx].clone();
        let urls = thumbnail_url_candidates(&item);
        let local_media = media::resolve_item_media_path_from_index(&output_dir, index, &item)
            .ok()
            .filter(|p| media::media_kind_for_path(p).is_some());
        (urls, c.http_client.clone(), local_media, ffmpeg_path, has_ffmpeg)
    };
    if let Some(path) = local_thumb.as_ref() {
        if let Some(bytes) = extract_local_video_thumbnail(path, &ffmpeg_path, has_ffmpeg).await {
            return Ok(thumbnail_response(bytes, "image/png"));
        }
    }
    for url in candidates {
        if let Some((bytes, content_type)) = fetch_thumbnail_image(&client, &url).await {
            return Ok(thumbnail_response(bytes, content_type));
        }
    }
    if let Some(path) = local_thumb {
        if let Some(bytes) = extract_local_video_thumbnail(&path, &ffmpeg_path, has_ffmpeg).await {
            return Ok(thumbnail_response(bytes, "image/png"));
        }
    }
    Err(StatusCode::NOT_FOUND)
}

async fn extract_local_video_thumbnail(
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
        crate::av1_transcode::extract_thumbnail_png_bytes(&path, &ffmpeg_path)
    })
    .await
    .ok()
    .flatten()
}

fn thumbnail_response(bytes: Vec<u8>, content_type: &'static str) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, content_type)],
        bytes,
    )
}

async fn fetch_thumbnail_image(
    client: &reqwest::Client,
    url: &str,
) -> Option<(Vec<u8>, &'static str)> {
    let mut req = client.get(url);
    if ytdlp::thumbnail_request_needs_referer(url) {
        req = req.header(axum::http::header::REFERER, "https://www.youtube.com/");
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let mime = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = resp.bytes().await.ok()?.to_vec();
    if bytes.len() < 32 {
        return None;
    }
    let mime_is_image = mime
        .as_deref()
        .is_some_and(|m| m.to_ascii_lowercase().starts_with("image/"));
    if !mime_is_image && !looks_like_image_bytes(&bytes) {
        return None;
    }
    let content_type = mime
        .as_deref()
        .and_then(content_type_from_mime)
        .unwrap_or_else(|| content_type_from_bytes(&bytes));
    Some((bytes, content_type))
}

fn looks_like_image_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || is_webp(bytes)
}

fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

fn content_type_from_mime(mime: &str) -> Option<&'static str> {
    let m = mime.split(';').next()?.trim().to_ascii_lowercase();
    match m.as_str() {
        "image/png" => Some("image/png"),
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        _ => None,
    }
}

fn content_type_from_bytes(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(b"GIF") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") {
        "image/webp"
    } else {
        "image/jpeg"
    }
}
