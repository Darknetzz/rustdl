use std::collections::HashSet;
use std::path::Path;

use eframe::egui::{self, Color32, RichText};

use crate::app_actions;
use crate::app_parsing::human_bytes_ui;
use crate::app_ui::{
    danger_button, draw_meta_badge, draw_status_dot, secondary_button, status_color,
    status_dot_with_label, success_button, MetaBadgeKind,
};
use crate::av1_transcode::{self, Av1Config, Av1Input};
use crate::models::{Av1QueueItem, ItemStatus};
use crate::theme;
use crate::theme::{text_muted};
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

pub(crate) fn av1_item_is_skipped(item: &Av1QueueItem) -> bool {
    item.status == ItemStatus::Done && item.detail.to_ascii_lowercase().starts_with("skipped")
}

fn av1_item_status_label(item: &Av1QueueItem) -> &'static str {
    match item.status {
        ItemStatus::Idle => "Ready",
        ItemStatus::Queued => "Queued",
        ItemStatus::Downloading => "Running",
        ItemStatus::Done if av1_item_is_skipped(item) => "Skipped",
        ItemStatus::Done => "Done",
        ItemStatus::Failed => "Failed",
        ItemStatus::Resolving => "Resolving",
    }
}

fn av1_item_status_color(item: &Av1QueueItem) -> Color32 {
    if av1_item_is_skipped(item) {
        return AV1_SKIPPED_COLOR;
    }
    status_color(item.status)
}

struct Av1BatchSummary {
    completed: usize,
    completed_input_bytes: u64,
    completed_output_bytes: u64,
    pending_count: usize,
    pending_input_bytes: u64,
}

fn compute_av1_batch_summary(items: &[Av1QueueItem]) -> Av1BatchSummary {
    let mut summary = Av1BatchSummary {
        completed: 0,
        completed_input_bytes: 0,
        completed_output_bytes: 0,
        pending_count: 0,
        pending_input_bytes: 0,
    };
    for item in items {
        let pending = matches!(
            item.status,
            ItemStatus::Idle | ItemStatus::Queued | ItemStatus::Downloading | ItemStatus::Resolving
        );
        if pending {
            summary.pending_count += 1;
            summary.pending_input_bytes =
                summary.pending_input_bytes.saturating_add(item.input_bytes);
            continue;
        }
        if item.status != ItemStatus::Done || av1_item_is_skipped(item) {
            continue;
        }
        let Some(output_bytes) = item.output_bytes else {
            continue;
        };
        summary.completed += 1;
        summary.completed_input_bytes = summary
            .completed_input_bytes
            .saturating_add(item.input_bytes);
        summary.completed_output_bytes =
            summary.completed_output_bytes.saturating_add(output_bytes);
    }
    summary
}

