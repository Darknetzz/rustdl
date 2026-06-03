use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{button_group, left_button_row, secondary_button};
use crate::config::{export_settings_json, import_settings_json, trim_activity_log};
use crate::profiles::{all_profiles, find_profile, save_user_profile, DownloadProfile};
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
            .default_height(560.0)
            .show(ctx, |ui| {
                left_button_row(ui, |ui| {
                    button_group(ui, "settings_tabs", |g| {
                    g.add(|ui| {
                        ui.selectable_value(
                            &mut self.settings_tab,
                            SettingsTab::Shared,
                            format!("{} Shared", ui_icons::TAB_SHARED),
                        )
                    });
                    g.add(|ui| {
                        ui.selectable_value(
                            &mut self.settings_tab,
                            SettingsTab::Downloader,
                            format!("{} Downloader", ui_icons::TAB_DOWNLOADER),
                        )
                    });
                    g.add(|ui| {
                        ui.selectable_value(
                            &mut self.settings_tab,
                            SettingsTab::Av1,
                            format!("{} AV1", ui_icons::TAB_AV1),
                        )
                    });
                    });
                });
                ui.separator();
                let scroll_h = ui.available_height().max(240.0);
                egui::ScrollArea::vertical()
                    .id_salt(super::settings_tab_to_str(self.settings_tab))
                    .auto_shrink([false, false])
                    .max_height(scroll_h)
                    .drag_to_scroll(true)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match self.settings_tab {
                    SettingsTab::Shared => {
                        ui.label(RichText::new("Global settings").strong());
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
                                &mut self.settings.card_list_layout,
                                "List layout for queue cards (denser)",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.autoscroll_log,
                                "Autoscroll log to latest line",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.videos_docked,
                                "Dock video / AV1 queue in main window",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.logs_docked,
                                "Dock activity log under video queue (when queue is docked)",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.log_relative_time,
                                "Relative timestamps in activity log",
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
                            ui.label("Theme");
                            egui::ComboBox::from_id_salt("settings_theme")
                                .selected_text(self.settings.theme.clone())
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.theme,
                                            "dark".to_owned(),
                                            "Dark",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.theme,
                                            "light".to_owned(),
                                            "Light",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.theme,
                                            "system".to_owned(),
                                            "System",
                                        )
                                        .changed();
                                });
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
                        ui.separator();
                        ui.label(RichText::new("LAN web UI").strong());
                        ui.label(
                            RichText::new(
                                "HTTP on your local network with a shared token. Not encrypted — use only on networks you trust.",
                            )
                            .color(crate::app_ui::ALERT_WARNING_TEXT),
                        );
                        let web_enabled_changed = ui
                            .checkbox(&mut self.settings.web_ui_enabled, "Enable web UI")
                            .changed();
                        changed |= web_enabled_changed;
                        ui.horizontal(|ui| {
                            ui.label("Bind address");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.web_bind_address)
                                        .hint_text("0.0.0.0:8765"),
                                )
                                .changed();
                        });
                        {
                            let show_token_id = ui.id().with("web_token_visible");
                            let mut show_token = ui.ctx().data_mut(|d| {
                                *d.get_temp_mut_or(show_token_id, false)
                            });
                            ui.horizontal(|ui| {
                                ui.label("API token");
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(
                                            &mut self.settings.web_auth_token,
                                        )
                                        .password(!show_token),
                                    )
                                    .changed();
                                if ui.checkbox(&mut show_token, "Show").changed() {
                                    ui.ctx().data_mut(|d| {
                                        *d.get_temp_mut_or(show_token_id, false) = show_token;
                                    });
                                }
                                let can_copy = !self.settings.web_auth_token.trim().is_empty();
                                if secondary_button(
                                    ui,
                                    &format!("{} Copy", ui_icons::COPY_CLIPBOARD),
                                    can_copy,
                                )
                                .on_hover_text("Copy API token to clipboard")
                                .clicked()
                                {
                                    ui.ctx()
                                        .copy_text(self.settings.web_auth_token.clone());
                                    self.append_log("Web API token copied to clipboard.");
                                }
                            });
                        }
                        if secondary_button(
                            ui,
                            &format!("{} Generate new API token", ui_icons::TOKEN),
                            true,
                        )
                        .clicked() {
                            self.settings.web_auth_token =
                                crate::config::generate_web_auth_token();
                            changed = true;
                            self.append_log(
                                "New web API token generated. Copy it (Copy button) and update browsers that use the web UI.",
                            );
                        }
                        if self.settings.web_ui_enabled {
                            let url =
                                crate::service::web::web_ui_browser_url(&self.settings.web_bind_address);
                            ui.horizontal_wrapped(|ui| {
                                ui.label("Open");
                                ui.hyperlink_to(&url, &url);
                                ui.label("in a browser, then paste the API token.");
                            });
                            if self.settings.web_bind_address.trim().contains("0.0.0.0") {
                                ui.label(
                                    RichText::new(
                                        "On other devices on your LAN, use this PC's IP address instead of 127.0.0.1.",
                                    )
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                                );
                            }
                        }
                        ui.separator();
                        ui.label(RichText::new("Shared executables").strong());
                        ui.label("Used by the downloader and AV1 converter.");
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
                        ui.separator();
                        ui.label(RichText::new("Settings portability").strong());
                        ui.horizontal_wrapped(|ui| {
                            if secondary_button(
                                ui,
                                &format!("{} Export settings", ui_icons::EXPORT),
                                true,
                            )
                            .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_file_name("rustdl_config_export.json")
                                    .save_file()
                                {
                                    match export_settings_json(&self.settings, &path) {
                                        Ok(()) => self.append_log(&format!(
                                            "Exported settings to {}",
                                            path.to_string_lossy()
                                        )),
                                        Err(e) => self.append_log(&format!(
                                            "Export settings failed: {e:#}"
                                        )),
                                    }
                                }
                            }
                            if secondary_button(
                                ui,
                                &format!("{} Import settings", ui_icons::IMPORT_FILE),
                                true,
                            )
                            .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("JSON", &["json"])
                                    .pick_file()
                                {
                                    match import_settings_json(&path) {
                                        Ok(imported) => {
                                            self.settings = imported;
                                            self.output_dir = self.settings.output_dir.clone();
                                            self.worker_count =
                                                self.settings.worker_count.clamp(1, 6);
                                            self.settings_tab =
                                                super::settings_tab_from_str(
                                                    &self.settings.settings_tab,
                                                );
                                            changed = true;
                                            self.append_log(&format!(
                                                "Imported settings from {}",
                                                path.to_string_lossy()
                                            ));
                                        }
                                        Err(e) => self.append_log(&format!(
                                            "Import settings failed: {e:#}"
                                        )),
                                    }
                                }
                            }
                            if secondary_button(
                                ui,
                                &format!("{} Reset to defaults", ui_icons::RESET),
                                true,
                            )
                            .clicked() {
                                let keep_output = self.settings.output_dir.clone();
                                self.settings = crate::config::AppSettings::default();
                                self.settings.output_dir = keep_output.clone();
                                self.output_dir = keep_output;
                                changed = true;
                            }
                        });
                    }
                    SettingsTab::Downloader => {
                        ui.label(RichText::new("Downloader behavior").strong());
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
                                &mut self.settings.enqueue_downloads_to_av1,
                                "Enqueue completed downloads in AV1 converter queue",
                            )
                            .changed();
                        ui.horizontal(|ui| {
                            ui.label("Parallel downloads");
                            changed |= ui
                                .add(egui::Slider::new(&mut self.worker_count, 1..=6).integer())
                                .changed();
                        });
                        ui.separator();
                        ui.label(RichText::new("Downloader executables").strong());
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
                        ui.separator();
                        ui.label(RichText::new("Download profile").strong());
                        let profiles = all_profiles(&self.profile_store);
                        let active = self.settings.active_profile.clone();
                        egui::ComboBox::from_id_salt("settings_active_profile")
                            .selected_text(active.clone())
                            .show_ui(ui, |ui| {
                                for p in &profiles {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.active_profile,
                                            p.name.clone(),
                                            &p.name,
                                        )
                                        .clicked()
                                    {
                                        if let Some(prof) =
                                            find_profile(&self.profile_store, &p.name)
                                        {
                                            prof.apply_to(&mut self.settings);
                                        }
                                        changed = true;
                                    }
                                }
                            });
                        ui.horizontal_wrapped(|ui| {
                            if secondary_button(
                                ui,
                                &format!("{} Save current as profile…", ui_icons::SAVE),
                                true,
                            )
                            .clicked()
                            {
                                self.new_profile_name_buffer = Some(String::new());
                            }
                        });
                        if self.new_profile_name_buffer.is_some() {
                            let mut name_buf = self
                                .new_profile_name_buffer
                                .take()
                                .unwrap_or_default();
                            let mut save_clicked = false;
                            ui.horizontal(|ui| {
                                ui.label("Profile name");
                                ui.text_edit_singleline(&mut name_buf);
                                save_clicked = secondary_button(
                                    ui,
                                    &format!("{} Save", ui_icons::SAVE),
                                    !name_buf.trim().is_empty(),
                                )
                                    .clicked();
                            });
                            if save_clicked {
                                let name = name_buf.trim().to_owned();
                                let profile =
                                    DownloadProfile::from_settings(&name, &self.settings, false);
                                if let Err(e) =
                                    save_user_profile(&mut self.profile_store, profile)
                                {
                                    self.append_log(&format!("Save profile failed: {e:#}"));
                                    self.new_profile_name_buffer = Some(name_buf);
                                } else {
                                    self.settings.active_profile = name.clone();
                                    self.append_log(&format!("Saved profile: {name}"));
                                    changed = true;
                                }
                            } else {
                                self.new_profile_name_buffer = Some(name_buf);
                            }
                        }
                        ui.separator();
                        ui.label(RichText::new("User profiles file").strong());
                        ui.horizontal_wrapped(|ui| {
                            if secondary_button(
                                ui,
                                &format!("{} Export profiles", ui_icons::EXPORT),
                                true,
                            )
                            .clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_file_name("rustdl_profiles.json")
                                    .save_file()
                                {
                                    if let Err(e) = crate::profiles::export_profiles_json(
                                        &self.profile_store,
                                        &path,
                                    ) {
                                        self.append_log(&format!("Export profiles failed: {e:#}"));
                                    } else {
                                        self.append_log(&format!(
                                            "Exported profiles to {}",
                                            path.to_string_lossy()
                                        ));
                                    }
                                }
                            }
                            if secondary_button(
                                ui,
                                &format!("{} Import profiles", ui_icons::IMPORT_FILE),
                                true,
                            )
                            .clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("JSON", &["json"])
                                    .pick_file()
                                {
                                    match crate::profiles::import_profiles_json(&path) {
                                        Ok(imported) => {
                                            self.profile_store = imported;
                                            self.append_log(&format!(
                                                "Imported profiles from {}",
                                                path.to_string_lossy()
                                            ));
                                        }
                                        Err(e) => {
                                            self.append_log(&format!(
                                                "Import profiles failed: {e:#}"
                                            ));
                                        }
                                    }
                                }
                            }
                        });
                        ui.separator();
                        ui.label(RichText::new("Output and quality").strong());
                        ui.label("Output filename template (-o)");
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(
                                    &mut self.settings.output_filename_template,
                                )
                                .hint_text(crate::config::DEFAULT_OUTPUT_FILENAME_TEMPLATE),
                            )
                            .changed();
                        ui.horizontal(|ui| {
                            ui.label("Quality preset");
                            egui::ComboBox::from_id_salt("settings_quality_preset")
                                .selected_text(self.settings.quality_preset.clone())
                                .show_ui(ui, |ui| {
                                    for (v, label) in [
                                        ("best", "Best"),
                                        ("1080p", "1080p max"),
                                        ("720p", "720p max"),
                                        ("audio", "Audio best"),
                                        ("custom", "Custom (-f)"),
                                    ] {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.quality_preset,
                                                v.to_owned(),
                                                label,
                                            )
                                            .changed();
                                    }
                                });
                        });
                        if self.settings.quality_preset == "custom" {
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(
                                        &mut self.settings.quality_format_custom,
                                    )
                                    .hint_text("bestvideo+bestaudio/best"),
                                )
                                .changed();
                        }
                        ui.horizontal(|ui| {
                            ui.label("Merge container");
                            egui::ComboBox::from_id_salt("settings_merge_container")
                                .selected_text(self.settings.merge_container.clone())
                                .show_ui(ui, |ui| {
                                    for (v, label) in [
                                        ("default", "Default"),
                                        ("mp4", "MP4"),
                                        ("mkv", "MKV"),
                                        ("webm", "WebM"),
                                    ] {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.merge_container,
                                                v.to_owned(),
                                                label,
                                            )
                                            .changed();
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Playlist preview limit");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.playlist_preview_cap)
                                        .range(1_usize..=500_usize),
                                )
                                .changed();
                        });
                        ui.separator();
                        ui.label(RichText::new("Network and archive").strong());
                        ui.label("Download archive file (--download-archive)");
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.yt_download_archive)
                                    .hint_text("optional path"),
                            )
                            .changed();
                        ui.label("Proxy URL (--proxy)");
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.yt_proxy)
                                    .hint_text("http://127.0.0.1:8080"),
                            )
                            .changed();
                        ui.label("Download speed limit (--limit-rate)");
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.yt_limit_rate)
                                    .hint_text("50K, 4M, or empty for unlimited"),
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.yt_sponsorblock_remove,
                                "Remove SponsorBlock segments",
                            )
                            .changed();
                        ui.label("SponsorBlock mark categories (--sponsorblock-mark)");
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.settings.yt_sponsorblock_mark)
                                    .hint_text("sponsor,intro (leave empty to disable)"),
                            )
                            .changed();
                        ui.separator();
                        ui.label(RichText::new("Downloader options").strong());
                        ui.label(RichText::new("Presets").strong());
                        left_button_row(ui, |ui| {
                            button_group(ui, "dl_presets", |g| {
                            if g.secondary(
                                &format!("{} Best quality", ui_icons::PRESET_BEST),
                                true,
                            )
                            .clicked()
                            {
                                self.apply_preset(DownloadPreset::BestQuality);
                            }
                            if g.secondary(
                                &format!("{} Audio only", ui_icons::PRESET_AUDIO),
                                true,
                            )
                            .clicked()
                            {
                                self.apply_preset(DownloadPreset::AudioOnly);
                            }
                            if g.warning(
                                &format!("{} Fast download", ui_icons::PRESET_FAST),
                                true,
                            )
                            .clicked()
                            {
                                self.apply_preset(DownloadPreset::FastDownload);
                            }
                            if g.secondary(
                                &format!("{} Archive mode", ui_icons::PRESET_ARCHIVE),
                                true,
                            )
                            .clicked()
                            {
                                self.apply_preset(DownloadPreset::ArchiveMode);
                            }
                            });
                        });
                        ui.label(
                            RichText::new(
                                "Best quality: highest quality, mp4 merge, faststart. Audio only: MP3 extraction. \
                                 Fast download: speed and fragment concurrency. Archive mode: extra metadata artifacts.",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
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
                                "Browser TLS fingerprint for yt-dlp, e.g. chrome. Some login-gated sites need \
                                 this with cookies (without it, you may see HTTP 410 even with a valid cookies.txt).",
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
                        ui.separator();
                        ui.label(RichText::new("Downloader post-process").strong());
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
                    SettingsTab::Av1 => {
                        ui.label(RichText::new("AV1 converter settings").strong());
                        ui.label(
                            RichText::new(
                                "FFmpeg and ffprobe paths are configured in Settings → Shared.",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        ui.separator();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.av1_remember_queue,
                                "Remember AV1 queue between sessions",
                            )
                            .on_hover_text(
                                "When enabled, queue items stay until you click Clear. \
                                 When off, the AV1 queue is cleared each time you start the app.",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.av1_recursive, "Recursive folder scan")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.av1_dry_run, "Dry run by default")
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.av1_delete_original,
                                "Delete original after success",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.av1_rename_original,
                                "Rename output to original filename",
                            )
                            .on_hover_text(
                                "After success, rename the encoded file back to the source \
                                 filename when it shares the output folder. Typically used with \
                                 delete original for in-place replacement.",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.av1_overwrite, "Overwrite output files")
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.av1_reencode_av1,
                                "Re-encode files already in AV1",
                            )
                            .changed();
                        ui.horizontal(|ui| {
                            ui.label("Target bitrate");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.av1_target_bitrate)
                                        .hint_text("auto"),
                                )
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max width");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.av1_max_width)
                                        .range(320_u32..=7680_u32)
                                        .speed(10),
                                )
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Min shrink %");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.av1_min_shrink_percent)
                                        .range(0.0_f32..=95.0_f32)
                                        .speed(0.5),
                                )
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Size preset");
                            egui::ComboBox::from_id_salt("settings_av1_preset")
                                .selected_text(self.settings.av1_size_preset.clone())
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.av1_size_preset,
                                            "light".to_owned(),
                                            "light",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.av1_size_preset,
                                            "balanced".to_owned(),
                                            "balanced",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.av1_size_preset,
                                            "aggressive".to_owned(),
                                            "aggressive",
                                        )
                                        .changed();
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Encoder override");
                            egui::ComboBox::from_id_salt("settings_av1_encoder")
                                .selected_text(if self.settings.av1_encoder_override.is_empty() {
                                    "Auto".to_owned()
                                } else {
                                    self.settings.av1_encoder_override.clone()
                                })
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.av1_encoder_override,
                                            String::new(),
                                            "Auto",
                                        )
                                        .changed();
                                    for enc in [
                                        "av1_nvenc",
                                        "av1_amf",
                                        "hevc_nvenc",
                                        "hevc_amf",
                                        "libsvtav1",
                                    ] {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.av1_encoder_override,
                                                enc.to_owned(),
                                                enc,
                                            )
                                            .changed();
                                    }
                                });
                        });
                    }
                }
            });
            });
        self.settings_open = settings_open;
        if changed {
            if self.settings.ffmpeg_extract_audio_mp3 {
                self.settings.ffmpeg_remux_mp4 = false;
            }
            self.settings.yt_dlp_retry_count = self.settings.yt_dlp_retry_count.clamp(1, 999);
            self.settings.worker_count = self.worker_count.clamp(1, 6);
            self.settings.output_dir = self.output_dir.clone();
            self.settings.playlist_preview_cap = self.settings.playlist_preview_cap.clamp(1, 500);
            self.settings.av1_max_width = self.settings.av1_max_width.clamp(320, 7680);
            self.settings.av1_min_shrink_percent =
                self.settings.av1_min_shrink_percent.clamp(0.0, 95.0);
            trim_activity_log(&mut self.log_lines, self.settings.log_max_chars);
            self.persist_settings();
            let shared = self.shared_core.clone();
            super::core_sync::push_app_to_core(self, &shared);
            self.restart_web_server();
            self.flush_log_to_disk();
            if executable_paths_changed {
                self.refresh_deps();
            }
        }
    }
}
