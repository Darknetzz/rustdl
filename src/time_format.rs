//! Log timestamps (local, full) and human-readable relative times for files on disk.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Datelike, Local};

/// Prefix for new activity-log lines: `[YYYY-MM-DD HH:MM:SS] ` (local time).
pub fn format_log_line(message: &str) -> String {
    let stamp = Local::now().format("[%Y-%m-%d %H:%M:%S] ");
    format!("{stamp}{message}")
}

/// Splits a stored log line into `(timestamp_prefix, message)` when prefixed by [`format_log_line`].
pub fn split_log_line(line: &str) -> (&str, &str) {
    if !line.starts_with('[') {
        return ("", line);
    }
    let Some(rest) = line.strip_prefix('[') else {
        return ("", line);
    };
    let Some((ts, msg)) = rest.split_once("] ") else {
        return ("", line);
    };
    if ts.len() != 19 || !looks_like_log_timestamp(ts) {
        return ("", line);
    }
    (ts, msg)
}

fn looks_like_log_timestamp(ts: &str) -> bool {
    let b = ts.as_bytes();
    b.len() == 19
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b' '
        && b[13] == b':'
        && b[16] == b':'
        && b[..4].iter().all(|c| c.is_ascii_digit())
        && b[5..7].iter().all(|c| c.is_ascii_digit())
        && b[8..10].iter().all(|c| c.is_ascii_digit())
        && b[11..13].iter().all(|c| c.is_ascii_digit())
        && b[14..16].iter().all(|c| c.is_ascii_digit())
        && b[17..19].iter().all(|c| c.is_ascii_digit())
}

/// Message body used for filters and semantic coloring (strips leading timestamp when present).
pub fn log_message_body(line: &str) -> &str {
    split_log_line(line).1
}

pub fn format_absolute_local(t: SystemTime) -> String {
    let dt: DateTime<Local> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Short relative label for UI (e.g. card file row): `just now`, `5 min ago`, `3 days ago`.
pub fn format_relative_ago(t: SystemTime) -> String {
    let Ok(ago) = SystemTime::now().duration_since(t) else {
        return "just now".to_owned();
    };
    if ago < Duration::from_secs(10) {
        return "just now".to_owned();
    }
    if ago < Duration::from_secs(60) {
        return format!("{} sec ago", ago.as_secs());
    }
    if ago < Duration::from_secs(3600) {
        let m = ago.as_secs() / 60;
        return if m == 1 {
            "1 min ago".to_owned()
        } else {
            format!("{m} min ago")
        };
    }
    if ago < Duration::from_secs(86_400) {
        let h = ago.as_secs() / 3600;
        return if h == 1 {
            "1 hr ago".to_owned()
        } else {
            format!("{h} hr ago")
        };
    }
    if ago < Duration::from_secs(604_800) {
        let d = ago.as_secs() / 86_400;
        return if d == 1 {
            "1 day ago".to_owned()
        } else {
            format!("{d} days ago")
        };
    }
    let dt: DateTime<Local> = t.into();
    if dt.year() == Local::now().year() {
        format!("{} {}", dt.format("%b"), dt.day())
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_log_line_with_timestamp() {
        let line = "[2026-05-20 14:32:15] [item 1] done";
        assert_eq!(
            split_log_line(line),
            ("2026-05-20 14:32:15", "[item 1] done")
        );
        assert_eq!(log_message_body(line), "[item 1] done");
    }

    #[test]
    fn split_log_line_without_timestamp() {
        let line = "[item 1] failed";
        assert_eq!(split_log_line(line), ("", "[item 1] failed"));
    }

    #[test]
    fn format_log_line_adds_bracketed_timestamp() {
        let line = format_log_line("hello");
        assert!(line.starts_with('['));
        assert!(line.ends_with("hello"));
        assert!(line.contains("] "));
    }
}
