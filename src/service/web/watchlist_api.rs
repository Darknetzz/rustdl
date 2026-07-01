//! Quality watchlist REST routes for the LAN web UI.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};

use crate::watchlist::WatchlistEntry;

use super::api::{api_err, ApiErrorBody, ApiState};

#[derive(Serialize)]
struct WatchlistResponse {
    generation: u64,
    entries: Vec<WatchlistEntry>,
}

#[derive(Deserialize)]
struct WatchlistAddBody {
    url: String,
}

#[derive(Deserialize)]
struct WatchlistPauseBody {
    paused: bool,
}

#[derive(Serialize)]
struct WatchlistAddResponse {
    entry_id: u64,
}

async fn watchlist_list(State(st): State<ApiState>) -> Json<WatchlistResponse> {
    let c = st.core.lock();
    Json(WatchlistResponse {
        generation: c.watchlist_generation,
        entries: c.watchlist.entries.clone(),
    })
}

async fn watchlist_add(
    State(st): State<ApiState>,
    Json(body): Json<WatchlistAddBody>,
) -> Result<Json<WatchlistAddResponse>, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    let entry_id = c
        .add_watchlist_url(body.url)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, e.message()))?;
    Ok(Json(WatchlistAddResponse { entry_id }))
}

async fn watchlist_add_from_queue(
    State(st): State<ApiState>,
    Path(item_id): Path<u64>,
) -> Result<Json<WatchlistAddResponse>, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    let entry_id = c
        .add_watchlist_from_queue_item(item_id)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, e.message()))?;
    Ok(Json(WatchlistAddResponse { entry_id }))
}

async fn watchlist_remove(
    State(st): State<ApiState>,
    Path(entry_id): Path<u64>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    if c.remove_watchlist_entry(entry_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(api_err(StatusCode::NOT_FOUND, "Watchlist entry not found."))
    }
}

async fn watchlist_set_paused(
    State(st): State<ApiState>,
    Path(entry_id): Path<u64>,
    Json(body): Json<WatchlistPauseBody>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    if c.set_watchlist_entry_paused(entry_id, body.paused) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(api_err(StatusCode::NOT_FOUND, "Watchlist entry not found."))
    }
}

async fn watchlist_enqueue(
    State(st): State<ApiState>,
    Path(entry_id): Path<u64>,
) -> Result<StatusCode, (StatusCode, Json<ApiErrorBody>)> {
    let mut c = st.core.lock();
    if c.enqueue_watchlist_entry(entry_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(api_err(StatusCode::NOT_FOUND, "Watchlist entry not found."))
    }
}

async fn watchlist_probe_now(State(st): State<ApiState>) -> StatusCode {
    let mut c = st.core.lock();
    c.request_watchlist_probe_now();
    StatusCode::NO_CONTENT
}

pub(super) fn register(router: Router<ApiState>) -> Router<ApiState> {
    router
        .route("/api/watchlist", get(watchlist_list))
        .route("/api/watchlist", post(watchlist_add))
        .route("/api/watchlist/probe", post(watchlist_probe_now))
        .route("/api/watchlist/from-queue/:item_id", post(watchlist_add_from_queue))
        .route("/api/watchlist/:entry_id/enqueue", post(watchlist_enqueue))
        .route("/api/watchlist/:entry_id/pause", post(watchlist_set_paused))
        .route("/api/watchlist/:entry_id", axum::routing::delete(watchlist_remove))
}
