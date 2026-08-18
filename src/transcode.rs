use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use eframe::egui;

use crate::convert_size_limit::{pre_encode_decision, ConvertSizeLimit, PreEncodeDecision};
use crate::external_tools::{
    apply_subprocess_launch, logical_cpu_count, no_console_window, normalize_subprocess_priority,
    resolve_executable,
};

const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv"];
const BITRATE_FALLBACK_BPS: i64 = 2_000_000;
const BITRATE_MAXRATE_MULTIPLIER: f64 = 1.2;
const BITRATE_BUFSIZE_MULTIPLIER: f64 = 2.0;

/// Session-wide target video codec: `av1`, `hevc`, or `h264`.
pub fn normalize_target_codec(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "hevc" | "h265" | "h.265" => "hevc",
        "h264" | "h.264" | "avc" => "h264",
        _ => "av1",
    }
}

/// True when ffmpeg could not open/demux the input at all (missing/truncated)—CPU encoder retry won't help.
/// Mid-stream bitstream damage ("Invalid data…") is *not* included: players often still play those files,
/// and convert can recover with resilient demux flags / audio copy.
pub fn convert_failure_is_unreadable_source(err: &str) -> bool {
    let s = err.to_ascii_lowercase();
    s.contains("moov atom not found")
        || s.contains("error opening input")
        || s.contains("no such file or directory")
}

/// True when encode failed on damaged packets (NAL/AAC) that often still play in VLC/mpv.
pub fn convert_failure_may_retry_audio_copy(err: &str) -> bool {
    let s = err.to_ascii_lowercase();
    s.contains("invalid data found when processing input")
        || s.contains("error splitting the input into nal")
        || s.contains("invalid nal unit size")
        || s.contains("error reinitializing filters")
        || s.contains("error submitting packet to decoder")
        || s.contains("error processing packet in decoder")
}

fn convert_source_error_summary(err: &str) -> Option<&'static str> {
    let s = err.to_ascii_lowercase();
    if s.contains("moov atom not found") {
        Some(
            "Source file appears incomplete or corrupt (MP4 moov atom missing). \
             This usually means the download was interrupted or the file is still being written. \
             Re-download the video fully, then convert again.",
        )
    } else if s.contains("error opening input") {
        Some(
            "ffmpeg could not open the source file—it may be missing, incomplete, or not a valid video container.",
        )
    } else if convert_failure_may_retry_audio_copy(err) {
        Some(
            "The source has bitstream damage that players may conceal, but ffmpeg could not finish encoding. \
             Try re-downloading a clean copy, or remux/repair the file first.",
        )
    } else {
        None
    }
}

/// User-facing convert failure text with a plain-language lead when the source file is the problem.
pub fn format_convert_failure(err: &str) -> String {
    if let Some(summary) = convert_source_error_summary(err) {
        format!("{summary}\n\n{err}")
    } else {
        err.to_owned()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ConvertInputMedia {
    pub codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub bitrate_bps: Option<u64>,
    pub duration_ms: Option<u64>,
    pub format_name: Option<String>,
    pub creation_time: Option<String>,
    pub encoder_tag: Option<String>,
    pub audio_codec: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ConvertConfig {
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    pub output_dir: String,
    pub recursive: bool,
    pub dry_run: bool,
    pub delete_original: bool,
    pub rename_original: bool,
    pub overwrite: bool,
    pub reencode_target: bool,
    pub target_codec: String,
    /// Output container: `auto` (recommended for codec), `source`, `mkv`, `mp4`, or `webm`.
    pub container: String,
    pub target_bitrate: String,
    pub max_width: u32,
    pub size_preset: String,
    pub size_limit: ConvertSizeLimit,
    pub encoder_override: String,
    /// `0` = ffmpeg default (all cores).
    pub cpu_threads: u32,
    pub subprocess_priority: String,
    /// Extra audio output after encode: `none`, `flac`, `aac`, or `opus`.
    pub audio_extract: String,
    /// In-encode subtitle handling: `none`, `soft`, or `burn`.
    pub subtitle_mode: String,
    /// When true, ffprobe must find video and audio on the encoded output before the source is removed.
    pub verify_output_video_audio: bool,
}

#[derive(Clone, Debug)]
pub struct ConvertInput {
    pub source_path: String,
}

#[derive(Clone, Debug)]
pub struct EncoderChoice {
    pub encoder: &'static str,
    pub codec: &'static str,
    pub hw_type: &'static str,
}

#[derive(Clone, Debug)]
pub struct ConvertPlanItem {
    pub input: PathBuf,
    pub output: PathBuf,
}

fn known_encoder(name: &str) -> Option<&'static str> {
    match name.trim() {
        "av1_nvenc" => Some("av1_nvenc"),
        "av1_amf" => Some("av1_amf"),
        "hevc_nvenc" => Some("hevc_nvenc"),
        "hevc_amf" => Some("hevc_amf"),
        "h264_nvenc" => Some("h264_nvenc"),
        "h264_amf" => Some("h264_amf"),
        "libsvtav1" => Some("libsvtav1"),
        "libx265" => Some("libx265"),
        "libx264" => Some("libx264"),
        _ => None,
    }
}

pub fn codec_for_encoder(encoder: &str) -> &'static str {
    if encoder.contains("hevc") || encoder == "libx265" {
        "hevc"
    } else if encoder.contains("h264") || encoder == "libx264" {
        "h264"
    } else {
        "av1"
    }
}

pub fn cpu_encoder_for_target(target_codec: &str) -> &'static str {
    match normalize_target_codec(target_codec) {
        "hevc" => "libx265",
        "h264" => "libx264",
        _ => "libsvtav1",
    }
}

fn encoder_chain_for_target(target_codec: &str) -> &'static [&'static str] {
    match normalize_target_codec(target_codec) {
        "hevc" => &["hevc_nvenc", "hevc_amf", "libx265"],
        "h264" => &["h264_nvenc", "h264_amf", "libx264"],
        _ => &["av1_nvenc", "av1_amf", "libsvtav1"],
    }
}

pub fn encoders_for_target(target_codec: &str) -> Vec<&'static str> {
    encoder_chain_for_target(target_codec).to_vec()
}

#[allow(dead_code)]
pub fn detect_encoder(ffmpeg_path: &str, target_codec: &str) -> EncoderChoice {
    detect_encoder_with_override(ffmpeg_path, "", target_codec)
}

pub fn detect_encoder_with_override(
    ffmpeg_path: &str,
    override_enc: &str,
    target_codec: &str,
) -> EncoderChoice {
    let target = normalize_target_codec(target_codec);
    let override_enc = override_enc.trim();
    let ffmpeg = resolve_executable(ffmpeg_path, "ffmpeg");
    if !override_enc.is_empty() {
        if let Some(enc) = known_encoder(override_enc) {
            if codec_for_encoder(enc) == target
                && encoder_supported(&ffmpeg, enc)
                && encoder_usable(&ffmpeg, enc)
            {
                return EncoderChoice {
                    encoder: enc,
                    codec: codec_for_encoder(enc),
                    hw_type: hw_type_for_encoder(enc),
                };
            }
        }
    }
    for enc in encoder_chain_for_target(target) {
        if encoder_supported(&ffmpeg, enc) && encoder_usable(&ffmpeg, enc) {
            return EncoderChoice {
                encoder: enc,
                codec: codec_for_encoder(enc),
                hw_type: hw_type_for_encoder(enc),
            };
        }
    }
    let cpu = cpu_encoder_for_target(target);
    EncoderChoice {
        encoder: cpu,
        codec: target,
        hw_type: "cpu",
    }
}