pub(crate) fn format_av1_saved_detail(input_bytes: u64, output_bytes: u64) -> String {
    if input_bytes == 0 {
        return format!("Output {}", human_bytes_ui(output_bytes));
    }
    if output_bytes <= input_bytes {
        let saved = input_bytes - output_bytes;
        let pct = (saved as f64 / input_bytes as f64) * 100.0;
        format!("Saved {} ({pct:.1}%)", human_bytes_ui(saved))
    } else {
        let growth = output_bytes - input_bytes;
        let grow_pct = (growth as f64 / input_bytes as f64) * 100.0;
        format!("Output +{} (+{grow_pct:.1}%)", human_bytes_ui(growth),)
    }
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

fn normalize_av1_source_key(path: &str) -> String {
    Path::new(path)
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn remove_scanned_av1_input_lines(input: &mut String, scanned: &[String]) {
    if scanned.is_empty() {
        return;
    }
    let remove: HashSet<String> = scanned
        .iter()
        .map(|s| normalize_av1_source_key(s))
        .collect();
    let remaining: Vec<String> = input
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| !remove.contains(&normalize_av1_source_key(s)))
        .map(str::to_owned)
        .collect();
    *input = if remaining.is_empty() {
        String::new()
    } else {
        format!("{}\n", remaining.join("\n"))
    };
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

    pub(super) fn av1_config(&self) -> Av1Config {
        Av1Config {
            ffmpeg_path: self.settings.ffmpeg_path.clone(),
            ffprobe_path: self.settings.ffprobe_path.clone(),
            output_dir: self.output_dir.clone(),
            recursive: self.settings.av1_recursive,
            dry_run: self.settings.av1_dry_run,
            delete_original: self.settings.av1_delete_original,
            rename_original: self.settings.av1_rename_original,
            overwrite: self.settings.av1_overwrite,
            reencode_av1: self.settings.av1_reencode_av1,
            target_bitrate: self.settings.av1_target_bitrate.clone(),
            max_width: self.settings.av1_max_width,
            size_preset: self.settings.av1_size_preset.clone(),
            min_shrink_percent: self.settings.av1_min_shrink_percent,
            encoder_override: self.settings.av1_encoder_override.clone(),
        }
    }

    pub(super) fn scan_av1_paths_into_queue(&mut self, path_lines: &[String]) {
        let lines: Vec<String> = path_lines
            .iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if lines.is_empty() {
            return;
        }

        let cfg = self.av1_config();
        let inputs: Vec<Av1Input> = lines
            .iter()
            .map(|source_path| Av1Input {
                source_path: source_path.clone(),
            })
            .collect();
        let plan = av1_transcode::collect_plan(&inputs, &cfg);
        if plan.is_empty() {
            self.append_log("AV1: no supported video files found in added path(s).");
            remove_scanned_av1_input_lines(&mut self.av1_input_paths, &lines);
            self.schedule_av1_queue_save();
            return;
        }

        let added = self.push_av1_plan_items(plan);

        if added > 0 {
            self.append_log(&format!("AV1: added {added} video(s) to queue as ready."));
            self.schedule_av1_queue_save();
        } else {
            self.append_log("AV1: all video(s) from path(s) are already in the queue.");
        }
        remove_scanned_av1_input_lines(&mut self.av1_input_paths, &lines);
        self.schedule_av1_queue_save();
    }

    /// Adds plan items not already in the AV1 queue. Returns how many were added.
    pub(super) fn push_av1_plan_items(&mut self, plan: Vec<av1_transcode::Av1PlanItem>) -> usize {
        if plan.is_empty() {
            return 0;
        }
        let existing: HashSet<String> = self
            .av1_items
            .iter()
            .map(|item| normalize_av1_source_key(&item.source_path))
            .collect();
        let cfg = self.av1_config();
        let ffmpeg_path = cfg.ffmpeg_path.clone();
        let ffprobe_path = cfg.ffprobe_path.clone();
        let mut added = 0usize;
        for plan_item in plan {
            let source = plan_item.input.to_string_lossy().to_string();
            if existing.contains(&normalize_av1_source_key(&source)) {
                continue;
            }

            let item_id = self.av1_next_item_id;
            self.av1_next_item_id = self.av1_next_item_id.saturating_add(1);
            if self.has_ffprobe {
                self.queue_av1_media_probe(item_id, plan_item.input.clone(), ffprobe_path.clone());
            }

            let input_bytes = std::fs::metadata(&plan_item.input)
                .map(|m| m.len())
                .unwrap_or(0);
            let ready_detail = if input_bytes > 0 {
                format!("Ready · {}", human_bytes_ui(input_bytes))
            } else {
                "Ready".to_owned()
            };

            self.av1_items.push(Av1QueueItem {
                item_id,
                source_path: source,
                output_path: plan_item.output.to_string_lossy().to_string(),
                status: ItemStatus::Idle,
                percent: 0.0,
                detail: ready_detail,
                input_bytes,
                output_bytes: None,
                video_codec: String::new(),
                width: None,
                height: None,
                fps: None,
                bitrate_bps: None,
            });
            if self.has_ffmpeg {
                self.queue_av1_local_thumbnail(item_id, plan_item.input, ffmpeg_path.clone());
            }
            added += 1;
        }
        if added > 0 {
            self.schedule_av1_queue_save();
        }
        added
    }

    pub(super) fn enqueue_completed_download_to_av1(&mut self, item_id: u64) {
        if !self.settings.enqueue_downloads_to_av1 || self.settings.ffmpeg_extract_audio_mp3 {
            return;
        }
        let Some(idx) = self.item_idx(item_id) else {
            return;
        };
        let item = self.items[idx].clone();
        let Some((path, _)) = self.find_downloaded_file_for_item(&item) else {
            return;
        };
        if !av1_transcode::is_video_path(&path) {
            return;
        }
        let source = path.to_string_lossy().into_owned();
        let plan = av1_transcode::collect_plan(
            &[Av1Input {
                source_path: source,
            }],
            &self.av1_config(),
        );
        let added = self.push_av1_plan_items(plan);
        if added > 0 {
            let label = item.title.trim();
            let label = if label.is_empty() {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("download")
            } else {
                label
            };
            self.append_log(&format!(
                "AV1: enqueued \"{label}\" from completed download."
            ));
        }
    }

    pub(super) fn queue_av1_restored_assets(&mut self) {
        if self.av1_items.is_empty() {
            return;
        }
        let cfg = self.av1_config();
        let ffmpeg_path = cfg.ffmpeg_path.clone();
        let ffprobe_path = cfg.ffprobe_path.clone();
        for item in self.av1_items.clone() {
            let source = std::path::PathBuf::from(&item.source_path);
            if self.has_ffprobe && item.video_codec.is_empty() {
                self.queue_av1_media_probe(item.item_id, source.clone(), ffprobe_path.clone());
            }
            if self.has_ffmpeg && !self.textures.contains_key(&item.item_id) {
                self.queue_av1_local_thumbnail(item.item_id, source, ffmpeg_path.clone());
            }
        }
    }

    pub(super) fn clear_av1_queue(&mut self) {
        for item in &self.av1_items {
            self.textures.remove(&item.item_id);
            self.thumbnail_inflight.remove(&item.item_id);
            self.thumbnail_attempted.remove(&item.item_id);
            self.av1_media_inflight.remove(&item.item_id);
        }
        self.av1_items.clear();
        self.av1_duration_ms.clear();
        self.av1_progress_state.clear();
        self.clear_av1_queue_persistence();
    }

    pub(super) fn draw_av1_panel(&mut self, ui: &mut egui::Ui) {
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
        ui.horizontal_wrapped(|ui| {
            if secondary_button(ui, &format!("{} Browse", ui_icons::BROWSE), true).clicked() {
                self.browse_av1_inputs();
            }
            if secondary_button(ui, &format!("{} Scan inputs", ui_icons::SCAN), true).clicked() {
                self.scan_av1_input_textbox();
            }
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
        let input_edit = ui.add_sized(
            [ui.available_width(), 90.0],
            egui::TextEdit::multiline(&mut self.av1_input_paths)
                .hint_text("D:\\Videos\\movie.mkv\nD:\\Videos\\Folder"),
        );
        if input_edit.changed() {
            self.schedule_av1_queue_save();
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Session").strong());
            if ui
                .checkbox(&mut self.settings.av1_dry_run, "Dry run this batch")
                .changed()
            {
                self.persist_settings();
            }
        });
        self.refresh_av1_encoder_detection();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Encode settings").small());
            let br = if self.settings.av1_target_bitrate.trim().is_empty() {
                "auto".to_owned()
            } else {
                self.settings.av1_target_bitrate.clone()
            };
            ui.label(format!(
                "Bitrate: {br} · Max width: {} · Preset: {} · Min shrink: {:.0}%",
                self.settings.av1_max_width,
                self.settings.av1_size_preset,
                self.settings.av1_min_shrink_percent,
            ));
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
            if secondary_button(
                ui,
                &format!("{} Edit in Settings", ui_icons::SETTINGS),
                true,
            )
            .clicked()
            {
                self.settings_open = true;
                self.settings_tab = super::SettingsTab::Av1;
            }
        });
        ui.horizontal_wrapped(|ui| {
            let ready_count = self
                .av1_items
                .iter()
                .filter(|item| item.status == ItemStatus::Idle)
                .count();
            if success_button(
                ui,
                &format!("{} Start AV1 batch", ui_icons::PLAY),
                !self.av1_running && self.has_ffmpeg && self.has_ffprobe && ready_count > 0,
            )
            .clicked()
            {
                self.start_av1_batch();
            }
            if danger_button(
                ui,
                &format!("{} Cancel AV1 batch", ui_icons::CANCEL_TO_READY),
                self.av1_running,
            )
            .clicked()
            {
                self.av1_cancel_flag
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if secondary_button(
                ui,
                &format!("{} Clear AV1 queue", ui_icons::CLEAR_QUEUE),
                !self.av1_running,
            )
            .clicked()
            {
                self.clear_av1_queue();
            }
        });
        const RESERVE_BOTTOM_PX: f32 = 20.0;
        const MIN_QUEUE_VIEWPORT: f32 = 220.0;
        let bottom_h =
            (ui.available_height() - RESERVE_BOTTOM_PX).max(MIN_QUEUE_VIEWPORT);

        if self.settings.videos_docked {
            self.draw_docked_videos_section(ui, bottom_h);
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

    fn draw_av1_queue_status_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Queue:").color(text_muted(&self.settings.theme)));
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
        self.scan_av1_paths_into_queue(&lines);
    }

    fn start_av1_batch(&mut self) {
        let idle_count = self
            .av1_items
            .iter()
            .filter(|item| item.status == ItemStatus::Idle)
            .count();
        if idle_count == 0 {
            self.scan_av1_input_textbox();
        }

        let jobs: Vec<(u64, Av1Input, String)> = self
            .av1_items
            .iter()
            .filter(|item| item.status == ItemStatus::Idle)
            .map(|item| {
                (
                    item.item_id,
                    Av1Input {
                        source_path: item.source_path.clone(),
                    },
                    item.output_path.clone(),
                )
            })
            .collect();
        if jobs.is_empty() {
            self.append_log("AV1: no ready items to convert.");
            return;
        }

        self.persist_settings();
        self.av1_cancel_flag
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let cfg = self.av1_config();

        for (item_id, _, _) in &jobs {
            if let Some(item) = self.av1_items.iter_mut().find(|x| x.item_id == *item_id) {
                item.status = ItemStatus::Queued;
                item.detail = "Queued".to_owned();
            }
        }

        self.av1_running = true;
        super::background_spawn::spawn_av1_worker(
            &self.runtime,
            &self.ui_bus,
            cfg,
            jobs,
            self.av1_cancel_flag.clone(),
        );
        self.append_log("AV1: batch started.");
        self.schedule_av1_queue_save();
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
