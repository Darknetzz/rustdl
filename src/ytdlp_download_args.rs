use std::io::Write;
use std::path::Path;

use crate::app_parsing::split_cli_like;
use crate::config::AppSettings;
use crate::download_organize::compose_output_template;
use crate::models::QueueItem;
use crate::profiles::{find_profile, ProfileStore};
use crate::ytdlp;

/// Permissive `-f` used when the quality selector matches nothing (muxed HLS, video-only leftovers).
pub const FALLBACK_FORMAT_SELECTOR: &str = "bestvideo*+bestaudio/bestvideo*/best*";

/// Cookies and impersonation flags for `yt-dlp -J` when resolving URLs.
pub fn metadata_extra_args(settings: &AppSettings) -> Vec<String> {
    let mut args = ytdlp::impersonate_args_from_setting(&settings.yt_dlp_impersonate);
    args.extend(ytdlp::cookie_args_from_setting(&settings.yt_dlp_cookies));
    args
}

/// yt-dlp `-f` filter clauses for minimum video height and frame rate (`0` = no minimum).
pub fn download_min_format_suffix(min_height: u32, min_fps: u32) -> String {
    let mut parts = Vec::new();
    if min_height > 0 {
        parts.push(format!("height>={min_height}"));
    }
    if min_fps > 0 {
        parts.push(format!("fps>={min_fps}"));
    }
    parts
        .into_iter()
        .map(|p| format!("[{p}]"))
        .collect::<String>()
}

fn apply_min_suffix_to_selector(selector: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        return selector.to_owned();
    }
    let lower = selector.trim().to_ascii_lowercase();
    if lower.starts_with("bestaudio") {
        return selector.to_owned();
    }
    format!("{selector}{suffix}")
}

