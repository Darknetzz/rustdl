use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::runtime::Runtime;
use tokio::task::JoinSet;

use crate::domain::events::{try_send_ui, UiEvent, UiEventBus};
use crate::models::VideoPreview;
use crate::pkg_version;
use crate::transcode::{self, ConvertConfig, ConvertInput};
use crate::ytdlp;
use crate::ytdlp_download_args::with_fallback_format_args;
use crate::ytdlp_errors::{is_format_unavailable_error, is_transient_download_error};

type DownloadJob = (u64, String, Arc<AtomicBool>, Vec<String>, String);

fn remove_embed_thumbnail_arg(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|arg| !arg.eq_ignore_ascii_case("--embed-thumbnail"))
        .cloned()
        .collect()
}

fn should_retry_without_embed_thumbnail(extra_args: &[String], err_text: &str) -> bool {
    if !extra_args
        .iter()
        .any(|arg| arg.eq_ignore_ascii_case("--embed-thumbnail"))
    {
        return false;
    }
    let msg = err_text.to_ascii_lowercase();
    msg.contains("embedthumbnail")
        || msg.contains("unable to embed")
        || msg.contains("could not determine image type")
        || msg.contains("conversion failed")
}

#[allow(clippy::too_many_arguments)]
async fn run_download_attempt(
    target_url: &str,
    output_dir: &str,
    output_filename_template: &str,
    extra_args: &[String],
    yt_bin: &str,
    ffmpeg_path: &str,
    subprocess_priority: &str,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    bus: &UiEventBus,
    item_id: u64,
) -> Result<(), anyhow::Error> {
    ytdlp::stream_download_with_bins(
        target_url,
        output_dir,
        output_filename_template,
        extra_args,
        yt_bin,
        ffmpeg_path,
        subprocess_priority,
        cancel_flag,
        |line| {
            try_send_ui(bus, UiEvent::DownloadLine { item_id, line });
        },
    )
    .await
}

async fn download_with_transient_retries(
    item_id: u64,
    target_url: &str,
    output_dir: &str,
    output_filename_template: &str,
    extra_args: &[String],
    yt_bin: &str,
    ffmpeg_path: &str,
    subprocess_priority: &str,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    bus: &UiEventBus,
    auto_retries: u32,
    retry_sleep_secs: u32,
) -> Result<(), String> {
    let max_transient = auto_retries;
    let mut transient_attempt = 0u32;
    loop {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Cancelled by user.".to_owned());
        }
        match run_download_attempt(
            target_url,
            output_dir,
            output_filename_template,
            extra_args,
            yt_bin,
            ffmpeg_path,
            subprocess_priority,
            cancel_flag.clone(),
            bus,
            item_id,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) => {
                let err_text = e.to_string();
                if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("Cancelled by user.".to_owned());
                }
                if transient_attempt < max_transient
                    && is_transient_download_error(&err_text)
                {
                    transient_attempt += 1;
                    try_send_ui(
                        bus,
                        UiEvent::DownloadLine {
                            item_id,
                            line: format!(
                                "Connection issue; retrying ({transient_attempt}/{max_transient})…"
                            ),
                        },
                    );
                    let sleep_secs =
                        u64::from(retry_sleep_secs.max(1)) * u64::from(transient_attempt);
                    tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
                    continue;
                }
                return Err(err_text);
            }
        }
    }
}

pub(crate) fn spawn_update_check(
    rt: &Arc<Runtime>,
    bus: &UiEventBus,
    client: reqwest::Client,
    github_token: Option<String>,
) {
    let bus = bus.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        let token_ref = github_token.as_deref();
        let result = crate::app::update_check::check_latest_release_async(&client, token_ref).await;
        let (
            latest_version,
            release_url,
            download_browser_url,
            download_api_url,
            has_update,
            message,
        ) = match result {
            Ok((latest, url, asset, newer)) => {
                let (browser, api) = asset
                    .map(|a| (Some(a.browser_download_url), Some(a.api_url)))
                    .unwrap_or((None, None));
                let msg = if newer {
                    if browser.is_some() {
                        format!("Update available: {latest}")
                    } else {
                        format!("Update available: {latest} (open release page to download)")
                    }
                } else {
                    format!("You are up to date ({})", pkg_version::VERSION)
                };
                (Some(latest), Some(url), browser, api, newer, msg)
            }
            Err(e) => (None, None, None, None, false, e),
        };
        try_send_ui(
            &bus,
            UiEvent::UpdateCheckDone {
                latest_version,
                release_url,
                download_browser_url,
                download_api_url,
                has_update,
                message,
            },
        );
    });
}

