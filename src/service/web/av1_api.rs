//! Web REST routes for the AV1 converter. State lives on the shared `DownloadCore`, so these
//! endpoints work identically in windowed and `--web-only` (headless) modes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::av1_state::{
    av1_item_is_skipped, av1_item_status_label, av1_item_will_skip_already_av1,
    compute_av1_batch_summary,
};
use crate::av1_transcode::{encoder_indicator_label, encoder_uses_hardware};
use crate::models::Av1QueueItem;

use super::api::{extract_local_video_thumbnail, thumbnail_response, ApiState};

#[derive(Serialize)]
struct Av1ItemView {
    #[serde(flatten)]
    item: Av1QueueItem,
    status_label: &'static str,
    skipped: bool,
    will_skip_av1: bool,
    probing: bool,
}

#[derive(Serialize)]
struct Av1EncoderJson {
    label: String,
    kind: &'static str,
    encoder: String,
}

#[derive(Serialize)]
struct Av1SummaryJson {
    completed: usize,
    completed_input_bytes: u64,
    completed_output_bytes: u64,
    pending_count: usize,
    pending_input_bytes: u64,
}

#[derive(Serialize)]
struct Av1QueueResponse {
    items: Vec<Av1ItemView>,
    running: bool,
    input_paths: String,
    encoder: Option<Av1EncoderJson>,
    has_ffmpeg: bool,
    has_ffprobe: bool,
    reencode_av1: bool,
    summary: Av1SummaryJson,
}

#[derive(Deserialize)]
struct Av1ScanBody {
    paths: Vec<String>,
}

/// Adds the AV1 routes to the (still unprotected) router so `api_router` can apply auth to them.
pub(super) fn register(router: Router<ApiState>) -> Router<ApiState> {
    router
        .route("/api/av1/queue", get(av1_queue))
        .route("/api/av1/scan", post(av1_scan))
        .route("/api/av1/start", post(av1_start))
        .route("/api/av1/cancel", post(av1_cancel))
        .route("/api/av1/clear", post(av1_clear))
        .route("/api/av1/thumbnail/{id}", get(av1_thumbnail))
}

async fn av1_queue(State(st): State<ApiState>) -> Json<Av1QueueResponse> {
    let mut c = st.core.lock();
    if !c.has_ffmpeg {
        c.refresh_deps();
    }
    c.refresh_av1_encoder_detection();
    let summary = compute_av1_batch_summary(&c.av1_items);
    let output_codec = c
        .av1_encoder_choice
        .as_ref()
        .map(|enc| enc.codec)
        .unwrap_or("av1");
    let reencode_av1 = c.settings.av1_reencode_av1;
    let items = c
        .av1_items
        .iter()
        .map(|item| Av1ItemView {
            status_label: av1_item_status_label(item),
            skipped: av1_item_is_skipped(item),
            will_skip_av1: av1_item_will_skip_already_av1(item, reencode_av1, output_codec),
            probing: c.av1_media_inflight.contains(&item.item_id),
            item: item.clone(),
        })
        .collect();
    let encoder = c.av1_encoder_choice.as_ref().map(|enc| Av1EncoderJson {
        label: encoder_indicator_label(enc),
        kind: if encoder_uses_hardware(enc) {
            "gpu"
        } else {
            "cpu"
        },
        encoder: enc.encoder.to_owned(),
    });
    Json(Av1QueueResponse {
        items,
        running: c.av1_running,
        input_paths: c.av1_input_paths.clone(),
        encoder,
        has_ffmpeg: c.has_ffmpeg,
        has_ffprobe: c.has_ffprobe,
        reencode_av1,
        summary: Av1SummaryJson {
            completed: summary.completed,
            completed_input_bytes: summary.completed_input_bytes,
            completed_output_bytes: summary.completed_output_bytes,
            pending_count: summary.pending_count,
            pending_input_bytes: summary.pending_input_bytes,
        },
    })
}

async fn av1_scan(State(st): State<ApiState>, Json(body): Json<Av1ScanBody>) -> StatusCode {
    let lines: Vec<String> = body
        .paths
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    let mut c = st.core.lock();
    // Mirror the web textarea into the core, then scan (which trims the lines it consumes).
    c.av1_input_paths = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    if !lines.is_empty() {
        c.scan_av1_paths_into_queue(&lines);
    } else {
        c.bump_generation();
    }
    StatusCode::OK
}

async fn av1_start(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.start_av1_batch();
    StatusCode::OK
}

async fn av1_cancel(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.cancel_av1_batch();
    StatusCode::OK
}

async fn av1_clear(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.clear_av1_queue();
    StatusCode::OK
}

async fn av1_thumbnail(
    State(st): State<ApiState>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, StatusCode> {
    let (source_path, ffmpeg_path, has_ffmpeg) = {
        let mut c = st.core.lock();
        if !c.has_ffmpeg {
            c.refresh_deps();
        }
        let item = c
            .av1_items
            .iter()
            .find(|it| it.item_id == id)
            .ok_or(StatusCode::NOT_FOUND)?;
        (
            std::path::PathBuf::from(&item.source_path),
            c.settings.ffmpeg_path.clone(),
            c.has_ffmpeg,
        )
    };
    match extract_local_video_thumbnail(&source_path, &ffmpeg_path, has_ffmpeg).await {
        Some(bytes) => Ok(thumbnail_response(bytes, "image/png")),
        None => Err(StatusCode::NOT_FOUND),
    }
}
