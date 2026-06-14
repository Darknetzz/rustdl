use eframe::egui::{self, Color32, RichText};

use crate::app_actions;
use crate::app_parsing::human_bytes_ui;
use crate::app_ui::{
    button_group, draw_labeled_meta_badge, draw_meta_badge, draw_status_dot, left_button_row,
    show_queue_group_section, status_color, status_dot_with_label, MetaBadgeKind,
};
use crate::config::AppSettings;
use crate::convert_state::{
    convert_batch_totals_grew, convert_item_is_skipped, convert_item_open_targets,
    convert_item_status_label, convert_item_will_skip_already_target, convert_skip_hint_label,
    format_convert_batch_saved_line,
};
use crate::models::{ConvertQueueItem, ItemStatus};
use crate::service::DownloadCore;
use crate::theme;
use crate::theme::text_muted;
use crate::transcode;
use crate::ui_icons;

use super::PydlApp;

const CONVERT_SKIPPED_COLOR: Color32 = Color32::from_rgb(255, 167, 38);

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

fn draw_convert_bytes_arrow(
    ui: &mut egui::Ui,
    from: &str,
    to: &str,
    text_color: Color32,
    theme: &str,
) {
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

fn draw_convert_path_line(ui: &mut egui::Ui, prefix: &str, path: &str, theme: &str) {
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

fn convert_item_status_color(item: &ConvertQueueItem) -> Color32 {
    if convert_item_is_skipped(item) {
        return CONVERT_SKIPPED_COLOR;
    }
    status_color(item.status)
}

fn format_convert_bitrate(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.2} Mbps", bps as f64 / 1_000_000.0)
    } else if bps >= 1_000 {
        format!("{:.0} kbps", bps as f64 / 1_000.0)
    } else {
        format!("{bps} bps")
    }
}

fn convert_item_has_media(item: &ConvertQueueItem) -> bool {
    !item.video_codec.is_empty()
        || item.width.is_some()
        || item.height.is_some()
        || item.fps.is_some()
        || item.bitrate_bps.is_some()
}

fn draw_convert_encode_settings_badges(ui: &mut egui::Ui, settings: &AppSettings, theme: &str) {
    let muted = text_muted(theme);
    let target = crate::transcode::target_codec_label(&settings.convert_target_codec);
    draw_labeled_meta_badge(ui, "Target:", target, MetaBadgeKind::Codec, muted);
    ui.label(RichText::new("·").small().color(muted));
    let bitrate = if settings.convert_target_bitrate.trim().is_empty() {
        "auto".to_owned()
    } else {
        settings.convert_target_bitrate.clone()
    };
    let max_width = format!("{}w", settings.convert_max_width);
    let min_shrink = format!("{:.0}%", settings.convert_min_shrink_percent);
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
        &settings.convert_size_preset,
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
    let container = if settings.convert_use_recommended_container {
        crate::transcode::recommended_container_for_target(&settings.convert_target_codec)
            .to_ascii_uppercase()
    } else {
        "same ext".to_owned()
    };
    draw_labeled_meta_badge(
        ui,
        "Container:",
        &container,
        MetaBadgeKind::SizePreset,
        muted,
    );
}

fn draw_convert_will_skip_notice(ui: &mut egui::Ui, target_codec: &str) {
    draw_meta_badge(
        ui,
        &convert_skip_hint_label(target_codec),
        MetaBadgeKind::ConvertWillSkip,
    );
}

fn draw_convert_media_badges(
    ui: &mut egui::Ui,
    item: &ConvertQueueItem,
    probing: bool,
    theme: &str,
) {
    if probing {
        ui.label(
            RichText::new("Probing metadata...")
                .small()
                .color(text_muted(theme)),
        );
        return;
    }
    if !convert_item_has_media(item) {
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
            draw_meta_badge(ui, &format_convert_bitrate(bps), MetaBadgeKind::Bitrate);
        }
    });
}

fn convert_encoder_detect_key(
    ffmpeg_path: &str,
    encoder_override: &str,
    target_codec: &str,
) -> String {
    format!("{ffmpeg_path}\0{encoder_override}\0{target_codec}")
}

