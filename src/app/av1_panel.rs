use std::collections::HashSet;
use std::path::Path;

use eframe::egui::{self, RichText};

use crate::app_actions;
use crate::app_ui::{danger_button, secondary_button, success_button};
use crate::av1_transcode::{self, Av1Config, Av1Input};
use crate::models::{Av1QueueItem, ItemStatus};
use crate::ui_icons;

use super::PydlApp;

fn normalize_av1_source_key(path: &str) -> String {
    Path::new(path)
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

impl PydlApp {
    pub(super) fn av1_effective_ffmpeg_path(&self) -> String {
        let override_path = self.settings.av1_ffmpeg_path.trim();
        if override_path.is_empty() {
            self.settings.ffmpeg_path.clone()
        } else {
            self.settings.av1_ffmpeg_path.clone()
        }
    }

    pub(super) fn av1_effective_ffprobe_path(&self) -> String {
        let override_path = self.settings.av1_ffprobe_path.trim();
        if override_path.is_empty() {
            self.settings.ffprobe_path.clone()
        } else {
            self.settings.av1_ffprobe_path.clone()
        }
    }

    pub(super) fn av1_config(&self) -> Av1Config {
        Av1Config {
            ffmpeg_path: self.av1_effective_ffmpeg_path(),
            ffprobe_path: self.av1_effective_ffprobe_path(),
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
                if let Some(ms) = av1_transcode::input_duration_ms(&plan_item.input, &ffprobe_path)
                {
                    self.av1_duration_ms.insert(item_id, ms);
                }
            }

            self.av1_items.push(Av1QueueItem {
                item_id,
                source_path: source,
                output_path: plan_item.output.to_string_lossy().to_string(),
                status: ItemStatus::Idle,
                percent: 0.0,
                detail: "Ready".to_owned(),
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
        } else {
            self.append_log("AV1: all video(s) from path(s) are already in the queue.");
        }
    }

    pub(super) fn clear_av1_queue(&mut self) {
        for item in &self.av1_items {
            self.textures.remove(&item.item_id);
            self.thumbnail_inflight.remove(&item.item_id);
            self.thumbnail_attempted.remove(&item.item_id);
        }
        self.av1_items.clear();
        self.av1_duration_ms.clear();
        self.av1_progress_state.clear();
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
            if secondary_button(ui, "Browse", true).clicked() {
                self.browse_av1_inputs();
            }
            if secondary_button(ui, "Scan inputs", true).clicked() {
                self.scan_av1_input_textbox();
            }
            let ready = self
                .av1_items
                .iter()
                .filter(|item| item.status == ItemStatus::Idle)
                .count();
            if ready > 0 {
                ui.label(RichText::new(format!("{ready} ready")).strong());
            }
        });
        ui.add_sized(
            [ui.available_width(), 90.0],
            egui::TextEdit::multiline(&mut self.av1_input_paths)
                .hint_text("D:\\Videos\\movie.mkv\nD:\\Videos\\Folder"),
        );
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
            if secondary_button(ui, "Clear AV1 queue", !self.av1_running).clicked() {
                self.clear_av1_queue();
            }
            if secondary_button(ui, "Save AV1 settings", true).clicked() {
                self.persist_settings();
            }
        });
        ui.separator();
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
                            let status_text = match it.status {
                                ItemStatus::Idle => "Ready",
                                ItemStatus::Queued => "Queued",
                                ItemStatus::Downloading => "Running",
                                ItemStatus::Done => {
                                    if it.detail.to_ascii_lowercase().starts_with("skipped") {
                                        "Skipped"
                                    } else {
                                        "Done"
                                    }
                                }
                                ItemStatus::Failed => "Failed",
                                ItemStatus::Resolving => "Resolving",
                            };
                            ui.label(status_text);
                            ui.add(
                                egui::ProgressBar::new((it.percent / 100.0).clamp(0.0, 1.0))
                                    .desired_width(180.0)
                                    .show_percentage(),
                            );
                        });
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
