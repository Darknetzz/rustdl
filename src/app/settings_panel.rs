use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{secondary_button, warning_button};
use crate::ui_icons;

use super::{DownloadPreset, PydlApp, SettingsTab, LOG_COLOR_WARN};

fn draw_effective_command_preview(ui: &mut egui::Ui, command_preview: &str) {
    let text_color = if ui.visuals().dark_mode {
        Color32::from_rgb(150, 215, 255)
    } else {
        Color32::from_rgb(0, 95, 125)
    };
    egui::Frame::default()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .inner_margin(egui::Margin::same(8.0))
        .rounding(egui::Rounding::same(4.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    RichText::new(command_preview)
                        .monospace()
                        .small()
                        .color(text_color),
                )
                .wrap(),
            );
        });
}

impl PydlApp {
    pub(super) fn draw_settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut changed = false;
        let mut executable_paths_changed = false;
        let command_preview = self.effective_download_command_preview();
        let mut settings_open = self.settings_open;
        egui::Window::new("Settings")
            .open(&mut settings_open)
            .resizable(true)
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::General, "General");
                    ui.selectable_value(
                        &mut self.settings_tab,
                        SettingsTab::Executables,
                        "Executables",
                    );
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Download, "Download");
                    ui.selectable_value(
                        &mut self.settings_tab,
                        SettingsTab::Postprocess,
                        "Post-process",
                    );
                });
                ui.separator();
                match self.settings_tab {
                    SettingsTab::General => {
                        let show_thumbnails_changed = ui
                            .checkbox(&mut self.settings.show_thumbnails, "Show thumbnails in cards")
                            .changed();
                        changed |= show_thumbnails_changed;
                        if show_thumbnails_changed && self.settings.show_thumbnails {
                            // Allow lazy loading for already-fetched items after re-enabling thumbnails.
                            self.thumbnail_attempted.clear();
                        }
                        changed |= ui
                            .checkbox(&mut self.settings.compact_cards, "Use compact cards")
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.hide_card_subtitle,
                                "Hide card subtitle/uploader",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.auto_add_pasted_urls,
                                "Auto-add pasted URLs after a short delay",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.auto_start_downloads,
                                "Auto-start downloads when new items become ready",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.autoscroll_log,
                                "Autoscroll log to latest line",
                            )
                            .changed();
                        ui.horizontal(|ui| {
                            ui.label("UI scale");
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut self.settings.ui_scale, 0.85..=1.5)
                                        .fixed_decimals(2),
                                )
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Parallel downloads");
                            changed |= ui
                                .add(egui::Slider::new(&mut self.worker_count, 1..=6).integer())
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max log chars");
                            changed |= ui
                                .add(
                                    egui::Slider::new(
                                        &mut self.settings.log_max_chars,
                                        2_000..=200_000,
                                    )
                                    .integer(),
                                )
                                .changed();
                        });
                    }
                    SettingsTab::Executables => {
                        ui.label("Leave empty to use PATH lookup.");
                        ui.horizontal(|ui| {
                            ui.label("yt-dlp");
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.yt_dlp_path)
                                    .hint_text("yt-dlp.exe or full path"),
                            );
                            changed |= resp.changed();
                            executable_paths_changed |= resp.changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("ffmpeg");
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.ffmpeg_path)
                                    .hint_text("ffmpeg.exe or full path"),
                            );
                            changed |= resp.changed();
                            executable_paths_changed |= resp.changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("ffprobe");
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.ffprobe_path)
                                    .hint_text("ffprobe.exe or full path"),
                            );
                            changed |= resp.changed();
                            executable_paths_changed |= resp.changed();
                        });
                    }
                    SettingsTab::Download => {
                        ui.label(RichText::new("Presets").strong());
                        ui.horizontal_wrapped(|ui| {
                            if secondary_button(
                                ui,
                                &format!("{} Best quality", ui_icons::PRESET_BEST),
                                true,
                            )
                            .clicked()
                            {
                                self.apply_preset(DownloadPreset::BestQuality);
                            }
                            ui.label(
                                RichText::new(
                                    "Highest quality video flow, mp4 merge preference, faststart enabled.",
                                )
                                .small()
                                .color(Color32::GRAY),
                            );
                        });
                        ui.horizontal_wrapped(|ui| {
                            if secondary_button(
                                ui,
                                &format!("{} Audio only", ui_icons::PRESET_AUDIO),
                                true,
                            )
                            .clicked()
                            {
                                self.apply_preset(DownloadPreset::AudioOnly);
                            }
                            ui.label(
                                RichText::new("Extract MP3 audio only; disables remux-to-mp4.")
                                    .small()
                                    .color(Color32::GRAY),
                            );
                        });
                        ui.horizontal_wrapped(|ui| {
                            if warning_button(
                                ui,
                                &format!("{} Fast download", ui_icons::PRESET_FAST),
                                true,
                            )
                            .clicked()
                            {
                                self.apply_preset(DownloadPreset::FastDownload);
                            }
                            ui.label(
                                RichText::new(
                                    "Prioritizes speed and resilience with fragment concurrency (retries are unlimited by default).",
                                )
                                .small()
                                .color(Color32::GRAY),
                            );
                        });
                        ui.horizontal_wrapped(|ui| {
                            if secondary_button(
                                ui,
                                &format!("{} Archive mode", ui_icons::PRESET_ARCHIVE),
                                true,
                            )
                            .clicked()
                            {
                                self.apply_preset(DownloadPreset::ArchiveMode);
                            }
                            ui.label(
                                RichText::new(
                                    "Keeps extra metadata artifacts like info JSON, subtitles, and description.",
                                )
                                .small()
                                .color(Color32::GRAY),
                            );
                        });
                        ui.separator();
                        ui.label(RichText::new("Retries").strong());
                        changed |= ui
                            .checkbox(
                                &mut self.settings.yt_dlp_unlimited_retries,
                                "Unlimited HTTP and fragment retries",
                            )
                            .on_hover_text(
                                "Maps to yt-dlp --retries and --fragment-retries (infinite or a fixed count).",
                            )
                            .changed();
                        ui.horizontal(|ui| {
                            ui.label("Retry count (when not unlimited)");
                            changed |= ui
                                .add_enabled(
                                    !self.settings.yt_dlp_unlimited_retries,
                                    egui::DragValue::new(&mut self.settings.yt_dlp_retry_count)
                                        .range(1_u32..=999)
                                        .speed(1),
                                )
                                .changed();
                        });
                        ui.label(
                            RichText::new(
                                "Applies to each download request and to DASH/HLS fragments.",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        ui.separator();
                        ui.label("Cookies (optional)");
                        ui.label(
                            RichText::new(
                                "Path to cookies.txt, or a browser for --cookies-from-browser (e.g. firefox, \
                                 brave:C:\\...\\Brave-Browser-Beta\\User Data\\Default). Used when adding URLs \
                                 and when downloading. On Windows, a cookies.txt export is most reliable.",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.yt_dlp_cookies)
                                    .hint_text(r"C:\Users\you\cookies.txt"),
                            )
                            .changed();
                        ui.label("Impersonate (optional)");
                        ui.label(
                            RichText::new(
                                "Browser TLS fingerprint for yt-dlp, e.g. chrome. Some login-gated sites need this with cookies\
                                 (without this, you may see HTTP 410 even with a valid cookies.txt).",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.yt_dlp_impersonate)
                                    .hint_text("chrome"),
                            )
                            .changed();
                        ui.separator();
                        ui.label("Extra args (space-separated) added to each download command");
                        ui.label(
                            RichText::new(
                                "Appended after the retry flags above; add --retries etc. here only if you need to override.",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        changed |= ui
                            .add(
                                egui::TextEdit::multiline(&mut self.settings.yt_dlp_extra_args)
                                    .desired_rows(2)
                                    .hint_text("--concurrent-fragments 4"),
                            )
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.embed_thumbnail, "Embed thumbnail")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.yt_embed_metadata, "Embed metadata")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.yt_ignore_errors, "Ignore errors")
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.yt_restrict_filenames,
                                "Restrict filenames",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.yt_write_info_json, "Write info JSON")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.yt_write_auto_subs, "Write auto subtitles")
                            .changed();
                        ui.separator();
                        ui.label(
                            RichText::new("Effective command preview")
                                .small()
                                .color(Color32::GRAY),
                        );
                        draw_effective_command_preview(ui, &command_preview);
                    }
                    SettingsTab::Postprocess => {
                        ui.label("Post-processor args passed as --postprocessor-args");
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.ffmpeg_post_args)
                                    .hint_text("-movflags +faststart"),
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.ffmpeg_faststart,
                                "Enable faststart (-movflags +faststart)",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.ffmpeg_remux_mp4, "Remux video to mp4")
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.ffmpeg_extract_audio_mp3,
                                "Extract audio as mp3",
                            )
                            .changed();
                        if self.settings.ffmpeg_extract_audio_mp3 {
                            ui.colored_label(
                                LOG_COLOR_WARN,
                                "MP3 extraction is enabled, so remux-to-mp4 is ignored for downloads.",
                            );
                        }
                        ui.add_enabled_ui(!self.settings.ffmpeg_extract_audio_mp3, |ui| {
                            changed |= ui
                                .checkbox(
                                    &mut self.settings.verify_output_video_audio,
                                    "Verify output has video and audio (ffprobe)",
                                )
                                .changed();
                        });
                        if !self.settings.ffmpeg_extract_audio_mp3 {
                            ui.label(
                                RichText::new(
                                    "Marks the item failed if the saved file has no video or no audio track.",
                                )
                                .small()
                                .color(Color32::GRAY),
                            );
                        }
                        ui.label(
                            RichText::new("Effective command preview")
                                .small()
                                .color(Color32::GRAY),
                        );
                        draw_effective_command_preview(ui, &command_preview);
                    }
                }
            });
        self.settings_open = settings_open;
        if changed {
            if self.settings.ffmpeg_extract_audio_mp3 {
                self.settings.ffmpeg_remux_mp4 = false;
            }
            self.settings.yt_dlp_retry_count = self.settings.yt_dlp_retry_count.clamp(1, 999);
            self.settings.worker_count = self.worker_count.clamp(1, 6);
            self.settings.output_dir = self.output_dir.clone();
            self.settings_dirty = true;
            if executable_paths_changed {
                // Keep dependency indicators responsive while editing executable paths.
                self.refresh_deps();
            }
        }
    }
}
