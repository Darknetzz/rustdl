//! Web REST routes for the video converter. State lives on the shared `DownloadCore`, so these
//! endpoints work identically in windowed and `--web-only` (headless) modes.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::convert_state::{
    compute_convert_batch_progress, compute_convert_batch_summary, convert_item_is_skipped,
    convert_item_open_targets, convert_item_playable_kind, convert_item_playable_path,
    convert_item_status_label, convert_item_will_skip_already_target,
};
use crate::models::ConvertQueueItem;
use crate::transcode::{encoder_indicator_label, encoder_uses_hardware, target_codec_label};

use super::api::{extract_local_video_thumbnail, thumbnail_response, ApiErrorBody, ApiState, BatchProgressJson};

#[derive(Serialize)]
struct ConvertItemView {
    #[serde(flatten)]
    item: ConvertQueueItem,
    status_label: &'static str,
    skipped: bool,
    will_skip_target: bool,
    probing: bool,
    can_open_file: bool,
    can_open_folder: bool,
    playable: bool,
    media_kind: Option<String>,
    media_filename: Option<String>,
}

#[derive(Serialize)]
struct ConvertEncoderJson {
    label: String,
    kind: &'static str,
    encoder: String,
}

#[derive(Serialize)]
struct ConvertSummaryJson {
    completed: usize,
    completed_input_bytes: u64,
    completed_output_bytes: u64,
    pending_count: usize,
    pending_input_bytes: u64,
}

#[derive(Serialize)]
struct ConvertQueueResponse {
    items: Vec<ConvertItemView>,
    running: bool,
    paused: bool,
    input_paths: String,
    encoder: Option<ConvertEncoderJson>,
    has_ffmpeg: bool,
    has_ffprobe: bool,
    target_codec: String,
    reencode_target: bool,
    summary: ConvertSummaryJson,
    batch_progress: BatchProgressJson,
}

#[derive(Deserialize)]
struct ConvertScanBody {
    paths: Vec<String>,
}

/// Adds the converter routes to the (still unprotected) router so `api_router` can apply auth to them.
pub(super) fn register(router: Router<ApiState>) -> Router<ApiState> {
    router
        .route("/api/convert/queue", get(convert_queue))
        .route("/api/convert/scan", post(convert_scan))
        .route("/api/convert/start", post(convert_start))
        .route("/api/convert/cancel", post(convert_cancel))
        .route("/api/convert/pause", post(convert_pause))
        .route("/api/convert/resume", post(convert_resume))
        .route("/api/convert/clear", post(convert_clear))
        .route("/api/convert/retry-skipped", post(convert_retry_skipped))
        .route(
            "/api/convert/fallback-software",
            post(convert_fallback_software),
        )
        .route("/api/convert/presets", get(convert_presets_list))
        .route("/api/convert/presets/apply", post(convert_presets_apply))
        .route("/api/convert/export-summary", get(convert_export_summary))
        .route("/api/convert/thumbnail/:id", get(convert_thumbnail))
        .route("/api/convert/media/:id", get(convert_media))
        .route("/api/convert/:id/open", post(convert_open))
}

async fn convert_queue(State(st): State<ApiState>) -> Json<ConvertQueueResponse> {
    let mut c = st.core.lock();
    if !c.has_ffmpeg {
        c.refresh_deps();
    }
    c.refresh_convert_encoder_detection();
    let summary = compute_convert_batch_summary(&c.convert_items);
    let target_codec = c.settings.convert_target_codec.clone();
    let reencode_target = c.settings.convert_reencode_target;
    let items = c
        .convert_items
        .iter()
        .map(|item| {
            let targets = convert_item_open_targets(item);
            let media_path = convert_item_playable_path(item);
            let media_filename = media_path
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map(str::to_owned);
            ConvertItemView {
                status_label: convert_item_status_label(item),
                skipped: convert_item_is_skipped(item),
                will_skip_target: convert_item_will_skip_already_target(
                    item,
                    reencode_target,
                    &target_codec,
                ),
                probing: c.convert_media_inflight.contains(&item.item_id),
                can_open_file: targets.file.is_some(),
                can_open_folder: targets.folder.is_some(),
                playable: media_path.is_some(),
                media_kind: convert_item_playable_kind(item).map(str::to_owned),
                media_filename,
                item: item.clone(),
            }
        })
        .collect();
    let encoder = c
        .convert_encoder_choice
        .as_ref()
        .map(|enc| ConvertEncoderJson {
            label: encoder_indicator_label(enc),
            kind: if encoder_uses_hardware(enc) {
                "gpu"
            } else {
                "cpu"
            },
            encoder: enc.encoder.to_owned(),
        });
    Json(ConvertQueueResponse {
        items,
        running: c.convert_running,
        paused: c.convert_paused,
        input_paths: c.convert_input_paths.clone(),
        encoder,
        has_ffmpeg: c.has_ffmpeg,
        has_ffprobe: c.has_ffprobe,
        target_codec: target_codec_label(&target_codec).to_owned(),
        reencode_target,
        summary: ConvertSummaryJson {
            completed: summary.completed,
            completed_input_bytes: summary.completed_input_bytes,
            completed_output_bytes: summary.completed_output_bytes,
            pending_count: summary.pending_count,
            pending_input_bytes: summary.pending_input_bytes,
        },
        batch_progress: compute_convert_batch_progress(&c.convert_items).into(),
    })
}