pub(crate) fn spawn_update_download(
    rt: &Arc<Runtime>,
    bus: &UiEventBus,
    client: reqwest::Client,
    asset: crate::app::update_check::PlatformReleaseAsset,
    version: String,
    github_token: Option<String>,
) {
    let bus = bus.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        let result = crate::app::update_check::download_release_asset_async(
            &client,
            &asset,
            &version,
            github_token.as_deref(),
        )
        .await;
        let (ok, pending_path, message) = match result {
            Ok(path) => (
                true,
                Some(path),
                "Update downloaded. Restart rustdl to apply.".to_owned(),
            ),
            Err(e) => (false, None, e),
        };
        try_send_ui(
            &bus,
            UiEvent::UpdateDownloadDone {
                ok,
                pending_path,
                message,
            },
        );
    });
}

pub(crate) fn spawn_url_resolve_pipeline(
    rt: &Arc<Runtime>,
    bus: &UiEventBus,
    yt_dlp_bin: String,
    metadata_args: Vec<String>,
    playlist_cap: usize,
    queued_lines: Vec<String>,
) {
    let bus = bus.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        let total = queued_lines.len();
        for (idx, line) in queued_lines.into_iter().enumerate() {
            try_send_ui(
                &bus,
                UiEvent::AddProgress {
                    processed: idx,
                    total,
                    current: Some(line.clone()),
                },
            );
            let bin = yt_dlp_bin.clone();
            let line_for_resolve = line.clone();
            let metadata_args = metadata_args.clone();
            let rows = match tokio::task::spawn_blocking(move || {
                ytdlp::resolve_url_to_previews_with_bin(
                    &line_for_resolve,
                    &bin,
                    &metadata_args,
                    playlist_cap,
                )
            })
            .await
            {
                Ok(r) => r,
                Err(_) => vec![VideoPreview {
                    source_line: line.clone(),
                    webpage_url: line.clone(),
                    title: String::new(),
                    error: Some("Metadata fetch task failed.".to_owned()),
                    ..Default::default()
                }],
            };
            try_send_ui(
                &bus,
                UiEvent::AddResolved {
                    rows,
                    source_line: line.clone(),
                },
            );
            try_send_ui(
                &bus,
                UiEvent::AddProgress {
                    processed: idx + 1,
                    total,
                    current: None,
                },
            );
        }
        try_send_ui(&bus, UiEvent::AddDone);
    });
}

