use std::collections::HashSet;
use std::path::Path;

use eframe::egui::{self, Color32, RichText};

use crate::app_actions;
use crate::app_parsing::human_bytes_ui;
use crate::app_ui::{
    danger_button, draw_status_dot, secondary_button, status_color, status_dot_with_label,
    success_button,
};
use crate::av1_transcode::{self, Av1Config, Av1Input};
use crate::models::{Av1QueueItem, ItemStatus};
use crate::theme::TEXT_MUTED;
use crate::ui_icons;

use super::PydlApp;

const AV1_SKIPPED_COLOR: Color32 = Color32::from_rgb(255, 167, 38);

fn av1_item_is_skipped(item: &Av1QueueItem) -> bool {
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
    pending_input_bytes: u64,
}

fn compute_av1_batch_summary(items: &[Av1QueueItem]) -> Av1BatchSummary {
    let mut summary = Av1BatchSummary {
        completed: 0,
        completed_input_bytes: 0,
        completed_output_bytes: 0,
        pending_input_bytes: 0,
    };
    for item in items {
        let pending = matches!(
            item.status,
            ItemStatus::Idle | ItemStatus::Queued | ItemStatus::Downloading | ItemStatus::Resolving
        );
        if pending {
            summary.pending_input_bytes = summary.pending_input_bytes.saturating_add(item.input_bytes);
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
        summary.completed_output_bytes = summary
            .completed_output_bytes
            .saturating_add(output_bytes);
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
        format!(
            "Saved {} ({pct:.1}%) · {} → {}",
            human_bytes_ui(saved),
            human_bytes_ui(input_bytes),
            human_bytes_ui(output_bytes),
        )
    } else {
        let growth = output_bytes - input_bytes;
        let grow_pct = (growth as f64 / input_bytes as f64) * 100.0;
        format!(
            "Output +{} (+{grow_pct:.1}%) · {} → {}",
            human_bytes_ui(growth),
            human_bytes_ui(input_bytes),
            human_bytes_ui(output_bytes),
        )
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

fn format_av1_media_line(item: &Av1QueueItem, probing: bool) -> String {
    if probing {
        return "Probing metadata…".to_owned();
    }
    let mut parts = Vec::new();
    if !item.video_codec.is_empty() {
        parts.push(item.video_codec.to_uppercase());
    }
    if let (Some(w), Some(h)) = (item.width, item.height) {
        parts.push(format!("{w}×{h}"));
    }
    if let Some(fps) = item.fps {
        parts.push(format!("{fps:.2} fps"));
    }
    if item.input_bytes > 0 {
        parts.push(human_bytes_ui(item.input_bytes));
    }
    if let Some(bps) = item.bitrate_bps {
        parts.push(format_av1_bitrate(bps));
    }
    if parts.is_empty() {
        "Metadata unavailable".to_owned()
    } else {
        parts.join(" · ")
    }
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

impl PydlApp {
    pub(super) fn av1_config(&self) -> Av1Config {
        Av1Config {
            ffmpeg_path: self.settings.ffmpeg_path.clone(),
            ffprobe_path: self.settings.ffprobe_path.clone(),
            output_dir: self.output_dir.clone(),
            recursive: self.settings.av1_recursive,
            dry_run: self.settings.av1_dry_run,
            delete_original: self.settings.av1_delete_original,
            overwrite: self.settings.av1_overwrite,
            reencode_av1: self.settings.av1_reencode_av1,
            target_bitrate: self.settings.av1_target_bitrate.clone(),
            max_width: self.settings.av1_max_width,
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

        let existing: HashSet<String> = self
            .av1_items
            .iter()
            .map(|item| normalize_av1_source_key(&item.source_path))
            .collect();

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
                self.queue_av1_media_probe(
                    item_id,
                    plan_item.input.clone(),
                    ffprobe_path.clone(),
                );
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
                self.queue_av1_local_thumbnail(
                    item_id,
                    plan_item.input,
                    ffmpeg_path.clone(),
                );
            }
            added += 1;
        }

        if added > 0 {
            self.append_log(&format!("AV1: added {added} video(s) to queue as ready."));
            self.schedule_av1_queue_save();
        } else {
            self.append_log("AV1: all video(s) from path(s) are already in the queue.");
        }
        remove_scanned_av1_input_lines(&mut self.av1_input_paths, &lines);
        self.schedule_av1_queue_save();
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
            if secondary_button(
                ui,
                &format!("{} Scan inputs", ui_icons::SCAN),
                true,
            )
            .clicked()
            {
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
            ui.checkbox(&mut self.settings.av1_recursive, "Recursive");
            ui.checkbox(&mut self.settings.av1_dry_run, "Dry run");
            ui.checkbox(&mut self.settings.av1_delete_original, "Delete original");
            ui.checkbox(&mut self.settings.av1_overwrite, "Overwrite");
            ui.checkbox(&mut self.settings.av1_reencode_av1, "Re-encode AV1");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Target bitrate");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.av1_target_bitrate)
                    .desired_width(90.0)
                    .hint_text("auto"),
            );
            ui.label("Max width");
            ui.add(egui::DragValue::new(&mut self.settings.av1_max_width).range(320..=7680));
            ui.label("Min shrink %");
            ui.add(
                egui::DragValue::new(&mut self.settings.av1_min_shrink_percent)
                    .speed(0.5)
                    .range(0.0..=95.0),
            );
            ui.label("Preset");
            egui::ComboBox::from_id_salt("av1_size_preset")
                .selected_text(self.settings.av1_size_preset.clone())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.settings.av1_size_preset,
                        "light".to_owned(),
                        "light",
                    );
                    ui.selectable_value(
                        &mut self.settings.av1_size_preset,
                        "balanced".to_owned(),
                        "balanced",
                    );
                    ui.selectable_value(
                        &mut self.settings.av1_size_preset,
                        "aggressive".to_owned(),
                        "aggressive",
                    );
                });
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
                !self.av1_running
                    && self.has_ffmpeg
                    && self.has_ffprobe
                    && ready_count > 0,
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
            if secondary_button(
                ui,
                &format!("{} Save AV1 settings", ui_icons::SAVE),
                true,
            )
            .clicked()
            {
                self.persist_settings();
            }
        });
        ui.separator();
        if !self.av1_items.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Queue:").color(TEXT_MUTED));
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
                for (idx, (name, count, color)) in parts.iter().enumerate() {
                    let suffix = if idx + 1 == parts.len() { "" } else { "," };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        draw_status_dot(ui, *color);
                        ui.label(RichText::new(format!("{count} {name}{suffix}")).color(*color));
                    });
                }
            });
            let batch = compute_av1_batch_summary(&self.av1_items);
            if batch.completed > 0 {
                let saved = batch
                    .completed_input_bytes
                    .saturating_sub(batch.completed_output_bytes);
                let pct = if batch.completed_input_bytes > 0 {
                    (saved as f64 / batch.completed_input_bytes as f64) * 100.0
                } else {
                    0.0
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Summary:").color(TEXT_MUTED));
                    ui.label(
                        RichText::new(format!(
                            "{} completed · {} → {} · saved {} ({pct:.1}%)",
                            batch.completed,
                            human_bytes_ui(batch.completed_input_bytes),
                            human_bytes_ui(batch.completed_output_bytes),
                            human_bytes_ui(saved),
                        ))
                        .color(status_color(ItemStatus::Done)),
                    );
                });
            }
            if batch.pending_input_bytes > 0 {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Pending input:").color(TEXT_MUTED));
                    ui.label(human_bytes_ui(batch.pending_input_bytes));
                });
            }
        }
        egui::ScrollArea::vertical()
            .id_salt("av1_queue_scroll")
            .max_height(340.0)
            .show(ui, |ui| {
                if self.av1_items.is_empty() {
                    ui.label(
                        RichText::new("No AV1 jobs yet. Browse, drop, or scan paths to add videos.")
                            .color(egui::Color32::GRAY),
                    );
                    return;
                }
                for it in &self.av1_items {
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            if let Some(tex) = self.textures.get(&it.item_id) {
                                ui.add(egui::Image::new(egui::load::SizedTexture::new(
                                    tex.id(),
                                    egui::vec2(90.0, 52.0),
                                )));
                            } else {
                                ui.allocate_space(egui::vec2(90.0, 52.0));
                            }
                            ui.label(RichText::new(format!("#{}", it.item_id)).strong());
                            let status_label = av1_item_status_label(it);
                            let item_color = av1_item_status_color(it);
                            status_dot_with_label(ui, status_label, item_color, false);
                            let mut pb = egui::ProgressBar::new((it.percent / 100.0).clamp(0.0, 1.0))
                                .desired_width(180.0)
                                .show_percentage()
                                .animate(it.status == ItemStatus::Downloading);
                            if it.status == ItemStatus::Done && !av1_item_is_skipped(it) {
                                pb = pb.fill(item_color);
                            } else if it.status == ItemStatus::Failed {
                                pb = pb.fill(item_color);
                            } else if it.status == ItemStatus::Downloading {
                                pb = pb.fill(item_color);
                            }
                            ui.add(pb);
                        });
                        let probing = self.av1_media_inflight.contains(&it.item_id);
                        let media_line = format_av1_media_line(it, probing);
                        ui.label(
                            RichText::new(media_line)
                                .small()
                                .color(if av1_item_has_media(it) {
                                    egui::Color32::from_rgb(180, 200, 220)
                                } else {
                                    TEXT_MUTED
                                }),
                        );
                        ui.label(format!("in: {}", it.source_path));
                        ui.label(format!("out: {}", it.output_path));
                        if !it.detail.is_empty() {
                            ui.label(RichText::new(it.detail.clone()).small());
                        }
                    });
                }
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
            &self.tx,
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
