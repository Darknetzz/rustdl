use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_parsing::{human_bytes_ui, queue_item_file_size_bytes};
use crate::app_ui::{
    clip_bounded_width, compact_button_group, draw_meta_badge, draw_status_chip, layout_breakpoint,
    left_button_row, popup_menu_above, queue_card_grid_width, queue_short_panel_list_fallback,
    should_flatten_nested_group_scroll, show_menu_popup, show_queue_group_section, status_color,
    status_dot_with_label, MetaBadgeKind, QUEUE_DL_LIST_ROW_H,
};
use crate::media_metadata::queue_item_more_info_rows;
use crate::models::{ItemStatus, QueueItem};
use crate::theme;
use crate::time_format::{format_absolute_local, format_relative_ago};
use crate::ui_icons;

use super::{log_line_color, CancelPostAction, PydlApp, LOG_COLOR_ERROR, LOG_COLOR_WARN};

impl PydlApp {
    pub(super) fn draw_card(&mut self, ui: &mut egui::Ui, idx: usize, allow_reorder: bool) {
        if self.effective_card_list_layout() {
            self.draw_card_list(ui, idx, allow_reorder);
            return;
        }
        let id = self.items[idx].item_id;
        let status = self.items[idx].status;
        let title = self.items[idx].title.clone();
        let subtitle = match (&self.items[idx].duration, &self.items[idx].uploader) {
            (Some(d), Some(u)) => format!("{} · {}", format_duration(*d), u),
            (Some(d), None) => format_duration(*d),
            (None, Some(u)) => u.clone(),
            (None, None) => "-".to_owned(),
        };
        let pct = self.items[idx].percent;
        let size_text = self.items[idx].size_text.clone();
        let speed_text = self.items[idx].speed_text.clone();
        let eta_text = self.items[idx].eta_text.clone();
        let detail = self.items[idx].detail.clone();
        let has_error = self.items[idx].error.clone();
        let thumbnail_url = self.items[idx].thumbnail_url.clone();
        let has_thumbnail_url = thumbnail_url.is_some();
        let resolving = status == ItemStatus::Resolving;
        let done_file: Option<(PathBuf, SystemTime)> = match status {
            ItemStatus::Done | ItemStatus::Failed => {
                let it = &self.items[idx];
                self.find_downloaded_file_for_item(it)
            }
            _ => None,
        };
        let video_id_nonempty = !self.items[idx].video_id.trim().is_empty();
        let done_but_file_missing = matches!(status, ItemStatus::Done | ItemStatus::Failed)
            && video_id_nonempty
            && done_file.is_none();
        let show_saved_file_actions = matches!(status, ItemStatus::Done | ItemStatus::Failed);
        let can_redownload = status == ItemStatus::Done && {
            let it = &self.items[idx];
            self.item_has_redownload_target(it)
        };
        let output_ready = Path::new(&self.output_dir).is_dir();
        let is_pre_download = matches!(status, ItemStatus::Idle | ItemStatus::Queued);
        let resolution_label =
            format_resolution_label(self.items[idx].width, self.items[idx].height)
                .map(|s| s.replace('x', "×"));
        let show_size_badge = is_pre_download && size_text != "-";
        let video_codec = self.items[idx].video_codec.clone();
        let item_fps = self.items[idx].fps;
        let show_done_media_badges = show_saved_file_actions;

        let highlight_completed = status == ItemStatus::Done && !done_but_file_missing;
        let done_fill = theme::done_card_fill(&self.settings.theme);

        let card_inner = |ui: &mut egui::Ui| {
            let compact = self.settings.compact_cards;
            let avail = ui.available_width().max(1.0);
            let card_w = queue_card_grid_width(avail, compact);
            let inner_w = (card_w - 20.0).max(1.0);
            let thumb_w = (card_w - 24.0).max(1.0);
            let thumb = if compact {
                egui::vec2(thumb_w, thumb_w * (104.0 / 296.0))
            } else {
                egui::vec2(thumb_w, thumb_w * (158.0 / 332.0))
            };
            let subtitle_h = 15.0;
            let detail_h = if compact { 0.0 } else { 15.0 };
            let progress_h = 14.0;
            let title_height = if compact { 30.0 } else { 36.0 };
            let removable = !matches!(status, ItemStatus::Queued | ItemStatus::Downloading);

            ui.set_width(card_w);
            ui.vertical(|ui| {
                ui.set_width(card_w);
                ui.horizontal(|ui| {
                    let mut sel = self.selected_item_ids.contains(&id);
                    if ui.checkbox(&mut sel, "").changed() {
                        if sel {
                            self.selected_item_ids.insert(id);
                        } else {
                            self.selected_item_ids.remove(&id);
                        }
                    }
                });
                if self.settings.show_thumbnails
                    && !self.textures.contains_key(&id)
                    && !self.thumbnail_inflight.contains(&id)
                {
                    if self.thumbnail_attempted.contains(&id)
                        && (self.items[idx].thumbnail_path.is_some()
                            || (done_file.is_some() && self.has_ffmpeg))
                    {
                        self.thumbnail_attempted.remove(&id);
                    }
                    if !self.thumbnail_attempted.contains(&id) {
                        let can_try = has_thumbnail_url
                            || !self.items[idx].video_id.trim().is_empty()
                            || done_file.is_some();
                        if can_try {
                            self.queue_thumbnail_load(id);
                        }
                    }
                }
                // Fixed max cell; image keeps aspect ratio and never exceeds thumb (no upscale).
                let (thumb_rect, _) = ui.allocate_exact_size(thumb, egui::Sense::hover());
                ui.painter().rect_filled(
                    thumb_rect,
                    egui::Rounding::same(8.0),
                    theme::THUMB_PLACEHOLDER,
                );
                if let Some(tex) = self.textures.get(&id) {
                    let nat = tex.size_vec2();
                    let draw_sz = fit_thumbnail_draw_size(nat, thumb_rect.size());
                    if draw_sz.x >= 1.0 && draw_sz.y >= 1.0 {
                        let img_rect = egui::Rect::from_center_size(thumb_rect.center(), draw_sz);
                        ui.painter().image(
                            tex.id(),
                            img_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                } else if done_but_file_missing {
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(thumb_rect), |ui| {
                        ui.centered_and_justified(|ui| {
                            draw_meta_badge(ui, "File missing", MetaBadgeKind::FileMissing);
                        });
                    });
                } else {
                    let center_msg = if !self.settings.show_thumbnails {
                        "Thumbnails off"
                    } else if self.thumbnail_inflight.contains(&id) {
                        "Fetching thumbnail..."
                    } else if self.thumbnail_attempted.contains(&id) {
                        "Thumbnail unavailable"
                    } else if has_thumbnail_url
                        || !self.items[idx].video_id.trim().is_empty()
                        || done_file.is_some()
                    {
                        "Fetching thumbnail..."
                    } else {
                        "No preview available"
                    };
                    ui.painter().text(
                        thumb_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        center_msg,
                        egui::TextStyle::Body.resolve(ui.style()),
                        Color32::from_gray(130),
                    );
                }
                if done_but_file_missing && self.textures.contains_key(&id) {
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(thumb_rect), |ui| {
                        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                            draw_meta_badge(ui, "File missing", MetaBadgeKind::FileMissing);
                        });
                    });
                }

                let title_max = if compact { 56 } else { 68 };
                let subtitle_max = if compact { 38 } else { 52 };
                let title_display = ellipsize(&title, title_max);
                ui.add_sized(
                    [inner_w, title_height],
                    egui::Label::new(RichText::new(title_display).strong()).wrap(),
                );
                let subtitle_text = if !self.settings.hide_card_subtitle {
                    ellipsize(&subtitle, subtitle_max)
                } else {
                    let mut hidden_sub_parts: Vec<String> = Vec::new();
                    if let Some(d) = self.items[idx].duration {
                        hidden_sub_parts.push(format_duration(d));
                    }
                    if let Some(u) = &self.items[idx].uploader {
                        if !u.trim().is_empty() {
                            hidden_sub_parts.push(u.clone());
                        }
                    }
                    if hidden_sub_parts.is_empty() {
                        String::new()
                    } else {
                        ellipsize(&hidden_sub_parts.join(" · "), subtitle_max)
                    }
                };
                ui.add_sized(
                    [inner_w, subtitle_h],
                    egui::Label::new(
                        RichText::new(subtitle_text)
                            .small()
                            .color(Color32::LIGHT_GRAY),
                    )
                    .wrap(),
                );
                if let Some((ref path, mtime)) = done_file {
                    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                    let rel = format_relative_ago(mtime);
                    let abs = format_absolute_local(mtime);
                    let file_line = format!("{fname} · {rel}");
                    let hover = format!("{}\nModified: {abs}", path.to_string_lossy());
                    let file_label = ui.add(
                        egui::Label::new(
                            RichText::new(file_line)
                                .small()
                                .color(Color32::from_gray(150)),
                        )
                        .wrap(),
                    );
                    file_label.on_hover_text(hover);
                }
                let footer_status = if resolving {
                    "metadata".to_owned()
                } else if is_pre_download {
                    "ready".to_owned()
                } else if matches!(status, ItemStatus::Done | ItemStatus::Failed) {
                    let mut parts = vec![format!("{pct:.1}%")];
                    if speed_text != "-" {
                        parts.push(speed_text);
                    }
                    if eta_text != "-" {
                        parts.push(eta_text);
                    }
                    parts.join(" · ")
                } else {
                    format!("{pct:.1}% · {size_text} · {speed_text} · {eta_text}")
                };
                ui.add_sized(
                    [inner_w, progress_h],
                    if resolving {
                        egui::ProgressBar::new(0.0)
                            .animate(true)
                            .text("Fetching metadata...")
                    } else {
                        let mut pb = egui::ProgressBar::new((pct / 100.0).clamp(0.0, 1.0))
                            .animate(status == ItemStatus::Downloading)
                            .show_percentage();
                        if status == ItemStatus::Done {
                            pb = pb.fill(status_color(ItemStatus::Done));
                        }
                        pb
                    },
                );
                if detail_h > 0.0 && !detail.trim().is_empty() {
                    let detail_short = ellipsize(&detail, 64);
                    let detail_color = log_line_color(&detail);
                    ui.add_sized(
                        [inner_w, detail_h],
                        egui::Label::new(RichText::new(detail_short).small().color(detail_color))
                            .wrap(),
                    );
                }

                if let Some(ref err) = has_error {
                    let err_display = ellipsize(err, 72);
                    ui.add_sized(
                        [inner_w, 16.0],
                        egui::Label::new(RichText::new(err_display).small().color(LOG_COLOR_ERROR))
                            .wrap(),
                    );
                }

                if let Some(url) = crate::app_state::resolve_item_download_url(&self.items[idx]) {
                    let ctx = ui.ctx().clone();
                    left_button_row(ui, |ui| {
                        compact_button_group(ui, ("card_url", id), |g| {
                            let mut copy_url = false;
                            let mut open_url = false;
                            g.url_menu(&url, &mut copy_url, &mut open_url);
                            if copy_url {
                                ctx.copy_text(url.clone());
                            }
                            if open_url {
                                if let Err(e) = crate::app_actions::open_browser(&url) {
                                    self.append_log(&format!("Failed to open URL: {e}"));
                                }
                            }
                        });
                    });
                }
                left_button_row(ui, |ui| {
                    self.draw_more_info_button(ui, id);
                });

                let can_retry_download = status == ItemStatus::Failed
                    && output_ready
                    && self.has_yt_dlp
                    && self.item_has_redownload_target(&self.items[idx]);
                let can_retry_metadata = matches!(status, ItemStatus::Idle)
                    && has_error.is_some()
                    && self.has_yt_dlp
                    && !self.add_in_progress
                    && !self.items[idx].source_line.trim().is_empty();
                self.draw_card_retry_buttons(
                    ui,
                    id,
                    "card_retry",
                    can_retry_download,
                    can_retry_metadata,
                );

                if show_saved_file_actions {
                    left_button_row(ui, |ui| {
                        compact_button_group(ui, ("card_done_actions", id), |g| {
                            let can_open_file = done_file.is_some();
                            let can_open_folder = done_file.is_some() || done_but_file_missing;
                            if can_open_file || can_open_folder {
                                let mut open_file = false;
                                let mut open_folder = false;
                                g.open_menu(
                                    can_open_file,
                                    can_open_folder,
                                    &mut open_file,
                                    &mut open_folder,
                                );
                                if open_file {
                                    if let Some((p, _)) = done_file.as_ref() {
                                        self.open_file_path(p);
                                    }
                                }
                                if open_folder {
                                    if let Some((p, _)) = done_file.as_ref() {
                                        self.reveal_file_path(p);
                                    } else {
                                        self.open_item_output_folder(id);
                                    }
                                }
                            }
                            let can_verify_file = done_file.is_some() && self.has_ffprobe;
                            let can_redownload_action = status == ItemStatus::Done
                                && self.has_yt_dlp
                                && output_ready
                                && can_redownload;
                            if done_file.is_some() || status == ItemStatus::Done {
                                let mut verify_file = false;
                                let mut redownload = false;
                                let mut watch_quality = false;
                                let can_watch = status == ItemStatus::Done
                                    && self.has_yt_dlp
                                    && self.watchlist_url_available_for_item(id);
                                g.verify_menu(
                                    done_file.is_some(),
                                    can_verify_file,
                                    status == ItemStatus::Done,
                                    can_redownload_action,
                                    status == ItemStatus::Done,
                                    can_watch,
                                    &mut verify_file,
                                    &mut redownload,
                                    &mut watch_quality,
                                );
                                if verify_file {
                                    self.check_streams_for_item_id(id);
                                }
                                if redownload {
                                    self.redownload_item_id(id);
                                }
                                if watch_quality {
                                    self.add_queue_item_to_watchlist(id);
                                }
                            }
                            let mut remove_from_queue = false;
                            let mut delete_file = false;
                            g.remove_menu(
                                removable || done_file.is_some(),
                                removable,
                                done_file.is_some(),
                                &mut remove_from_queue,
                                &mut delete_file,
                            );
                            if remove_from_queue {
                                if !self.remove_item_by_id(id) {
                                    self.append_log(&format!(
                                        "[item {id}] Could not remove item from the queue."
                                    ));
                                }
                                self.update_status();
                                self.refresh_input_line_info();
                                self.schedule_queue_save();
                            }
                            if delete_file {
                                if let Some((p, _)) = done_file.as_ref() {
                                    self.delete_file_path(p);
                                }
                            }
                        });
                    });
                }

                ui.separator();
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if self.items[idx].playlist_capped {
                        draw_meta_badge(ui, "Playlist capped", MetaBadgeKind::SizeEstimate);
                    }
                    if let Some(ref res) = resolution_label {
                        draw_meta_badge(ui, res, MetaBadgeKind::Resolution);
                    }
                    if show_size_badge {
                        let est = if size_text.starts_with('~') {
                            size_text.trim().to_owned()
                        } else {
                            format!("~{size_text}")
                        };
                        draw_meta_badge(ui, &est, MetaBadgeKind::SizeEstimate);
                    }
                    if show_done_media_badges {
                        let local_path = done_file.as_ref().map(|(p, _)| p.as_path());
                        if let Some(bytes) =
                            queue_item_file_size_bytes(&self.items[idx], local_path)
                        {
                            draw_meta_badge(ui, &human_bytes_ui(bytes), MetaBadgeKind::FileSize);
                        }
                        if !video_codec.is_empty() {
                            draw_meta_badge(ui, &video_codec.to_uppercase(), MetaBadgeKind::Codec);
                        }
                        if let Some(fps) = item_fps {
                            draw_meta_badge(ui, &format!("{fps:.2} fps"), MetaBadgeKind::FrameRate);
                        }
                    }
                    draw_status_chip(ui, status);
                });
                let footer_color = match status {
                    ItemStatus::Done if done_but_file_missing => LOG_COLOR_WARN,
                    ItemStatus::Done => status_color(ItemStatus::Done),
                    ItemStatus::Failed => status_color(ItemStatus::Failed),
                    ItemStatus::Resolving => status_color(ItemStatus::Resolving),
                    ItemStatus::Idle => status_color(ItemStatus::Idle),
                    ItemStatus::Queued => status_color(ItemStatus::Queued),
                    ItemStatus::Downloading => status_color(ItemStatus::Downloading),
                };
                if !footer_status.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&footer_status).small().color(footer_color))
                            .wrap(),
                    );
                }
                ui.set_width(inner_w);
                if matches!(status, ItemStatus::Queued | ItemStatus::Downloading)
                    || (!show_saved_file_actions && removable)
                {
                    left_button_row(ui, |ui| {
                        compact_button_group(ui, ("card_actions", id), |g| {
                            if matches!(status, ItemStatus::Queued | ItemStatus::Downloading)
                                && g.warning(&format!("{} Ready", ui_icons::CANCEL_TO_READY), true)
                                    .on_hover_text("Cancel download and mark as ready")
                                    .clicked()
                            {
                                self.request_cancel_item(id, CancelPostAction::Ready);
                            }
                            if matches!(status, ItemStatus::Queued | ItemStatus::Downloading)
                                && g.danger(&format!("{} Drop", ui_icons::CANCEL_TO_REMOVE), true)
                                    .on_hover_text("Cancel download and remove from queue")
                                    .clicked()
                            {
                                self.request_cancel_item(id, CancelPostAction::Remove);
                            }
                            if !show_saved_file_actions
                                && g.danger(&format!("{} Remove", ui_icons::REMOVE), removable)
                                    .clicked()
                            {
                                if !self.remove_item_by_id(id) {
                                    self.append_log(&format!(
                                        "[item {id}] Could not remove item from the queue."
                                    ));
                                }
                                self.update_status();
                                self.refresh_input_line_info();
                                self.schedule_queue_save();
                            }
                        });
                    });
                }
            });
        };

        if highlight_completed {
            egui::Frame::none()
                .fill(done_fill)
                .stroke(egui::Stroke::new(2.0, status_color(ItemStatus::Done)))
                .rounding(egui::Rounding::same(10.0))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, card_inner);
        } else {
            ui.group(card_inner);
        }
    }

    fn draw_card_retry_buttons(
        &mut self,
        ui: &mut egui::Ui,
        id: u64,
        group_key: &str,
        can_retry_download: bool,
        can_retry_metadata: bool,
    ) {
        if !can_retry_download && !can_retry_metadata {
            return;
        }
        left_button_row(ui, |ui| {
            compact_button_group(ui, (group_key, id), |g| {
                if can_retry_download {
                    let btn = g
                        .warning(&format!("{} Retry", ui_icons::RETRY), true)
                        .on_hover_text(
                            "Queue this video for download again using the same URL as this row.",
                        );
                    if btn.clicked() {
                        self.retry_download_item_id(id);
                    }
                }
                if can_retry_metadata {
                    let btn = g
                        .secondary(&format!("{} Refetch", ui_icons::RETRY), true)
                        .on_hover_text(
                            "Run yt-dlp metadata again for this URL (after errors or no preview).",
                        );
                    if btn.clicked() {
                        self.retry_metadata_item_id(id);
                    }
                }
            });
        });
    }

    fn draw_card_list(&mut self, ui: &mut egui::Ui, idx: usize, allow_reorder: bool) {
        let id = self.items[idx].item_id;
        let status = self.items[idx].status;
        let pct = self.items[idx].percent;
        let selected = self.selected_item_ids.contains(&id);
        let output_ready = Path::new(&self.output_dir).is_dir();
        let failure_text =
            crate::app_state::queue_item_failure_text(&self.items[idx]).map(|t| t.to_owned());
        let has_error = self.items[idx].error.is_some();
        let can_retry_download = status == ItemStatus::Failed
            && output_ready
            && self.has_yt_dlp
            && self.item_has_redownload_target(&self.items[idx]);
        let can_retry_metadata = matches!(status, ItemStatus::Idle)
            && has_error
            && self.has_yt_dlp
            && !self.add_in_progress
            && !self.items[idx].source_line.trim().is_empty();
        let row_w = crate::app_ui::clip_bounded_width(ui);
        ui.set_max_width(row_w);
        let title = ellipsize(
            &self.items[idx].title,
            ((row_w / 7.0).floor() as usize).clamp(24, 80),
        );
        let row_response = ui.horizontal(|ui| {
            if allow_reorder && status == ItemStatus::Idle {
                let drag_id = egui::Id::new(("ready_drag", id));
                let _drag = ui.dnd_drag_source(drag_id, std::sync::Arc::new(id), |ui| {
                    ui.label(RichText::new("↕").weak());
                });
            }
            let mut sel = selected;
            if ui.checkbox(&mut sel, "").changed() {
                if sel {
                    self.selected_item_ids.insert(id);
                } else {
                    self.selected_item_ids.remove(&id);
                }
            }
            draw_status_chip(ui, status);
            let title_w = ui.available_width().max(40.0);
            ui.add_sized(
                [title_w, ui.spacing().interact_size.y],
                egui::Label::new(RichText::new(title).strong()).truncate(),
            );
            if status == ItemStatus::Downloading || status == ItemStatus::Queued {
                let bar_w = ui.available_width().clamp(48.0, 120.0);
                ui.add_sized(
                    [bar_w, ui.spacing().interact_size.y],
                    egui::ProgressBar::new((pct / 100.0).clamp(0.0, 1.0)).show_percentage(),
                );
            }
            if status == ItemStatus::Idle {
                let current = self.items[idx].format_override.clone().unwrap_or_default();
                let mut fmt_buf = current.clone();
                let fmt_w = ui.available_width().clamp(72.0, 140.0);
                let response = ui.add_sized(
                    [fmt_w, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut fmt_buf).hint_text("Format (-f)"),
                );
                if response.lost_focus() && fmt_buf.trim() != current.trim() {
                    let trimmed = fmt_buf.trim();
                    let format_override = if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_owned())
                    };
                    let profile_override = self.items[idx].profile_override.clone();
                    self.set_item_download_overrides(id, format_override, profile_override);
                }
            }
            if let Some(url) = crate::app_state::resolve_item_download_url(&self.items[idx]) {
                let ctx = ui.ctx().clone();
                let mut copy_url = false;
                let mut open_url = false;
                ui.push_id(("list_url_menu", id), |ui| {
                    let popup_id = ui.make_persistent_id("popup");
                    popup_menu_above(
                        ui,
                        popup_id,
                        format!("{} URL...", ui_icons::PAGE_URL),
                        |ui| {
                            if ui
                                .button(format!("{} Copy URL", ui_icons::COPY_CLIPBOARD))
                                .on_hover_text(&url)
                                .clicked()
                            {
                                copy_url = true;
                            }
                            if ui
                                .button(format!("{} Open URL", ui_icons::UPDATE_OPEN))
                                .on_hover_text("Open in your default browser")
                                .clicked()
                            {
                                open_url = true;
                            }
                        },
                    )
                    .on_hover_text(&url);
                });
                if copy_url {
                    ctx.copy_text(url.clone());
                }
                if open_url {
                    if let Err(e) = crate::app_actions::open_browser(&url) {
                        self.append_log(&format!("Failed to open URL: {e}"));
                    }
                }
            }
            self.draw_more_info_button(ui, id);
        });
        if let Some(ref fail) = failure_text {
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                let err_display = ellipsize(fail, 96);
                ui.label(RichText::new(err_display).small().color(LOG_COLOR_ERROR));
            });
        }
        if can_retry_download || can_retry_metadata {
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                self.draw_card_retry_buttons(
                    ui,
                    id,
                    "list_retry",
                    can_retry_download,
                    can_retry_metadata,
                );
            });
        }
        if allow_reorder && status == ItemStatus::Idle {
            if let Some(dragged) = row_response.response.dnd_release_payload::<u64>() {
                if *dragged != id {
                    self.reorder_ready_items(*dragged, id);
                }
            }
        }
        ui.separator();
    }

    fn draw_more_info_button(&mut self, ui: &mut egui::Ui, item_id: u64) {
        let popup_id = ui.id().with(("more_info_popup", item_id));
        let label = format!("{} More info", ui_icons::MORE_INFO);
        let button = ui
            .button(label)
            .on_hover_text("Source metadata from yt-dlp; file details from ffprobe when downloaded");
        if button.clicked() {
            self.ensure_queue_item_metadata(item_id);
            ui.memory_mut(|mem| mem.toggle_popup(popup_id));
        }
        if ui.memory(|mem| mem.is_popup_open(popup_id)) {
            let rows = self
                .item_idx(item_id)
                .map(|idx| queue_item_more_info_rows(&self.items[idx]))
                .unwrap_or_default();
            show_menu_popup(ui, popup_id, &button, |ui| {
                ui.set_min_width(360.0);
                if rows.is_empty() {
                    ui.label("No metadata yet — wait for resolve or finish the download.");
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        let mut last_section = "";
                        for row in &rows {
                            if row.section != last_section {
                                if !last_section.is_empty() {
                                    ui.add_space(6.0);
                                    ui.separator();
                                    ui.add_space(4.0);
                                }
                                ui.label(RichText::new(row.section).strong());
                                last_section = row.section;
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                ui.label(RichText::new(format!("{}:", row.label)).weak());
                                ui.label(&row.value);
                            });
                        }
                    });
            });
        }
    }

    fn item_in_queue_group(&self, it: &QueueItem, label: &str) -> bool {
        if !self.item_matches_search(it) {
            return false;
        }
        match label {
            "Active" => matches!(it.status, ItemStatus::Downloading | ItemStatus::Queued),
            "Ready" => it.status == ItemStatus::Idle && it.error.is_none(),
            "Issues" => {
                it.status == ItemStatus::Failed
                    || (it.status == ItemStatus::Idle && it.error.is_some())
            }
            "Done" => it.status == ItemStatus::Done && self.item_matches_history_filter(it),
            "Resolving" => it.status == ItemStatus::Resolving,
            _ => false,
        }
    }

    fn queue_group_default_open(&self, label: &str, scroll_here: bool) -> bool {
        if scroll_here || self.queue_group_focus.is_some_and(|f| f == label) {
            return true;
        }
        let done_collapse = if self.settings.ui_power_save { 15 } else { 30 };
        if label == "Done" && self.items.len() > done_collapse {
            return false;
        }
        match label {
            "Done" => {
                self.status_done > 0
                    && self.status_active == 0
                    && self.status_queued == 0
                    && self.status_ready == 0
                    && self.status_resolving == 0
            }
            "Ready" => self.items.len() <= 12,
            "Issues" => true,
            _ => self.queue_search.is_empty(),
        }
    }

    fn rebuild_queue_group_cache(&mut self) {
        use std::collections::HashMap;

        let groups = ["Active", "Ready", "Issues", "Done", "Resolving"];
        let mut map = HashMap::new();
        for label in groups {
            let mut ids: Vec<u64> = self
                .items
                .iter()
                .filter(|it| self.item_in_queue_group(it, label))
                .map(|it| it.item_id)
                .collect();
            if label == "Ready" {
                ids.sort_by_key(|id| {
                    self.item_idx(*id)
                        .map(|idx| {
                            let it = &self.items[idx];
                            if it.sort_order == 0 {
                                it.item_id
                            } else {
                                it.sort_order
                            }
                        })
                        .unwrap_or(*id)
                });
            } else if label == "Done" {
                ids.sort_by(|a, b| {
                    let key = |id: u64| {
                        self.item_idx(id)
                            .map(|idx| crate::app_state::done_item_sort_key(&self.items[idx]))
                            .unwrap_or((0, id))
                    };
                    key(*b).cmp(&key(*a))
                });
            }
            map.insert(label.to_owned(), ids);
        }
        self.queue_group_cache = super::queue_cache::QueueGroupCache {
            core_generation: self.core_generation,
            queue_search: self.queue_search.clone(),
            history_filter_days: self.history_filter_days,
            queue_group_focus: self.queue_group_focus.map(|s| s.to_owned()),
            groups: map,
        };
    }

    fn ensure_queue_group_cache(&mut self) {
        if self.queue_group_cache.is_current(
            self.core_generation,
            &self.queue_search,
            self.history_filter_days,
            self.queue_group_focus,
        ) {
            return;
        }
        self.rebuild_queue_group_cache();
    }

    fn draw_done_group_history_controls(
        &mut self,
        ui: &mut egui::Ui,
        row_w: f32,
        done_ids: &[u64],
    ) {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label("History:");
        let combo_w = (row_w * 0.38).clamp(72.0, 120.0);
        egui::ComboBox::from_id_salt("history_filter_Done")
            .width(combo_w)
            .selected_text(match self.history_filter_days {
                None => "All time".to_owned(),
                Some(1) => "Last 24h".to_owned(),
                Some(7) => "Last 7 days".to_owned(),
                Some(30) => "Last 30 days".to_owned(),
                Some(d) => format!("Last {d} days"),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.history_filter_days, None, "All time");
                ui.selectable_value(&mut self.history_filter_days, Some(1), "Last 24h");
                ui.selectable_value(&mut self.history_filter_days, Some(7), "Last 7 days");
                ui.selectable_value(&mut self.history_filter_days, Some(30), "Last 30 days");
            });
        let requeue_label = if row_w < 420.0 {
            "Re-queue"
        } else {
            "Re-queue visible"
        };
        if ui
            .small_button(requeue_label)
            .on_hover_text("Move visible Done items back to Ready")
            .clicked()
        {
            let n = self.requeue_done_items(done_ids);
            if n > 0 {
                self.append_log(&format!("Re-queued {n} done item(s)."));
            }
        }
    }

    pub(super) fn draw_grouped_cards(&mut self, ui: &mut egui::Ui, outer_scroll_h: f32) {
        if self.item_index_by_id.len() != self.items.len() {
            self.rebuild_item_index();
        }
        self.ensure_queue_group_cache();
        let groups = ["Active", "Ready", "Issues", "Done", "Resolving"];
        for label in groups {
            if self.queue_group_focus.is_some_and(|f| f != label) {
                continue;
            }
            let ids = self
                .queue_group_cache
                .groups
                .get(label)
                .cloned()
                .unwrap_or_default();
            if ids.is_empty() {
                continue;
            }
            let header_color = match label {
                "Active" => status_color(ItemStatus::Downloading),
                "Ready" => status_color(ItemStatus::Idle),
                "Issues" => status_color(ItemStatus::Failed),
                "Done" => status_color(ItemStatus::Done),
                "Resolving" => status_color(ItemStatus::Resolving),
                _ => Color32::LIGHT_GRAY,
            };
            let scroll_here = self.scroll_to_queue_group == Some(label);
            let default_open = self.queue_group_default_open(label, scroll_here);
            let header_text = format!("{label} ({})", ids.len());
            let id = ui.make_persistent_id(label);
            let theme = self.settings.theme.clone();
            let mut header_inner = None;
            show_queue_group_section(ui, &theme, header_color, |ui| {
                let header = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    default_open,
                )
                .show_header(ui, |ui| {
                    let row_w = clip_bounded_width(ui);
                    ui.set_max_width(row_w);
                    let ui_scale = crate::config::snap_ui_scale(self.settings.ui_scale);
                    let done_inline_min = layout_breakpoint(420.0, ui_scale);
                    if label == "Done" && !ids.is_empty() {
                        let mut drew_inline = false;
                        if row_w >= done_inline_min {
                            ui.horizontal(|ui| {
                                ui.set_max_width(row_w);
                                status_dot_with_label(ui, &header_text, header_color, true);
                                let tail_w = ui.available_width().max(0.0);
                                if tail_w >= 160.0 {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.set_max_width(tail_w);
                                            self.draw_done_group_history_controls(ui, tail_w, &ids);
                                        },
                                    );
                                    drew_inline = true;
                                }
                            });
                        }
                        if !drew_inline {
                            if row_w < done_inline_min {
                                status_dot_with_label(ui, &header_text, header_color, true);
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.set_max_width(row_w);
                                self.draw_done_group_history_controls(ui, row_w, &ids);
                            });
                        }
                    } else {
                        status_dot_with_label(ui, &header_text, header_color, true);
                    }
                });
                let (_toggle, inner, _) = header.body(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);
                    let allow_reorder = label == "Ready";
                    let use_list = self.effective_card_list_layout_for_panel(outer_scroll_h)
                        || ids.len() >= self.queue_card_list_fallback_threshold()
                        || queue_short_panel_list_fallback(outer_scroll_h, false);
                    let flatten = should_flatten_nested_group_scroll(outer_scroll_h);
                    if use_list {
                        let list_row_h = QUEUE_DL_LIST_ROW_H;
                        let row_count = ids.len().max(1);
                        let outer_cap = outer_scroll_h.max(list_row_h);
                        let max_h = if flatten {
                            outer_cap
                        } else {
                            (row_count as f32 * list_row_h + 8.0)
                                .clamp(list_row_h, 600.0)
                                .min(outer_cap)
                        };
                        egui::ScrollArea::vertical()
                            .id_salt(format!("rustdl_list_{label}"))
                            .max_height(max_h)
                            .auto_shrink([false, true])
                            .show_rows(ui, list_row_h, ids.len(), |ui, row_range| {
                                for row in row_range {
                                    if let Some(item_id) = ids.get(row) {
                                        if let Some(idx) = self.item_idx(*item_id) {
                                            self.draw_card_list(ui, idx, allow_reorder);
                                        }
                                    }
                                }
                            });
                    } else {
                        let row_width = ui.available_width().max(1.0);
                        ui.set_width(row_width);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                            for id in &ids {
                                let idx = self
                                    .item_idx(*id)
                                    .or_else(|| self.items.iter().position(|it| it.item_id == *id));
                                if let Some(idx) = idx {
                                    self.draw_card(ui, idx, allow_reorder);
                                }
                            }
                        });
                    }
                });
                header_inner = Some(inner);
            });
            if scroll_here {
                if let Some(inner) = header_inner {
                    ui.scroll_to_rect(inner.response.rect, Some(egui::Align::TOP));
                }
                self.scroll_to_queue_group = None;
            }
        }
        if !self.queue_search.is_empty()
            && !self.items.iter().any(|it| self.item_matches_search(it))
        {
            ui.label(RichText::new("No items match your search.").color(Color32::GRAY));
        }
    }
}

