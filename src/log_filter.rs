//! Shared activity-log filter rules (desktop GUI and LAN web UI).

pub const ERROR_KEYWORDS: &[&str] = &[
    "error",
    "failed",
    "failure",
    "not found",
    "invalid",
    "missing",
    "denied",
];

pub const IMPORTANT_KEYWORDS: &[&str] = &[
    "metadata fetch failed",
    "download failed",
    "starting",
    "started",
    "completed",
    "done",
    "queue",
    "convert",
    "skipped",
    "skip_reason",
];

pub fn is_error_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    ERROR_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

pub fn log_filter_accepts(slug: &str, line: &str) -> bool {
    let body = crate::time_format::log_message_body(line);
    match slug.trim().to_ascii_lowercase().as_str() {
        "errors" => is_error_line(body),
        "important" => {
            let lower = body.to_ascii_lowercase();
            is_error_line(body) || IMPORTANT_KEYWORDS.iter().any(|kw| lower.contains(kw))
        }
        _ => true,
    }
}
