use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use url::Url;

use crate::models::VideoPreview;

pub const PLAYLIST_PREVIEW_CAP: usize = 20;
/// Last N stderr lines kept for error messages when yt-dlp exits non-zero.
const STDERR_TAIL_LINES: usize = 16;
const PROGRESS_PREFIX: &str = "progress:";
static PROGRESS_PERCENT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d+(?:\.\d+)?)%").expect("valid progress percent regex"));
static PROGRESS_SIZE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"of\s+([0-9.]+\s*[KMGTP]?i?B)").expect("valid progress size regex"));

pub fn get_external_tools_with_paths(
    yt_dlp_path: &str,
    ffmpeg_path: &str,
    ffprobe_path: &str,
) -> (bool, bool, bool) {
    (
        executable_exists(yt_dlp_path, "yt-dlp"),
        executable_exists(ffmpeg_path, "ffmpeg"),
        executable_exists(ffprobe_path, "ffprobe"),
    )
}

/// Shown in the main window tool strip; keep lines from growing the layout.
const TOOL_VERSION_DISPLAY_MAX_CHARS: usize = 56;

fn truncate_version_display(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    let n = t.chars().count();
    if n <= TOOL_VERSION_DISPLAY_MAX_CHARS {
        t.to_string()
    } else {
        t.chars()
            .take(TOOL_VERSION_DISPLAY_MAX_CHARS.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

/// Same binary we would use for metadata/download: full path when resolving via PATH (Windows-safe).
fn resolve_exe_for_version_spawn(custom_path: &str, default_exe: &str) -> String {
    let trimmed = custom_path.trim();
    if !trimmed.is_empty() {
        return trimmed.to_owned();
    }
    which(default_exe)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| default_exe.to_owned())
}

/// First non-empty line from stdout, else stderr (e.g. `ffmpeg -version` prints the banner to stderr).
/// Does not require exit code 0; some launcher shims return non-zero while still printing a version line.
fn read_version_from_exe(exe: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(exe)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    let line = out
        .lines()
        .find(|l| !l.trim().is_empty())
        .or_else(|| err.lines().find(|l| !l.trim().is_empty()));
    line.map(truncate_version_display)
}

/// Version string for the status bar, or None if the tool is missing or did not return a version.
pub fn read_yt_dlp_version(yt_dlp_path: &str) -> Option<String> {
    if !executable_exists(yt_dlp_path, "yt-dlp") {
        return None;
    }
    let exe = resolve_exe_for_version_spawn(yt_dlp_path, "yt-dlp");
    read_version_from_exe(&exe, &["--version"])
}

/// Version string for the status bar, or None if the tool is missing or did not return a version.
pub fn read_ffmpeg_version(ffmpeg_path: &str) -> Option<String> {
    if !executable_exists(ffmpeg_path, "ffmpeg") {
        return None;
    }
    let exe = resolve_exe_for_version_spawn(ffmpeg_path, "ffmpeg");
    read_version_from_exe(&exe, &["-version"])
}

/// Version string for the status bar, or None if the tool is missing or did not return a version.
pub fn read_ffprobe_version(ffprobe_path: &str) -> Option<String> {
    if !executable_exists(ffprobe_path, "ffprobe") {
        return None;
    }
    let exe = if ffprobe_path.trim().is_empty() {
        resolve_exe_for_version_spawn("", "ffprobe")
    } else {
        resolve_ffprobe_exe(ffprobe_path)
    };
    read_version_from_exe(&exe, &["-version"])
}

fn executable_exists(custom_path: &str, default_exe: &str) -> bool {
    let trimmed = custom_path.trim();
    if !trimmed.is_empty() {
        return PathBuf::from(trimmed).is_file() || which(trimmed).is_some();
    }
    which(default_exe).is_some()
}

fn resolve_executable(custom_path: &str, default_exe: &str) -> String {
    let trimmed = custom_path.trim();
    if trimmed.is_empty() {
        default_exe.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn yt_dlp_spawn_error_message(custom_path: &str, source_err: &std::io::Error) -> String {
    let trimmed = custom_path.trim();
    if trimmed.is_empty() {
        format!("Could not run yt-dlp (not on PATH or failed to start): {source_err}")
    } else {
        format!("Could not run yt-dlp at {trimmed:?}: {source_err}. Check Settings → Executables.")
    }
}

fn resolve_ffprobe_exe(ffprobe_path: &str) -> String {
    let trimmed = ffprobe_path.trim();
    if trimmed.is_empty() {
        "ffprobe".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn which(exe: &str) -> Option<PathBuf> {
    which::which(exe).ok()
}

pub fn normalize_url_for_dedupe(input: &str) -> String {
    let raw = input.trim();
    if raw.is_empty() {
        return String::new();
    }
    let Ok(u) = Url::parse(raw) else {
        return raw.to_owned();
    };
    let host = u
        .host_str()
        .unwrap_or_default()
        .trim_start_matches("www.")
        .to_lowercase();
    let path = u.path().trim_end_matches('/').to_owned();
    let query_pairs: HashMap<String, String> = u
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    if host == "youtu.be" {
        if let Some(vid) = path.split('/').next_back() {
            if !vid.is_empty() {
                return format!("youtube:{vid}");
            }
        }
    }
    if host.contains("youtube.com")
        || host.contains("youtube-nocookie.com")
        || host.contains("music.youtube.com")
    {
        if let Some(v) = query_pairs.get("v") {
            return format!("youtube:{v}");
        }
    }

    let mut q = query_pairs.into_iter().collect::<Vec<_>>();
    q.sort_by(|a, b| a.0.cmp(&b.0));
    let qs = q
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let mut out = format!("{}://{}{}", u.scheme().to_lowercase(), host, path);
    if !qs.is_empty() {
        out.push('?');
        out.push_str(&qs);
    }
    out
}

fn parse_preview_entry(entry: &Value, source: &str) -> VideoPreview {
    let title = entry
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("(no title)")
        .to_owned();
    let webpage_url = entry
        .get("webpage_url")
        .and_then(Value::as_str)
        .or_else(|| entry.get("url").and_then(Value::as_str))
        .unwrap_or(source)
        .to_owned();
    let thumbs = entry.get("thumbnails").and_then(Value::as_array);
    let thumbnail_url = thumbs
        .and_then(|arr| {
            arr.iter()
                .filter_map(|v| v.get("url").and_then(Value::as_str).map(str::to_owned))
                .next_back()
        })
        .or_else(|| {
            entry
                .get("thumbnail")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    let (expected_size_bytes, expected_size_approx) = estimate_preview_size(entry);
    let (width, height) = estimate_preview_resolution(entry);
    VideoPreview {
        video_id: entry
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        title,
        webpage_url,
        thumbnail_url,
        duration: entry.get("duration").and_then(Value::as_i64),
        uploader: entry
            .get("uploader")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                entry
                    .get("channel")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
        width,
        height,
        expected_size_bytes,
        expected_size_approx,
        source_line: source.to_owned(),
        error: None,
        playlist_capped: false,
    }
}

fn estimate_preview_size(entry: &Value) -> (Option<u64>, bool) {
    let exact_size = entry.get("filesize").and_then(Value::as_u64);
    if let Some(v) = exact_size {
        return (Some(v), false);
    }
    let approx_size = entry.get("filesize_approx").and_then(Value::as_u64);
    if let Some(v) = approx_size {
        return (Some(v), true);
    }
    if let Some(req) = entry.get("requested_formats").and_then(Value::as_array) {
        let mut total_exact = 0u64;
        let mut total_approx = 0u64;
        for f in req {
            if let Some(v) = f.get("filesize").and_then(Value::as_u64) {
                total_exact += v;
            } else if let Some(v) = f.get("filesize_approx").and_then(Value::as_u64) {
                total_approx += v;
            }
        }
        if total_exact > 0 {
            return (Some(total_exact + total_approx), total_approx > 0);
        }
        if total_approx > 0 {
            return (Some(total_approx), true);
        }
    }
    let duration_s = entry.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
    let tbr_kbps = entry.get("tbr").and_then(Value::as_f64).unwrap_or(0.0);
    if duration_s > 0.0 && tbr_kbps > 0.0 {
        // tbr is kilobits/s, convert to bytes.
        let estimated = (duration_s * tbr_kbps * 1000.0 / 8.0).round() as u64;
        if estimated > 0 {
            return (Some(estimated), true);
        }
    }
    (None, false)
}

fn estimate_preview_resolution(entry: &Value) -> (Option<u32>, Option<u32>) {
    let width = entry.get("width").and_then(Value::as_u64).map(|v| v as u32);
    let height = entry
        .get("height")
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    if width.is_some() || height.is_some() {
        return (width, height);
    }
    if let Some(req) = entry.get("requested_formats").and_then(Value::as_array) {
        let mut best_w: Option<u32> = None;
        let mut best_h: Option<u32> = None;
        for f in req {
            let w = f.get("width").and_then(Value::as_u64).map(|v| v as u32);
            let h = f.get("height").and_then(Value::as_u64).map(|v| v as u32);
            if let Some(v) = w {
                best_w = Some(best_w.map_or(v, |cur| cur.max(v)));
            }
            if let Some(v) = h {
                best_h = Some(best_h.map_or(v, |cur| cur.max(v)));
            }
        }
        if best_w.is_some() || best_h.is_some() {
            return (best_w, best_h);
        }
    }
    (None, None)
}

#[derive(Debug, Deserialize)]
struct FfprobeStreamsRoot {
    #[serde(default)]
    streams: Vec<FfprobeStreamEntry>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStreamEntry {
    codec_type: Option<String>,
}

fn stream_presence_from_ffprobe_json(bytes: &[u8]) -> Option<(bool, bool)> {
    let root: FfprobeStreamsRoot = serde_json::from_slice(bytes).ok()?;
    let mut has_video = false;
    let mut has_audio = false;
    for s in root.streams {
        match s.codec_type.as_deref() {
            Some("video") => has_video = true,
            Some("audio") => has_audio = true,
            _ => {}
        }
    }
    Some((has_video, has_audio))
}

/// Returns `(has_video, has_audio)` when ffprobe succeeds; `None` if ffprobe failed or output was not valid JSON.
pub fn probe_video_audio_stream_presence(
    file_path: &str,
    ffprobe_path: &str,
) -> Option<(bool, bool)> {
    let ffprobe = resolve_ffprobe_exe(ffprobe_path);
    let out = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "json",
            file_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    stream_presence_from_ffprobe_json(&out.stdout)
}

pub fn probe_video_resolution_with_path(file_path: &str, ffprobe_path: &str) -> Option<(u32, u32)> {
    let ffprobe = resolve_ffprobe_exe(ffprobe_path);
    let out = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=s=x:p=0",
            file_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let (w, h) = text.split_once('x')?;
    let width = w.trim().parse::<u32>().ok()?;
    let height = h.trim().parse::<u32>().ok()?;
    if width > 0 && height > 0 {
        Some((width, height))
    } else {
        None
    }
}

/// Build `--cookies` or `--cookies-from-browser` args from settings text.
///
/// Paths (contain `/` or `\`, end in `.txt`, or exist as a file) use `--cookies`.
/// Anything else is passed to `--cookies-from-browser` (e.g. `firefox`, `chrome:Profile`).
pub fn cookie_args_from_setting(cookies: &str) -> Vec<String> {
    let t = cookies.trim();
    if t.is_empty() {
        return Vec::new();
    }
    let path = Path::new(t);
    let looks_like_file = path.is_file()
        || t.contains('\\')
        || t.contains('/')
        || t.ends_with(".txt");
    if looks_like_file {
        vec!["--cookies".to_owned(), t.to_owned()]
    } else {
        vec![
            "--cookies-from-browser".to_owned(),
            t.to_owned(),
        ]
    }
}

pub fn resolve_url_to_previews_with_bin(
    url: &str,
    yt_dlp_path: &str,
    extra_args: &[String],
) -> Vec<VideoPreview> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    let bin = resolve_executable(yt_dlp_path, "yt-dlp");
    let mut cmd = Command::new(&bin);
    cmd.args(["-J", "--no-warnings", "--skip-download"]);
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.arg(trimmed);
    let output = match cmd.output()
    {
        Ok(o) => o,
        Err(e) => {
            return vec![VideoPreview {
                source_line: trimmed.to_owned(),
                webpage_url: trimmed.to_owned(),
                title: String::new(),
                error: Some(yt_dlp_spawn_error_message(yt_dlp_path, &e)),
                ..Default::default()
            }];
        }
    };
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return vec![VideoPreview {
            source_line: trimmed.to_owned(),
            webpage_url: trimmed.to_owned(),
            title: String::new(),
            error: Some(if err.is_empty() {
                "yt-dlp failed".to_owned()
            } else {
                err
            }),
            ..Default::default()
        }];
    }
    let Ok(root) = serde_json::from_slice::<Value>(&output.stdout) else {
        return vec![VideoPreview {
            source_line: trimmed.to_owned(),
            webpage_url: trimmed.to_owned(),
            title: String::new(),
            error: Some("Invalid yt-dlp JSON".to_owned()),
            ..Default::default()
        }];
    };

    if let Some(entries) = root.get("entries").and_then(Value::as_array) {
        let capped = entries.len() > PLAYLIST_PREVIEW_CAP;
        let mut rows = Vec::new();
        for e in entries.iter().take(PLAYLIST_PREVIEW_CAP) {
            let mut p = parse_preview_entry(e, trimmed);
            p.playlist_capped = capped;
            rows.push(p);
        }
        return rows;
    }
    vec![parse_preview_entry(&root, trimmed)]
}

pub fn dedupe_previews(
    existing_keys: &HashSet<String>,
    previews: &[VideoPreview],
) -> Vec<VideoPreview> {
    let mut seen = existing_keys.clone();
    let mut out = Vec::new();
    for p in previews {
        let web = normalize_url_for_dedupe(&p.webpage_url);
        let vid = if p.video_id.is_empty() {
            String::new()
        } else {
            format!("vid:{}", p.video_id)
        };
        if (!web.is_empty() && seen.contains(&web)) || (!vid.is_empty() && seen.contains(&vid)) {
            continue;
        }
        if !web.is_empty() {
            seen.insert(web);
        }
        if !vid.is_empty() {
            seen.insert(vid);
        }
        out.push(p.clone());
    }
    out
}

pub fn parse_progress_line(line: &str) -> (Option<f32>, Option<String>) {
    let clean = line.trim();
    if let Some(payload) = clean.strip_prefix(PROGRESS_PREFIX) {
        let parts = payload.split('|').collect::<Vec<_>>();
        if parts.len() >= 4 {
            let pct = parts[0].trim().trim_end_matches('%').parse::<f32>().ok();
            let d = parts[1].trim().parse::<u64>().ok();
            let t = parts[2]
                .trim()
                .parse::<u64>()
                .ok()
                .or_else(|| parts[3].trim().parse::<u64>().ok());
            let text = match (d, t) {
                (Some(a), Some(b)) => Some(format!("{}/{}", human_bytes(a), human_bytes(b))),
                (_, Some(b)) => Some(human_bytes(b)),
                _ => None,
            };
            return (pct, text);
        }
    }
    let pct = PROGRESS_PERCENT_RE
        .captures(clean)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<f32>().ok());
    let size = PROGRESS_SIZE_RE
        .captures(clean)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().replace(' ', ""));
    (pct, size)
}

fn push_line_tail(deque: &mut VecDeque<String>, line: String) {
    if deque.len() >= STDERR_TAIL_LINES {
        deque.pop_front();
    }
    deque.push_back(line);
}

pub async fn stream_download_with_bins<F>(
    url: &str,
    output_dir: &str,
    extra_args: &[String],
    yt_dlp_path: &str,
    ffmpeg_path: &str,
    cancel_flag: Arc<AtomicBool>,
    mut on_line: F,
) -> Result<()>
where
    F: FnMut(String) + Send,
{
    let output_template = format!("{output_dir}/%(title)s [%(id)s].%(ext)s");
    let mut cmd = TokioCommand::new(resolve_executable(yt_dlp_path, "yt-dlp"));
    cmd.arg("--newline")
        .arg("--progress-template")
        .arg(format!(
            "{PROGRESS_PREFIX}%(progress._percent_str)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s"
        ))
        .arg("-o")
        .arg(output_template);
    if !ffmpeg_path.trim().is_empty() {
        cmd.arg("--ffmpeg-location").arg(ffmpeg_path.trim());
    }
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.arg(url);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("failed to spawn yt-dlp: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to read stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to read stderr"))?;
    let mut out_lines = BufReader::new(stdout).lines();
    let mut err_lines = BufReader::new(stderr).lines();
    let mut stderr_tail = VecDeque::with_capacity(STDERR_TAIL_LINES);

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = child.start_kill();
            return Err(anyhow!("Cancelled by user."));
        }
        tokio::select! {
            line = out_lines.next_line() => {
                match line? {
                    Some(s) => on_line(s),
                    None => break,
                }
            }
            line = err_lines.next_line() => {
                if let Some(s) = line? {
                    push_line_tail(&mut stderr_tail, s.clone());
                    on_line(s);
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => {}
        }
    }

    while let Some(line) = err_lines.next_line().await? {
        push_line_tail(&mut stderr_tail, line.clone());
        on_line(line);
    }

    let status = child.wait().await?;
    if !status.success() {
        let code = status.code().unwrap_or(-1);
        if stderr_tail.is_empty() {
            return Err(anyhow!("yt-dlp exited with {code}"));
        }
        let tail = stderr_tail.into_iter().collect::<Vec<_>>().join("\n");
        return Err(anyhow!(
            "yt-dlp exited with {code}\n--- recent stderr ---\n{tail}"
        ));
    }
    Ok(())
}

fn human_bytes(bytes: u64) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_maps_youtube_variants() {
        let a = normalize_url_for_dedupe("https://www.youtube.com/watch?v=abc123&list=foo");
        let b = normalize_url_for_dedupe("https://youtu.be/abc123");
        let c = normalize_url_for_dedupe("https://music.youtube.com/watch?v=abc123");
        assert_eq!(a, "youtube:abc123");
        assert_eq!(b, "youtube:abc123");
        assert_eq!(c, "youtube:abc123");
    }

    #[test]
    fn normalize_url_sorts_query_keys() {
        let a = normalize_url_for_dedupe("https://example.com/x?b=2&a=1");
        let b = normalize_url_for_dedupe("https://example.com/x?a=1&b=2");
        assert_eq!(a, b);
    }

    #[test]
    fn dedupe_previews_filters_existing_and_incoming_duplicates() {
        let mut existing = HashSet::new();
        existing.insert("youtube:already".to_owned());

        let previews = vec![
            VideoPreview {
                video_id: "already".to_owned(),
                webpage_url: "https://youtu.be/already".to_owned(),
                ..Default::default()
            },
            VideoPreview {
                video_id: "new1".to_owned(),
                webpage_url: "https://youtu.be/new1".to_owned(),
                ..Default::default()
            },
            VideoPreview {
                video_id: "new1".to_owned(),
                webpage_url: "https://www.youtube.com/watch?v=new1".to_owned(),
                ..Default::default()
            },
        ];

        let out = dedupe_previews(&existing, &previews);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].video_id, "new1");
    }

    #[test]
    fn cookie_args_empty_when_unset() {
        assert!(super::cookie_args_from_setting("").is_empty());
        assert!(super::cookie_args_from_setting("   ").is_empty());
    }

    #[test]
    fn cookie_args_file_path() {
        let args = super::cookie_args_from_setting(r"C:\Users\me\cookies.txt");
        assert_eq!(
            args,
            vec![
                "--cookies".to_owned(),
                r"C:\Users\me\cookies.txt".to_owned()
            ]
        );
    }

    #[test]
    fn cookie_args_browser() {
        let args = super::cookie_args_from_setting("firefox");
        assert_eq!(
            args,
            vec!["--cookies-from-browser".to_owned(), "firefox".to_owned()]
        );
        let profile = super::cookie_args_from_setting("firefox:rustdl");
        assert_eq!(
            profile,
            vec![
                "--cookies-from-browser".to_owned(),
                "firefox:rustdl".to_owned()
            ]
        );
    }

    #[test]
    fn parse_progress_line_handles_template_prefix() {
        let (pct, size) = parse_progress_line("progress:42.5%|1048576|2097152|0");
        assert_eq!(pct, Some(42.5));
        assert_eq!(size.as_deref(), Some("1.0MiB/2.0MiB"));
    }

    #[test]
    fn parse_progress_line_handles_regular_output() {
        let (pct, size) =
            parse_progress_line("[download] 73.1% of 12.3 MiB at 1.2 MiB/s ETA 00:12");
        assert_eq!(pct, Some(73.1));
        assert_eq!(size.as_deref(), Some("12.3MiB"));
    }

    #[test]
    fn stream_presence_from_ffprobe_json_audio_only() {
        let raw = br#"{"streams":[{"codec_type":"audio"}]}"#;
        assert_eq!(
            super::stream_presence_from_ffprobe_json(raw),
            Some((false, true))
        );
    }

    #[test]
    fn stream_presence_from_ffprobe_json_video_and_audio() {
        let raw = br#"{"streams":[{"codec_type":"video"},{"codec_type":"audio"}]}"#;
        assert_eq!(
            super::stream_presence_from_ffprobe_json(raw),
            Some((true, true))
        );
    }
}
