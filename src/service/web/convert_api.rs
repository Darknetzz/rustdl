//! Web REST routes for the video converter. State lives on the shared `DownloadCore`, so these
//! endpoints work identically in windowed and `--web-only` (headless) modes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::convert_state::{
    compute_convert_batch_progress, compute_convert_batch_summary, convert_item_is_skipped,
    convert_item_status_label, convert_item_will_skip_already_target,
};
use crate::models::ConvertQueueItem;
use crate::transcode::{encoder_indicator_label, encoder_uses_hardware, target_codec_label};

use super::api::{extract_local_video_thumbnail, thumbnail_response, ApiState, BatchProgressJson};

#[derive(Serialize)]
struct ConvertItemView {
    #[serde(flatten)]
    item: ConvertQueueItem,
    status_label: &'static str,
    skipped: bool,
    will_skip_target: bool,
    probing: bool,
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
        .route("/api/convert/clear", post(convert_clear))
        .route("/api/convert/retry-skipped", post(convert_retry_skipped))
        .route("/api/convert/thumbnail/{id}", get(convert_thumbnail))
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
        .map(|item| ConvertItemView {
            status_label: convert_item_status_label(item),
            skipped: convert_item_is_skipped(item),
            will_skip_target: convert_item_will_skip_already_target(
                item,
                reencode_target,
                &target_codec,
            ),
            probing: c.convert_media_inflight.contains(&item.item_id),
            item: item.clone(),
        })
        .collect();
    let encoder = c.convert_encoder_choice.as_ref().map(|enc| ConvertEncoderJson {
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
