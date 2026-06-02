use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

pub const AUTH_HEADER: &str = "X-Rustdl-Token";

/// Reads the token from request headers/query-style values and compares to `expected` (trimmed).
pub fn token_matches(expected: &str, presented: Option<&str>) -> bool {
    let expected = expected.trim();
    if expected.is_empty() {
        return false;
    }
    presented
        .map(str::trim)
        .is_some_and(|t| !t.is_empty() && t == expected)
}

pub fn token_from_request(request: &Request<Body>, expected: &str) -> bool {
    let header_ok = request
        .headers()
        .get(AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| token_matches(expected, Some(t)));
    let bearer_ok = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| token_matches(expected, Some(t)));
    header_ok || bearer_ok
}

pub async fn require_token(
    expected: String,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if expected.trim().is_empty() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    if token_from_request(&request, &expected) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
