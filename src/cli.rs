use std::path::PathBuf;
use std::process;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::app_state::is_queueable_http_url;
use crate::config::{generate_web_auth_token, load_settings, save_settings, AppSettings};
use crate::profiles::{all_profiles, find_profile, load_profiles};
use crate::service::web::{resolve_web_bind_address, spawn_web_server_at, web_ui_browser_url};
use crate::service::RustdlService;
use crate::ytdlp;
use crate::ytdlp_download_args::{build_download_extra_args, output_filename_template};

pub struct CliDownloadOptions {
    pub url: String,
    pub profile: Option<String>,
    pub output_dir: Option<String>,
    pub dry_run: bool,
}

pub struct CliWebOnlyOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
}

/// Drop the console window when starting the egui UI (Explorer / shortcut launch).
#[cfg(windows)]
pub fn detach_console_for_gui() {
    use windows_sys::Win32::System::Console::FreeConsole;
    unsafe {
        let _ = FreeConsole();
    }
}

#[cfg(not(windows))]
pub fn detach_console_for_gui() {}

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

pub fn parse_web_only_args(args: &[String]) -> Result<CliWebOnlyOptions> {
    let mut host = None;
    let mut port = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--host" => {
                i += 1;
                host = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow!("--host requires a value"))?
                        .clone(),
                );
            }
            "--port" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or_else(|| anyhow!("--port requires a number"))?;
                let p: u16 = raw
                    .parse()
                    .map_err(|_| anyhow!("invalid port number: {raw}"))?;
                port = Some(p);
            }
            s => return Err(anyhow!("unknown option: {s}")),
        }
        i += 1;
    }
    Ok(CliWebOnlyOptions { host, port })
}

pub async fn run_headless_web(opts: CliWebOnlyOptions) -> Result<()> {
    let mut settings = load_settings();
    let bind = resolve_web_bind_address(
        opts.host.as_deref(),
        opts.port,
        &settings.web_bind_address,
    )
    .map_err(|e| anyhow!(e.message()))?;
    settings.web_bind_address = bind.clone();
    settings.web_ui_enabled = true;
    if settings.web_auth_token.trim().is_empty() {
        settings.web_auth_token = generate_web_auth_token();
        save_settings(&settings)?;
        eprintln!("rustdl: generated a new API token (saved to settings).");
    }

    let rt = Arc::new(tokio::runtime::Runtime::new()?);
    let (service, _rx) = RustdlService::new(rt.clone());
    let core = service.shared_core();
    {
        let mut c = core.lock();
        c.settings = settings.clone();
        c.output_dir = settings.output_dir.clone();
        c.worker_count = settings.worker_count.clamp(1, 6);
        c.refresh_deps();
        c.update_status();
    }

    let token = settings.web_auth_token.trim();
    let (mut handle, api_state) = spawn_web_server_at(rt.clone(), core, &bind, token)
        .map_err(|e| anyhow!(e.message()))?;
    let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<()>();
    api_state.set_process_exit_notifier(exit_tx);

    let local_url = web_ui_browser_url(&bind);
    {
        use std::io::{self, Write};
        let mut out = io::stdout().lock();
        writeln!(out, "rustdl {} (web-only)", crate::pkg_version::VERSION)?;
        writeln!(out, "  LAN bind:  http://{bind}")?;
        writeln!(out, "  Local URL: {local_url}")?;
        writeln!(out, "  API token: {token}")?;
        writeln!(out, "Press Ctrl+C to stop.")?;
        out.flush()?;
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = exit_rx => {},
    }
    handle.stop();
    println!("Stopped.");
    Ok(())
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
        "--web-only" => match parse_web_only_args(&args[1..]) {
            Ok(opts) => {
                let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
                if let Err(e) = rt.block_on(run_headless_web(opts)) {
                    eprintln!("Web server failed: {e:#}");
                    process::exit(1);
                }
                true
            }
            Err(e) => {
                eprintln!("{e:#}");
                process::exit(2);
            }
        },
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
    println!("  rustdl --web-only [OPTS]        Headless LAN web UI (no GUI)");
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
    println!("  --web-only           Serve the LAN web UI without opening a window");
    println!("  --host ADDR          Bind host (default: from saved settings, else 0.0.0.0)");
    println!("  --port PORT          Bind port (default: from saved settings, else 8765)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_web_only_args_accepts_host_and_port() {
        let args = vec![
            "--host".to_owned(),
            "0.0.0.0".to_owned(),
            "--port".to_owned(),
            "8765".to_owned(),
        ];
        let opts = parse_web_only_args(&args).unwrap();
        assert_eq!(opts.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(opts.port, Some(8765));
    }

    #[test]
    fn parse_url_lines_skips_comments_and_blanks() {
        let text = "# comment\n\nhttps://a.test\n  https://b.test  \n";
        assert_eq!(
            parse_url_lines(text),
            vec!["https://a.test".to_owned(), "https://b.test".to_owned()]
        );
    }
}