fn hw_type_for_encoder(encoder: &str) -> &'static str {
    match encoder {
        "av1_nvenc" | "hevc_nvenc" | "h264_nvenc" => "nvidia",
        "av1_amf" | "hevc_amf" | "h264_amf" => "amd",
        _ => "cpu",
    }
}

pub fn encoder_uses_hardware(enc: &EncoderChoice) -> bool {
    enc.hw_type != "cpu"
}

pub fn encoder_hw_vendor_label(hw_type: &str) -> &'static str {
    match hw_type {
        "nvidia" => "NVIDIA",
        "amd" => "AMD",
        _ => "CPU",
    }
}

pub fn target_codec_label(target_codec: &str) -> &'static str {
    match normalize_target_codec(target_codec) {
        "hevc" => "H.265",
        "h264" => "H.264",
        _ => "AV1",
    }
}

/// Human-readable label for a probed source codec (`h264` → `H.264`).
pub fn display_video_codec_label(codec: &str) -> String {
    let raw = codec.trim();
    if raw.is_empty() {
        return String::new();
    }
    let c = raw.to_ascii_lowercase().replace(['.', '-', ' ', '_'], "");
    match c.as_str() {
        "h264" | "avc" | "avc1" => "H.264".to_owned(),
        "hevc" | "h265" | "hev1" | "hvc1" => "H.265".to_owned(),
        "av1" | "av01" => "AV1".to_owned(),
        "vp9" | "vp09" => "VP9".to_owned(),
        "vp8" | "vp08" => "VP8".to_owned(),
        "mpeg4" | "mp4v" => "MPEG-4".to_owned(),
        "mpeg2video" | "mpeg2" => "MPEG-2".to_owned(),
        "mpeg1video" | "mpeg1" => "MPEG-1".to_owned(),
        _ => raw.to_ascii_uppercase(),
    }
}

pub fn output_suffix_for_target(target_codec: &str) -> &'static str {
    match normalize_target_codec(target_codec) {
        "hevc" => "H265",
        "h264" => "H264",
        _ => "AV1",
    }
}

pub fn encoder_indicator_label(enc: &EncoderChoice) -> String {
    if encoder_uses_hardware(enc) {
        format!(
            "GPU · {} ({})",
            enc.encoder,
            encoder_hw_vendor_label(enc.hw_type)
        )
    } else {
        format!("CPU · {}", enc.encoder)
    }
}

pub fn encoder_indicator_color(enc: &EncoderChoice) -> egui::Color32 {
    if encoder_uses_hardware(enc) {
        egui::Color32::from_rgb(118, 185, 0)
    } else {
        egui::Color32::from_rgb(255, 167, 38)
    }
}

fn encoder_supported(ffmpeg_bin: &str, encoder: &str) -> bool {
    let mut cmd = Command::new(ffmpeg_bin);
    no_console_window(&mut cmd);
    let Ok(out) = cmd
        .arg("-hide_banner")
        .arg("-encoders")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    text.contains(&encoder.to_ascii_lowercase())
}

fn smoke_test_rate_args(encoder: &str, hw_type: &str) -> Vec<&'static str> {
    if hw_type == "nvidia" {
        vec!["-preset", "p7", "-rc", "vbr", "-b:v", "2M"]
    } else if hw_type == "amd" {
        vec!["-usage", "0", "-quality", "70", "-rc", "1", "-b:v", "2M"]
    } else if encoder == "libsvtav1" {
        vec!["-preset", "8", "-b:v", "2M"]
    } else if encoder == "libx265" || encoder == "libx264" {
        vec!["-preset", "medium", "-b:v", "2M"]
    } else {
        vec!["-b:v", "2M"]
    }
}

fn encoder_usable(ffmpeg_bin: &str, encoder: &str) -> bool {
    let hw_type = hw_type_for_encoder(encoder);
    let vf = if hw_type == "cpu" {
        "format=yuv420p"
    } else {
        "format=nv12"
    };
    let mut cmd = Command::new(ffmpeg_bin);
    no_console_window(&mut cmd);
    cmd.arg("-hide_banner").arg("-loglevel").arg("error").args([
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=1280x720:rate=30:duration=0.5",
        "-vf",
        vf,
        "-c:v",
        encoder,
        "-f",
        "null",
        "-",
    ]);
    cmd.args(smoke_test_rate_args(encoder, hw_type));
    let Ok(out) = cmd.stdout(Stdio::null()).stderr(Stdio::null()).output() else {
        return false;
    };
    out.status.success()
}

fn parse_bitrate_to_bps(bitrate: &str) -> Option<i64> {
    let normalized = bitrate.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if let Some(num) = normalized.strip_suffix('m') {
        return Some((num.trim().parse::<f64>().ok()? * 1_000_000.0).round() as i64);
    }
    if let Some(num) = normalized.strip_suffix('k') {
        return Some((num.trim().parse::<f64>().ok()? * 1_000.0).round() as i64);
    }
    normalized.parse().ok()
}

fn preset_bitrate_multiplier(preset: &str) -> f64 {
    match preset.trim().to_ascii_lowercase().as_str() {
        "light" => 1.25,
        "aggressive" => 0.72,
        _ => 1.0,
    }
}

fn effective_target_bitrate_bps(cfg: &ConvertConfig) -> i64 {
    let base = parse_bitrate_to_bps(&cfg.target_bitrate).unwrap_or(BITRATE_FALLBACK_BPS);
    (base as f64 * preset_bitrate_multiplier(&cfg.size_preset)).round() as i64
}

fn select_pixel_format(hw_type: &str) -> &'static str {
    if hw_type == "cpu" {
        "yuv420p"
    } else {
        "nv12"
    }
}

fn build_video_filter_chain(hw_type: &str, max_video_width: u32, pix_fmt: &str) -> String {
    let w = max_video_width;
    let scale = if hw_type == "amd" {
        format!(
            "scale='trunc(min({w},iw)/64)*64':'trunc(trunc(min({w},iw)/64)*64*ih/iw/16)*16',format={pix_fmt}"
        )
    } else {
        format!("scale='min({w},iw)':-2:force_original_aspect_ratio=decrease,format={pix_fmt}")
    };
    format!("{scale},setsar=1")
}

/// Per-job ffmpeg thread cap when CPU threads is left on auto (`0`).
const CONVERT_AUTO_THREADS_PER_JOB_CAP: u32 = 4;

/// Resolves stored converter CPU-thread setting into a per-transcode thread budget.
///
/// `0` (auto) uses ~75% of logical CPUs split across parallel jobs, capped per job so
/// multiple encodes do not each claim every core (or GPU session).
pub fn resolve_convert_cpu_threads(configured: u32, parallel: usize) -> u32 {
    let parallel = parallel.clamp(1, 6) as u32;
    let cpus = logical_cpu_count();
    if configured > 0 {
        return configured.clamp(1, cpus);
    }
    let budget = cpus.saturating_mul(3).saturating_div(4).max(1);
    (budget / parallel).clamp(1, CONVERT_AUTO_THREADS_PER_JOB_CAP)
}

