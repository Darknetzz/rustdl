use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::{ItemStatus, QueueItem};

static SPEED_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"at\s+([0-9.]+\s*[KMGTP]?i?B/s)").expect("valid speed regex"));
static ETA_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"ETA\s+([0-9:]+)").expect("valid ETA regex"));

pub fn parse_speed_eta(line: &str) -> Option<(String, String)> {
    let speed = SPEED_RE
        .captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().replace(' ', ""))
        .unwrap_or_else(|| "-".to_owned());
    let eta = ETA_RE
        .captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
        .unwrap_or_else(|| "-".to_owned());
    if speed == "-" && eta == "-" {
        None
    } else {
        Some((speed, eta))
    }
}

pub fn parse_item_size_text(size_text: &str) -> Option<(u64, Option<u64>)> {
    let clean = size_text.trim();
    if clean.is_empty() || clean == "-" {
        return None;
    }
    if let Some((left, right)) = clean.split_once('/') {
        let dl = parse_human_size(left.trim())?;
        let total = parse_human_size(right.trim());
        return Some((dl, total));
    }
    parse_human_size(clean).map(|total| (0, Some(total)))
}

pub fn parse_human_size(raw: &str) -> Option<u64> {
    let s = raw.trim().to_ascii_uppercase();
    if s.is_empty() {
        return None;
    }
    let split_idx = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let num = s[..split_idx].trim().parse::<f64>().ok()?;
    let unit = s[split_idx..].trim();
    let mul = match unit {
        "" | "B" => 1f64,
        "KIB" => 1024f64,
        "MIB" => 1024f64 * 1024f64,
        "GIB" => 1024f64 * 1024f64 * 1024f64,
        "TIB" => 1024f64 * 1024f64 * 1024f64 * 1024f64,
        "KB" => 1000f64,
        "MB" => 1000f64 * 1000f64,
        "GB" => 1000f64 * 1000f64 * 1000f64,
        "TB" => 1000f64 * 1000f64 * 1000f64 * 1000f64,
        _ => return None,
    };
    Some((num * mul).round() as u64)
}

pub fn human_bytes_ui(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut n = bytes as f64;
    let mut idx = 0usize;
    while n >= 1024.0 && idx < UNITS.len() - 1 {
        n /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{}{}", n as u64, UNITS[idx])
    } else {
        format!("{n:.1}{}", UNITS[idx])
    }
}

pub fn split_cli_like(raw: &str) -> Vec<String> {
    shlex::split(raw).unwrap_or_else(|| raw.split_whitespace().map(str::to_owned).collect())
}

pub fn parse_urls_from_text_blob(raw: &str) -> Vec<String> {
    raw.split(|c: char| c == ',' || c == ';' || c.is_ascii_whitespace())
        .map(str::trim)
        .map(|s| s.trim_matches('"').trim_matches('\''))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn normalize_restored_item(item: &mut QueueItem) {
    match item.status {
        ItemStatus::Done | ItemStatus::Failed => {}
        _ => {
            item.status = ItemStatus::Idle;
            item.percent = 0.0;
            item.speed_text = "-".to_owned();
            item.eta_text = "-".to_owned();
            if item.detail.trim().is_empty() {
                item.detail = "Restored from previous session".to_owned();
            }
        }
    }
}

pub fn is_version_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> [u32; 3] {
        let mut out = [0u32; 3];
        for (idx, seg) in v.split('.').take(3).enumerate() {
            out[idx] = seg.parse::<u32>().unwrap_or(0);
        }
        out
    }
    parts(latest) > parts(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ItemStatus, QueueItem};

    #[test]
    fn parse_speed_eta_extracts_speed_and_eta() {
        let out = parse_speed_eta("[download] 12.3% of 7.2MiB at 3.5 MiB/s ETA 00:09");
        assert_eq!(out, Some(("3.5MiB/s".to_owned(), "00:09".to_owned())));
    }

    #[test]
    fn split_cli_like_respects_quotes() {
        let args = split_cli_like(r#"--postprocessor-args "-movflags +faststart" --ignore-errors"#);
        assert_eq!(
            args,
            vec![
                "--postprocessor-args".to_owned(),
                "-movflags +faststart".to_owned(),
                "--ignore-errors".to_owned()
            ]
        );
    }

    #[test]
    fn parse_urls_from_text_blob_supports_lines_and_csv() {
        let raw = "https://a.test/v1\nhttps://b.test/v2, https://c.test/v3;https://d.test/v4";
        let out = parse_urls_from_text_blob(raw);
        assert_eq!(
            out,
            vec![
                "https://a.test/v1".to_owned(),
                "https://b.test/v2".to_owned(),
                "https://c.test/v3".to_owned(),
                "https://d.test/v4".to_owned(),
            ]
        );
    }

    #[test]
    fn parse_urls_from_text_blob_trims_optional_quotes() {
        let raw = r#""https://a.test/v1";'https://b.test/v2'"#;
        let out = parse_urls_from_text_blob(raw);
        assert_eq!(
            out,
            vec![
                "https://a.test/v1".to_owned(),
                "https://b.test/v2".to_owned(),
            ]
        );
    }

    #[test]
    fn is_version_newer_works_for_semver_like_values() {
        assert!(is_version_newer("1.2.0", "1.1.9"));
        assert!(!is_version_newer("1.2.0", "1.2.0"));
        assert!(!is_version_newer("1.0.0", "1.2.0"));
    }

    #[test]
    fn parse_item_size_text_supports_total_or_pair() {
        assert_eq!(
            parse_item_size_text("1.0MiB/2.0MiB"),
            Some((1_048_576, Some(2_097_152)))
        );
        assert_eq!(parse_item_size_text("3.0MiB"), Some((0, Some(3_145_728))));
    }

    #[test]
    fn parse_human_size_accepts_binary_and_decimal_units() {
        assert_eq!(parse_human_size("1024"), Some(1024));
        assert_eq!(parse_human_size("1KIB"), Some(1024));
        assert_eq!(parse_human_size("1MIB"), Some(1_048_576));
        assert_eq!(parse_human_size("1.5MIB"), Some(1_572_864));
        assert_eq!(parse_human_size("2MB"), Some(2_000_000));
        assert_eq!(parse_human_size(""), None);
        assert_eq!(parse_human_size("12XY"), None);
    }

    #[test]
    fn normalize_restored_item_resets_in_progress_states() {
        let mut item = QueueItem {
            status: ItemStatus::Downloading,
            percent: 50.0,
            speed_text: "1MiB/s".to_owned(),
            eta_text: "00:10".to_owned(),
            detail: String::new(),
            ..Default::default()
        };
        normalize_restored_item(&mut item);
        assert_eq!(item.status, ItemStatus::Idle);
        assert_eq!(item.percent, 0.0);
        assert_eq!(item.speed_text, "-");
        assert_eq!(item.eta_text, "-");
        assert!(item.detail.contains("Restored"));
    }

    #[test]
    fn normalize_restored_item_leaves_done_unchanged() {
        let mut item = QueueItem {
            status: ItemStatus::Done,
            percent: 100.0,
            detail: "Completed".to_owned(),
            ..Default::default()
        };
        normalize_restored_item(&mut item);
        assert_eq!(item.status, ItemStatus::Done);
        assert_eq!(item.percent, 100.0);
        assert_eq!(item.detail, "Completed");
    }
}