/// Fit `natural` inside `max` preserving aspect ratio; never larger than `max`; never upscale past native size.
fn fit_thumbnail_draw_size(natural: egui::Vec2, max: egui::Vec2) -> egui::Vec2 {
    if natural.x < 1.0 || natural.y < 1.0 {
        return egui::Vec2::ZERO;
    }
    let scale = (max.x / natural.x).min(max.y / natural.y).min(1.0);
    natural * scale
}

fn format_duration(sec: i64) -> String {
    let m = sec / 60;
    let s = sec % 60;
    let h = m / 60;
    let m2 = m % 60;
    if h > 0 {
        format!("{h}:{m2:02}:{s:02}")
    } else {
        format!("{m2}:{s:02}")
    }
}

fn ellipsize(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut iter = input.chars();
    for _ in 0..max_chars {
        match iter.next() {
            Some(ch) => out.push(ch),
            None => return input.to_owned(),
        }
    }
    if iter.next().is_some() {
        out.push('…');
    }
    out
}

fn format_resolution_label(width: Option<u32>, height: Option<u32>) -> Option<String> {
    match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => Some(format!("{w}x{h}")),
        (Some(w), None) if w > 0 => Some(format!("{w}w")),
        (None, Some(h)) if h > 0 => Some(format!("{h}p")),
        _ => None,
    }
}
