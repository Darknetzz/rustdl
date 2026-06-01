use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crossbeam_channel::Sender;
use tokio::runtime::Runtime;

use crate::app::events::{try_send_ui, UiEvent};
use crate::models::VideoPreview;
use crate::pkg_version;
use crate::ytdlp;

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

pub(crate) fn spawn_update_check(rt: &Arc<Runtime>, tx: &Sender<UiEvent>, client: reqwest::Client) {
    let tx = tx.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        let result = super::update_check::check_latest_release_async(&client).await;
        let (latest_version, release_url, has_update, message) = match result {
            Ok((latest, url, newer)) => {
                let msg = if newer {
                    format!("Update available: {latest}")
                } else {
                    format!("You are up to date ({})", pkg_version::VERSION)
                };
                (Some(latest), Some(url), newer, msg)
            }
            Err(e) => (None, None, false, format!("Update check failed: {e}")),
        };
        try_send_ui(
            &tx,
            UiEvent::UpdateCheckDone {
                latest_version,
                release_url,
                has_update,
                message,
            },
        );
    });
}

pub(crate) fn spawn_url_resolve_pipeline(
    rt: &Arc<Runtime>,
    tx: &Sender<UiEvent>,
    yt_dlp_bin: String,
    metadata_args: Vec<String>,
    queued_lines: Vec<String>,
) {
    let tx = tx.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        let total = queued_lines.len();
        for (idx, line) in queued_lines.into_iter().enumerate() {
            try_send_ui(
                &tx,
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
                ytdlp::resolve_url_to_previews_with_bin(&line_for_resolve, &bin, &metadata_args)
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
                &tx,
                UiEvent::AddResolved {
                    rows,
                    source_line: line.clone(),
                },
            );
            try_send_ui(
                &tx,
                UiEvent::AddProgress {
                    processed: idx + 1,
                    total,
                    current: None,
                },
            );
        }
        try_send_ui(&tx, UiEvent::AddDone);
    });
}

pub(crate) fn spawn_download_worker(
    rt: &Arc<Runtime>,
    tx: &Sender<UiEvent>,
    output_dir: String,
    extra_args: Vec<String>,
    yt_bin: String,
    ffmpeg_path: String,
    urls: Vec<(u64, String, String, Arc<AtomicBool>)>,
) {
    let tx = tx.clone();
    let rt = rt.clone();
    rt.spawn(async move {
        for (item_id, web, source, cancel_flag) in urls {
            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                try_send_ui(
                    &tx,
                    UiEvent::DownloadDone {
                        item_id,
                        ok: false,
                        detail: "Cancelled by user.".to_owned(),
                    },
                );
                continue;
            }
            let target = if web.is_empty() { source } else { web };
            try_send_ui(
                &tx,
                UiEvent::DownloadLine {
                    item_id,
                    line: "starting".to_owned(),
                },
            );
            let res = ytdlp::stream_download_with_bins(
                &target,
                &output_dir,
                &extra_args,
                &yt_bin,
                &ffmpeg_path,
                cancel_flag.clone(),
                |line| {
                    try_send_ui(&tx, UiEvent::DownloadLine { item_id, line });
                },
            )
            .await;
            match res {
                Ok(_) => {
                    try_send_ui(
                        &tx,
                        UiEvent::DownloadDone {
                            item_id,
                            ok: true,
                            detail: "Completed".to_owned(),
                        },
                    );
                }
                Err(e) => {
                    let err_text = e.to_string();
                    if should_retry_without_embed_thumbnail(&extra_args, &err_text) {
                        let retry_args = remove_embed_thumbnail_arg(&extra_args);
                        try_send_ui(
                            &tx,
                            UiEvent::DownloadLine {
                                item_id,
                                line: "Embed thumbnail failed; retrying without --embed-thumbnail."
                                    .to_owned(),
                            },
                        );
                        let retry_res = ytdlp::stream_download_with_bins(
                            &target,
                            &output_dir,
                            &retry_args,
                            &yt_bin,
                            &ffmpeg_path,
                            cancel_flag.clone(),
                            |line| {
                                try_send_ui(&tx, UiEvent::DownloadLine { item_id, line });
                            },
                        )
                        .await;
                        match retry_res {
                            Ok(_) => {
                                try_send_ui(
                                    &tx,
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
                                    &tx,
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
                        &tx,
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