async fn convert_scan(State(st): State<ApiState>, Json(body): Json<ConvertScanBody>) -> StatusCode {
    let lines: Vec<String> = body
        .paths
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    let mut c = st.core.lock();
    c.convert_input_paths = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    if !lines.is_empty() {
        c.scan_convert_paths_into_queue(&lines);
    } else {
        c.bump_generation();
    }
    StatusCode::OK
}

async fn convert_start(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.start_convert_batch();
    StatusCode::OK
}

async fn convert_cancel(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.cancel_convert_batch();
    StatusCode::OK
}

async fn convert_pause(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.pause_convert_batch();
    StatusCode::OK
}

async fn convert_resume(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.resume_convert_batch();
    StatusCode::OK
}

async fn convert_clear(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.clear_convert_queue();
    StatusCode::OK
}

async fn convert_retry_skipped(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.retry_skipped_convert_items();
    StatusCode::OK
}

async fn convert_fallback_software(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.fallback_convert_encoder_to_software();
    StatusCode::OK
}

#[derive(Deserialize)]
struct ConvertPresetApplyBody {
    name: String,
}

async fn convert_presets_list(_st: State<ApiState>) -> Json<serde_json::Value> {
    let store = crate::convert_presets::load_convert_presets();
    let mut names: Vec<String> = crate::convert_presets::builtin_convert_presets()
        .into_iter()
        .map(|p| p.name)
        .collect();
    for p in store.presets {
        if !names.iter().any(|n| n == &p.name) {
            names.push(p.name);
        }
    }
    Json(serde_json::json!({ "presets": names }))
}

async fn convert_presets_apply(
    State(st): State<ApiState>,
    Json(body): Json<ConvertPresetApplyBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let name = body.name.trim();
    let preset = crate::convert_presets::builtin_convert_presets()
        .into_iter()
        .find(|p| p.name == name)
        .or_else(|| {
            crate::convert_presets::load_convert_presets()
                .presets
                .into_iter()
                .find(|p| p.name == name)
        })
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(ApiErrorBody {
                error: "preset not found".to_owned(),
            }),
        ))?;
    let mut c = st.core.lock();
    preset.fields.apply_to(&mut c.settings);
    c.persist_settings();
    c.bump_generation();
    Ok(StatusCode::OK)
}

async fn convert_export_summary(State(st): State<ApiState>) -> impl IntoResponse {
    let c = st.core.lock();
    let mut lines =
        vec!["item_id,source_path,output_path,status,input_bytes,output_bytes".to_owned()];
    for it in &c.convert_items {
        lines.push(format!(
            "{},{},{},{},{},{}",
            it.item_id,
            csv_escape(&it.source_path),
            csv_escape(&it.output_path),
            it.status.as_str(),
            it.input_bytes,
            it.output_bytes.unwrap_or(0),
        ));
    }
    (
        [(axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8")],
        lines.join("\n"),
    )
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

#[derive(Deserialize)]
struct ConvertOpenBody {
    target: String,
}

async fn convert_open(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
    Json(body): Json<ConvertOpenBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let path = {
        let c = st.core.lock();
        let item = c
            .convert_items
            .iter()
            .find(|it| it.item_id == id)
            .ok_or((
                StatusCode::NOT_FOUND,
                Json(ApiErrorBody {
                    error: "convert item not found".to_owned(),
                }),
            ))?;
        let targets = convert_item_open_targets(item);
        match body.target.trim() {
            "file" => targets.file.ok_or((
                StatusCode::NOT_FOUND,
                Json(ApiErrorBody {
                    error: "no file on disk for this row".to_owned(),
                }),
            ))?,
            "folder" => {
                if let Some(file) = targets.file {
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
                } else {
                    targets.folder.ok_or((
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

async fn convert_media(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
    headers: HeaderMap,
) -> Result<axum::response::Response, StatusCode> {
    let path = {
        let c = st.core.lock();
        let item = c
            .convert_items
            .iter()
            .find(|it| it.item_id == id)
            .ok_or(StatusCode::NOT_FOUND)?;
        convert_item_playable_path(item).ok_or(StatusCode::NOT_FOUND)?
    };
    super::media::stream_media_path(&path, &headers).await
}

async fn convert_thumbnail(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> Result<axum::response::Response, StatusCode> {
    let (source_path, ffmpeg_path, has_ffmpeg, source_key, cached, core_ref) = {
        let mut c = st.core.lock();
        if !c.has_ffmpeg {
            c.refresh_deps();
        }
        let item = c
            .convert_items
            .iter()
            .find(|it| it.item_id == id)
            .ok_or(StatusCode::NOT_FOUND)?;
        let source_key =
            crate::service::core::DownloadCore::convert_thumbnail_source_key(&item.source_path);
        let cached = c.cached_thumbnail_bytes(id, &source_key);
        (
            std::path::PathBuf::from(&item.source_path),
            c.settings.ffmpeg_path.clone(),
            c.has_ffmpeg,
            source_key,
            cached,
            st.core.clone(),
        )
    };
    if let Some((bytes, content_type)) = cached {
        return Ok(super::api::thumbnail_response_owned(bytes, content_type));
    }
    match extract_local_video_thumbnail(&source_path, &ffmpeg_path, has_ffmpeg).await {
        Some(bytes) => {
            {
                let mut c = core_ref.lock();
                c.cache_thumbnail_bytes(id, source_key, bytes.clone(), "image/png");
            }
            Ok(thumbnail_response(bytes, "image/png"))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}
