use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

pub const AUTH_HEADER: &str = "X-Rustdl-Token";

pub async fn require_token(
    expected: String,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if expected.is_empty() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let authorized = request
        .headers()
        .get(AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|t| t == expected)
        .unwrap_or(false)
        || request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|t| t == expected)
            .unwrap_or(false);
    if authorized {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