pub fn effective_cpu_threads(configured: u32) -> Option<u32> {
    if configured == 0 {
        None
    } else {
        Some(configured.clamp(1, logical_cpu_count()))
    }
}

fn append_cpu_thread_args(cmd: &mut Command, enc: &EncoderChoice, cpu_threads: u32) {
    let Some(threads) = effective_cpu_threads(cpu_threads) else {
        return;
    };
    let threads_s = threads.to_string();
    cmd.arg("-threads").arg(&threads_s);
    match enc.encoder {
        "libsvtav1" => {
            cmd.args(["-svtav1-params", &format!("lp={threads_s}")]);
        }
        "libx265" => {
            cmd.args([
                "-x265-params",
                &format!("pools={threads_s}:frame-threads=1"),
            ]);
        }
        _ => {}
    }
}

fn append_encoder_rate_control(cmd: &mut Command, enc: &EncoderChoice, target_bitrate_bps: i64) {
    let maxrate = (target_bitrate_bps as f64 * BITRATE_MAXRATE_MULTIPLIER).round() as i64;
    let bufsize = (target_bitrate_bps as f64 * BITRATE_BUFSIZE_MULTIPLIER).round() as i64;
    match enc.hw_type {
        "nvidia" => {
            cmd.args(["-preset", "p7", "-rc", "vbr"]);
            cmd.arg("-b:v").arg(target_bitrate_bps.to_string());
            cmd.arg("-maxrate").arg(maxrate.to_string());
            cmd.arg("-bufsize").arg(bufsize.to_string());
        }
        "amd" => {
            cmd.args([
                "-usage",
                "0",
                "-quality",
                "70",
                "-profile:v",
                "1",
                "-rc",
                "1",
                "-align",
                "3",
            ]);
            cmd.arg("-b:v").arg(target_bitrate_bps.to_string());
        }
        _ => match enc.encoder {
            "libsvtav1" => {
                cmd.args(["-preset", "8", "-g", "240"]);
                cmd.arg("-b:v").arg(target_bitrate_bps.to_string());
            }
            "libx265" => {
                cmd.args(["-preset", "medium", "-tag:v", "hvc1"]);
                cmd.arg("-b:v").arg(target_bitrate_bps.to_string());
            }
            "libx264" => {
                cmd.args(["-preset", "medium", "-profile:v", "high"]);
                cmd.arg("-b:v").arg(target_bitrate_bps.to_string());
            }
            _ => {
                cmd.arg("-b:v").arg(target_bitrate_bps.to_string());
            }
        },
    }
}

pub fn recommended_container_for_target(target_codec: &str) -> &'static str {
    match normalize_target_codec(target_codec) {
        "av1" => "mkv",
        _ => "mp4",
    }
}

fn planned_output_extension(input: &Path, cfg: &ConvertConfig) -> String {
    match cfg.container.as_str() {
        "mkv" => "mkv".to_owned(),
        "mp4" => "mp4".to_owned(),
        "webm" => "webm".to_owned(),
        "source" => input
            .extension()
            .and_then(|s| s.to_str())
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| VIDEO_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
            .unwrap_or_else(|| recommended_container_for_target(&cfg.target_codec).to_owned()),
        // "auto" or any unrecognized value → recommended for target codec
        _ => recommended_container_for_target(&cfg.target_codec).to_owned(),
    }
}

pub fn collect_plan(inputs: &[ConvertInput], cfg: &ConvertConfig) -> Vec<ConvertPlanItem> {
    let mut out = Vec::new();
    for item in inputs {
        let p = PathBuf::from(item.source_path.trim());
        if p.is_file() {
            maybe_push_file(&mut out, &p, cfg);
        } else if p.is_dir() {
            walk_dir(&mut out, &p, cfg);
        }
    }
    out
}

fn walk_dir(out: &mut Vec<ConvertPlanItem>, root: &Path, cfg: &ConvertConfig) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if cfg.recursive {
                walk_dir(out, &p, cfg);
            }
            continue;
        }
        maybe_push_file(out, &p, cfg);
    }
}

