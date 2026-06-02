use std::path::PathBuf;
use std::process;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::app_state::is_queueable_http_url;
use crate::config::{load_settings, AppSettings};
use crate::profiles::{all_profiles, find_profile, load_profiles};
use crate::ytdlp;
use crate::ytdlp_download_args::{build_download_extra_args, output_filename_template};

pub struct CliDownloadOptions {
    pub url: String,
    pub profile: Option<String>,
    pub output_dir: Option<String>,
    pub dry_run: bool,
}

pub async fn run_headless_download(opts: CliDownloadOptions) -> Result<()> {
    let mut settings = load_settings();
    if let Some(name) = opts.profile {
        let store = load_profiles();
        let profile =
            find_profile(&store, &name).ok_or_else(|| anyhow!("unknown profile: {name}"))?;
        profile.apply_to(&mut settings);
    }
    let output_dir = opts
        .output_dir
        .unwrap_or_else(|| settings.output_dir.clone());
    if !PathBuf::from(&output_dir).is_dir() {
        return Err(anyhow!("output directory does not exist: {output_dir}"));
    }
    let extra = build_download_extra_args(&settings);
    let yt_dlp = yt_dlp_bin(&settings);
    let ffmpeg = settings.ffmpeg_path.trim().to_owned();
    let url = opts.url.trim().to_owned();
    if !is_queueable_http_url(&url) {
        return Err(anyhow!("not a valid http(s) URL: {url}"));
    }
    let template = output_filename_template(&settings);
    if opts.dry_run {
        print_dry_run(&url, &output_dir, &template, &extra, &yt_dlp, &ffmpeg);
        return Ok(());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    println!("Downloading {url} -> {output_dir}");
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

fn yt_dlp_bin(settings: &AppSettings) -> String {
    if settings.yt_dlp_path.trim().is_empty() {
        "yt-dlp".to_owned()
    } else {
        settings.yt_dlp_path.trim().to_owned()
    }
}

fn print_dry_run(
    url: &str,
    output_dir: &str,
    template: &str,
    extra: &[String],
    yt_dlp: &str,
    ffmpeg: &str,
) {
    println!("dry-run: would download");
    println!("  url: {url}");
    println!("  output_dir: {output_dir}");
    println!("  template: {template}");
    if !ffmpeg.is_empty() {
        println!("  ffmpeg: {ffmpeg}");
    }
    println!("  command: {yt_dlp} --newline -o {output_dir}/{template} ...");
    for chunk in extra.chunks(2) {
        if chunk.len() == 2 {
            println!("    {} {}", chunk[0], chunk[1]);
        } else if !chunk.is_empty() {
            println!("    {}", chunk[0]);
        }
    }
}

pub fn parse_cli_download_args(args: &[String]) -> Result<Option<CliDownloadOptions>> {
    let mut url = None;
    let mut profile = None;
    let mut output_dir = None;
    let mut dry_run = false;
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
            "--dry-run" => {
                dry_run = true;
            }
            s => return Err(anyhow!("unknown option: {s}")),
        }
        i += 1;
    }
    Ok(url.map(|u| CliDownloadOptions {
        url: u,
        profile,
        output_dir,
        dry_run,
    }))
}

pub async fn run_headless_batch(urls: Vec<String>, opts: CliDownloadOptions) -> Result<()> {
    let total = urls.len();
    for (idx, url) in urls.into_iter().enumerate() {
        if total > 1 {
            println!("--- [{}/{}] ---", idx + 1, total);
        }
        let item_opts = CliDownloadOptions {
            url,
            profile: opts.profile.clone(),
            output_dir: opts.output_dir.clone(),
            dry_run: opts.dry_run,
        };
        run_headless_download(item_opts).await?;
    }
    Ok(())
}

fn read_urls_from_batch_source(source: &str) -> Result<Vec<String>> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("batch source is empty"));
    }
    if trimmed == "-" {
        use std::io::{self, Read};
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        return Ok(parse_url_lines(&buf));
    }
    if let Some(path) = trimmed.strip_prefix('@') {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("failed to read batch file {path}: {e}"))?;
        return Ok(parse_url_lines(&content));
    }
    Ok(vec![trimmed.to_owned()])
}

fn parse_url_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
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
                match read_urls_from_batch_source(&opts.url) {
                    Ok(urls) if urls.is_empty() => {
                        eprintln!("no URLs to download");
                        process::exit(2);
                    }
                    Ok(urls) if urls.len() > 1 => {
                        if let Err(e) = rt.block_on(run_headless_batch(urls, opts)) {
                            eprintln!("Download failed: {e:#}");
                            process::exit(1);
                        }
                    }
                    Ok(mut urls) => {
                        let mut opts = opts;
                        opts.url = urls.pop().unwrap_or_default();
                        if let Err(e) = rt.block_on(run_headless_download(opts)) {
                            eprintln!("Download failed: {e:#}");
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("{e:#}");
                        process::exit(2);
                    }
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
    println!(
        "  --download URL       Download using saved settings (URL, @file.txt, or - for stdin)"
    );
    println!("  --profile NAME       Apply named profile before download");
    println!("  --output-dir PATH    Override output folder");
    println!("  --dry-run            Print planned download args without executing");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_lines_skips_comments_and_blanks() {
        let text = "# comment\n\nhttps://a.test\n  https://b.test  \n";
        assert_eq!(
            parse_url_lines(text),
            vec!["https://a.test".to_owned(), "https://b.test".to_owned()]
        );
    }
}
