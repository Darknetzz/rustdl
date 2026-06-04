use eframe::egui::{self, Color32, RichText};

use crate::app_actions;
use crate::app_parsing::human_bytes_ui;
use crate::app_ui::{
    button_group, button_toolbar_wrapped, compute_main_column_split, constrain_content_width,
    draw_labeled_meta_badge, draw_meta_badge, draw_status_dot, left_button_row, status_color,
    status_dot_with_label, MetaBadgeKind,
};
use crate::config::AppSettings;
use crate::av1_state::{av1_item_is_skipped, av1_item_status_label, compute_av1_batch_summary};
use crate::av1_transcode;
use crate::models::{Av1QueueItem, ItemStatus};
use crate::service::DownloadCore;
use crate::theme;
use crate::theme::text_muted;
use crate::ui_icons;

use super::PydlApp;

const AV1_SKIPPED_COLOR: Color32 = Color32::from_rgb(255, 167, 38);

fn ellipsize_str(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut iter = input.chars();
    for _ in 0..max_chars {
        match iter.next() {
            Some(ch) => out.push(ch),
            None => return input.to_owned(),
        }
    }
    if iter.next().is_some() {
        out.push_str("...");
    }
    out
}

fn draw_av1_bytes_arrow(ui: &mut egui::Ui, from: &str, to: &str, text_color: Color32, theme: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(from).small().color(text_color));
        ui.label(
            RichText::new(ui_icons::ARROW_FORWARD)
                .small()
                .color(text_muted(theme)),
        );
        ui.label(RichText::new(to).small().color(text_color));
    });
}

fn draw_av1_path_line(ui: &mut egui::Ui, prefix: &str, path: &str, theme: &str) {
    let shortened = ellipsize_str(path, 76);
    let response = ui.add(
        egui::Label::new(
            RichText::new(format!("{prefix} {shortened}"))
                .small()
                .color(text_muted(theme)),
        )
        .truncate(),
    );
    if shortened != path {
        response.on_hover_text(path);
    }
}

fn av1_item_status_color(item: &Av1QueueItem) -> Color32 {
    if av1_item_is_skipped(item) {
        return AV1_SKIPPED_COLOR;
    }
    status_color(item.status)
}

fn format_av1_bitrate(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.2} Mbps", bps as f64 / 1_000_000.0)
    } else if bps >= 1_000 {
        format!("{:.0} kbps", bps as f64 / 1_000.0)
    } else {
        format!("{bps} bps")
    }
}

fn av1_item_has_media(item: &Av1QueueItem) -> bool {
    !item.video_codec.is_empty()
        || item.width.is_some()
        || item.height.is_some()
        || item.fps.is_some()
        || item.bitrate_bps.is_some()
}

fn draw_av1_encode_settings_badges(ui: &mut egui::Ui, settings: &AppSettings, theme: &str) {
    let muted = text_muted(theme);
    let bitrate = if settings.av1_target_bitrate.trim().is_empty() {
        "auto".to_owned()
    } else {
        settings.av1_target_bitrate.clone()
    };
    let max_width = format!("{}w", settings.av1_max_width);
    let min_shrink = format!("{:.0}%", settings.av1_min_shrink_percent);
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
    draw_labeled_meta_badge(ui, "Bitrate:", &bitrate, MetaBadgeKind::Bitrate, muted);
    ui.label(RichText::new("·").small().color(muted));
    draw_labeled_meta_badge(
        ui,
        "Max width:",
        &max_width,
        MetaBadgeKind::Resolution,
        muted,
    );
    ui.label(RichText::new("·").small().color(muted));
    draw_labeled_meta_badge(
        ui,
        "Preset:",
        &settings.av1_size_preset,
        MetaBadgeKind::SizePreset,
        muted,
    );
    ui.label(RichText::new("·").small().color(muted));
    draw_labeled_meta_badge(
        ui,
        "Min shrink:",
        &min_shrink,
        MetaBadgeKind::ShrinkPercent,
        muted,
    );
    ui.label(RichText::new("·").small().color(muted));
    let container = if settings.av1_use_recommended_container {
        "MKV"
    } else {
        "same ext"
    };
    draw_labeled_meta_badge(
        ui,
        "Container:",
        container,
        MetaBadgeKind::SizePreset,
        muted,
    );
}

