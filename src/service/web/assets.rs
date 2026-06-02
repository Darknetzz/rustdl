use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web-assets/"]
struct WebAssets;

pub fn static_router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
}

async fn index() -> impl IntoResponse {
    serve("index.html", "text/html; charset=utf-8")
}

async fn app_js() -> impl IntoResponse {
    serve("app.js", "text/javascript; charset=utf-8")
}

async fn style_css() -> impl IntoResponse {
    serve("style.css", "text/css; charset=utf-8")
}

fn serve(path: &str, content_type: &'static str) -> impl IntoResponse {
    match WebAssets::get(path) {
        Some(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, content_type)],
            content.data.into_owned(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
