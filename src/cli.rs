use std::path::PathBuf;
use std::process;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::app_state::is_queueable_http_url;
use crate::config::{
    ensure_web_auth_token_if_enabled, load_settings, save_settings, validate_web_tls_settings,
    web_tls_enabled, AppSettings,
};
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
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Console::{
        FreeConsole, SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    unsafe {
        // FreeConsole does not clear stdin/stdout/stderr; inherited invalid handles make
        // Command::spawn fail with os error 50 ("The request is not supported").
        let null_handle: HANDLE = 0;
        let _ = SetStdHandle(STD_INPUT_HANDLE, null_handle);
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, null_handle);
        let _ = SetStdHandle(STD_ERROR_HANDLE, null_handle);
        let _ = FreeConsole();
    }
}

/// Re-attach to the parent terminal so startup errors are visible after [`detach_console_for_gui`].
#[cfg(windows)]
pub fn reattach_console_for_error() {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ACCESS_DENIED};
    use windows_sys::Win32::System::Console::{AllocConsole, AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 && GetLastError() != ERROR_ACCESS_DENIED {
            let _ = AllocConsole();
        }
    }
}

#[cfg(not(windows))]
pub fn detach_console_for_gui() {}

#[cfg(not(windows))]
pub fn reattach_console_for_error() {}

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
        &settings.subprocess_priority,
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
    let bind =
        resolve_web_bind_address(opts.host.as_deref(), opts.port, &settings.web_bind_address)
            .map_err(|e| anyhow!(e.message()))?;
    settings.web_bind_address = bind.clone();
    settings.web_ui_enabled = true;
    if ensure_web_auth_token_if_enabled(&mut settings) {
        save_settings(&settings)?;
        eprintln!("rustdl: generated a new API token (saved to settings).");
    }

    let rt = Arc::new(tokio::runtime::Runtime::new()?);
    let (service, _rx) = RustdlService::new(rt.clone());
    let core = service.shared_core();
    for issue in core.lock().config_load_issues.iter() {
        eprintln!(
            "rustdl: warning: could not load {} from {} — using defaults",
            issue.label,
            issue.path.display()
        );
    }
    {
        let mut c = core.lock();
        c.settings = settings.clone();
        c.output_dir = settings.output_dir.clone();
        c.worker_count = settings.worker_count.clamp(1, 6);
        c.refresh_deps();
        c.update_status();
    }

    let token = settings.web_auth_token.trim();
    validate_web_tls_settings(&settings).map_err(|e| anyhow!(e))?;
    let tls = web_tls_enabled(&settings);
    let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<()>();
    let mut handle = spawn_web_server_at(
        rt.clone(),
        core.clone(),
        &bind,
        token,
        if tls {
            Some(settings.web_tls_cert_path.as_str())
        } else {
            None
        },
        if tls {
            Some(settings.web_tls_key_path.as_str())
        } else {
            None
        },
        Some(exit_tx),
    )
    .map_err(|e| anyhow!(e.message()))?;

    let local_url = web_ui_browser_url(&settings);
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
    {
        let mut c = core.lock();
        c.maybe_flush_queue_save();
        c.maybe_flush_convert_queue_save();
        c.flush_queue_to_disk();
        c.flush_convert_queue_to_disk();
        c.flush_log_to_disk();
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

pub async fn run_headless_enqueue(urls: Vec<String>) -> Result<()> {
    if urls.is_empty() {
        return Err(anyhow!("no URLs to enqueue"));
    }
    let rt = Arc::new(tokio::runtime::Runtime::new()?);
    let (service, _rx) = RustdlService::new(rt);
    let core = service.shared_core();
    let stats = {
        let mut c = core.lock();
        c.queue_urls_for_resolve(urls)
    };
    println!(
        "Enqueued {} URL(s) (skipped {} duplicate(s), {} invalid).",
        stats.accepted,
        stats.duplicate_in_input + stats.duplicate_existing,
        stats.invalid
    );
    Ok(())
}

pub async fn run_headless_start_queue() -> Result<()> {
    let rt = Arc::new(tokio::runtime::Runtime::new()?);
    let (service, _rx) = RustdlService::new(rt.clone());
    let core = service.shared_core();
    {
        let mut c = core.lock();
        c.start_downloads_from_queue();
    }
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let c = core.lock();
        if c.status_active == 0 && c.status_queued == 0 && !c.add_in_progress {
            break;
        }
    }
    {
        let mut c = core.lock();
        c.maybe_flush_queue_save();
        c.flush_queue_to_disk();
        c.flush_log_to_disk();
    }
    Ok(())
}

