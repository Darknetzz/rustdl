use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{
    compact_button_group, draw_meta_badge, draw_status_chip, left_button_row, status_color,
    status_dot_with_label, MetaBadgeKind,
};
use crate::models::{ItemStatus, QueueItem};
use crate::theme;
use crate::time_format::{format_absolute_local, format_relative_ago};
use crate::ui_icons;

use super::{log_line_color, CancelPostAction, PydlApp, LOG_COLOR_ERROR, LOG_COLOR_WARN};

impl PydlApp {
    pub(super) fn draw_card(&mut self, ui: &mut egui::Ui, idx: usize, allow_reorder: bool) {
        if self.settings.card_list_layout {
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
        let can_redownload = show_saved_file_actions && {
            let it = &self.items[idx];
            self.item_has_redownload_target(it)
        };
        let output_ready = Path::new(&self.output_dir).is_dir();
        let is_pre_download = matches!(status, ItemStatus::Idle | ItemStatus::Queued);
        let resolution_label =
            format_resolution_label(self.items[idx].width, self.items[idx].height)
                .map(|s| s.replace('x', "×"));
        let show_size_badge = is_pre_download && size_text != "-";

        let highlight_completed = status == ItemStatus::Done && !done_but_file_missing;
        let done_fill = theme::done_card_fill(&self.settings.theme);

        let card_inner = |ui: &mut egui::Ui| {
            let compact = self.settings.compact_cards;
            let card_w = if compact { 320.0 } else { 360.0 };
            let inner_w = (card_w - 20.0_f32).max(1.0);
            let thumb = if compact {
                egui::vec2(296.0, 104.0)
            } else {
                egui::vec2(332.0, 158.0)
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
                    && !self.thumbnail_attempted.contains(&id)
                    && !self.thumbnail_inflight.contains(&id)
                {
                    if let Some(url) = &thumbnail_url {
                        self.queue_thumbnail_load(id, url.clone());
                    }
                }
                // Fixed max cell; image keeps aspect ratio and never exceeds thumb (no upscale).
                let (thumb_rect, _) = ui.allocate_exact_size(thumb, egui::Sense::hover());
                let thumb_painter = ui.painter();
                thumb_painter.rect_filled(
                    thumb_rect,
                    egui::Rounding::same(8.0),
                    theme::THUMB_PLACEHOLDER,
                );
                if let Some(tex) = self.textures.get(&id) {
                    let nat = tex.size_vec2();
                    let draw_sz = fit_thumbnail_draw_size(nat, thumb_rect.size());
                    if draw_sz.x >= 1.0 && draw_sz.y >= 1.0 {
                        let img_rect =
                            egui::Rect::from_center_size(thumb_rect.center(), draw_sz);
                        thumb_painter.image(
                            tex.id(),
                            img_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                } else {
                    let center_msg = if done_but_file_missing {
                        "Removed"
                    } else if !self.settings.show_thumbnails {
                        "Thumbnails off"
                    } else if has_thumbnail_url {
                        "Fetching thumbnail..."
                    } else {
                        "No preview available"
                    };
                    thumb_painter.text(
                        thumb_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        center_msg,
                        egui::TextStyle::Body.resolve(ui.style()),
                        if done_but_file_missing {
                            LOG_COLOR_WARN
                        } else {
                            Color32::from_gray(130)
                        },
                    );
                }
                if done_but_file_missing && self.textures.contains_key(&id) {
                    thumb_painter.text(
                        thumb_rect.center_bottom() + egui::vec2(0.0, -6.0),
                        egui::Align2::CENTER_BOTTOM,
                        "Removed",
                        egui::TextStyle::Small.resolve(ui.style()),
                        LOG_COLOR_WARN,
                    );
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
                    egui::Label::new(RichText::new(subtitle_text).small().color(Color32::LIGHT_GRAY))
                        .wrap(),
                );
                if let Some((ref path, mtime)) = done_file {
                    let fname = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file");
                    let rel = format_relative_ago(mtime);
                    let abs = format_absolute_local(mtime);
                    let file_line = format!("{fname} · {rel}");
                    let hover = format!(
                        "{}\nModified: {abs}",
                        path.to_string_lossy()
                    );
                    let file_label = ui.add(
                        egui::Label::new(RichText::new(file_line).small().color(Color32::from_gray(150)))
                            .wrap(),
                    );
                    file_label.on_hover_text(hover);
                }
                let footer_status = if resolving {
                    "metadata".to_owned()
                } else if is_pre_download {
                    "ready".to_owned()
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
                if detail_h > 0.0 {
                    let detail_short = ellipsize(&detail, 64);
                    let detail_color = log_line_color(&detail);
                    ui.add_sized(
                        [inner_w, detail_h],
                        egui::Label::new(
                            RichText::new(detail_short)
                                .small()
                                .monospace()
                                .color(detail_color),
                        )
                        .wrap(),
                    );
                }

                if let Some(ref err) = has_error {
                    let err_display = ellipsize(err, 72);
                    ui.add_sized(
                        [inner_w, 16.0],
                        egui::Label::new(
                            RichText::new(err_display)
                                .small()
                                .color(LOG_COLOR_ERROR),
                        )
                        .wrap(),
                    );
                }

                let can_retry_download = status == ItemStatus::Failed
                    && output_ready
                    && self.has_yt_dlp
                    && {
                        let it = &self.items[idx];
                        self.item_has_redownload_target(it)
                    };
                let can_retry_metadata = matches!(status, ItemStatus::Idle)
                    && has_error.is_some()
                    && self.has_yt_dlp
                    && !self.add_in_progress
                    && !self.items[idx].source_line.trim().is_empty();
                if can_retry_download || can_retry_metadata {
                    left_button_row(ui, |ui| {
                        compact_button_group(ui, ("card_retry", id), |g| {
                            if can_retry_download {
                                let btn = g
                                    .warning(
                                        &format!("{} Retry", ui_icons::RETRY),
                                        true,
                                    )
                                    .on_hover_text(
                                        "Queue this video for download again using the same URL as this row.",
                                    );
                                if btn.clicked() {
                                    self.retry_download_item_id(id);
                                }
                            }
                            if can_retry_metadata {
                                let btn = g
                                    .secondary(
                                        &format!("{} Refetch", ui_icons::RETRY),
                                        true,
                                    )
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

                if show_saved_file_actions {
                    left_button_row(ui, |ui| {
                        compact_button_group(ui, ("card_done_actions", id), |g| {
                            if let Some((p, _)) = done_file.as_ref() {
                                if g.success(
                                    &format!("{} Open", ui_icons::OPEN_FILE),
                                    true,
                                )
                                .on_hover_text("Open with the default app for this file type")
                                .clicked()
                                {
                                    self.open_file_path(p);
                                }
                                if g.secondary(
                                    &format!("{} Folder", ui_icons::REVEAL_FOLDER),
                                    true,
                                )
                                .on_hover_text("Show the file in Explorer / file manager")
                                .clicked()
                                {
                                    self.reveal_file_path(p);
                                }
                            } else if done_but_file_missing {
                                if g.secondary(
                                    &format!("{} Folder", ui_icons::REVEAL_FOLDER),
                                    true,
                                )
                                .on_hover_text("Open the output folder for this download")
                                .clicked()
                                {
                                    self.open_item_output_folder(id);
                                }
                            }
                            if done_file.is_some() {
                                if g.secondary(
                                    &format!("{} Streams", ui_icons::CHECK_STREAMS),
                                    self.has_ffprobe,
                                )
                                .on_hover_text(
                                    "Run ffprobe on the saved file and refresh resolution on the card.",
                                )
                                .on_disabled_hover_text(
                                    "Configure ffprobe in Settings → Executables.",
                                )
                                .clicked()
                                {
                                    self.check_streams_for_item_id(id);
                                }
                            }
                            if g.secondary(
                                &format!("{} Redo", ui_icons::REDOWNLOAD),
                                self.has_yt_dlp && output_ready && can_redownload,
                            )
                            .on_hover_text(
                                "Deletes the matched file in the output folder (if found), then downloads this URL again.",
                            )
                            .on_disabled_hover_text(
                                "Needs a video URL on this row, a valid output folder, and yt-dlp.",
                            )
                            .clicked()
                            {
                                self.redownload_item_id(id);
                            }
                            if let Some((p, _)) = done_file.as_ref() {
                                if g.danger(
                                    &format!("{} Delete", ui_icons::CARD_DELETE),
                                    true,
                                )
                                .on_hover_text(
                                    "Delete only this file; the queue row stays until you remove it",
                                )
                                .clicked()
                                {
                                    self.delete_file_path(p);
                                }
                            }
                            if g.secondary(
                                &format!("{} Remove", ui_icons::REMOVE),
                                removable,
                            )
                            .on_hover_text(
                                "Remove this row from the list (does not delete the file unless you use Delete above).",
                            )
                            .clicked()
                            {
                                let _ = self.remove_item_by_id(id);
                                self.update_status();
                                self.refresh_input_line_info();
                                self.schedule_queue_save();
                            }
                        });
                    });
                }

                ui.separator();
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
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
                ui.add(
                    egui::Label::new(RichText::new(&footer_status).small().color(footer_color))
                        .wrap(),
                );
                ui.set_width(inner_w);
                if matches!(status, ItemStatus::Queued | ItemStatus::Downloading)
                    || (!show_saved_file_actions && removable)
                {
                    left_button_row(ui, |ui| {
                        compact_button_group(ui, ("card_actions", id), |g| {
                            if matches!(status, ItemStatus::Queued | ItemStatus::Downloading)
                                && g.warning(
                                    &format!("{} Ready", ui_icons::CANCEL_TO_READY),
                                    true,
                                )
                                .on_hover_text("Cancel download and mark as ready")
                                .clicked()
                            {
                                self.request_cancel_item(id, CancelPostAction::Ready);
                            }
                            if matches!(status, ItemStatus::Queued | ItemStatus::Downloading)
                                && g.danger(
                                    &format!("{} Drop", ui_icons::CANCEL_TO_REMOVE),
                                    true,
                                )
                                .on_hover_text("Cancel download and remove from queue")
                                .clicked()
                            {
                                self.request_cancel_item(id, CancelPostAction::Remove);
                            }
                            if !show_saved_file_actions
                                && g.secondary(
                                    &format!("{} Remove", ui_icons::REMOVE),
                                    removable,
                                )
                                .clicked()
                            {
                                let _ = self.remove_item_by_id(id);
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

    fn draw_card_list(&mut self, ui: &mut egui::Ui, idx: usize, allow_reorder: bool) {
        let id = self.items[idx].item_id;
        let status = self.items[idx].status;
        let title = ellipsize(&self.items[idx].title, 80);
        let pct = self.items[idx].percent;
        let selected = self.selected_item_ids.contains(&id);
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
            ui.label(RichText::new(title).strong());
            if status == ItemStatus::Downloading || status == ItemStatus::Queued {
                ui.add(egui::ProgressBar::new((pct / 100.0).clamp(0.0, 1.0)).show_percentage());
            }
        });
        if allow_reorder && status == ItemStatus::Idle {
            if let Some(dragged) = row_response.response.dnd_release_payload::<u64>() {
                if *dragged != id {
                    self.reorder_ready_items(*dragged, id);
                }
            }
        }
        ui.separator();
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
            "Done" => it.status == ItemStatus::Done,
            "Resolving" => it.status == ItemStatus::Resolving,
            _ => false,
        }
    }

    fn queue_group_default_open(&self, label: &str, scroll_here: bool) -> bool {
        if scroll_here || self.queue_group_focus.is_some_and(|f| f == label) {
            return true;
        }
        if label == "Done" && self.items.len() > 30 {
            return false;
        }
        match label {
            "Done" => false,
            "Ready" => self.items.len() <= 12,
            "Issues" => true,
            _ => self.queue_search.is_empty(),
        }
    }

    pub(super) fn draw_grouped_cards(&mut self, ui: &mut egui::Ui) {
        if self.item_index_by_id.len() != self.items.len() {
            self.rebuild_item_index();
        }
        let groups = ["Active", "Ready", "Issues", "Done", "Resolving"];
        for label in groups {
            if self.queue_group_focus.is_some_and(|f| f != label) {
                continue;
            }
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
            }
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
            let header = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                default_open,
            )
            .show_header(ui, |ui| {
                status_dot_with_label(ui, &header_text, header_color, true)
            });
            let (_toggle, header_inner, _) = header.body(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                if self.settings.card_list_layout {
                    let allow_reorder = label == "Ready";
                    const LIST_ROW_H: f32 = 42.0;
                    let max_h = ui.available_height().clamp(LIST_ROW_H * 3.0, 480.0);
                    egui::ScrollArea::vertical()
                        .id_salt(format!("rustdl_list_{label}"))
                        .max_height(max_h)
                        .auto_shrink([false; 2])
                        .show_rows(ui, LIST_ROW_H, ids.len(), |ui, row_range| {
                            for row in row_range {
                                if let Some(item_id) = ids.get(row) {
                                    if let Some(idx) = self.item_idx(*item_id) {
                                        self.draw_card(ui, idx, allow_reorder);
                                    }
                                }
                            }
                        });
                } else {
                    let row_width = ui.available_width().max(1.0);
                    let card_h = if self.settings.compact_cards {
                        240.0
                    } else {
                        360.0
                    };
                    egui::ScrollArea::horizontal()
                        .id_salt(format!("rustdl_cards_{label}"))
                        .auto_shrink([false, false])
                        .max_width(row_width)
                        .max_height(card_h + 16.0)
                        .animated(true)
                        .drag_to_scroll(true)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                                for id in &ids {
                                    let idx = self.item_idx(*id).or_else(|| {
                                        self.items.iter().position(|it| it.item_id == *id)
                                    });
                                    if let Some(idx) = idx {
                                        self.draw_card(ui, idx, false);
                                    }
                                }
                            });
                        });
                }
            });
            if scroll_here {
                ui.scroll_to_rect(header_inner.response.rect, Some(egui::Align::TOP));
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