/// Applies minimum height/fps filters to each selector in a yt-dlp `-f` format string.
pub fn apply_download_min_requirements(fmt: &str, min_height: u32, min_fps: u32) -> String {
    let suffix = download_min_format_suffix(min_height, min_fps);
    if suffix.is_empty() {
        return fmt.to_owned();
    }
    fmt.split('/')
        .map(|fallback| {
            fallback
                .split('+')
                .map(|sel| apply_min_suffix_to_selector(sel.trim(), &suffix))
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn build_quality_format(max_height: Option<u32>, min_height: u32, min_fps: u32) -> String {
    let video = match max_height {
        Some(h) => format!("bestvideo*[height<={h}]"),
        None => "bestvideo*".to_owned(),
    };
    let combined = match max_height {
        Some(h) => format!("best[height<={h}]"),
        None => "best".to_owned(),
    };
    let fmt = format!("{video}+bestaudio/{combined}");
    apply_download_min_requirements(&fmt, min_height, min_fps)
}

pub fn quality_format_args(settings: &AppSettings) -> Vec<String> {
    let preset = settings.quality_preset.trim().to_ascii_lowercase();
    let custom = settings.quality_format_custom.trim();
    let min_h = settings.download_min_height;
    let min_fps = settings.download_min_fps;
    let fmt = match preset.as_str() {
        "1080p" => build_quality_format(Some(1080), min_h, min_fps),
        "720p" => build_quality_format(Some(720), min_h, min_fps),
        "audio" => "bestaudio/best".to_owned(),
        "custom" if !custom.is_empty() => apply_download_min_requirements(custom, min_h, min_fps),
        _ => build_quality_format(None, min_h, min_fps),
    };
    vec!["-f".to_owned(), fmt]
}

pub fn output_filename_template(settings: &AppSettings) -> String {
    compose_output_template(settings)
}

/// Builds the output template for a queue row, honoring optional per-item profile overrides.
pub fn output_filename_template_for_item(
    settings: &AppSettings,
    profile_store: &ProfileStore,
    item: &QueueItem,
) -> String {
    let mut effective = settings.clone();
    if let Some(name) = item.profile_override.as_deref() {
        if let Some(profile) = find_profile(profile_store, name.trim()) {
            profile.apply_to(&mut effective);
        }
    }
    compose_output_template(&effective)
}

/// Retry flags, cookies, user extra args, quality, merge format, and postprocessors.
/// User "Extra args" follow early flags and can override (yt-dlp: last wins).
pub fn build_download_extra_args(settings: &AppSettings) -> Vec<String> {
    let mut args = Vec::new();
    if settings.yt_dlp_unlimited_retries {
        args.push("--retries".to_owned());
        args.push("infinite".to_owned());
        args.push("--fragment-retries".to_owned());
        args.push("infinite".to_owned());
        args.push("--extractor-retries".to_owned());
        args.push("infinite".to_owned());
        args.push("--file-access-retries".to_owned());
        args.push("infinite".to_owned());
    } else {
        let n = settings.yt_dlp_retry_count.to_string();
        args.push("--retries".to_owned());
        args.push(n.clone());
        args.push("--fragment-retries".to_owned());
        args.push(n.clone());
        args.push("--extractor-retries".to_owned());
        args.push(n.clone());
        args.push("--file-access-retries".to_owned());
        args.push(n);
    }
    if settings.yt_dlp_socket_timeout_secs > 0 {
        args.push("--socket-timeout".to_owned());
        args.push(settings.yt_dlp_socket_timeout_secs.to_string());
    }
    if settings.yt_dlp_retry_sleep_secs > 0 {
        let sleep = settings.yt_dlp_retry_sleep_secs.to_string();
        args.push("--retry-sleep".to_owned());
        args.push(sleep.clone());
        args.push("--retry-sleep".to_owned());
        args.push("fragment:linear=1:10".to_owned());
    }
    args.push("--continue".to_owned());
    args.extend(ytdlp::impersonate_args_from_setting(
        &settings.yt_dlp_impersonate,
    ));
    args.extend(ytdlp::cookie_args_from_setting(&settings.yt_dlp_cookies));
    if !settings.yt_proxy.trim().is_empty() {
        args.push("--proxy".to_owned());
        args.push(settings.yt_proxy.trim().to_owned());
    }
    if !settings.yt_limit_rate.trim().is_empty() {
        args.push("--limit-rate".to_owned());
        args.push(settings.yt_limit_rate.trim().to_owned());
    }
    if !settings.yt_download_archive.trim().is_empty() {
        args.push("--download-archive".to_owned());
        args.push(settings.yt_download_archive.trim().to_owned());
    }
    if settings.yt_sponsorblock_remove {
        args.push("--sponsorblock-remove".to_owned());
        args.push("all".to_owned());
    } else if !settings.yt_sponsorblock_mark.trim().is_empty() {
        args.push("--sponsorblock-mark".to_owned());
        args.push(settings.yt_sponsorblock_mark.trim().to_owned());
    }
    args.extend(split_cli_like(&settings.yt_dlp_extra_args));
    args.extend(quality_format_args(settings));
    match settings
        .merge_container
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" => {
            args.push("--merge-output-format".to_owned());
            args.push("mp4".to_owned());
        }
        "mkv" => {
            args.push("--merge-output-format".to_owned());
            args.push("mkv".to_owned());
        }
        "webm" => {
            args.push("--merge-output-format".to_owned());
            args.push("webm".to_owned());
        }
        _ => {}
    }
    if settings.yt_ignore_errors {
        args.push("--ignore-errors".to_owned());
    }
    if settings.yt_restrict_filenames {
        args.push("--restrict-filenames".to_owned());
    }
    if settings.yt_write_info_json {
        args.push("--write-info-json".to_owned());
    }
    if settings.yt_write_auto_subs {
        args.push("--write-auto-subs".to_owned());
    }
    if settings.embed_thumbnail {
        args.push("--embed-thumbnail".to_owned());
    }
    if settings.yt_embed_metadata {
        args.push("--embed-metadata".to_owned());
    }
    if settings.ffmpeg_extract_audio_mp3 {
        args.push("--extract-audio".to_owned());
        args.push("--audio-format".to_owned());
        args.push("mp3".to_owned());
    } else if settings.ffmpeg_remux_mp4 {
        args.push("--remux-video".to_owned());
        args.push("mp4".to_owned());
    }
    let mut post_args = settings.ffmpeg_post_args.trim().to_owned();
    if settings.ffmpeg_faststart {
        if !post_args.is_empty() {
            post_args.push(' ');
        }
        post_args.push_str("-movflags +faststart");
    }
    if !post_args.trim().is_empty() {
        args.push("--postprocessor-args".to_owned());
        args.push(post_args);
    }
    args
}

/// Builds download args for a queue row, honoring optional per-item profile/format overrides.
pub fn build_download_extra_args_for_item(
    settings: &AppSettings,
    profile_store: &ProfileStore,
    item: &QueueItem,
) -> Vec<String> {
    let mut effective = settings.clone();
    if let Some(name) = item.profile_override.as_deref() {
        if let Some(profile) = find_profile(profile_store, name.trim()) {
            profile.apply_to(&mut effective);
        }
    }
    let mut args = build_download_extra_args(&effective);
    if let Some(fmt) = item.format_override.as_deref() {
        let fmt = fmt.trim();
        if !fmt.is_empty() {
            strip_cli_flag_pair(&mut args, "-f");
            args.push("-f".to_owned());
            args.push(fmt.to_owned());
        }
    }
    args
}

/// Drops `-f` / `--format` selectors and appends a permissive muxed-HLS-friendly fallback.
pub fn with_fallback_format_args(args: &[String]) -> Vec<String> {
    let mut out = args.to_vec();
    strip_cli_flag_pair(&mut out, "-f");
    strip_cli_flag_pair(&mut out, "--format");
    out.push("-f".to_owned());
    out.push(FALLBACK_FORMAT_SELECTOR.to_owned());
    out
}

/// Args for re-downloading: bypass archive skip and replace an existing output file.
pub fn build_redownload_extra_args(settings: &AppSettings) -> Vec<String> {
    let mut args = build_download_extra_args(settings);
    strip_cli_flag_pair(&mut args, "--download-archive");
    strip_cli_flag(&mut args, "--no-overwrites");
    if !args.iter().any(|a| a == "--force-overwrites") {
        args.push("--force-overwrites".to_owned());
    }
    args
}

fn strip_cli_flag_pair(args: &mut Vec<String>, flag: &str) {
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            args.remove(i);
            if i < args.len() {
                args.remove(i);
            }
        } else {
            i += 1;
        }
    }
}