pub async fn run_headless_convert_batch() -> Result<()> {
    let rt = Arc::new(tokio::runtime::Runtime::new()?);
    let (service, _rx) = RustdlService::new(rt.clone());
    let core = service.shared_core();
    {
        let mut c = core.lock();
        let _ = c.start_convert_batch();
    }
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let c = core.lock();
        if !c.convert_running {
            break;
        }
    }
    {
        let mut c = core.lock();
        c.maybe_flush_convert_queue_save();
        c.flush_convert_queue_to_disk();
        c.flush_log_to_disk();
    }
    Ok(())
}

pub fn parse_cli_enqueue_args(args: &[String]) -> Result<(String, bool)> {
    let mut source = None;
    let mut start = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--enqueue" => {
                i += 1;
                while i < args.len() {
                    match args[i].as_str() {
                        "--start" => {
                            start = true;
                            i += 1;
                        }
                        s if s.starts_with('-') => {
                            return Err(anyhow!("unknown option: {s}"));
                        }
                        s => {
                            if source.is_some() {
                                return Err(anyhow!(
                                    "--enqueue accepts only one URL, @file, or - source"
                                ));
                            }
                            source = Some(s.to_owned());
                            i += 1;
                            break;
                        }
                    }
                }
            }
            "--start" => {
                start = true;
                i += 1;
            }
            s => return Err(anyhow!("unknown option: {s}")),
        }
    }
    let source = source.ok_or_else(|| anyhow!("--enqueue requires a URL, @file, or -"))?;
    Ok((source, start))
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
            println!("Build: {}", crate::pkg_version::build_date_local());
            true
        }
        "--help" | "-h" => {
            print_help();
            true
        }
        "--enqueue" => match parse_cli_enqueue_args(&args) {
            Ok((source, start)) => {
                let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
                match read_urls_from_batch_source(&source) {
                    Ok(urls) if urls.is_empty() => {
                        eprintln!("no URLs to enqueue");
                        process::exit(2);
                    }
                    Ok(urls) => {
                        if let Err(e) = rt.block_on(run_headless_enqueue(urls)) {
                            eprintln!("Enqueue failed: {e:#}");
                            process::exit(1);
                        }
                        if start {
                            if let Err(e) = rt.block_on(run_headless_start_queue()) {
                                eprintln!("Start queue failed: {e:#}");
                                process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("{e:#}");
                        process::exit(2);
                    }
                }
                true
            }
            Err(e) => {
                eprintln!("{e:#}");
                process::exit(2);
            }
        },
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
        "--start-queue" => {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            if let Err(e) = rt.block_on(run_headless_start_queue()) {
                eprintln!("Start queue failed: {e:#}");
                process::exit(1);
            }
            true
        }
        "--convert-batch" => {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            if let Err(e) = rt.block_on(run_headless_convert_batch()) {
                eprintln!("Convert batch failed: {e:#}");
                process::exit(1);
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
    println!("  rustdl --enqueue URL|@file|-   Append URLs to the saved download queue");
    println!("  rustdl --enqueue --start URL   Enqueue URLs, then start the download queue");
    println!("  rustdl --download URL [OPTS]    Headless download (no GUI)");
    println!("  rustdl --web-only [OPTS]        Headless LAN web UI (no GUI)");
    println!("  rustdl --start-queue            Start persisted download queue and wait");
    println!("  rustdl --convert-batch          Start persisted convert batch and wait");
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
    println!("  --start              With --enqueue: start the download queue after enqueue");
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
    fn parse_cli_enqueue_args_accepts_start_flag() {
        let args = vec![
            "--enqueue".to_owned(),
            "--start".to_owned(),
            "https://example.test".to_owned(),
        ];
        let (source, start) = parse_cli_enqueue_args(&args).unwrap();
        assert_eq!(source, "https://example.test");
        assert!(start);

        let args = vec!["--enqueue".to_owned(), "https://example.test".to_owned()];
        let (_, start) = parse_cli_enqueue_args(&args).unwrap();
        assert!(!start);

        let args = vec![
            "--enqueue".to_owned(),
            "https://example.test".to_owned(),
            "--start".to_owned(),
        ];
        let (source, start) = parse_cli_enqueue_args(&args).unwrap();
        assert_eq!(source, "https://example.test");
        assert!(start);
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