pub(crate) fn spawn_queue_thumbnail_prefetch(core: crate::service::core::SharedCore, item_id: u64) {
    let (client, urls, rt) = {
        let c = core.lock();
        let Some(idx) = c.item_idx(item_id) else {
            return;
        };
        let urls = ytdlp::thumbnail_url_candidates(&c.items[idx]);
        if urls.is_empty() {
            return;
        }
        (c.http_client.clone(), urls, c.runtime.clone())
    };
    rt.spawn(async move {
        for url in urls {
            if let Some((bytes, content_type)) = ytdlp::fetch_thumbnail_bytes(&client, &url).await {
                let mut c = core.lock();
                let Some(idx) = c.item_idx(item_id) else {
                    return;
                };
                let source_key =
                    crate::service::core::DownloadCore::queue_thumbnail_source_key(&c.items[idx]);
                c.cache_thumbnail_bytes(item_id, source_key, bytes, content_type);
                return;
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_download_worker(
    rt: &Arc<Runtime>,
    bus: &UiEventBus,
    output_dir: String,
    yt_bin: String,
    ffmpeg_path: String,
    subprocess_priority: String,
    download_auto_retries: u32,
    retry_sleep_secs: u32,
    urls: Vec<DownloadJob>,
) {
    let bus = bus.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        for (item_id, target_url, cancel_flag, extra_args, output_filename_template) in urls {
            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                try_send_ui(
                    &bus,
                    UiEvent::DownloadDone {
                        item_id,
                        ok: false,
                        detail: "Cancelled by user.".to_owned(),
                    },
                );
                continue;
            }
            try_send_ui(
                &bus,
                UiEvent::DownloadLine {
                    item_id,
                    line: "starting".to_owned(),
                },
            );
            let res = download_with_transient_retries(
                item_id,
                &target_url,
                &output_dir,
                &output_filename_template,
                &extra_args,
                &yt_bin,
                &ffmpeg_path,
                &subprocess_priority,
                cancel_flag.clone(),
                &bus,
                download_auto_retries,
                retry_sleep_secs,
            )
            .await;
            match res {
                Ok(()) => {
                    try_send_ui(
                        &bus,
                        UiEvent::DownloadDone {
                            item_id,
                            ok: true,
                            detail: "Completed".to_owned(),
                        },
                    );
                }
                Err(err_text) => {
                    let mut err_text = err_text;
                    let mut active_args = extra_args.clone();

                    if is_format_unavailable_error(&err_text) {
                        active_args = with_fallback_format_args(&extra_args);
                        try_send_ui(
                            &bus,
                            UiEvent::DownloadLine {
                                item_id,
                                line: "Requested format not available; retrying with -f best."
                                    .to_owned(),
                            },
                        );
                        match download_with_transient_retries(
                            item_id,
                            &target_url,
                            &output_dir,
                            &output_filename_template,
                            &active_args,
                            &yt_bin,
                            &ffmpeg_path,
                            &subprocess_priority,
                            cancel_flag.clone(),
                            &bus,
                            download_auto_retries,
                            retry_sleep_secs,
                        )
                        .await
                        {
                            Ok(()) => {
                                try_send_ui(
                                    &bus,
                                    UiEvent::DownloadDone {
                                        item_id,
                                        ok: true,
                                        detail: "Completed (used -f best after format error)."
                                            .to_owned(),
                                    },
                                );
                                continue;
                            }
                            Err(retry_e) => err_text = retry_e,
                        }
                    }

                    if should_retry_without_embed_thumbnail(&active_args, &err_text) {
                        let retry_args = remove_embed_thumbnail_arg(&active_args);
                        try_send_ui(
                            &bus,
                            UiEvent::DownloadLine {
                                item_id,
                                line: "Embed thumbnail failed; retrying without --embed-thumbnail."
                                    .to_owned(),
                            },
                        );
                        let retry_res = download_with_transient_retries(
                            item_id,
                            &target_url,
                            &output_dir,
                            &output_filename_template,
                            &retry_args,
                            &yt_bin,
                            &ffmpeg_path,
                            &subprocess_priority,
                            cancel_flag.clone(),
                            &bus,
                            download_auto_retries,
                            retry_sleep_secs,
                        )
                        .await;
                        match retry_res {
                            Ok(()) => {
                                try_send_ui(
                                    &bus,
                                    UiEvent::DownloadDone {
                                        item_id,
                                        ok: true,
                                        detail: "Completed (thumbnail embedding skipped after failure)."
                                            .to_owned(),
                                    },
                                );
                                continue;
                            }
                            Err(retry_e) => {
                                try_send_ui(
                                    &bus,
                                    UiEvent::DownloadDone {
                                        item_id,
                                        ok: false,
                                        detail: format!(
                                            "Initial download failed while embedding thumbnail, and retry without embedding also failed.\n--- first error ---\n{err_text}\n--- retry error ---\n{retry_e}"
                                        ),
                                    },
                                );
                                continue;
                            }
                        }
                    }
                    try_send_ui(
                        &bus,
                        UiEvent::DownloadDone {
                            item_id,
                            ok: false,
                            detail: err_text,
                        },
                    );
                }
            }
        }
    });
}

pub(crate) fn spawn_convert_local_thumbnail(
    rt: &Arc<Runtime>,
    bus: &UiEventBus,
    shared_core: &crate::service::SharedCore,
    thumb_semaphore: Arc<tokio::sync::Semaphore>,
    item_id: u64,
    file_path: std::path::PathBuf,
    ffmpeg_path: String,
) {
    let bus = bus.clone();
    let rt = rt.clone();
    let shared_core = shared_core.clone();
    let source_key = crate::service::core::DownloadCore::convert_thumbnail_source_key(
        file_path.to_string_lossy().as_ref(),
    );
    rt.spawn(async move {
        let permit = thumb_semaphore.acquire_owned().await;
        let Ok(_permit) = permit else {
            let _ = try_send_ui(
                &bus,
                UiEvent::ThumbnailFetched {
                    item_id,
                    image: None,
                },
            );
            return;
        };
        let outcome = tokio::task::spawn_blocking(move || {
            let png = transcode::extract_thumbnail_png_bytes(&file_path, &ffmpeg_path)?;
            let image = crate::app::thumbnails::decode_thumbnail_image(png.clone());
            Some((png, image))
        })
        .await
        .ok()
        .flatten();
        if let Some((png, _)) = &outcome {
            shared_core.lock().cache_thumbnail_bytes(
                item_id,
                source_key.clone(),
                png.clone(),
                "image/png",
            );
        }
        let image = outcome.and_then(|(_, image)| image);
        let _ = try_send_ui(&bus, UiEvent::ThumbnailFetched { item_id, image });
    });
}

pub(crate) fn spawn_convert_media_probe(
    rt: &Arc<Runtime>,
    bus: &UiEventBus,
    probe_semaphore: Arc<tokio::sync::Semaphore>,
    item_id: u64,
    file_path: std::path::PathBuf,
    ffprobe_path: String,
) {
    let bus = bus.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        let permit = probe_semaphore.acquire_owned().await;
        let Ok(_permit) = permit else {
            return;
        };
        let media = tokio::task::spawn_blocking(move || {
            transcode::probe_input_media(&file_path, &ffprobe_path)
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        let _ = try_send_ui(&bus, UiEvent::ConvertMediaProbed { item_id, media });
    });
}

async fn run_convert_job(
    bus: &UiEventBus,
    cfg: &ConvertConfig,
    enc: &transcode::EncoderChoice,
    cancel_flag: &Arc<AtomicBool>,
    item_id: u64,
    item: transcode::ConvertPlanItem,
) {
    if let Some(ms) = transcode::input_duration_ms(&item.input, &cfg.ffprobe_path) {
        let _ = try_send_ui(
            bus,
            UiEvent::ConvertDuration {
                item_id,
                duration_ms: ms,
            },
        );
    }
    let _ = try_send_ui(
        bus,
        UiEvent::ConvertLine {
            item_id,
            line: format!("starting with {} ({})", enc.encoder, enc.hw_type),
        },
    );
    let item_for_primary = item.clone();
    let res = tokio::task::spawn_blocking({
        let cfg = cfg.clone();
        let bus = bus.clone();
        let enc = enc.clone();
        let cancel_flag = cancel_flag.clone();
        move || {
            transcode::run_single(&item_for_primary, &cfg, &enc, Some(cancel_flag), |line| {
                let _ = try_send_ui(&bus, UiEvent::ConvertLine { item_id, line });
            })
        }
    })
    .await;
    match res {
        Ok(Ok(final_path)) => {
            let _ = try_send_ui(
                bus,
                UiEvent::ConvertDone {
                    item_id,
                    ok: true,
                    detail: "Completed".to_owned(),
                    final_output_path: Some(final_path.to_string_lossy().into_owned()),
                },
            );
        }
        Ok(Err(e)) => {
            let err_text = e.to_string();
            if err_text.to_ascii_lowercase().starts_with("skipped") {
                let _ = try_send_ui(
                    bus,
                    UiEvent::ConvertDone {
                        item_id,
                        ok: true,
                        detail: err_text,
                        final_output_path: None,
                    },
                );
                return;
            }
            // Hardware encoders can fail at runtime (driver/session/caps); retry once on CPU.
            let cpu_name = transcode::cpu_encoder_for_target(&cfg.target_codec);
            if enc.encoder != cpu_name && enc.hw_type != "cpu" {
                let _ = try_send_ui(
                    bus,
                    UiEvent::ConvertLine {
                        item_id,
                        line: format!("encoder {} failed; retrying with {}", enc.encoder, cpu_name),
                    },
                );
                let cpu_enc = transcode::EncoderChoice {
                    encoder: cpu_name,
                    codec: transcode::normalize_target_codec(&cfg.target_codec),
                    hw_type: "cpu",
                };
                let retry = tokio::task::spawn_blocking({
                    let cfg = cfg.clone();
                    let bus = bus.clone();
                    let item = item.clone();
                    let cancel_flag = cancel_flag.clone();
                    move || {
                        transcode::run_single(&item, &cfg, &cpu_enc, Some(cancel_flag), |line| {
                            let _ = try_send_ui(&bus, UiEvent::ConvertLine { item_id, line });
                        })
                    }
                })
                .await;
                match retry {
                    Ok(Ok(final_path)) => {
                        let _ = try_send_ui(
                            bus,
                            UiEvent::ConvertDone {
                                item_id,
                                ok: true,
                                detail: "Completed (CPU fallback)".to_owned(),
                                final_output_path: Some(final_path.to_string_lossy().into_owned()),
                            },
                        );
                    }
                    Ok(Err(retry_err)) => {
                        let _ = try_send_ui(
                            bus,
                            UiEvent::ConvertDone {
                                item_id,
                                ok: false,
                                detail: format!(
                                    "Primary encoder failed: {err_text}\nCPU fallback failed: {retry_err}"
                                ),
                                final_output_path: None,
                            },
                        );
                    }
                    Err(retry_join_err) => {
                        let _ = try_send_ui(
                            bus,
                            UiEvent::ConvertDone {
                                item_id,
                                ok: false,
                                detail: format!(
                                    "Primary encoder failed: {err_text}\nCPU fallback task failed: {retry_join_err}"
                                ),
                                final_output_path: None,
                            },
                        );
                    }
                }
                return;
            }
            let _ = try_send_ui(
                bus,
                UiEvent::ConvertDone {
                    item_id,
                    ok: false,
                    detail: err_text,
                    final_output_path: None,
                },
            );
        }
        Err(e) => {
            let _ = try_send_ui(
                bus,
                UiEvent::ConvertDone {
                    item_id,
                    ok: false,
                    detail: format!("worker failed: {e}"),
                    final_output_path: None,
                },
            );
        }
    }
}

pub(crate) fn spawn_convert_worker(
    rt: &Arc<Runtime>,
    bus: &UiEventBus,
    cfg: ConvertConfig,
    jobs: Vec<(u64, ConvertInput, String, crate::convert_size_limit::ConvertSizeLimit)>,
    cancel_flag: Arc<AtomicBool>,
    parallel: usize,
) {
    let bus = bus.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        let enc = transcode::detect_encoder_with_override(
            &cfg.ffmpeg_path,
            &cfg.encoder_override,
            &cfg.target_codec,
        );
        let parallel = parallel.clamp(1, 6);
        let mut jobs = jobs.into_iter();
        let mut join_set = JoinSet::new();

        let mut spawn_next = |join_set: &mut JoinSet<()>| -> bool {
            if cancel_flag.load(Ordering::Relaxed) {
                return false;
            }
            let Some((item_id, input, output_path, size_limit)) = jobs.next() else {
                return false;
            };
            let item = transcode::ConvertPlanItem {
                input: std::path::PathBuf::from(input.source_path),
                output: std::path::PathBuf::from(output_path),
            };
            let bus = bus.clone();
            let mut cfg = cfg.clone();
            cfg.size_limit = size_limit;
            let enc = enc.clone();
            let cancel_flag = cancel_flag.clone();
            join_set.spawn(async move {
                run_convert_job(&bus, &cfg, &enc, &cancel_flag, item_id, item).await;
            });
            true
        };

        for _ in 0..parallel {
            if !spawn_next(&mut join_set) {
                break;
            }
        }

        while let Some(_res) = join_set.join_next().await {
            if !spawn_next(&mut join_set) && join_set.is_empty() {
                break;
            }
        }
        let _ = try_send_ui(&bus, UiEvent::ConvertBatchDone);
    });
}