fn strip_cli_flag(args: &mut Vec<String>, flag: &str) {
    args.retain(|a| a != flag);
}

fn archive_line_matches_video_id(line: &str, id: &str) -> bool {
    line == id || line.split_whitespace().last() == Some(id)
}

/// Remove yt-dlp archive lines for the given video ids (`extractor id` or bare id).
pub fn remove_video_ids_from_download_archive(
    archive_path: &str,
    video_ids: &[String],
) -> std::io::Result<bool> {
    let path = archive_path.trim();
    if path.is_empty() {
        return Ok(false);
    }
    let ids: Vec<&str> = video_ids
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if ids.is_empty() {
        return Ok(false);
    }
    let file = Path::new(path);
    if !file.is_file() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(file)?;
    let had_trailing_nl = content.ends_with('\n');
    let mut removed = false;
    let kept: Vec<&str> = content
        .lines()
        .filter(|line| {
            let t = line.trim();
            if t.is_empty() {
                return true;
            }
            let drop = ids.iter().any(|id| archive_line_matches_video_id(t, id));
            if drop {
                removed = true;
            }
            !drop
        })
        .collect();
    if !removed {
        return Ok(false);
    }
    let mut out = std::fs::File::create(file)?;
    for (i, line) in kept.iter().enumerate() {
        if i > 0 {
            writeln!(out)?;
        }
        write!(out, "{line}")?;
    }
    if had_trailing_nl || !kept.is_empty() {
        writeln!(out)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppSettings;

    fn base_settings() -> AppSettings {
        AppSettings::default()
    }

    #[test]
    fn quality_preset_best_includes_muxed_video() {
        let s = base_settings();
        assert_eq!(
            quality_format_args(&s),
            vec!["-f".to_owned(), "bestvideo*+bestaudio/best".to_owned()]
        );
    }

    #[test]
    fn quality_preset_1080p() {
        let mut s = base_settings();
        s.quality_preset = "1080p".to_owned();
        assert_eq!(
            quality_format_args(&s),
            vec![
                "-f".to_owned(),
                "bestvideo*[height<=1080]+bestaudio/best[height<=1080]".to_owned()
            ]
        );
    }

    #[test]
    fn quality_preset_with_min_height_and_fps() {
        let mut s = base_settings();
        s.quality_preset = "1080p".to_owned();
        s.download_min_height = 720;
        s.download_min_fps = 30;
        assert_eq!(
            quality_format_args(&s),
            vec![
                "-f".to_owned(),
                "bestvideo*[height<=1080][height>=720][fps>=30]+bestaudio/best[height<=1080][height>=720][fps>=30]"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn quality_preset_custom_with_min_requirements() {
        let mut s = base_settings();
        s.quality_preset = "custom".to_owned();
        s.quality_format_custom = "bestvideo*+bestaudio/best".to_owned();
        s.download_min_height = 1080;
        assert_eq!(
            quality_format_args(&s),
            vec![
                "-f".to_owned(),
                "bestvideo*[height>=1080]+bestaudio/best[height>=1080]".to_owned()
            ]
        );
    }

    #[test]
    fn audio_preset_ignores_min_requirements() {
        let mut s = base_settings();
        s.quality_preset = "audio".to_owned();
        s.download_min_height = 1080;
        s.download_min_fps = 60;
        assert_eq!(
            quality_format_args(&s),
            vec!["-f".to_owned(), "bestaudio/best".to_owned()]
        );
    }

    #[test]
    fn quality_preset_custom() {
        let mut s = base_settings();
        s.quality_preset = "custom".to_owned();
        s.quality_format_custom = "best".to_owned();
        assert_eq!(
            quality_format_args(&s),
            vec!["-f".to_owned(), "best".to_owned()]
        );
    }

    #[test]
    fn output_template_default_when_empty() {
        let mut s = base_settings();
        s.output_filename_template = String::new();
        assert_eq!(
            output_filename_template(&s),
            crate::config::DEFAULT_OUTPUT_FILENAME_TEMPLATE
        );
    }

    #[test]
    fn output_template_for_item_profile_override() {
        use crate::profiles::{builtin_profiles, ProfileStore};
        let mut s = base_settings();
        s.download_organize_folder = crate::download_organize::FOLDER_UPLOADER.to_owned();
        let store = ProfileStore {
            user_profiles: vec![],
        };
        let item = QueueItem {
            profile_override: Some("Audio only".to_owned()),
            ..Default::default()
        };
        let prof = builtin_profiles()
            .into_iter()
            .find(|p| p.name == "Audio only")
            .unwrap();
        let tpl_with_profile = output_filename_template_for_item(&s, &store, &item);
        let mut s2 = s.clone();
        prof.apply_to(&mut s2);
        assert_eq!(tpl_with_profile, compose_output_template(&s2));
    }

    #[test]
    fn with_fallback_format_args_replaces_selector() {
        let args = vec![
            "-f".to_owned(),
            "bestvideo*+bestaudio/best".to_owned(),
            "--continue".to_owned(),
        ];
        let fallback = with_fallback_format_args(&args);
        assert!(!fallback.contains(&"bestvideo*+bestaudio/best".to_owned()));
        assert_eq!(
            fallback
                .windows(2)
                .find(|w| w[0] == "-f")
                .map(|w| w[1].as_str()),
            Some(FALLBACK_FORMAT_SELECTOR)
        );
    }

    #[test]
    fn download_args_network_resilience_flags() {
        let mut s = base_settings();
        s.yt_dlp_socket_timeout_secs = 60;
        s.yt_dlp_retry_sleep_secs = 3;
        let args = build_download_extra_args(&s);
        assert!(args.contains(&"--socket-timeout".to_owned()));
        assert!(args.contains(&"60".to_owned()));
        assert!(args.contains(&"--retry-sleep".to_owned()));
        assert!(args.contains(&"3".to_owned()));
        assert!(args.contains(&"fragment:linear=1:10".to_owned()));
        assert!(args.contains(&"--continue".to_owned()));
        assert!(args.contains(&"--extractor-retries".to_owned()));
        assert!(args.contains(&"--file-access-retries".to_owned()));
    }

    #[test]
    fn download_args_skips_zero_timeout_and_sleep() {
        let mut s = base_settings();
        s.yt_dlp_socket_timeout_secs = 0;
        s.yt_dlp_retry_sleep_secs = 0;
        let args = build_download_extra_args(&s);
        assert!(!args.contains(&"--socket-timeout".to_owned()));
        assert!(!args.contains(&"--retry-sleep".to_owned()));
        assert!(args.contains(&"--continue".to_owned()));
    }

    #[test]
    fn download_args_retries_and_sponsorblock() {
        let mut s = base_settings();
        s.yt_dlp_unlimited_retries = true;
        s.yt_sponsorblock_remove = true;
        let args = build_download_extra_args(&s);
        assert!(args.contains(&"--retries".to_owned()));
        assert!(args.contains(&"infinite".to_owned()));
        assert!(args.contains(&"--sponsorblock-remove".to_owned()));
        assert!(args.contains(&"all".to_owned()));
    }

    #[test]
    fn download_args_merge_mp4_and_postprocessor() {
        let mut s = base_settings();
        s.merge_container = "mp4".to_owned();
        s.ffmpeg_faststart = true;
        let args = build_download_extra_args(&s);
        assert!(args.contains(&"--merge-output-format".to_owned()));
        assert!(args.contains(&"mp4".to_owned()));
        let pp = args
            .windows(2)
            .find(|w| w[0] == "--postprocessor-args")
            .map(|w| w[1].clone());
        assert_eq!(pp.as_deref(), Some("-movflags +faststart"));
    }

    #[test]
    fn metadata_extra_args_uses_impersonate_and_cookies() {
        let mut s = base_settings();
        s.yt_dlp_impersonate = "chrome".to_owned();
        s.yt_dlp_cookies = "firefox".to_owned();
        let args = metadata_extra_args(&s);
        assert!(args.contains(&"--impersonate".to_owned()));
        assert!(args.contains(&"chrome".to_owned()));
        assert!(args.contains(&"--cookies-from-browser".to_owned()));
        assert!(args.contains(&"firefox".to_owned()));
    }

    #[test]
    fn download_args_limit_rate() {
        let mut s = base_settings();
        s.yt_limit_rate = "4M".to_owned();
        let args = build_download_extra_args(&s);
        assert!(args.contains(&"--limit-rate".to_owned()));
        assert!(args.contains(&"4M".to_owned()));
    }

    #[test]
    fn redownload_args_drop_archive_and_force_overwrite() {
        let mut s = base_settings();
        s.yt_download_archive = "/tmp/archive.txt".to_owned();
        let args = build_redownload_extra_args(&s);
        assert!(!args.contains(&"--download-archive".to_owned()));
        assert!(!args.contains(&"/tmp/archive.txt".to_owned()));
        assert!(args.contains(&"--force-overwrites".to_owned()));
    }

    #[test]
    fn remove_video_ids_from_download_archive() {
        let dir = std::env::temp_dir().join("rustdl_archive_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("dl.txt");
        std::fs::write(&archive, "youtube abc123\nvimeo xyz\n").unwrap();
        let path = archive.to_string_lossy();
        let changed =
            super::remove_video_ids_from_download_archive(&path, &["abc123".to_owned()]).unwrap();
        assert!(changed);
        let left = std::fs::read_to_string(&archive).unwrap();
        assert!(left.contains("vimeo xyz"));
        assert!(!left.contains("abc123"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
