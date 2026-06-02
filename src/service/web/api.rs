use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
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
use crate::service::core::{CancelPostAction, SharedCore};
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
struct QueueResponse {
    items: Vec<QueueItem>,
}

#[derive(Deserialize)]
struct AddUrlsBody {
    urls: Vec<String>,
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
        .route("/api/downloads/start", post(downloads_start))
        .route("/api/downloads/pause", post(downloads_pause))
        .route("/api/downloads/resume", post(downloads_resume))
        .route("/api/downloads/cancel/{id}", post(downloads_cancel))
        .route("/api/settings", get(settings_get))
        .route("/api/settings", post(settings_patch))
        .route("/api/profiles", get(profiles_list))
        .route("/api/profiles/apply", post(profiles_apply))
        .route("/api/tools/refresh", post(tools_refresh))
        .route("/api/logs", get(logs_get))
        .route("/api/events", get(events_sse))
        .route("/api/thumbnail/{id}", get(thumbnail_proxy))
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
    let c = st.core.lock();
    Json(QueueResponse {
        items: c.snapshot_queue(),
    })
}

async fn queue_add(
    State(st): State<ApiState>,
    Json(body): Json<AddUrlsBody>,
) -> Result<StatusCode, StatusCode> {
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
    c.queue_urls_for_resolve(lines);
    Ok(StatusCode::ACCEPTED)
}

async fn queue_remove(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> StatusCode {
    let mut c = st.core.lock();
    if c.remove_item_by_id(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
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
    let (url, client) = {
        let c = st.core.lock();
        let idx = c.item_idx(id).ok_or(StatusCode::NOT_FOUND)?;
        let item = &c.items[idx];
        let url = item.thumbnail_url.clone().ok_or(StatusCode::NOT_FOUND)?;
        (url, c.http_client.clone())
    };
    let resp = client.get(&url).send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_owned();
    let bytes = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok((
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK),
        [(axum::http::header::CONTENT_TYPE, content_type)],
        bytes,
    ))
}
