use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use eframe::egui;

use crate::external_tools::{no_console_window, resolve_executable};

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

#[derive(Clone, Debug, Default)]
pub struct ConvertInputMedia {
    pub codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub bitrate_bps: Option<u64>,
    pub duration_ms: Option<u64>,
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
    /// When true, outputs use a recommended container for the target codec.
    pub use_recommended_container: bool,
    pub target_bitrate: String,
    pub max_width: u32,
    pub size_preset: String,
    pub min_shrink_percent: f32,
    pub encoder_override: String,
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
    if cfg.use_recommended_container {
        return recommended_container_for_target(&cfg.target_codec).to_owned();
    }
    input
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| VIDEO_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or_else(|| recommended_container_for_target(&cfg.target_codec).to_owned())
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
        input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(&cfg.output_dir))
    } else {
        PathBuf::from(&cfg.output_dir)
    }
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

#[derive(serde::Deserialize)]
struct FfprobeMediaFormat {
    bit_rate: Option<String>,
    duration: Option<String>,
}

#[derive(serde::Deserialize)]
struct FfprobeMediaStream {
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
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,avg_frame_rate,r_frame_rate,bit_rate",
            "-show_entries",
            "format=bit_rate,duration",
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
    let stream = root.streams.into_iter().next()?;
    let format = root.format.unwrap_or(FfprobeMediaFormat {
        bit_rate: None,
        duration: None,
    });

    let codec = stream
        .codec_name
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let width = stream.width.filter(|w| *w > 0);
    let height = stream.height.filter(|h| *h > 0);
    let fps = stream
        .avg_frame_rate
        .as_deref()
        .and_then(parse_ffprobe_fraction)
        .or_else(|| {
            stream
                .r_frame_rate
                .as_deref()
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

    let mut bitrate_bps = stream
        .bit_rate
        .as_deref()
        .and_then(parse_bitrate_field)
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
pub fn resolve_original_output_path(input: &Path, output: &Path) -> Option<PathBuf> {
    if !paths_same_directory(input, output) {
        return None;
    }
    let file_name = input.file_name()?;
    let original_path = output.parent()?.join(file_name);
    if original_path == output {
        return None;
    }
    Some(original_path)
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
        if let Some(codec) = input_codec(&plan.input, &cfg.ffprobe_path) {
            if codec_matches_target(&codec, target) && enc.codec == target {
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
    if cfg.min_shrink_percent > 0.0 {
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
                let estimated_out = (target_bitrate_bps as f64 * duration_secs / 8.0).max(1.0);
                let max_allowed =
                    input_bytes as f64 * (1.0 - cfg.min_shrink_percent as f64 / 100.0);
                if estimated_out > max_allowed {
                    on_line(format!(
                        "skip_reason=estimated output {:.0} B exceeds shrink target ({:.0}% of source)",
                        estimated_out, 100.0 - cfg.min_shrink_percent
                    ));
                    return Err(anyhow!(
                        "Skipped: estimated output would not shrink by at least {:.0}%",
                        cfg.min_shrink_percent
                    ));
                }
            }
        }
    }
    if let Some(parent) = plan.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ffmpeg = resolve_executable(&cfg.ffmpeg_path, "ffmpeg");
    let pix_fmt = select_pixel_format(enc.hw_type);
    let vf = build_video_filter_chain(enc.hw_type, cfg.max_width, pix_fmt);
    let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let mut cmd = Command::new(ffmpeg);
    no_console_window(&mut cmd);
    cmd.arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg(if cfg.overwrite { "-y" } else { "-n" })
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg("-i")
        .arg(&plan.input)
        .arg("-vf")
        .arg(vf)
        .arg("-c:v")
        .arg(enc.encoder);
    if enc.codec == "hevc" && enc.hw_type != "cpu" {
        cmd.args(["-tag:v", "hvc1"]);
    }
    append_encoder_rate_control(&mut cmd, enc, target_bitrate_bps);
    let (audio_codec, audio_bitrate) = default_audio_for_output(&plan.output, target);
    cmd.arg("-c:a")
        .arg(audio_codec)
        .arg("-b:a")
        .arg(audio_bitrate);
    append_container_mux_args(&mut cmd, &plan.output, enc);
    cmd.arg(&plan.output)
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
        if cancel_flag
            .as_ref()
            .is_some_and(|f| f.load(Ordering::Relaxed))
        {
            let _ = child.kill();
            let _ = child.wait();
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
        if stderr_text.is_empty() {
            return Err(anyhow!("ffmpeg failed with status {st}"));
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
        return Err(anyhow!("ffmpeg failed with status {st}\n{short}"));
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
            use_recommended_container: recommended,
            target_bitrate: String::new(),
            max_width: 1920,
            size_preset: "balanced".to_owned(),
            min_shrink_percent: 0.0,
            encoder_override: String::new(),
        }
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
            Some(tmp.path().join("movie.mp4"))
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
}
