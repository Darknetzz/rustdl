use eframe::egui::{self, RichText};

use crate::app_actions;
use crate::app_ui::{danger_button, secondary_button, success_button};
use crate::av1_transcode::{self, Av1Config, Av1Input};
use crate::models::{Av1QueueItem, ItemStatus};
use crate::ui_icons;

use super::PydlApp;

impl PydlApp {
    pub(super) fn draw_av1_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("AV1 Converter").heading());
            ui.label(
                RichText::new("Near-parity mode for local video transcoding.")
                    .small()
                    .color(egui::Color32::GRAY),
            );
        });
        let (ffmpeg_ok, ffprobe_ok) = av1_transcode::av1_tools_available(
            &self.settings.av1_ffmpeg_path,
            &self.settings.av1_ffprobe_path,
        );
        ui.horizontal_wrapped(|ui| {
            let ffmpeg_txt = if ffmpeg_ok { "✔ ffmpeg" } else { "✖ ffmpeg" };
            let ffmpeg_fg = if ffmpeg_ok {
                egui::Color32::from_rgb(132, 235, 156)
            } else {
                egui::Color32::from_rgb(70, 15, 15)
            };
            ui.label(
                RichText::new(ffmpeg_txt)
                    .strong()
                    .color(ffmpeg_fg),
            );
            ui.separator();
            let ffprobe_txt = if ffprobe_ok { "✔ ffprobe" } else { "✖ ffprobe" };
            let ffprobe_fg = if ffprobe_ok {
                egui::Color32::from_rgb(132, 235, 156)
            } else {
                egui::Color32::from_rgb(70, 15, 15)
            };
            ui.label(
                RichText::new(ffprobe_txt)
                    .strong()
                    .color(ffprobe_fg),
            );
        });
        ui.separator();
        ui.label("Input paths (file/folder, one per line)");
        ui.horizontal_wrapped(|ui| {
            if secondary_button(ui, "Browse", true).clicked() {
                self.browse_av1_inputs();
            }
        });
        ui.add_sized(
            [ui.available_width(), 90.0],
            egui::TextEdit::multiline(&mut self.av1_input_paths)
                .hint_text(r"D:\Videos\movie.mkv\nD:\Videos\Folder"),
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
        ui.label(
            RichText::new("FFmpeg/FFprobe paths are configured in Settings -> AV1.")
                .small()
                .color(egui::Color32::GRAY),
        );
        ui.horizontal_wrapped(|ui| {
            if success_button(
                ui,
                &format!("{} Start AV1 batch", ui_icons::PLAY),
                !self.av1_running && ffmpeg_ok && ffprobe_ok,
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
                self.av1_items.clear();
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
                    ui.label(RichText::new("No AV1 jobs yet.").color(egui::Color32::GRAY));
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
                            }
                            ui.label(RichText::new(format!("#{}", it.item_id)).strong());
                            let status_text = match it.status {
                                ItemStatus::Idle => "Idle",
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
                                    .desired_width(180.0),
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

    fn start_av1_batch(&mut self) {
        let lines: Vec<String> = self
            .av1_input_paths
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if lines.is_empty() {
            self.append_log("AV1: add at least one input path.");
            return;
        }
        self.persist_settings();
        self.av1_cancel_flag
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let cfg = Av1Config {
            ffmpeg_path: self.settings.av1_ffmpeg_path.clone(),
            ffprobe_path: self.settings.av1_ffprobe_path.clone(),
            output_dir: self.output_dir.clone(),
            recursive: self.settings.av1_recursive,
            dry_run: self.settings.av1_dry_run,
            delete_original: self.settings.av1_delete_original,
            overwrite: self.settings.av1_overwrite,
            reencode_av1: self.settings.av1_reencode_av1,
            target_bitrate: self.settings.av1_target_bitrate.clone(),
            max_width: self.settings.av1_max_width,
        };
        let inputs: Vec<Av1Input> = lines
            .iter()
            .map(|x| Av1Input {
                source_path: x.clone(),
            })
            .collect();
        let plan = av1_transcode::collect_plan(&inputs, &cfg);
        if plan.is_empty() {
            self.append_log("AV1: no supported video files found.");
            return;
        }
        self.av1_items.clear();
        self.av1_duration_ms.clear();
        let mut jobs = Vec::new();
        for p in plan {
            let item_id = self.av1_next_item_id;
            self.av1_next_item_id = self.av1_next_item_id.saturating_add(1);
            if let Some(ms) = av1_transcode::input_duration_ms(&p.input, &cfg.ffprobe_path) {
                self.av1_duration_ms.insert(item_id, ms);
            }
            self.av1_items.push(Av1QueueItem {
                item_id,
                source_path: p.input.to_string_lossy().to_string(),
                output_path: p.output.to_string_lossy().to_string(),
                status: ItemStatus::Queued,
                percent: 0.0,
                detail: "Queued".to_owned(),
            });
            jobs.push((
                item_id,
                Av1Input {
                    source_path: p.input.to_string_lossy().to_string(),
                },
                p.output.to_string_lossy().to_string(),
            ));
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

