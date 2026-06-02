use std::collections::HashSet;
use std::fs;
use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;
use url::Url;

use crate::models::{Av1QueueItem, ItemStatus, QueueItem};

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
    let raw = strip_utf8_bom(raw);
    raw.split(|c: char| c == ',' || c == ';' || c.is_ascii_whitespace())
        .map(str::trim)
        .map(|s| s.trim_matches('"').trim_matches('\''))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[inline]
pub fn strip_utf8_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// Extracts `http`/`https` URL lines from plain-text list files (`.txt`, `.csv`, `.m3u`, `.m3u8`).
pub fn http_url_lines_from_plain_list_content(content: &str) -> Vec<String> {
    parse_urls_from_text_blob(content)
        .into_iter()
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .filter(|u| Url::parse(u).is_ok())
        .collect()
}

/// Deduplicate strings while keeping the first occurrence order.
pub fn dedupe_preserve_order_strings(items: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for s in items {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    }
    out
}

static EMBEDDED_HTTP_URL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"https?://[^\s<>'"]+"#).expect("valid embedded URL regex"));

/// Reads `URL=` from a Windows Internet Shortcut (`.url`).
pub fn parse_internet_shortcut_url(content: &str) -> Option<String> {
    let content = strip_utf8_bom(content);
    for line in content.lines() {
        let t = line.trim();
        let rest = if t.len() >= 4 && t[..4].eq_ignore_ascii_case("url=") {
            t[4..].trim()
        } else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        if Url::parse(rest).is_ok() {
            return Some(rest.to_owned());
        }
    }
    None
}

/// Pulls `http(s)` URLs out of plist/XML (e.g. Safari `.webloc`).
pub fn parse_embedded_http_urls(content: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for m in EMBEDDED_HTTP_URL.find_iter(content) {
        let mut s = m.as_str().to_owned();
        while matches!(s.chars().last(), Some(')' | ',' | '.' | ';' | '"' | '\'')) {
            s.pop();
        }
        if Url::parse(&s).is_ok() && seen.insert(s.clone()) {
            out.push(s);
        }
    }
    out
}

/// Resolves dropped filesystem paths that carry URLs (shortcuts, lists, playlists).
pub fn urls_from_dropped_os_path(path: &Path) -> Option<Vec<String>> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "url" => {
            let text = fs::read_to_string(path).ok()?;
            parse_internet_shortcut_url(&text).map(|u| vec![u])
        }
        "webloc" => {
            let text = fs::read_to_string(path).ok()?;
            let urls = parse_embedded_http_urls(&text);
            if urls.is_empty() {
                None
            } else {
                Some(urls)
            }
        }
        "txt" | "csv" | "m3u" | "m3u8" => {
            let text = fs::read_to_string(path).ok()?;
            let urls = http_url_lines_from_plain_list_content(&text);
            if urls.is_empty() {
                None
            } else {
                Some(urls)
            }
        }
        _ => None,
    }
}

pub fn normalize_restored_item(item: &mut QueueItem) {
    if item.sort_order == 0 {
        item.sort_order = item.item_id;
    }
    const DUPLICATE_MSG: &str = "already in the list";
    if item
        .error
        .as_deref()
        .is_some_and(|e| e.to_ascii_lowercase().contains(DUPLICATE_MSG))
    {
        item.error = None;
    }
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

pub fn av1_detail_is_user_cancellation(detail: &str) -> bool {
    let d = detail.trim().to_ascii_lowercase();
    d.starts_with("cancelled") || d.contains("cancelled by user")
}

pub fn reset_av1_item_to_ready(item: &mut Av1QueueItem) {
    item.status = ItemStatus::Idle;
    item.percent = 0.0;
    item.detail = if item.input_bytes > 0 {
        format!("Ready · {}", human_bytes_ui(item.input_bytes))
    } else {
        "Ready".to_owned()
    };
}

pub fn normalize_restored_av1_item(item: &mut Av1QueueItem) {
    match item.status {
        ItemStatus::Done => {}
        ItemStatus::Failed if av1_detail_is_user_cancellation(&item.detail) => {
            reset_av1_item_to_ready(item);
        }
        ItemStatus::Failed => {}
        _ => reset_av1_item_to_ready(item),
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
    fn strip_utf8_bom_parse_urls() {
        let raw = "\u{feff}https://a.test/x https://b.test/y";
        assert_eq!(
            parse_urls_from_text_blob(raw),
            vec!["https://a.test/x".to_owned(), "https://b.test/y".to_owned()]
        );
    }

    #[test]
    fn http_url_lines_from_plain_list_filters_non_urls() {
        let raw = "https://ok.test/a\nnot-a-url\n# comment line\nhttps://ok.test/b";
        assert_eq!(
            http_url_lines_from_plain_list_content(raw),
            vec![
                "https://ok.test/a".to_owned(),
                "https://ok.test/b".to_owned()
            ]
        );
    }

    #[test]
    fn parse_internet_shortcut_url_reads_url_key() {
        let ini = "[InternetShortcut]\r\nURL=https://example.com/watch?v=1\r\n";
        assert_eq!(
            parse_internet_shortcut_url(ini),
            Some("https://example.com/watch?v=1".to_owned())
        );
    }

    #[test]
    fn dedupe_preserve_order_strings_keeps_first() {
        let out = dedupe_preserve_order_strings(vec![
            "https://a.test/1".to_owned(),
            "https://b.test/2".to_owned(),
            "https://a.test/1".to_owned(),
        ]);
        assert_eq!(
            out,
            vec!["https://a.test/1".to_owned(), "https://b.test/2".to_owned()]
        );
    }

    #[test]
    fn parse_embedded_http_urls_finds_url_in_xml() {
        let xml = r#"<plist><string>https://youtu.be/abc123</string></plist>"#;
        assert_eq!(
            parse_embedded_http_urls(xml),
            vec!["https://youtu.be/abc123".to_owned()]
        );
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

    #[test]
    fn normalize_restored_av1_item_restores_cancelled_failures_to_ready() {
        use crate::models::Av1QueueItem;

        let mut item = Av1QueueItem {
            status: ItemStatus::Failed,
            detail: "Cancelled by user.".to_owned(),
            input_bytes: 1_048_576,
            ..Default::default()
        };
        normalize_restored_av1_item(&mut item);
        assert_eq!(item.status, ItemStatus::Idle);
        assert_eq!(item.percent, 0.0);
        assert!(item.detail.starts_with("Ready ·"));
    }

    #[test]
    fn normalize_restored_av1_item_keeps_real_failures() {
        use crate::models::Av1QueueItem;

        let mut item = Av1QueueItem {
            status: ItemStatus::Failed,
            detail: "ffmpeg failed with status exit status: 1".to_owned(),
            ..Default::default()
        };
        normalize_restored_av1_item(&mut item);
        assert_eq!(item.status, ItemStatus::Failed);
    }
}