fn draw_av1_media_badges(ui: &mut egui::Ui, item: &Av1QueueItem, probing: bool, theme: &str) {
    if probing {
        ui.label(
            RichText::new("Probing metadata...")
                .small()
                .color(text_muted(theme)),
        );
        return;
    }
    if !av1_item_has_media(item) {
        ui.label(
            RichText::new("Metadata unavailable")
                .small()
                .color(text_muted(theme)),
        );
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
        if !item.video_codec.is_empty() {
            draw_meta_badge(ui, &item.video_codec.to_uppercase(), MetaBadgeKind::Codec);
        }
        if let (Some(w), Some(h)) = (item.width, item.height) {
            draw_meta_badge(ui, &format!("{w}x{h}"), MetaBadgeKind::Resolution);
        }
        if let Some(fps) = item.fps {
            draw_meta_badge(ui, &format!("{fps:.2} fps"), MetaBadgeKind::FrameRate);
        }
        if item.input_bytes > 0 {
            draw_meta_badge(
                ui,
                &human_bytes_ui(item.input_bytes),
                MetaBadgeKind::FileSize,
            );
        }
        if let Some(bps) = item.bitrate_bps {
            draw_meta_badge(ui, &format_av1_bitrate(bps), MetaBadgeKind::Bitrate);
        }
    });
}

fn av1_encoder_detect_key(ffmpeg_path: &str, encoder_override: &str) -> String {
    format!("{ffmpeg_path}\0{encoder_override}")
}

impl PydlApp {
    pub(super) fn refresh_av1_encoder_detection(&mut self) {
        if !self.has_ffmpeg {
            self.av1_encoder_choice = None;
            self.av1_encoder_detect_key.clear();
            return;
        }
        let key = av1_encoder_detect_key(
            &self.settings.ffmpeg_path,
            &self.settings.av1_encoder_override,
        );
        if self.av1_encoder_detect_key == key {
            return;
        }
        self.av1_encoder_choice = Some(av1_transcode::detect_encoder_with_override(
            &self.settings.ffmpeg_path,
            &self.settings.av1_encoder_override,
        ));
        self.av1_encoder_detect_key = key;
    }

    /// Runs an AV1 mutation against the shared `DownloadCore` (the single source of truth) and
    /// refreshes the GUI mirror fields from it. The editable textarea is pushed in first so the
    /// core sees the latest paths, then read back (a scan trims the lines it consumed).
    pub(super) fn av1_core_action(&mut self, f: impl FnOnce(&mut DownloadCore)) {
        {
            let mut core = self.shared_core.lock();
            core.av1_input_paths = self.av1_input_paths.clone();
            f(&mut core);
            self.av1_input_paths = core.av1_input_paths.clone();
            self.av1_items = core.av1_items.clone();
            self.av1_running = core.av1_running;
            self.av1_media_inflight = core.av1_media_inflight.clone();
            self.core_generation = core.generation;
        }
        self.ensure_av1_thumbnails();
    }

    fn clear_av1_queue(&mut self) {
        let ids: Vec<u64> = self.av1_items.iter().map(|it| it.item_id).collect();
        self.av1_core_action(|core| core.clear_av1_queue());
        for id in ids {
            self.textures.remove(&id);
            self.thumbnail_inflight.remove(&id);
            self.thumbnail_attempted.remove(&id);
        }
    }

