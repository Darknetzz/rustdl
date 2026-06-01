use std::path::PathBuf;
use std::process;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::config::{load_settings, AppSettings};
use crate::profiles::{all_profiles, find_profile, load_profiles};
use crate::ytdlp;

pub struct CliDownloadOptions {
    pub url: String,
    pub profile: Option<String>,
    pub output_dir: Option<String>,
}

fn quality_format_args(settings: &AppSettings) -> Vec<String> {
    let preset = settings.quality_preset.trim().to_ascii_lowercase();
    let custom = settings.quality_format_custom.trim();
    let fmt = match preset.as_str() {
        "1080p" => "bestvideo[height<=1080]+bestaudio/best[height<=1080]",
        "720p" => "bestvideo[height<=720]+bestaudio/best[height<=720]",
        "audio" => "bestaudio/best",
        "custom" if !custom.is_empty() => custom,
        _ => "bestvideo+bestaudio/best",
    };
    vec!["-f".to_owned(), fmt.to_owned()]
}

pub fn build_download_extra_args(settings: &AppSettings) -> Vec<String> {
    let mut args = Vec::new();
    if settings.yt_dlp_unlimited_retries {
        args.push("--retries".to_owned());
        args.push("infinite".to_owned());
        args.push("--fragment-retries".to_owned());
        args.push("infinite".to_owned());
    } else {
        let n = settings.yt_dlp_retry_count.to_string();
        args.push("--retries".to_owned());
        args.push(n.clone());
        args.push("--fragment-retries".to_owned());
        args.push(n);
    }
    let imp = settings.yt_dlp_impersonate.trim();
    if !imp.is_empty() {
        args.push("--impersonate".to_owned());
        args.push(imp.to_owned());
    }
    args.extend(ytdlp::cookie_args_from_setting(&settings.yt_dlp_cookies));
    if !settings.yt_proxy.trim().is_empty() {
        args.push("--proxy".to_owned());
        args.push(settings.yt_proxy.trim().to_owned());
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
    args.extend(crate::app_parsing::split_cli_like(&settings.yt_dlp_extra_args));
    args.extend(quality_format_args(settings));
    match settings.merge_container.trim().to_ascii_lowercase().as_str() {
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

pub fn output_template(settings: &AppSettings, _output_dir: &str) -> String {
    let template = settings.output_filename_template.trim();
    let template = if template.is_empty() {
        crate::config::DEFAULT_OUTPUT_FILENAME_TEMPLATE
    } else {
        template
    };
    template.to_owned()
}

pub async fn run_headless_download(opts: CliDownloadOptions) -> Result<()> {
    let mut settings = load_settings();
    if let Some(name) = opts.profile {
        let store = load_profiles();
        let profile = find_profile(&store, &name)
            .ok_or_else(|| anyhow!("unknown profile: {name}"))?;
        profile.apply_to(&mut settings);
    }
    let output_dir = opts
        .output_dir
        .unwrap_or_else(|| settings.output_dir.clone());
    if !PathBuf::from(&output_dir).is_dir() {
        return Err(anyhow!("output directory does not exist: {output_dir}"));
    }
    let extra = build_download_extra_args(&settings);
    let yt_dlp = if settings.yt_dlp_path.trim().is_empty() {
        "yt-dlp".to_owned()
    } else {
        settings.yt_dlp_path.trim().to_owned()
    };
    let ffmpeg = settings.ffmpeg_path.trim().to_owned();
    let cancel = Arc::new(AtomicBool::new(false));
    let url = opts.url.trim().to_owned();
    if url.is_empty() {
        return Err(anyhow!("URL is empty"));
    }
    println!("Downloading {url} -> {output_dir}");
    let template = output_template(&settings, &output_dir);
    ytdlp::stream_download_with_bins(
        &url,
        &output_dir,
        &template,
        &extra,
        &yt_dlp,
        &ffmpeg,
        cancel,
        |line| {
            if line.contains("download") || line.starts_with(ytdlp::PROGRESS_PREFIX) {
                println!("{line}");
            }
        },
    )
    .await?;
    println!("Done.");
    Ok(())
}

pub fn parse_cli_download_args(args: &[String]) -> Result<Option<CliDownloadOptions>> {
    let mut url = None;
    let mut profile = None;
    let mut output_dir = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--download" => {
                i += 1;
                url = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow!("--download requires a URL"))?
                        .clone(),
                );
            }
            "--profile" => {
                i += 1;
                profile = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow!("--profile requires a name"))?
                        .clone(),
                );
            }
            "--output-dir" => {
                i += 1;
                output_dir = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow!("--output-dir requires a path"))?
                        .clone(),
                );
            }
            s => return Err(anyhow!("unknown option: {s}")),
        }
        i += 1;
    }
    Ok(url.map(|u| CliDownloadOptions {
        url: u,
        profile,
        output_dir,
    }))
}

pub fn run_cli_or_exit(args: Vec<String>) -> bool {
    if args.is_empty() {
        return false;
    }
    match args[0].as_str() {
        "--version" | "-V" => {
            println!("rustdl {}", crate::pkg_version::VERSION);
            println!("Build: {}", crate::pkg_version::BUILD_DATE);
            true
        }
        "--help" | "-h" => {
            print_help();
            true
        }
        "--download" => match parse_cli_download_args(&args) {
            Ok(Some(opts)) => {
                let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
                if let Err(e) = rt.block_on(run_headless_download(opts)) {
                    eprintln!("Download failed: {e:#}");
                    process::exit(1);
                }
                true
            }
            Ok(None) => {
                eprintln!("--download requires a URL");
                process::exit(2);
            }
            Err(e) => {
                eprintln!("{e:#}");
                process::exit(2);
            }
        },
        "--list-profiles" => {
            for p in all_profiles(&load_profiles()) {
                println!("{}", p.name);
            }
            true
        }
        s if s.starts_with('-') => {
            eprintln!("Unknown option: {s}");
            eprintln!("Try `rustdl --help`.");
            process::exit(2);
        }
        _ => {
            eprintln!("Unexpected argument: {}", args[0]);
            eprintln!("Try `rustdl --help`.");
            process::exit(2);
        }
    }
}

fn print_help() {
    println!(
        "rustdl {} — desktop GUI for yt-dlp (egui).\n",
        crate::pkg_version::VERSION
    );
    println!("Usage:");
    println!("  rustdl                          Start the graphical interface");
    println!("  rustdl --download URL [OPTS]    Headless download (no GUI)");
    println!("  rustdl --list-profiles          List download profile names");
    println!("  rustdl [OPTIONS]\n");
    println!("Options:");
    println!("  -h, --help           Print this help message");
    println!("  -V, --version        Print version and build date (UTC)");
    println!("  --download URL       Download a single URL using saved settings");
    println!("  --profile NAME       Apply named profile before download");
    println!("  --output-dir PATH    Override output folder");
}
