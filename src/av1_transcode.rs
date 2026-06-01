use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use eframe::egui;

use crate::external_tools::resolve_executable;

const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv"];

#[derive(Clone, Debug)]
pub struct Av1Config {
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    pub output_dir: String,
    pub recursive: bool,
    pub dry_run: bool,
    pub delete_original: bool,
    pub overwrite: bool,
    pub reencode_av1: bool,
    pub target_bitrate: String,
    pub max_width: u32,
}

#[derive(Clone, Debug)]
pub struct Av1Input {
    pub source_path: String,
}

#[derive(Clone, Debug)]
pub struct EncoderChoice {
    pub encoder: &'static str,
    pub codec: &'static str,
}

#[derive(Clone, Debug)]
pub struct Av1PlanItem {
    pub input: PathBuf,
    pub output: PathBuf,
}

pub fn detect_encoder(ffmpeg_path: &str) -> EncoderChoice {
    let ffmpeg = resolve_executable(ffmpeg_path, "ffmpeg");
    for enc in ["av1_nvenc", "av1_amf", "hevc_nvenc", "hevc_amf", "libsvtav1"] {
        if encoder_supported(&ffmpeg, enc) {
            return EncoderChoice {
                encoder: enc,
                codec: if enc.contains("hevc") { "hevc" } else { "av1" },
            };
        }
    }
    EncoderChoice {
        encoder: "libsvtav1",
        codec: "av1",
    }
}

fn encoder_supported(ffmpeg_bin: &str, encoder: &str) -> bool {
    let Ok(out) = Command::new(ffmpeg_bin)
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

pub fn collect_plan(inputs: &[Av1Input], cfg: &Av1Config) -> Vec<Av1PlanItem> {
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

fn walk_dir(out: &mut Vec<Av1PlanItem>, root: &Path, cfg: &Av1Config) {
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

fn maybe_push_file(out: &mut Vec<Av1PlanItem>, input: &Path, cfg: &Av1Config) {
    if !is_video_path(input) {
        return;
    }
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
    let output = PathBuf::from(&cfg.output_dir).join(format!("{stem}-AV1.mkv"));
    out.push(Av1PlanItem {
        input: input.to_path_buf(),
        output,
    });
}

fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| VIDEO_EXTS.iter().any(|x| x.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

pub fn input_codec(file_path: &Path, ffprobe_path: &str) -> Option<String> {
    let ffprobe = resolve_executable(ffprobe_path, "ffprobe");
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            &file_path.to_string_lossy(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_ascii_lowercase();
    if text.is_empty() { None } else { Some(text) }
}

pub fn input_duration_ms(file_path: &Path, ffprobe_path: &str) -> Option<u64> {
    let ffprobe = resolve_executable(ffprobe_path, "ffprobe");
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            &file_path.to_string_lossy(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let secs = String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().ok()?;
    if secs <= 0.0 {
        return None;
    }
    Some((secs * 1000.0) as u64)
}

pub fn extract_thumbnail(file_path: &Path, ffmpeg_path: &str) -> Option<egui::ColorImage> {
    let ffmpeg = resolve_executable(ffmpeg_path, "ffmpeg");
    let out = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            "00:00:01.000",
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
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    let dyn_img = image::load_from_memory(&out.stdout).ok()?;
    let rgba = dyn_img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

pub fn run_single<F>(
    plan: &Av1PlanItem,
    cfg: &Av1Config,
    enc: &EncoderChoice,
    cancel_flag: Option<Arc<AtomicBool>>,
    mut on_line: F,
) -> Result<()>
where
    F: FnMut(String),
{
    if !cfg.reencode_av1 {
        if let Some(codec) = input_codec(&plan.input, &cfg.ffprobe_path) {
            if codec == "av1" && enc.codec == "av1" {
                on_line("skip_reason=already AV1 input and re-encode disabled".to_owned());
                return Err(anyhow!("Skipped: already AV1 ({})", plan.input.display()));
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
        return Ok(());
    }
    if let Some(parent) = plan.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ffmpeg = resolve_executable(&cfg.ffmpeg_path, "ffmpeg");
    let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let mut cmd = Command::new(ffmpeg);
    cmd.arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg(if cfg.overwrite { "-y" } else { "-n" })
        .arg("-i")
        .arg(&plan.input)
        .arg("-map")
        .arg("0")
        .arg("-c:v")
        .arg(enc.encoder)
        .arg("-vf")
        .arg(format!(
            "scale='if(gt(iw,{w}),{w},iw)':-2:flags=lanczos",
            w = cfg.max_width
        ))
        .arg("-c:a")
        .arg("libopus")
        .arg("-b:a")
        .arg("64k");
    if !cfg.target_bitrate.trim().is_empty() {
        cmd.arg("-b:v").arg(cfg.target_bitrate.trim());
    }
    cmd.arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(&plan.output)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("missing stdout"))?;
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
    if cfg.delete_original {
        let _ = std::fs::remove_file(&plan.input);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_plan_detects_video_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let movie = root.join("movie.mp4");
        let note = root.join("note.txt");
        std::fs::write(&movie, b"x").expect("write movie");
        std::fs::write(&note, b"x").expect("write note");
        let cfg = Av1Config {
            ffmpeg_path: String::new(),
            ffprobe_path: String::new(),
            output_dir: root.to_string_lossy().to_string(),
            recursive: false,
            dry_run: true,
            delete_original: false,
            overwrite: false,
            reencode_av1: false,
            target_bitrate: String::new(),
            max_width: 1920,
        };
        let plan = collect_plan(
            &[Av1Input {
                source_path: root.to_string_lossy().to_string(),
            }],
            &cfg,
        );
        assert_eq!(plan.len(), 1);
        assert!(plan[0].input.to_string_lossy().ends_with("movie.mp4"));
        assert!(plan[0].output.to_string_lossy().ends_with("movie-AV1.mkv"));
    }
}