fn planned_output_directory(input: &Path, cfg: &ConvertConfig) -> PathBuf {
    if cfg.delete_original && cfg.rename_original {
        return input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    let configured = cfg.output_dir.trim();
    if !configured.is_empty() {
        return PathBuf::from(configured);
    }
    input
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn maybe_push_file(out: &mut Vec<ConvertPlanItem>, input: &Path, cfg: &ConvertConfig) {
    if !is_video_path(input) {
        return;
    }
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let ext = planned_output_extension(input, cfg);
    let suffix = output_suffix_for_target(&cfg.target_codec);
    let output = planned_output_directory(input, cfg).join(format!("{stem}-{suffix}.{ext}"));
    out.push(ConvertPlanItem {
        input: input.to_path_buf(),
        output,
    });
}

fn default_audio_for_output(output: &Path, target_codec: &str) -> (&'static str, &'static str) {
    let ext = output
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("mp4" | "m4v") => ("aac", "128k"),
        Some("mkv") if normalize_target_codec(target_codec) == "av1" => ("libopus", "64k"),
        Some("webm") => ("libopus", "64k"),
        _ if normalize_target_codec(target_codec) == "av1" => ("libopus", "64k"),
        _ => ("aac", "128k"),
    }
}

fn append_container_mux_args(cmd: &mut Command, output: &Path, enc: &EncoderChoice) {
    let ext = output
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    let Some(ext) = ext else {
        return;
    };
    if !matches!(ext.as_str(), "mp4" | "m4v") {
        return;
    }
    match enc.codec {
        "av1" => {
            cmd.args(["-tag:v", "av01"]);
        }
        "hevc" => {
            cmd.args(["-tag:v", "hvc1"]);
        }
        "h264" => {
            cmd.args(["-tag:v", "avc1"]);
        }
        _ => {}
    }
}

pub fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| VIDEO_EXTS.iter().any(|x| x.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

pub fn codec_matches_target(input_codec: &str, target_codec: &str) -> bool {
    let c = input_codec
        .trim()
        .to_ascii_lowercase()
        .replace(['.', '-', ' ', '_'], "");
    match normalize_target_codec(target_codec) {
        "av1" => c == "av1" || c.contains("av01"),
        "hevc" => {
            c.contains("hevc")
                || c.contains("h265")
                || c == "hev1"
                || c == "hvc1"
                || c.contains("x265")
        }
        "h264" => {
            c.contains("h264")
                || c.contains("avc")
                || c == "avc1"
                || c.contains("x264")
                || c == "264"
        }
        _ => false,
    }
}

fn parse_ffprobe_fraction(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() || value == "0/0" {
        return None;
    }
    if let Some((num, den)) = value.split_once('/') {
        let num: f64 = num.trim().parse().ok()?;
        let den: f64 = den.trim().parse().ok()?;
        if den > 0.0 {
            return Some(num / den);
        }
        return None;
    }
    value.parse().ok()
}

fn parse_bitrate_field(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse().ok()
}

#[derive(serde::Deserialize, Default)]
struct FfprobeFormatTags {
    #[serde(default)]
    creation_time: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    encoder: Option<String>,
}

#[derive(serde::Deserialize)]
struct FfprobeMediaFormat {
    format_name: Option<String>,
    bit_rate: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    tags: Option<FfprobeFormatTags>,
}

#[derive(serde::Deserialize)]
struct FfprobeMediaStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    bit_rate: Option<String>,
}

#[derive(serde::Deserialize)]
struct FfprobeMediaRoot {
    streams: Vec<FfprobeMediaStream>,
    format: Option<FfprobeMediaFormat>,
}

pub fn probe_input_media(file_path: &Path, ffprobe_path: &str) -> Option<ConvertInputMedia> {
    let ffprobe = resolve_executable(ffprobe_path, "ffprobe");
    let mut cmd = Command::new(ffprobe);
    no_console_window(&mut cmd);
    let out = cmd
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,codec_name,width,height,avg_frame_rate,r_frame_rate,bit_rate",
            "-show_entries",
            "format=format_name,bit_rate,duration,tags",
            "-of",
            "json",
            &file_path.to_string_lossy(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let root: FfprobeMediaRoot = serde_json::from_slice(&out.stdout).ok()?;
    let video = root
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"));
    let audio = root
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"));
    if video.is_none() && audio.is_none() {
        return None;
    }
    let audio_codec = audio
        .and_then(|s| s.codec_name.as_deref())
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty());
    let format = root.format.unwrap_or(FfprobeMediaFormat {
        format_name: None,
        bit_rate: None,
        duration: None,
        tags: None,
    });

    let codec = video
        .and_then(|s| s.codec_name.as_deref())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let width = video.and_then(|s| s.width).filter(|w| *w > 0);
    let height = video.and_then(|s| s.height).filter(|h| *h > 0);
    let fps = video
        .and_then(|s| s.avg_frame_rate.as_deref())
        .and_then(parse_ffprobe_fraction)
        .or_else(|| {
            video
                .and_then(|s| s.r_frame_rate.as_deref())
                .and_then(parse_ffprobe_fraction)
        })
        .filter(|f| *f > 0.0)
        .map(|f| f as f32);
    let duration_ms = format
        .duration
        .as_deref()
        .and_then(|d| d.trim().parse::<f64>().ok())
        .filter(|d| *d > 0.0)
        .map(|d| (d * 1000.0) as u64);

    let mut bitrate_bps = video
        .and_then(|s| s.bit_rate.as_deref())
        .and_then(parse_bitrate_field)
        .or_else(|| {
            audio
                .and_then(|s| s.bit_rate.as_deref())
                .and_then(parse_bitrate_field)
        })
        .or_else(|| format.bit_rate.as_deref().and_then(parse_bitrate_field));

    if bitrate_bps.is_none() {
        if let (Some(ms), Ok(meta)) = (duration_ms, std::fs::metadata(file_path)) {
            let secs = ms as f64 / 1000.0;
            if secs > 0.0 {
                bitrate_bps = Some(((meta.len() as f64 * 8.0 / secs) * 0.9) as u64);
            }
        }
    }

    Some(ConvertInputMedia {
        codec,
        width,
        height,
        fps,
        bitrate_bps,
        duration_ms,
        format_name: format
            .format_name
            .map(|n| n.split(',').next().unwrap_or(&n).trim().to_owned())
            .filter(|n| !n.is_empty()),
        creation_time: format.tags.as_ref().and_then(|t| {
            t.creation_time
                .as_deref()
                .or(t.date.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        }),
        encoder_tag: format
            .tags
            .and_then(|t| t.encoder)
            .map(|e| e.trim().to_owned())
            .filter(|e| !e.is_empty()),
        audio_codec,
    })
}

pub fn input_codec(file_path: &Path, ffprobe_path: &str) -> Option<String> {
    probe_input_media(file_path, ffprobe_path).and_then(|m| {
        if m.codec.is_empty() {
            None
        } else {
            Some(m.codec)
        }
    })
}

pub fn input_duration_ms(file_path: &Path, ffprobe_path: &str) -> Option<u64> {
    probe_input_media(file_path, ffprobe_path).and_then(|m| m.duration_ms)
}

/// True when the input has at least one subtitle stream.
pub fn input_has_subtitle_streams(file_path: &Path, ffprobe_path: &str) -> bool {
    let ffprobe = resolve_executable(ffprobe_path, "ffprobe");
    let mut cmd = Command::new(ffprobe);
    no_console_window(&mut cmd);
    let out = cmd
        .args([
            "-v",
            "error",
            "-select_streams",
            "s",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
            &file_path.to_string_lossy(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => !String::from_utf8_lossy(&o.stdout).trim().is_empty(),
        _ => false,
    }
}

fn escape_ffmpeg_subtitle_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace(':', "\\:")
}

/// Appends a burn-in subtitle filter when subtitle mode is `burn`.
pub fn build_video_filter_with_subtitles(
    hw_type: &str,
    max_video_width: u32,
    pix_fmt: &str,
    subtitle_mode: &str,
    input: &Path,
    has_subtitles: bool,
) -> String {
    let mut vf = build_video_filter_chain(hw_type, max_video_width, pix_fmt);
    if subtitle_mode == "burn" && has_subtitles {
        let escaped = escape_ffmpeg_subtitle_path(input);
        vf = format!("{vf},subtitles='{escaped}'");
    }
    vf
}

/// Maps subtitle streams during encode when mode is `soft`.
pub fn append_soft_subtitle_maps(cmd: &mut Command) {
    cmd.args([
        "-map", "0:v:0", "-map", "0:a?", "-map", "0:s?", "-c:s", "copy",
    ]);
}

fn audio_extract_output_path(video_output: &Path, mode: &str) -> Option<PathBuf> {
    let stem = video_output.file_stem()?;
    let parent = video_output.parent()?;
    let ext = match mode {
        "flac" => "flac",
        "aac" => "m4a",
        "opus" => "opus",
        _ => return None,
    };
    Some(parent.join(format!("{}.{}", stem.to_string_lossy(), ext)))
}

fn audio_extract_codec_args(mode: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match mode {
        "flac" => Some(("flac", vec![])),
        "aac" => Some(("aac", vec!["-b:a", "192k"])),
        "opus" => Some(("libopus", vec!["-b:a", "128k"])),
        _ => None,
    }
}

/// Extracts an audio sidecar next to the encoded video output.
pub fn extract_audio_sidecar<F>(
    source: &Path,
    video_output: &Path,
    cfg: &ConvertConfig,
    cancel_flag: Option<Arc<AtomicBool>>,
    mut on_line: F,
) -> Result<PathBuf>
where
    F: FnMut(String),
{
    let mode = crate::config::normalize_convert_audio_extract(&cfg.audio_extract);
    let Some(out_path) = audio_extract_output_path(video_output, &mode) else {
        return Err(anyhow!("Audio extract disabled"));
    };
    let Some((codec, extra)) = audio_extract_codec_args(&mode) else {
        return Err(anyhow!("Audio extract disabled"));
    };
    if out_path.exists() && !cfg.overwrite {
        return Err(anyhow!(
            "Audio extract output already exists: {}",
            out_path.display()
        ));
    }
    let ffmpeg = resolve_executable(&cfg.ffmpeg_path, "ffmpeg");
    let mut cmd = Command::new(ffmpeg);
    apply_subprocess_launch(
        &mut cmd,
        normalize_subprocess_priority(&cfg.subprocess_priority),
    );
    cmd.arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg(if cfg.overwrite { "-y" } else { "-n" })
        .arg("-i")
        .arg(source)
        .arg("-vn")
        .arg("-c:a")
        .arg(codec);
    for arg in extra {
        cmd.arg(arg);
    }
    cmd.arg(&out_path);
    let mut child = cmd.spawn()?;
    let st = loop {
        if cancel_flag
            .as_ref()
            .is_some_and(|f| f.load(Ordering::Relaxed))
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("Cancelled by user."));
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    if !st.success() {
        return Err(anyhow!("Audio extract failed with status {st}"));
    }
    on_line(format!("audio_extract={}", out_path.display()));
    Ok(out_path)
}

pub fn parse_ffmpeg_out_time_secs(value: &str) -> Option<f64> {
    let value = value.trim();
    let (hours, rest) = value.split_once(':')?;
    let (minutes, seconds) = rest.split_once(':')?;
    let h: f64 = hours.parse().ok()?;
    let m: f64 = minutes.parse().ok()?;
    let s: f64 = seconds.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

pub fn parse_ffmpeg_speed(value: &str) -> Option<f64> {
    let mut cleaned = value.trim().to_ascii_lowercase();
    if cleaned.ends_with('x') {
        cleaned.pop();
    }
    let speed: f64 = cleaned.parse().ok()?;
    if speed > 0.0 {
        Some(speed)
    } else {
        None
    }
}

/// PNG frame via ffmpeg. Used by GUI and web thumbnail fallback.
pub fn extract_thumbnail_png_bytes(file_path: &Path, ffmpeg_path: &str) -> Option<Vec<u8>> {
    for seek in ["00:00:00.000", "00:00:01.000", "00:00:03.000"] {
        if let Some(bytes) = extract_thumbnail_png_bytes_at(file_path, ffmpeg_path, seek) {
            return Some(bytes);
        }
    }
    None
}

fn extract_thumbnail_png_bytes_at(
    file_path: &Path,
    ffmpeg_path: &str,
    seek: &str,
) -> Option<Vec<u8>> {
    let ffmpeg = resolve_executable(ffmpeg_path, "ffmpeg");
    let mut cmd = Command::new(ffmpeg);
    no_console_window(&mut cmd);
    let out = cmd
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            seek,
            "-i",
            &file_path.to_string_lossy(),
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.len() < 64 {
        return None;
    }
    Some(out.stdout)
}

pub fn extract_thumbnail(file_path: &Path, ffmpeg_path: &str) -> Option<egui::ColorImage> {
    let png = extract_thumbnail_png_bytes(file_path, ffmpeg_path)?;
    let dyn_img = image::load_from_memory(&png).ok()?;
    let rgba = dyn_img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}

fn paths_same_directory(a: &Path, b: &Path) -> bool {
    let (Some(pa), Some(pb)) = (a.parent(), b.parent()) else {
        return false;
    };
    match (pa.canonicalize(), pb.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => pa == pb,
    }
}

/// Target path when restoring the source basename in the output directory (Python parity).
///
/// Important: keeps the *output* extension so the container matches the filename.
pub fn resolve_original_output_path(input: &Path, output: &Path) -> Option<PathBuf> {
    if !paths_same_directory(input, output) {
        return None;
    }
    let parent = output.parent()?;
    let stem = input.file_stem()?.to_str()?.trim();
    if stem.is_empty() {
        return None;
    }
    let out_ext = output.extension().and_then(|s| s.to_str()).map(str::trim);
    let file_name = match out_ext {
        Some(ext) if !ext.is_empty() => format!("{stem}.{ext}"),
        _ => stem.to_owned(),
    };
    let original_path = parent.join(file_name);
    if original_path == output {
        return None;
    }
    Some(original_path)
}

fn remove_partial_output(output: &Path) {
    if output.is_file() {
        let _ = std::fs::remove_file(output);
    }
}

/// Minimum encoded duration as a fraction of the source duration (allows small container rounding drift).
const OUTPUT_DURATION_MIN_RATIO: f64 = 0.95;
/// Extra slack when comparing source vs output duration (milliseconds).
const OUTPUT_DURATION_SLACK_MS: u64 = 2_000;

fn output_duration_looks_complete(
    input_duration_ms: Option<u64>,
    output_duration_ms: Option<u64>,
) -> Result<(), String> {
    let Some(in_ms) = input_duration_ms.filter(|d| *d > 0) else {
        return Ok(());
    };
    let Some(out_ms) = output_duration_ms.filter(|d| *d > 0) else {
        return Err(
            "Encoded output is missing duration metadata (often means the file is incomplete)."
                .to_owned(),
        );
    };
    let threshold_ms = ((in_ms as f64) * OUTPUT_DURATION_MIN_RATIO).floor() as u64;
    if out_ms.saturating_add(OUTPUT_DURATION_SLACK_MS) < threshold_ms {
        return Err(format!(
            "Encoded output duration ({:.1}s) is much shorter than the source ({:.1}s); the encode may be incomplete.",
            out_ms as f64 / 1000.0,
            in_ms as f64 / 1000.0
        ));
    }
    Ok(())
}

fn output_decodes_without_premature_end(output: &Path, ffmpeg_path: &str) -> Result<()> {
    let ffmpeg = resolve_executable(ffmpeg_path, "ffmpeg");
    let mut cmd = Command::new(ffmpeg);
    no_console_window(&mut cmd);
    let out = cmd
        .args([
            "-v",
            "error",
            "-i",
            &output.to_string_lossy(),
            "-f",
            "null",
            "-",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("file ended prematurely")
        || lower.contains("invalid data found when processing input")
    {
        let hint = stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("decode error");
        return Err(anyhow!(
            "Encoded output appears incomplete or corrupt ({hint})."
        ));
    }
    if !out.status.success() {
        let hint = stderr.trim();
        if hint.is_empty() {
            return Err(anyhow!("Encoded output failed decode verification."));
        }
        return Err(anyhow!("Encoded output failed decode verification: {hint}"));
    }
    Ok(())
}

/// Returns an error when the encoded file looks truncated or is missing required streams.
pub fn validate_convert_output(
    input: &Path,
    output: &Path,
    ffprobe_path: &str,
    ffmpeg_path: &str,
    verify_streams: bool,
) -> Result<()> {
    let output_media = probe_input_media(output, ffprobe_path)
        .ok_or_else(|| anyhow!("Could not probe encoded output."))?;
    if verify_streams {
        if output_media.codec.is_empty() {
            return Err(anyhow!("Encoded output has no video stream."));
        }
        if output_media.audio_codec.is_none() {
            return Err(anyhow!("Encoded output has no audio stream."));
        }
    }
    let input_media = probe_input_media(input, ffprobe_path);
    let input_duration_ms = input_media.and_then(|m| m.duration_ms);
    if let Err(msg) = output_duration_looks_complete(input_duration_ms, output_media.duration_ms) {
        return Err(anyhow!("{msg}"));
    }
    let output_duration_known = output_media.duration_ms.is_some_and(|d| d > 0);
    if !output_duration_known {
        output_decodes_without_premature_end(output, ffmpeg_path)?;
    }
    Ok(())
}

fn finalize_output_file(plan: &ConvertPlanItem, cfg: &ConvertConfig) -> Result<PathBuf> {
    let output = plan.output.clone();
    let original_deleted = if cfg.delete_original {
        match std::fs::remove_file(&plan.input) {
            Ok(()) => true,
            Err(_) if !plan.input.exists() => true,
            Err(err) => {
                return Err(anyhow!(
                    "Failed to delete original {}: {err}",
                    plan.input.display()
                ));
            }
        }
    } else {
        !plan.input.exists()
    };

    if cfg.rename_original && original_deleted {
        if let Some(target) = resolve_original_output_path(&plan.input, &output) {
            if target.exists() && !cfg.overwrite {
                return Err(anyhow!(
                    "Cannot rename output to original name; file exists: {}",
                    target.display()
                ));
            }
            std::fs::rename(&output, &target).map_err(|err| {
                anyhow!(
                    "Failed to rename output to original name ({}): {err}",
                    target.display()
                )
            })?;
            return Ok(target);
        }
    }
    Ok(output)
}

fn append_resilient_input_args(cmd: &mut Command) {
    // Damaged H.264/AAC streams often still play in GUI players; these flags keep encode going.
    cmd.args([
        "-err_detect",
        "ignore_err",
        "-fflags",
        "+genpts+discardcorrupt",
        "-max_error_rate",
        "1.0",
    ]);
}

fn ffmpeg_status_error(status: std::process::ExitStatus, stderr_text: &str) -> anyhow::Error {
    if stderr_text.trim().is_empty() {
        return anyhow!("ffmpeg failed with status {status}");
    }
    let short = stderr_text
        .lines()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    anyhow!("ffmpeg failed with status {status}\n{short}")
}

struct FfmpegEncodePass<'a> {
    plan: &'a ConvertPlanItem,
    cfg: &'a ConvertConfig,
    enc: &'a EncoderChoice,
    target: &'a str,
    target_bitrate_bps: i64,
    vf: &'a str,
    subtitle_mode: &'a str,
    has_subtitles: bool,
    copy_audio: bool,
}

fn run_ffmpeg_convert_encode<F>(
    pass: &FfmpegEncodePass<'_>,
    cancel_flag: Option<&Arc<AtomicBool>>,
    mut on_line: F,
) -> Result<()>
where
    F: FnMut(String),
{
    let ffmpeg = resolve_executable(&pass.cfg.ffmpeg_path, "ffmpeg");
    let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let mut cmd = Command::new(ffmpeg);
    apply_subprocess_launch(
        &mut cmd,
        normalize_subprocess_priority(&pass.cfg.subprocess_priority),
    );
    cmd.arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg(if pass.cfg.overwrite { "-y" } else { "-n" })
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats");
    if let Some(threads) = effective_cpu_threads(pass.cfg.cpu_threads) {
        cmd.arg("-threads").arg(threads.to_string());
    }
    append_resilient_input_args(&mut cmd);
    cmd.arg("-i")
        .arg(&pass.plan.input)
        .arg("-vf")
        .arg(pass.vf)
        .arg("-c:v")
        .arg(pass.enc.encoder);
    append_cpu_thread_args(&mut cmd, pass.enc, pass.cfg.cpu_threads);
    if pass.enc.codec == "hevc" && pass.enc.hw_type != "cpu" {
        cmd.args(["-tag:v", "hvc1"]);
    }
    append_encoder_rate_control(&mut cmd, pass.enc, pass.target_bitrate_bps);
    if pass.subtitle_mode == "soft" && pass.has_subtitles {
        append_soft_subtitle_maps(&mut cmd);
    }
    if pass.copy_audio {
        cmd.args(["-c:a", "copy"]);
    } else {
        let (audio_codec, audio_bitrate) = default_audio_for_output(&pass.plan.output, pass.target);
        cmd.arg("-c:a")
            .arg(audio_codec)
            .arg("-b:a")
            .arg(audio_bitrate);
    }
    append_container_mux_args(&mut cmd, &pass.plan.output, pass.enc);
    cmd.arg(&pass.plan.output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("missing stdout"))?;
    let stderr = child.stderr.take();
    let stderr_capture = stderr_buf.clone();
    let stderr_thread = std::thread::spawn(move || {
        if let Some(mut s) = stderr {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut text);
            if let Ok(mut guard) = stderr_capture.lock() {
                *guard = text;
            }
        }
    });
    let mut rdr = std::io::BufReader::new(stdout);
    let mut line = String::new();
    loop {
        if cancel_flag.is_some_and(|f| f.load(Ordering::Relaxed)) {
            let _ = child.kill();
            let _ = child.wait();
            remove_partial_output(&pass.plan.output);
            return Err(anyhow!("Cancelled by user."));
        }
        line.clear();
        let n = std::io::BufRead::read_line(&mut rdr, &mut line)?;
        if n == 0 {
            break;
        }
        let t = line.trim().to_owned();
        if !t.is_empty() {
            on_line(t);
        }
    }
    let st = child.wait()?;
    let _ = stderr_thread.join();
    if !st.success() {
        let stderr_text = stderr_buf
            .lock()
            .ok()
            .map(|g| g.trim().to_owned())
            .unwrap_or_default();
        remove_partial_output(&pass.plan.output);
        return Err(ffmpeg_status_error(st, &stderr_text));
    }
    Ok(())
}

pub fn run_single<F>(
    plan: &ConvertPlanItem,
    cfg: &ConvertConfig,
    enc: &EncoderChoice,
    cancel_flag: Option<Arc<AtomicBool>>,
    mut on_line: F,
) -> Result<PathBuf>
where
    F: FnMut(String),
{
    let target = normalize_target_codec(&cfg.target_codec);
    if !cfg.reencode_target {
        if let Some(media) = probe_input_media(&plan.input, &cfg.ffprobe_path) {
            let within_max_width = media.width.map_or(true, |w| w <= cfg.max_width);
            if codec_matches_target(&media.codec, target) && enc.codec == target && within_max_width
            {
                on_line(format!(
                    "skip_reason=already {} input and re-encode disabled",
                    target_codec_label(target)
                ));
                return Err(anyhow!(
                    "Skipped: already {} ({})",
                    target_codec_label(target),
                    plan.input.display()
                ));
            }
        }
    }
    if cfg.dry_run {
        on_line(format!(
            "dry-run: {} -> {} [{}]",
            plan.input.display(),
            plan.output.display(),
            enc.encoder
        ));
        return Ok(plan.output.clone());
    }
    let target_bitrate_bps = effective_target_bitrate_bps(cfg);
    if cfg.size_limit.is_active() {
        if let Ok(meta) = std::fs::metadata(&plan.input) {
            let input_bytes = meta.len();
            if input_bytes > 0 {
                let media = probe_input_media(&plan.input, &cfg.ffprobe_path);
                let duration_secs = media
                    .as_ref()
                    .and_then(|m| m.duration_ms)
                    .map(|ms| ms as f64 / 1000.0)
                    .filter(|s| *s > 0.0)
                    .unwrap_or(3600.0);
                let estimated_out =
                    (target_bitrate_bps as f64 * duration_secs / 8.0).max(1.0) as u64;
                match pre_encode_decision(&cfg.size_limit, input_bytes, estimated_out) {
                    PreEncodeDecision::Proceed => {}
                    PreEncodeDecision::Skip => {
                        let msg = cfg.size_limit.violation_message(input_bytes, estimated_out);
                        on_line(format!(
                            "skip_reason=estimated output exceeds size limit ({})",
                            cfg.size_limit.limit_label(input_bytes)
                        ));
                        return Err(anyhow!("Skipped: estimated {msg}"));
                    }
                    PreEncodeDecision::Fail => {
                        let msg = cfg.size_limit.violation_message(input_bytes, estimated_out);
                        on_line(format!("size_limit=estimated output exceeds limit ({msg})"));
                        return Err(anyhow!("Failed: estimated {msg}"));
                    }
                }
            }
        }
    }
    if let Some(parent) = plan.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pix_fmt = select_pixel_format(enc.hw_type);
    let subtitle_mode = crate::config::normalize_convert_subtitle_mode(&cfg.subtitle_mode);
    let has_subtitles =
        subtitle_mode != "none" && input_has_subtitle_streams(&plan.input, &cfg.ffprobe_path);
    let vf = build_video_filter_with_subtitles(
        enc.hw_type,
        cfg.max_width,
        pix_fmt,
        &subtitle_mode,
        &plan.input,
        has_subtitles,
    );
    let cancel = cancel_flag.as_ref();
    let mut pass = FfmpegEncodePass {
        plan,
        cfg,
        enc,
        target,
        target_bitrate_bps,
        vf: &vf,
        subtitle_mode: &subtitle_mode,
        has_subtitles,
        copy_audio: false,
    };
    if let Err(err) = run_ffmpeg_convert_encode(&pass, cancel, |line| on_line(line)) {
        let err_text = err.to_string();
        if err_text.to_ascii_lowercase().contains("cancelled") {
            return Err(err);
        }
        if convert_failure_may_retry_audio_copy(&err_text) {
            on_line(
                "audio_reencode_failed; retrying with audio copy (damaged source bitstream)"
                    .to_owned(),
            );
            pass.copy_audio = true;
            run_ffmpeg_convert_encode(&pass, cancel, |line| on_line(line))
                .map_err(|retry_err| anyhow!("{err_text}\nAudio-copy retry failed: {retry_err}"))?;
        } else {
            return Err(err);
        }
    }
    if let Err(err) = validate_convert_output(
        &plan.input,
        &plan.output,
        &cfg.ffprobe_path,
        &cfg.ffmpeg_path,
        cfg.verify_output_video_audio,
    ) {
        remove_partial_output(&plan.output);
        return Err(err.context("Encode output failed verification; the original file was kept."));
    }
    finalize_output_file(plan, cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(output_dir: &Path, target_codec: &str, recommended: bool) -> ConvertConfig {
        ConvertConfig {
            ffmpeg_path: String::new(),
            ffprobe_path: String::new(),
            output_dir: output_dir.to_string_lossy().to_string(),
            recursive: false,
            dry_run: true,
            delete_original: false,
            rename_original: false,
            overwrite: false,
            reencode_target: false,
            target_codec: target_codec.to_owned(),
            container: if recommended {
                "auto".to_owned()
            } else {
                "source".to_owned()
            },
            target_bitrate: String::new(),
            max_width: 1920,
            size_preset: "balanced".to_owned(),
            size_limit: ConvertSizeLimit::default(),
            encoder_override: String::new(),
            cpu_threads: 0,
            subprocess_priority: "normal".to_owned(),
            audio_extract: "none".to_owned(),
            subtitle_mode: "none".to_owned(),
            verify_output_video_audio: true,
        }
    }

    #[test]
    fn effective_cpu_threads_zero_means_auto() {
        assert_eq!(effective_cpu_threads(0), None);
    }

    #[test]
    fn convert_failure_detects_incomplete_mp4() {
        let err = "ffmpeg failed with status exit code: 1\nmoov atom not found";
        assert!(convert_failure_is_unreadable_source(err));
        let msg = format_convert_failure(err);
        assert!(msg.contains("incomplete or corrupt"));
        assert!(msg.contains("moov atom not found"));
    }

    #[test]
    fn convert_failure_midstream_damage_is_not_unreadable() {
        let err = "ffmpeg failed with status exit code: 1\n\
Invalid NAL unit size (-807092710 > 14555).\n\
Error splitting the input into NAL units.\n\
Invalid data found when processing input";
        assert!(!convert_failure_is_unreadable_source(err));
        assert!(convert_failure_may_retry_audio_copy(err));
        let msg = format_convert_failure(err);
        assert!(msg.contains("bitstream damage"));
    }

    #[test]
    fn resolve_convert_cpu_threads_auto_splits_parallel_jobs() {
        let cpus = logical_cpu_count();
        let single = resolve_convert_cpu_threads(0, 1);
        assert!(single >= 1);
        assert!(single <= CONVERT_AUTO_THREADS_PER_JOB_CAP);
        if cpus >= 8 {
            assert!(single <= cpus / 2);
        }
        let dual = resolve_convert_cpu_threads(0, 2);
        assert!(dual <= single);
        assert!(dual >= 1);
    }

    #[test]
    fn resolve_convert_cpu_threads_honors_explicit_value() {
        assert_eq!(resolve_convert_cpu_threads(6, 3), 6);
    }

    #[test]
    fn effective_cpu_threads_clamps_to_logical_cpus() {
        let max = logical_cpu_count();
        assert_eq!(effective_cpu_threads(max.saturating_add(8)), Some(max));
    }

    #[test]
    fn collect_plan_detects_video_files_av1() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let movie = root.join("movie.mp4");
        let note = root.join("note.txt");
        std::fs::write(&movie, b"x").expect("write movie");
        std::fs::write(&note, b"x").expect("write note");
        let cfg = test_config(root, "av1", true);
        let plan = collect_plan(
            &[ConvertInput {
                source_path: root.to_string_lossy().to_string(),
            }],
            &cfg,
        );
        assert_eq!(plan.len(), 1);
        assert!(plan[0].output.to_string_lossy().ends_with("movie-AV1.mkv"));
    }

    #[test]
    fn collect_plan_uses_h264_suffix_and_mp4() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let movie = root.join("movie.mp4");
        std::fs::write(&movie, b"x").expect("write movie");
        let cfg = test_config(root, "h264", true);
        let plan = collect_plan(
            &[ConvertInput {
                source_path: movie.to_string_lossy().to_string(),
            }],
            &cfg,
        );
        assert_eq!(plan.len(), 1);
        assert!(plan[0].output.to_string_lossy().ends_with("movie-H264.mp4"));
    }

    #[test]
    fn collect_plan_keeps_source_extension_when_not_recommended() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let movie = root.join("movie.mp4");
        std::fs::write(&movie, b"x").expect("write movie");
        let cfg = test_config(root, "av1", false);
        let plan = collect_plan(
            &[ConvertInput {
                source_path: movie.to_string_lossy().to_string(),
            }],
            &cfg,
        );
        assert_eq!(plan.len(), 1);
        assert!(plan[0].output.to_string_lossy().ends_with("movie-AV1.mp4"));
    }

    #[test]
    fn collect_plan_defaults_to_input_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source_dir = tmp.path().join("media");
        std::fs::create_dir_all(&source_dir).expect("create source dir");
        let movie = source_dir.join("movie.mp4");
        std::fs::write(&movie, b"x").expect("write movie");
        let cfg = test_config(Path::new(""), "av1", true);
        let plan = collect_plan(
            &[ConvertInput {
                source_path: movie.to_string_lossy().to_string(),
            }],
            &cfg,
        );
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].output, source_dir.join("movie-AV1.mkv"));
    }

    #[test]
    fn collect_plan_uses_input_directory_for_inplace_replace() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source_dir = tmp.path().join("media");
        let output_dir = tmp.path().join("downloads");
        std::fs::create_dir_all(&source_dir).expect("create source dir");
        std::fs::create_dir_all(&output_dir).expect("create output dir");
        let movie = source_dir.join("movie.mp4");
        std::fs::write(&movie, b"x").expect("write movie");
        let mut cfg = test_config(&output_dir, "av1", true);
        cfg.delete_original = true;
        cfg.rename_original = true;
        let plan = collect_plan(
            &[ConvertInput {
                source_path: movie.to_string_lossy().to_string(),
            }],
            &cfg,
        );
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].output, source_dir.join("movie-AV1.mkv"));
    }

    #[test]
    fn codec_matches_target_handles_aliases() {
        assert!(codec_matches_target("av01", "av1"));
        assert!(codec_matches_target("hevc", "hevc"));
        assert!(codec_matches_target("h264", "h264"));
        assert!(codec_matches_target("avc1", "h264"));
        assert!(!codec_matches_target("h264", "av1"));
    }

    #[test]
    fn normalize_target_codec_maps_aliases() {
        assert_eq!(normalize_target_codec("H.265"), "hevc");
        assert_eq!(normalize_target_codec("h264"), "h264");
        assert_eq!(normalize_target_codec(""), "av1");
    }

    #[test]
    fn resolve_original_output_path_requires_same_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let other = tempfile::tempdir().expect("tempdir2");
        let input = tmp.path().join("movie.mp4");
        let output = tmp.path().join("movie-AV1.mkv");
        assert_eq!(
            resolve_original_output_path(&input, &output),
            Some(tmp.path().join("movie.mkv"))
        );
        let output_else = other.path().join("movie-AV1.mkv");
        assert!(resolve_original_output_path(&input, &output_else).is_none());
    }

    #[test]
    fn parse_ffmpeg_out_time_handles_hms() {
        assert_eq!(
            parse_ffmpeg_out_time_secs("00:01:23.456789"),
            Some(83.456789)
        );
        assert_eq!(parse_ffmpeg_out_time_secs("01:02:03"), Some(3723.0));
    }

    #[test]
    fn parse_ffmpeg_speed_handles_x_suffix() {
        assert_eq!(parse_ffmpeg_speed("2.35x"), Some(2.35));
    }

    #[test]
    fn parse_bitrate_to_bps_handles_suffixes() {
        assert_eq!(parse_bitrate_to_bps("2500k"), Some(2_500_000));
        assert_eq!(parse_bitrate_to_bps("2.5m"), Some(2_500_000));
        assert_eq!(parse_bitrate_to_bps("1800000"), Some(1_800_000));
    }

    #[test]
    fn build_video_filter_chain_uses_nv12_for_nvidia() {
        let vf = build_video_filter_chain("nvidia", 1920, "nv12");
        assert!(vf.contains("format=nv12"));
        assert!(vf.contains("setsar=1"));
    }

    #[test]
    fn encoder_indicator_label_distinguishes_gpu_and_cpu() {
        let gpu = EncoderChoice {
            encoder: "av1_nvenc",
            codec: "av1",
            hw_type: "nvidia",
        };
        assert_eq!(encoder_indicator_label(&gpu), "GPU · av1_nvenc (NVIDIA)");
        let cpu = EncoderChoice {
            encoder: "libsvtav1",
            codec: "av1",
            hw_type: "cpu",
        };
        assert_eq!(encoder_indicator_label(&cpu), "CPU · libsvtav1");
    }

    #[test]
    fn parse_ffprobe_fraction_handles_rational_fps() {
        assert_eq!(
            parse_ffprobe_fraction("30000/1001"),
            Some(29.970_029_970_029_97)
        );
        assert_eq!(parse_ffprobe_fraction("24/1"), Some(24.0));
        assert!(parse_ffprobe_fraction("0/0").is_none());
    }

    #[test]
    fn subtitle_burn_filter_appends_to_chain() {
        let input = Path::new("C:/videos/sample.mkv");
        let vf = build_video_filter_with_subtitles("cpu", 1920, "yuv420p", "burn", input, true);
        assert!(vf.contains("subtitles="));
        assert!(vf.contains("sample.mkv"));
    }

    #[test]
    fn soft_subtitle_maps_include_copy() {
        let mut cmd = Command::new("ffmpeg");
        append_soft_subtitle_maps(&mut cmd);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"-map".to_owned()));
        assert!(args.contains(&"0:s?".to_owned()));
        assert!(args.contains(&"-c:s".to_owned()));
        assert!(args.contains(&"copy".to_owned()));
    }

    #[test]
    fn output_duration_looks_complete_rejects_short_or_missing() {
        assert!(output_duration_looks_complete(Some(600_000), Some(590_000)).is_ok());
        assert!(output_duration_looks_complete(Some(600_000), Some(500_000)).is_err());
        assert!(output_duration_looks_complete(Some(600_000), None).is_err());
        assert!(output_duration_looks_complete(None, None).is_ok());
    }

    #[test]
    fn audio_extract_output_paths() {
        let out = Path::new("C:/out/video.mkv");
        assert_eq!(
            audio_extract_output_path(out, "flac")
                .unwrap()
                .extension()
                .and_then(|e| e.to_str()),
            Some("flac")
        );
        assert!(audio_extract_output_path(out, "none").is_none());
    }

    #[test]
    fn display_video_codec_label_maps_common_ffprobe_names() {
        assert_eq!(display_video_codec_label("h264"), "H.264");
        assert_eq!(display_video_codec_label("avc1"), "H.264");
        assert_eq!(display_video_codec_label("hevc"), "H.265");
        assert_eq!(display_video_codec_label("av1"), "AV1");
        assert_eq!(display_video_codec_label("vp9"), "VP9");
        assert_eq!(display_video_codec_label("prores"), "PRORES");
        assert_eq!(display_video_codec_label(""), "");
    }
}