pub(crate) fn spawn_playlist_preview(
    rt: &Arc<Runtime>,
    bus: &UiEventBus,
    yt_dlp_bin: String,
    url: String,
    cap: usize,
) {
    let bus = bus.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        let source_url = url.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            ytdlp::flat_playlist_preview(&yt_dlp_bin, &url, cap)
        })
        .await;
        let (title, urls, error) = match outcome {
            Ok(Ok(preview)) => (preview.title, preview.urls, None),
            Ok(Err(e)) => (None, Vec::new(), Some(format!("{e:#}"))),
            Err(e) => (
                None,
                Vec::new(),
                Some(format!("playlist preview task failed: {e}")),
            ),
        };
        let _ = try_send_ui(
            &bus,
            UiEvent::PlaylistPreviewDone {
                source_url,
                title,
                urls,
                error,
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::should_retry_without_embed_thumbnail;

    #[test]
    fn retry_without_embed_when_thumbnail_error() {
        let args = vec!["--embed-thumbnail".to_owned()];
        assert!(should_retry_without_embed_thumbnail(
            &args,
            "Unable to embed thumbnail in file"
        ));
    }

    #[test]
    fn no_retry_without_embed_flag() {
        assert!(!should_retry_without_embed_thumbnail(
            &[],
            "Unable to embed thumbnail"
        ));
    }
}