impl PydlApp {
    /// Probes ffmpeg encoders on a worker thread so the UI stays responsive (smoke tests can take many seconds).
    pub(super) fn refresh_convert_encoder_detection(&mut self, ctx: &egui::Context) {
        if !self.has_ffmpeg {
            self.convert_encoder_choice = None;
            self.convert_encoder_detect_key.clear();
            self.convert_encoder_detection_inflight = false;
            return;
        }
        let key = convert_encoder_detect_key(
            &self.settings.ffmpeg_path,
            &self.settings.convert_encoder_override,
            &self.settings.convert_target_codec,
        );
        if self.convert_encoder_detect_key == key && self.convert_encoder_choice.is_some() {
            return;
        }
        if self.convert_encoder_detection_inflight {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        self.convert_encoder_detection_inflight = true;
        let shared = self.shared_core.clone();
        let rt = self.runtime.clone();
        let ffmpeg_path = self.settings.ffmpeg_path.clone();
        let override_enc = self.settings.convert_encoder_override.clone();
        let target_codec = self.settings.convert_target_codec.clone();
        let ctx = ctx.clone();
        rt.spawn(async move {
            let choice = tokio::task::spawn_blocking(move || {
                transcode::detect_encoder_with_override(&ffmpeg_path, &override_enc, &target_codec)
            })
            .await
            .ok();
            if let Some(choice) = choice {
                let mut core = shared.lock();
                core.convert_encoder_choice = Some(choice);
                core.convert_encoder_detect_key = key;
                core.bump_generation();
            }
            ctx.request_repaint();
        });
    }

    /// Runs an AV1 mutation against the shared `DownloadCore` (the single source of truth) and
    /// refreshes the GUI mirror fields from it. The editable textarea is pushed in first so the
    /// core sees the latest paths, then read back (a scan trims the lines it consumed).
    pub(super) fn convert_core_action(&mut self, f: impl FnOnce(&mut DownloadCore)) {
        let mirror = {
            let mut core = self.shared_core.lock();
            core.convert_input_paths = self.convert_input_paths.clone();
            // Keep core settings in sync before mutations that may call `DownloadCore::persist_settings`
            // (e.g. starting a batch), so disk is not overwritten with a stale copy.
            core.settings = self.settings.clone();
            core.output_dir = self.output_dir.clone();
            core.worker_count = self.worker_count.clamp(1, 6);
            core.convert_paused = self.convert_paused;
            f(&mut core);
            (
                core.convert_input_paths.clone(),
                core.convert_items.clone(),
                core.convert_status_counts,
                core.convert_batch_summary,
                core.convert_batch_progress,
                core.convert_running,
                core.convert_paused,
                core.convert_media_inflight.clone(),
                core.convert_save_deadline,
                core.generation,
            )
        };
        self.convert_input_paths = mirror.0;
        self.convert_items = mirror.1;
        self.rebuild_convert_item_index();
        self.convert_status_counts = mirror.2;
        self.convert_batch_summary = mirror.3;
        self.convert_batch_progress = mirror.4;
        self.convert_running = mirror.5;
        self.convert_paused = mirror.6;
        self.convert_media_inflight = mirror.7;
        self.convert_save_deadline = mirror.8;
        self.core_generation = mirror.9;
        self.ensure_convert_thumbnails();
    }

    fn clear_convert_queue(&mut self) {
        let ids: Vec<u64> = self.convert_items.iter().map(|it| it.item_id).collect();
        self.convert_core_action(|core| core.clear_convert_queue());
        for id in ids {
            self.textures.remove(&id);
            self.thumbnail_inflight.remove(&id);
            self.thumbnail_attempted.remove(&id);
        }
    }

    /// Start/cancel/clear groups for the Convert queue footer (wrap-friendly).
    pub(super) fn draw_convert_queue_action_groups(&mut self, ui: &mut egui::Ui, compact: bool) {
        let ready_count = self
            .convert_items
            .iter()
            .filter(|item| item.status == ItemStatus::Idle)
            .count();
        let convert_active = self.convert_running
            || self
                .convert_items
                .iter()
                .any(|item| matches!(item.status, ItemStatus::Queued | ItemStatus::Downloading));
        let skipped_count = self
            .convert_items
            .iter()
            .filter(|item| convert_item_is_skipped(item))
            .count();
        let draw = |ui: &mut egui::Ui,
                    id: &str,
                    add: &mut dyn FnMut(&mut crate::app_ui::ButtonGroup<'_>)| {
            if compact {
                crate::app_ui::compact_button_group(ui, id, |g| add(g));
            } else {
                crate::app_ui::button_group(ui, id, |g| add(g));
            }
        };
        draw(ui, "convert_batch", &mut |g| {
            if g.success(
                &format!("{} Start Convert batch", ui_icons::PLAY),
                !self.convert_running
                    && !self.convert_paused
                    && self.has_ffmpeg
                    && self.has_ffprobe
                    && ready_count > 0,
            )
            .clicked()
            {
                self.start_convert_batch();
            }
            if self.convert_paused {
                if g.success(
                    &format!("{} Resume Convert batch", ui_icons::PLAY),
                    !self.convert_running && self.has_ffmpeg && self.has_ffprobe && ready_count > 0,
                )
                .clicked()
                {
                    self.convert_core_action(|core| core.resume_convert_batch());
                }
            } else if g
                .warning(
                    &format!("{} Pause Convert batch", ui_icons::CANCEL_TO_READY),
                    convert_active,
                )
                .clicked()
            {
                self.convert_core_action(|core| core.pause_convert_batch());
            }
            if g.danger(
                &format!("{} Cancel Convert batch", ui_icons::CANCEL_TO_READY),
                self.convert_running,
            )
            .clicked()
            {
                self.convert_core_action(|core| core.cancel_convert_batch());
            }
            if g.secondary(
                &format!("{} Retry skipped", ui_icons::RETRY),
                !self.convert_running && skipped_count > 0,
            )
            .clicked()
            {
                self.convert_core_action(|core| core.retry_skipped_convert_items());
            }
        });
        draw(ui, "av1_queue", &mut |g| {
            if g.secondary(
                &format!("{} Export batch CSV", ui_icons::EXPORT),
                !self.convert_items.is_empty(),
            )
            .clicked()
            {
                self.export_convert_batch_csv();
            }
            if g.warning(
                &format!("{} Fallback to software encoder", ui_icons::RETRY),
                self.convert_running,
            )
            .on_hover_text("Switch remaining jobs to libx264/libx265/libsvtav1 if GPU encode fails")
            .clicked()
            {
                self.convert_core_action(|core| core.fallback_convert_encoder_to_software());
                self.append_log("Convert batch: switched to software encoder fallback.");
            }
            if g.secondary(
                &format!("{} Clear Convert queue", ui_icons::CLEAR_QUEUE),
                !self.convert_running,
            )
            .clicked()
            {
                self.clear_convert_queue();
            }
        });
    }

    pub(super) fn draw_convert_panel(&mut self, ui: &mut egui::Ui) {
        self.constrain_content(ui);

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Video Converter").heading());
            ui.label(
                RichText::new("Near-parity mode for local video transcoding.")
                    .small()
                    .color(egui::Color32::GRAY),
            );
        });
        ui.separator();
        ui.label("Input paths (file/folder, one per line)");
        left_button_row(ui, |ui| {
            button_group(ui, "convert_input", |g| {
                if g.secondary(&format!("{} Add folder", ui_icons::OPEN_FOLDER), true)
                    .clicked()
                {
                    self.add_convert_input_folder();
                }
                if g.secondary(&format!("{} Add file(s)", ui_icons::ADD), true)
                    .clicked()
                {
                    self.add_convert_input_files();
                }
                if g.secondary(&format!("{} Scan inputs", ui_icons::SCAN), true)
                    .clicked()
                {
                    self.scan_convert_input_textbox();
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            let ready = self
                .convert_items
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
        let convert_paths_edit = ui.add_sized(
            [ui.available_width(), 90.0],
            egui::TextEdit::multiline(&mut self.convert_input_paths)
                .hint_text("D:\\Videos\\movie.mkv\nD:\\Videos\\Folder — paste or drop paths"),
        );
        super::attach_paste_context_menu(
            &convert_paths_edit,
            &mut self.deferred_menu_paste_convert_paths,
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Session").strong());
            if ui
                .checkbox(
                    &mut self.settings.convert_recursive,
                    "Recursive folder scan",
                )
                .on_hover_text(
                    "When enabled, scanning a folder also includes supported videos in subfolders.",
                )
                .changed()
            {
                self.persist_settings();
            }
            if ui
                .checkbox(&mut self.settings.convert_dry_run, "Dry run this batch")
                .changed()
            {
                self.persist_settings();
            }
            if ui
                .checkbox(
                    &mut self.settings.convert_auto_start_on_add,
                    "Start batch when paths are added",
                )
                .on_hover_text(
                    "Automatically run Start Convert batch after Add folder, Add file(s), \
                             Scan inputs, or drag-and-drop adds new ready items.",
                )
                .changed()
            {
                self.persist_settings();
            }
        });
        self.refresh_convert_encoder_detection(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Encode settings").small());
            draw_convert_encode_settings_badges(ui, &self.settings, &self.settings.theme);
            if let Some(enc) = &self.convert_encoder_choice {
                status_dot_with_label(
                    ui,
                    transcode::encoder_indicator_label(enc),
                    transcode::encoder_indicator_color(enc),
                    true,
                );
            } else if !self.has_ffmpeg {
                status_dot_with_label(
                    ui,
                    "Encoder: ffmpeg not found",
                    Color32::from_rgb(255, 193, 120),
                    true,
                );
            } else if self.convert_encoder_detection_inflight {
                status_dot_with_label(
                    ui,
                    "Encoder: detecting…",
                    Color32::from_rgb(180, 180, 180),
                    true,
                );
            }
        });
        left_button_row(ui, |ui| {
            button_group(ui, "convert_settings", |g| {
                if g.secondary(&format!("{} Edit in Settings", ui_icons::SETTINGS), true)
                    .clicked()
                {
                    self.settings_open = true;
                    self.settings_tab = super::SettingsTab::Convert;
                }
            });
        });
    }

    pub(super) fn draw_convert_queue_list_scroll(
        &mut self,
        ui: &mut egui::Ui,
        scroll_max: f32,
        min_h: f32,
    ) {
        let scroll_h = scroll_max.max(min_h);
        egui::ScrollArea::vertical()
            .id_salt("convert_queue_scroll")
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .animated(true)
            .drag_to_scroll(true)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                if self.convert_items.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Nothing here yet")
                                .color(text_muted(&self.settings.theme)),
                        );
                        ui.label(
                            RichText::new(
                                "Add folder or file(s), drop paths, or scan inputs to queue videos.",
                            )
                            .small(),
                        );
                    });
                    return;
                }
                let profile = std::env::var("RUSTDL_PROFILE").ok().as_deref() == Some("1");
                let t0 = profile.then(std::time::Instant::now);
                self.draw_convert_grouped_cards(ui);
                if let Some(t0) = t0 {
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    if ms > 8.0 {
                        eprintln!(
                            "rustdl profile: draw_convert_grouped_cards {} items in {ms:.1}ms",
                            self.convert_items.len()
                        );
                    }
                }
            });
    }

    fn convert_item_in_queue_group(item: &ConvertQueueItem, label: &str) -> bool {
        match label {
            "Active" => matches!(item.status, ItemStatus::Queued | ItemStatus::Downloading),
            "Ready" => item.status == ItemStatus::Idle,
            "Failed" => item.status == ItemStatus::Failed,
            "Skipped" => convert_item_is_skipped(item),
            "Done" => item.status == ItemStatus::Done && !convert_item_is_skipped(item),
            _ => false,
        }
    }

    fn convert_queue_group_default_open(&self, label: &str, scroll_here: bool) -> bool {
        if scroll_here || self.queue_group_focus.is_some_and(|f| f == label) {
            return true;
        }
        match label {
            "Done" | "Ready" => false,
            "Failed" | "Skipped" => true,
            _ => true,
        }
    }

    fn convert_queue_group_color(label: &str) -> Color32 {
        match label {
            "Active" => status_color(ItemStatus::Downloading),
            "Ready" => status_color(ItemStatus::Idle),
            "Failed" => status_color(ItemStatus::Failed),
            "Skipped" => CONVERT_SKIPPED_COLOR,
            "Done" => status_color(ItemStatus::Done),
            _ => Color32::LIGHT_GRAY,
        }
    }

    fn draw_convert_grouped_cards(&mut self, ui: &mut egui::Ui) {
        let groups = ["Active", "Ready", "Failed", "Skipped", "Done"];
        for label in groups {
            if self.queue_group_focus.is_some_and(|f| f != label) {
                continue;
            }
            let mut ids: Vec<u64> = self
                .convert_items
                .iter()
                .filter(|it| Self::convert_item_in_queue_group(it, label))
                .map(|it| it.item_id)
                .collect();
            if label == "Active" {
                ids.sort_by_key(|id| {
                    self.convert_item_idx(*id)
                        .and_then(|idx| self.convert_items.get(idx))
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
            let default_open = self.convert_queue_group_default_open(label, scroll_here);
            let header_text = format!("{label} ({})", ids.len());
            let id = ui.make_persistent_id(("convert_queue_group", label));
            let group_color = Self::convert_queue_group_color(label);
            let theme = self.settings.theme.clone();
            let mut header_inner = None;
            show_queue_group_section(ui, &theme, group_color, |ui| {
                let header = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    default_open,
                )
                .show_header(ui, |ui| {
                    status_dot_with_label(ui, &header_text, group_color, true);
                });
                let (_toggle, inner, _) = header.body(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 8.0);
                    if self.effective_convert_list_layout() {
                        const LIST_ROW_H: f32 = 118.0;
                        let row_count = ids.len().max(1);
                        let max_h = (row_count as f32 * LIST_ROW_H + 8.0).clamp(LIST_ROW_H, 360.0);
                        egui::ScrollArea::vertical()
                            .id_salt(format!("rustdl_convert_list_{label}"))
                            .max_height(max_h)
                            .auto_shrink([false, true])
                            .show_rows(ui, LIST_ROW_H, ids.len(), |ui, row_range| {
                                for row in row_range {
                                    if let Some(item_id) = ids.get(row) {
                                        if let Some(idx) = self.convert_item_idx(*item_id) {
                                            let it = self.convert_items[idx].clone();
                                            ui.group(|ui| {
                                                self.draw_convert_queue_card(ui, &it);
                                            });
                                        }
                                    }
                                }
                            });
                    } else {
                        for item_id in &ids {
                            let Some(idx) = self.convert_item_idx(*item_id) else {
                                continue;
                            };
                            let it = self.convert_items[idx].clone();
                            ui.group(|ui| {
                                self.draw_convert_queue_card(ui, &it);
                            });
                        }
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
    }

    pub(super) fn draw_convert_queue_status_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let heading = if self.convert_items.is_empty() {
                "Queue:".to_owned()
            } else {
                format!("Queue ({}):", self.convert_items.len())
            };
            ui.label(RichText::new(heading).color(text_muted(&self.settings.theme)));
            let mut parts: Vec<(&str, usize, Color32)> = Vec::new();
            let counts = self.convert_status_counts;
            let ready = counts.ready;
            let queued = counts.queued;
            let running = counts.running;
            let done = counts.done;
            let skipped = counts.skipped;
            let failed = counts.failed;
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
                parts.push(("skipped", skipped, CONVERT_SKIPPED_COLOR));
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

    pub(super) fn draw_convert_batch_summary_row(&self, ui: &mut egui::Ui) {
        let batch = self.convert_batch_summary;
        if batch.completed == 0 && batch.pending_count == 0 {
            return;
        }

        let theme = &self.settings.theme;
        let done_color = status_color(ItemStatus::Done);
        let pending_color = status_color(ItemStatus::Idle);
        let grew =
            convert_batch_totals_grew(batch.completed_input_bytes, batch.completed_output_bytes);
        let savings_color = if grew {
            CONVERT_SKIPPED_COLOR
        } else {
            done_color
        };

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new("Summary:").color(text_muted(theme)));

            if batch.completed > 0 {
                ui.label(RichText::new(format!("{} completed", batch.completed)).color(done_color));
                draw_convert_bytes_arrow(
                    ui,
                    &human_bytes_ui(batch.completed_input_bytes),
                    &human_bytes_ui(batch.completed_output_bytes),
                    done_color,
                    theme,
                );
                ui.label(
                    RichText::new(format_convert_batch_saved_line(
                        batch.completed_input_bytes,
                        batch.completed_output_bytes,
                    ))
                    .color(savings_color),
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

    fn draw_convert_queue_card(&mut self, ui: &mut egui::Ui, it: &ConvertQueueItem) {
        let theme = self.settings.theme.clone();
        let done = it.status == ItemStatus::Done && !convert_item_is_skipped(it);
        let item_color = convert_item_status_color(it);
        let output_codec = transcode::normalize_target_codec(&self.settings.convert_target_codec);
        let will_skip_target = convert_item_will_skip_already_target(
            it,
            self.settings.convert_reencode_target,
            output_codec,
        );
        let fill = if done {
            theme::done_card_fill(&theme)
        } else {
            Color32::TRANSPARENT
        };
        let stroke = if will_skip_target {
            egui::Stroke::new(1.5, CONVERT_SKIPPED_COLOR)
        } else {
            egui::Stroke::NONE
        };

        egui::Frame::none()
            .fill(fill)
            .stroke(stroke)
            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
            .rounding(egui::Rounding::same(6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let thumb_size = egui::vec2(90.0, 52.0);
                    let (thumb_rect, _) = ui.allocate_exact_size(thumb_size, egui::Sense::hover());
                    ui.painter().rect_filled(
                        thumb_rect,
                        egui::Rounding::same(4.0),
                        theme::THUMB_PLACEHOLDER,
                    );
                    if let Some(tex) = self.textures.get(&it.item_id) {
                        ui.painter().image(
                            tex.id(),
                            thumb_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else if it.source_missing {
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(thumb_rect), |ui| {
                            ui.centered_and_justified(|ui| {
                                draw_meta_badge(ui, "File missing", MetaBadgeKind::FileMissing);
                            });
                        });
                    } else {
                        let center_msg = if !self.has_ffmpeg {
                            "ffmpeg not found"
                        } else if self.thumbnail_inflight.contains(&it.item_id) {
                            "Fetching thumbnail..."
                        } else if self.thumbnail_attempted.contains(&it.item_id) {
                            "No preview"
                        } else {
                            "Fetching thumbnail..."
                        };
                        ui.painter().text(
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
                            status_dot_with_label(
                                ui,
                                convert_item_status_label(it),
                                item_color,
                                false,
                            );
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

                        let probing = self.convert_media_inflight.contains(&it.item_id);
                        if will_skip_target {
                            draw_convert_will_skip_notice(ui, &self.settings.convert_target_codec);
                        }
                        draw_convert_media_badges(ui, it, probing, &theme);

                        draw_convert_path_line(ui, "in:", &it.source_path, &theme);
                        draw_convert_path_line(ui, "out:", &it.output_path, &theme);

                        if it.status == ItemStatus::Done
                            && !convert_item_is_skipped(it)
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
                                    draw_convert_bytes_arrow(
                                        ui,
                                        &human_bytes_ui(it.input_bytes),
                                        &human_bytes_ui(output_bytes),
                                        item_color,
                                        &theme,
                                    );
                                });
                            }
                        } else if !it.detail.is_empty() {
                            ui.label(RichText::new(&it.detail).small());
                        }

                        let targets = convert_item_open_targets(it);
                        if targets.file.is_some() || targets.folder.is_some() {
                            left_button_row(ui, |ui| {
                                button_group(ui, ("convert_open", it.item_id), |g| {
                                    let mut open_file = false;
                                    let mut open_folder = false;
                                    g.open_menu(
                                        targets.file.is_some(),
                                        targets.folder.is_some(),
                                        &mut open_file,
                                        &mut open_folder,
                                    );
                                    if open_file {
                                        if let Some(p) = &targets.file {
                                            self.open_file_path(p);
                                        }
                                    }
                                    if open_folder {
                                        if let Some(p) = &targets.file {
                                            self.reveal_file_path(p);
                                        } else if let Some(p) = &targets.folder {
                                            if let Err(e) = app_actions::open_path(p) {
                                                self.append_log(&format!(
                                                    "Failed to open folder: {e}"
                                                ));
                                            }
                                        }
                                    }
                                });
                            });
                        }
                    });
                });
            });
    }

    fn scan_convert_input_textbox(&mut self) {
        let lines: Vec<String> = self
            .convert_input_paths
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if lines.is_empty() {
            return;
        }
        self.convert_core_action(|core| core.scan_convert_paths_into_queue(&lines));
    }

    fn start_convert_batch(&mut self) {
        // Persist current AV1 settings first so the worker (in the core) uses the latest config.
        self.persist_settings();
        self.convert_core_action(|core| core.start_convert_batch());
    }

    fn add_convert_input_folder(&mut self) {
        if let Some(folder) = app_actions::pick_convert_input_folder() {
            self.extend_convert_input_paths_with_lines(vec![folder.to_string_lossy().to_string()]);
        }
    }

    fn add_convert_input_files(&mut self) {
        let files = app_actions::pick_convert_input_files();
        if files.is_empty() {
            return;
        }
        let lines: Vec<String> = files
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        self.extend_convert_input_paths_with_lines(lines);
    }
}