    pub(super) fn draw_av1_panel(&mut self, ui: &mut egui::Ui) {
        let main_split = compute_main_column_split(
            ui.available_height(),
            self.settings.videos_docked,
            self.settings.compact_cards,
        );

        egui::ScrollArea::vertical()
            .id_salt("rustdl_av1_controls_v5")
            .hscroll(false)
            .auto_shrink([false, true])
            .max_height(main_split.controls_max_height)
            .show(ui, |ui| {
                constrain_content_width(ui);

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("AV1 Converter").heading());
                    ui.label(
                        RichText::new("Near-parity mode for local video transcoding.")
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                });
                ui.separator();
                ui.label("Input paths (file/folder, one per line)");
                left_button_row(ui, |ui| {
                    button_group(ui, "av1_input", |g| {
                        if g.secondary(&format!("{} Browse", ui_icons::BROWSE), true).clicked() {
                            self.browse_av1_inputs();
                        }
                        if g.secondary(&format!("{} Scan inputs", ui_icons::SCAN), true).clicked()
                        {
                            self.scan_av1_input_textbox();
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    let ready = self
                        .av1_items
                        .iter()
                        .filter(|item| item.status == ItemStatus::Idle)
                        .count();
                    if ready > 0 {
                        status_dot_with_label(
                            ui,
                            format!("{ready} ready"),
                            status_color(ItemStatus::Idle),
                            true,
                        );
                    }
                });
                // The buffer is mirrored to DownloadCore each frame (see core_sync); persistence happens
                // there on scan / exit, so no per-keystroke save is needed here.
                ui.add_sized(
                    [ui.available_width(), 90.0],
                    egui::TextEdit::multiline(&mut self.av1_input_paths)
                        .hint_text("D:\\Videos\\movie.mkv\nD:\\Videos\\Folder"),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Session").strong());
                    if ui
                        .checkbox(&mut self.settings.av1_dry_run, "Dry run this batch")
                        .changed()
                    {
                        self.persist_settings();
                    }
                    if ui
                        .checkbox(
                            &mut self.settings.av1_auto_start_on_add,
                            "Start batch when paths are added",
                        )
                        .on_hover_text(
                            "Automatically run Start AV1 batch after Browse, Scan inputs, \
                             or drag-and-drop adds new ready items.",
                        )
                        .changed()
                    {
                        self.persist_settings();
                    }
                });
                self.refresh_av1_encoder_detection();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Encode settings").small());
                    draw_av1_encode_settings_badges(ui, &self.settings, &self.settings.theme);
                    if let Some(enc) = &self.av1_encoder_choice {
                        status_dot_with_label(
                            ui,
                            av1_transcode::encoder_indicator_label(enc),
                            av1_transcode::encoder_indicator_color(enc),
                            true,
                        );
                    } else if !self.has_ffmpeg {
                        status_dot_with_label(
                            ui,
                            "Encoder: ffmpeg not found",
                            Color32::from_rgb(255, 193, 120),
                            true,
                        );
                    }
                });
                left_button_row(ui, |ui| {
                    button_group(ui, "av1_settings", |g| {
                        if g.secondary(
                            &format!("{} Edit in Settings", ui_icons::SETTINGS),
                            true,
                        )
                        .clicked()
                        {
                            self.settings_open = true;
                            self.settings_tab = super::SettingsTab::Av1;
                        }
                    });
                });
                button_toolbar_wrapped(ui, |ui| {
                    let ready_count = self
                        .av1_items
                        .iter()
                        .filter(|item| item.status == ItemStatus::Idle)
                        .count();
                    button_group(ui, "av1_batch", |g| {
                        if g.success(
                            &format!("{} Start AV1 batch", ui_icons::PLAY),
                            !self.av1_running
                                && self.has_ffmpeg
                                && self.has_ffprobe
                                && ready_count > 0,
                        )
                        .clicked()
                        {
                            self.start_av1_batch();
                        }
                        if g.danger(
                            &format!("{} Cancel AV1 batch", ui_icons::CANCEL_TO_READY),
                            self.av1_running,
                        )
                        .clicked()
                        {
                            self.av1_core_action(|core| core.cancel_av1_batch());
                        }
                    });
                    button_group(ui, "av1_queue", |g| {
                        if g.secondary(
                            &format!("{} Clear AV1 queue", ui_icons::CLEAR_QUEUE),
                            !self.av1_running,
                        )
                        .clicked()
                        {
                            self.clear_av1_queue();
                        }
                    });
                });
            }); // av1 controls scroll

        if self.settings.videos_docked {
            self.draw_docked_videos_section(ui, main_split.videos_height);
        } else {
            self.draw_videos_undocked_strip(ui);
            if self.settings.logs_open && self.settings.logs_docked {
                self.draw_docked_log_only_section(ui);
            }
        }
    }

    /// AV1 queue scroll area (docked panel or floating window).
    pub(super) fn draw_av1_queue_cards(&mut self, ui: &mut egui::Ui, max_height: f32) {
        if !self.av1_items.is_empty() {
            ui.add_space(4.0);
            self.draw_av1_queue_status_row(ui);
            self.draw_av1_batch_summary_row(ui);
            ui.add_space(6.0);
        }
        egui::ScrollArea::vertical()
            .id_salt("av1_queue_scroll")
            .auto_shrink([false, false])
            .max_height(max_height.max(120.0))
            .animated(true)
            .drag_to_scroll(true)
            .show(ui, |ui| {
                if self.av1_items.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(32.0);
                        ui.label(
                            RichText::new("Nothing here yet")
                                .color(text_muted(&self.settings.theme)),
                        );
                        ui.label(
                            RichText::new(
                                "Browse, drop, or scan paths to add videos to the queue.",
                            )
                            .small(),
                        );
                    });
                    return;
                }
                self.draw_av1_grouped_cards(ui);
            });
    }

    fn av1_item_in_queue_group(item: &Av1QueueItem, label: &str) -> bool {
        match label {
            "Active" => matches!(item.status, ItemStatus::Queued | ItemStatus::Downloading),
            "Ready" => item.status == ItemStatus::Idle,
            "Failed" => item.status == ItemStatus::Failed,
            "Skipped" => av1_item_is_skipped(item),
            "Done" => item.status == ItemStatus::Done && !av1_item_is_skipped(item),
            _ => false,
        }
    }

    fn av1_queue_group_default_open(&self, label: &str, scroll_here: bool) -> bool {
        if scroll_here || self.queue_group_focus.is_some_and(|f| f == label) {
            return true;
        }
        match label {
            "Done" | "Ready" => false,
            "Failed" | "Skipped" => true,
            _ => true,
        }
    }

    fn av1_queue_group_color(label: &str) -> Color32 {
        match label {
            "Active" => status_color(ItemStatus::Downloading),
            "Ready" => status_color(ItemStatus::Idle),
            "Failed" => status_color(ItemStatus::Failed),
            "Skipped" => AV1_SKIPPED_COLOR,
            "Done" => status_color(ItemStatus::Done),
            _ => Color32::LIGHT_GRAY,
        }
    }

    fn draw_av1_grouped_cards(&mut self, ui: &mut egui::Ui) {
        let groups = ["Active", "Ready", "Failed", "Skipped", "Done"];
        for label in groups {
            if self.queue_group_focus.is_some_and(|f| f != label) {
                continue;
            }
            let mut ids: Vec<u64> = self
                .av1_items
                .iter()
                .filter(|it| Self::av1_item_in_queue_group(it, label))
                .map(|it| it.item_id)
                .collect();
            if label == "Active" {
                ids.sort_by_key(|id| {
                    self.av1_items
                        .iter()
                        .find(|it| it.item_id == *id)
                        .map(|it| match it.status {
                            ItemStatus::Downloading => (0, it.item_id),
                            ItemStatus::Queued => (1, it.item_id),
                            _ => (2, it.item_id),
                        })
                        .unwrap_or((2, *id))
                });
            }
            if ids.is_empty() {
                continue;
            }
            let scroll_here = self.scroll_to_queue_group == Some(label);
            let default_open = self.av1_queue_group_default_open(label, scroll_here);
            let header_text = format!("{label} ({})", ids.len());
            let id = ui.make_persistent_id(("av1_queue_group", label));
            let header = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                default_open,
            )
            .show_header(ui, |ui| {
                status_dot_with_label(ui, &header_text, Self::av1_queue_group_color(label), true);
            });
            let (_toggle, header_inner, _) = header.body(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 8.0);
                for item_id in &ids {
                    let Some(it) = self.av1_items.iter().find(|x| x.item_id == *item_id) else {
                        continue;
                    };
                    ui.group(|ui| {
                        self.draw_av1_queue_card(ui, it);
                    });
                }
            });
            if scroll_here {
                ui.scroll_to_rect(header_inner.response.rect, Some(egui::Align::TOP));
                self.scroll_to_queue_group = None;
            }
        }
    }

    pub(super) fn draw_av1_queue_status_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let heading = if self.av1_items.is_empty() {
                "Queue:".to_owned()
            } else {
                format!("Queue ({}):", self.av1_items.len())
            };
            ui.label(RichText::new(heading).color(text_muted(&self.settings.theme)));
            let mut parts: Vec<(&str, usize, Color32)> = Vec::new();
            let ready = self
                .av1_items
                .iter()
                .filter(|i| i.status == ItemStatus::Idle)
                .count();
            let queued = self
                .av1_items
                .iter()
                .filter(|i| i.status == ItemStatus::Queued)
                .count();
            let running = self
                .av1_items
                .iter()
                .filter(|i| i.status == ItemStatus::Downloading)
                .count();
            let done = self
                .av1_items
                .iter()
                .filter(|i| i.status == ItemStatus::Done && !av1_item_is_skipped(i))
                .count();
            let skipped = self
                .av1_items
                .iter()
                .filter(|i| av1_item_is_skipped(i))
                .count();
            let failed = self
                .av1_items
                .iter()
                .filter(|i| i.status == ItemStatus::Failed)
                .count();
            if ready > 0 {
                parts.push(("ready", ready, status_color(ItemStatus::Idle)));
            }
            if queued > 0 {
                parts.push(("queued", queued, status_color(ItemStatus::Queued)));
            }
            if running > 0 {
                parts.push(("running", running, status_color(ItemStatus::Downloading)));
            }
            if done > 0 {
                parts.push(("done", done, status_color(ItemStatus::Done)));
            }
            if skipped > 0 {
                parts.push(("skipped", skipped, AV1_SKIPPED_COLOR));
            }
            if failed > 0 {
                parts.push(("failed", failed, status_color(ItemStatus::Failed)));
            }
            if self.queue_group_focus.is_some()
                && ui
                    .small_button(format!("{} Show all", ui_icons::SHOW_ALL))
                    .clicked()
            {
                self.queue_group_focus = None;
            }
            for (idx, (name, count, color)) in parts.iter().enumerate() {
                let suffix = if idx + 1 == parts.len() { "" } else { "," };
                let group = match *name {
                    "ready" => "Ready",
                    "queued" | "running" => "Active",
                    "done" => "Done",
                    "skipped" => "Skipped",
                    "failed" => "Failed",
                    _ => "Active",
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    draw_status_dot(ui, *color);
                    let label = format!("{count} {name}{suffix}");
                    let r = ui.add(
                        egui::Label::new(RichText::new(label).color(*color))
                            .sense(egui::Sense::click()),
                    );
                    if r.clicked() {
                        self.focus_queue_group(group);
                    }
                    r.on_hover_text(format!("Show {group} items"));
                });
            }
        });
    }

    fn draw_av1_batch_summary_row(&self, ui: &mut egui::Ui) {
        let batch = compute_av1_batch_summary(&self.av1_items);
        if batch.completed == 0 && batch.pending_count == 0 {
            return;
        }

        let theme = &self.settings.theme;
        let done_color = status_color(ItemStatus::Done);
        let pending_color = status_color(ItemStatus::Idle);
        let saved = batch
            .completed_input_bytes
            .saturating_sub(batch.completed_output_bytes);
        let pct = if batch.completed_input_bytes > 0 {
            (saved as f64 / batch.completed_input_bytes as f64) * 100.0
        } else {
            0.0
        };

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new("Summary:").color(text_muted(theme)));

            if batch.completed > 0 {
                ui.label(RichText::new(format!("{} completed", batch.completed)).color(done_color));
                draw_av1_bytes_arrow(
                    ui,
                    &human_bytes_ui(batch.completed_input_bytes),
                    &human_bytes_ui(batch.completed_output_bytes),
                    done_color,
                    theme,
                );
                ui.label(
                    RichText::new(format!("saved {} ({pct:.1}%)", human_bytes_ui(saved),))
                        .color(done_color),
                );
            }

            if batch.pending_count > 0 {
                if batch.completed > 0 {
                    ui.label(RichText::new("·").color(text_muted(theme)));
                }
                let remaining = if batch.pending_input_bytes > 0 {
                    format!(
                        "{} file(s) · {} remaining",
                        batch.pending_count,
                        human_bytes_ui(batch.pending_input_bytes),
                    )
                } else {
                    format!("{} file(s) remaining", batch.pending_count)
                };
                ui.label(RichText::new(remaining).color(pending_color));
            }
        });
    }

    fn draw_av1_queue_card(&self, ui: &mut egui::Ui, it: &Av1QueueItem) {
        let theme = &self.settings.theme;
        let done = it.status == ItemStatus::Done && !av1_item_is_skipped(it);
        let item_color = av1_item_status_color(it);
        let fill = if done {
            theme::done_card_fill(theme)
        } else {
            Color32::TRANSPARENT
        };

        egui::Frame::none()
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
            .rounding(egui::Rounding::same(6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let thumb_size = egui::vec2(90.0, 52.0);
                    let (thumb_rect, _) = ui.allocate_exact_size(thumb_size, egui::Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(
                        thumb_rect,
                        egui::Rounding::same(4.0),
                        theme::THUMB_PLACEHOLDER,
                    );
                    if let Some(tex) = self.textures.get(&it.item_id) {
                        painter.image(
                            tex.id(),
                            thumb_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        let center_msg =
                            if !self.has_ffmpeg || self.thumbnail_attempted.contains(&it.item_id) {
                                "No preview available"
                            } else {
                                "Fetching thumbnail..."
                            };
                        painter.text(
                            thumb_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            center_msg,
                            egui::TextStyle::Small.resolve(ui.style()),
                            Color32::from_gray(130),
                        );
                    }

                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.set_min_width(ui.available_width());

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.label(RichText::new(format!("#{}", it.item_id)).strong());
                            status_dot_with_label(ui, av1_item_status_label(it), item_color, false);
                        });

                        if matches!(it.status, ItemStatus::Downloading | ItemStatus::Queued) {
                            let mut pb =
                                egui::ProgressBar::new((it.percent / 100.0).clamp(0.0, 1.0))
                                    .desired_width(ui.available_width().min(280.0))
                                    .show_percentage()
                                    .animate(it.status == ItemStatus::Downloading);
                            if it.status == ItemStatus::Downloading {
                                pb = pb.fill(item_color);
                            }
                            ui.add(pb);
                        }

                        let probing = self.av1_media_inflight.contains(&it.item_id);
                        draw_av1_media_badges(ui, it, probing, theme);

                        draw_av1_path_line(ui, "in:", &it.source_path, theme);
                        draw_av1_path_line(ui, "out:", &it.output_path, theme);

                        if it.status == ItemStatus::Done
                            && !av1_item_is_skipped(it)
                            && it.input_bytes > 0
                        {
                            if let Some(output_bytes) = it.output_bytes {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    if !it.detail.is_empty() {
                                        ui.label(
                                            RichText::new(&it.detail).small().color(item_color),
                                        );
                                    }
                                    draw_av1_bytes_arrow(
                                        ui,
                                        &human_bytes_ui(it.input_bytes),
                                        &human_bytes_ui(output_bytes),
                                        item_color,
                                        theme,
                                    );
                                });
                            }
                        } else if !it.detail.is_empty() {
                            ui.label(RichText::new(&it.detail).small());
                        }
                    });
                });
            });
    }

    fn scan_av1_input_textbox(&mut self) {
        let lines: Vec<String> = self
            .av1_input_paths
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if lines.is_empty() {
            return;
        }
        self.av1_core_action(|core| core.scan_av1_paths_into_queue(&lines));
    }

    fn start_av1_batch(&mut self) {
        // Persist current AV1 settings first so the worker (in the core) uses the latest config.
        self.persist_settings();
        self.av1_core_action(|core| core.start_av1_batch());
    }

    fn browse_av1_inputs(&mut self) {
        let files = app_actions::pick_av1_input_files();
        if !files.is_empty() {
            let lines: Vec<String> = files
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            self.extend_av1_input_paths_with_lines(lines);
            return;
        }
        if let Some(folder) = app_actions::pick_av1_input_folder() {
            self.extend_av1_input_paths_with_lines(vec![folder.to_string_lossy().to_string()]);
        }
    }
}
