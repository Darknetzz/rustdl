use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{button_group, left_button_row};
use crate::config::{export_settings_json, import_settings_json, trim_activity_log, AppSettings};
use crate::profiles::{
    all_profiles, delete_user_profile, find_profile, rename_user_profile, save_user_profile,
    DownloadProfile,
};
use crate::ui_icons;

use super::{DownloadPreset, PydlApp, SettingsTab, LOG_COLOR_WARN};

const WEB_TOKEN_COPY_FEEDBACK_SECS: f64 = 2.0;
const SETTINGS_FORM_LABEL_WIDTH: f32 = 240.0;
const UI_SCALE_MIN: f32 = 0.85;
const UI_SCALE_MAX: f32 = 1.5;
const UI_SCALE_STEP: f32 = 0.05;

fn bump_ui_scale(scale: &mut f32, delta: f32) {
    *scale = (*scale + delta).clamp(UI_SCALE_MIN, UI_SCALE_MAX);
    *scale = ((*scale * 100.0).round()) / 100.0;
}

fn settings_form_grid<R>(
    ui: &mut egui::Ui,
    id_salt: &str,
    f: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Grid::new(ui.id().with(id_salt))
        .num_columns(2)
        .min_col_width(SETTINGS_FORM_LABEL_WIDTH)
        .spacing([16.0, 6.0])
        .show(ui, f)
        .inner
}

fn settings_checkbox(ui: &mut egui::Ui, label: &str, value: &mut bool) -> bool {
    ui.label(label);
    let changed = ui.checkbox(value, "").changed();
    ui.end_row();
    changed
}

fn apply_layout_preset(settings: &mut AppSettings, preset: &str) {
    match preset {
        "compact" => {
            settings.card_list_layout = true;
            settings.compact_cards = true;
            settings.hide_card_subtitle = true;
            settings.show_thumbnails = true;
        }
        "review" => {
            settings.card_list_layout = false;
            settings.compact_cards = false;
            settings.hide_card_subtitle = false;
            settings.show_thumbnails = true;
        }
        "minimal" => {
            settings.card_list_layout = true;
            settings.compact_cards = true;
            settings.hide_card_subtitle = true;
            settings.show_thumbnails = false;
        }
        _ => {}
    }
}

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
                let prev_settings_tab = self.settings_tab;
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
                            SettingsTab::Convert,
                            format!("{} AV1", ui_icons::TAB_AV1),
                        )
                    });
                    g.add(|ui| {
                        ui.selectable_value(
                            &mut self.settings_tab,
                            SettingsTab::WebUi,
                            format!("{} Web UI", ui_icons::WEB_UI),
                        )
                    });
                    });
                });
                if self.settings_tab != prev_settings_tab {
                    self.sync_settings_tab_to_disk();
                }
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
                        settings_form_grid(ui, "shared_global", |ui| {
                            let show_thumbnails_changed =
                                settings_checkbox(ui, "Show thumbnails in cards", &mut self.settings.show_thumbnails);
                            changed |= show_thumbnails_changed;
                            if show_thumbnails_changed && self.settings.show_thumbnails {
                                // Allow lazy loading for already-fetched items after re-enabling thumbnails.
                                self.thumbnail_attempted.clear();
                            }
                            changed |= settings_checkbox(ui, "Use compact cards", &mut self.settings.compact_cards);
                            changed |= settings_checkbox(
                                ui,
                                "Hide card subtitle/uploader",
                                &mut self.settings.hide_card_subtitle,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "List layout for queue cards (denser)",
                                &mut self.settings.card_list_layout,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Autoscroll log to latest line",
                                &mut self.settings.autoscroll_log,
                            );
                            let videos_docked_before = self.settings.videos_docked;
                            changed |= settings_checkbox(
                                ui,
                                "Dock video / Convert queue in main window",
                                &mut self.settings.videos_docked,
                            );
                            if self.settings.videos_docked != videos_docked_before {
                                self.note_videos_dock_user_choice(self.settings.videos_docked);
                            }
                            changed |= settings_checkbox(
                                ui,
                                "Dock activity log under video queue (when queue is docked)",
                                &mut self.settings.logs_docked,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Relative timestamps in activity log",
                                &mut self.settings.log_relative_time,
                            );
                            ui.label("UI scale");
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let pct = (self.settings.ui_scale * 100.0).round() as i32;
                                let at_min = self.settings.ui_scale <= UI_SCALE_MIN;
                                let at_max = self.settings.ui_scale >= UI_SCALE_MAX;
                                left_button_row(ui, |ui| {
                                    button_group(ui, "ui_scale_minus", |g| {
                                        if g
                                            .secondary("−", !at_min)
                                            .on_hover_text("Decrease UI scale")
                                            .clicked()
                                        {
                                            bump_ui_scale(&mut self.settings.ui_scale, -UI_SCALE_STEP);
                                            changed = true;
                                        }
                                    });
                                });
                                ui.label(RichText::new(format!("{pct}%")).strong());
                                left_button_row(ui, |ui| {
                                    button_group(ui, "ui_scale_plus", |g| {
                                        if g
                                            .secondary("+", !at_max)
                                            .on_hover_text("Increase UI scale")
                                            .clicked()
                                        {
                                            bump_ui_scale(&mut self.settings.ui_scale, UI_SCALE_STEP);
                                            changed = true;
                                        }
                                    });
                                });
                            });
                            ui.end_row();
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
                            ui.end_row();
                        });
                        ui.label(RichText::new("Mode panel colors").strong());
                        ui.label(
                            RichText::new(
                                "Tint and accent stripe for Downloader and Video Converter panels.",
                            )
                            .small()
                            .color(crate::theme::text_hint(&self.settings.theme)),
                        );
                        settings_form_grid(ui, "shared_mode_colors", |ui| {
                            ui.label("Downloader");
                            changed |= crate::theme::draw_mode_color_controls(
                                ui,
                                &mut self.settings.mode_downloader_color,
                                crate::theme::MODE_DOWNLOADER,
                            );
                            ui.end_row();
                            ui.label("Video Converter");
                            changed |= crate::theme::draw_mode_color_controls(
                                ui,
                                &mut self.settings.mode_convert_color,
                                crate::theme::MODE_CONVERT,
                            );
                            ui.end_row();
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
                            ui.end_row();
                            ui.label("Max content width");
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut self.settings.max_content_width, 0.0..=1600.0)
                                        .custom_formatter(|v, _| {
                                            if v <= 0.0 {
                                                "Full width".to_owned()
                                            } else {
                                                format!("{:.0} px", v)
                                            }
                                        }),
                                )
                                .on_hover_text(
                                    "Limits how wide controls stretch on ultrawide monitors (0 = full panel width).",
                                )
                                .changed();
                            ui.end_row();
                        });
                        ui.separator();
                        ui.label(RichText::new("Layout presets").strong());
                        ui.label(
                            RichText::new(
                                "One-click display bundles (does not change download or AV1 options).",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        left_button_row(ui, |ui| {
                            button_group(ui, "layout_presets", |g| {
                                if g
                                    .secondary("Compact queue", true)
                                    .on_hover_text("List layout, compact cards, hide subtitle")
                                    .clicked()
                                {
                                    apply_layout_preset(&mut self.settings, "compact");
                                    changed = true;
                                }
                                if g
                                    .secondary("Review mode", true)
                                    .on_hover_text("Horizontal cards with thumbnails")
                                    .clicked()
                                {
                                    apply_layout_preset(&mut self.settings, "review");
                                    changed = true;
                                }
                                if g
                                    .secondary("Minimal", true)
                                    .on_hover_text("Compact list without thumbnails")
                                    .clicked()
                                {
                                    apply_layout_preset(&mut self.settings, "minimal");
                                    changed = true;
                                }
                            });
                        });
                        ui.separator();
                        ui.label(RichText::new("Session restore").strong());
                        settings_form_grid(ui, "shared_session_restore", |ui| {
                            ui.label("On startup");
                            egui::ComboBox::from_id_salt("settings_session_restore")
                                .selected_text(match self.settings.session_restore_preference.as_str() {
                                    "always" => "Always restore saved queues",
                                    "never" => "Never restore (start fresh)",
                                    _ => "Ask each startup",
                                })
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.session_restore_preference,
                                            "ask".to_owned(),
                                            "Ask each startup",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.session_restore_preference,
                                            "always".to_owned(),
                                            "Always restore saved queues",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.session_restore_preference,
                                            "never".to_owned(),
                                            "Never restore (start fresh)",
                                        )
                                        .changed();
                                });
                            ui.end_row();
                        });
                        ui.separator();
                        ui.label(RichText::new("Shared executables").strong());
                        ui.label("Used by the downloader and Video Converter.");
                        settings_form_grid(ui, "shared_executables", |ui| {
                            ui.label("ffmpeg");
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.ffmpeg_path)
                                    .hint_text("ffmpeg.exe or full path"),
                            );
                            changed |= resp.changed();
                            executable_paths_changed |= resp.changed();
                            ui.end_row();
                            ui.label("ffprobe");
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.ffprobe_path)
                                    .hint_text("ffprobe.exe or full path"),
                            );
                            changed |= resp.changed();
                            executable_paths_changed |= resp.changed();
                            ui.end_row();
                        });
                        ui.separator();
                        ui.label(RichText::new("Settings portability").strong());
                        left_button_row(ui, |ui| {
                            let mut export_settings = false;
                            let mut import_settings = false;
                            button_group(ui, "settings_portability", |g| {
                                g.import_export_menu(true, |ui| {
                                    if ui
                                        .button(format!(
                                            "{} Export settings",
                                            ui_icons::EXPORT
                                        ))
                                        .clicked()
                                    {
                                        export_settings = true;
                                    }
                                    if ui
                                        .button(format!(
                                            "{} Import settings",
                                            ui_icons::IMPORT_FILE
                                        ))
                                        .clicked()
                                    {
                                        import_settings = true;
                                    }
                                });
                                if g.secondary(
                                    &format!("{} Reset to defaults", ui_icons::RESET),
                                    true,
                                )
                                .clicked()
                                {
                                    let keep_output = self.settings.output_dir.clone();
                                    self.settings = crate::config::AppSettings::default();
                                    self.settings.output_dir = keep_output.clone();
                                    self.output_dir = keep_output;
                                    changed = true;
                                }
                            });
                            if export_settings {
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
                            if import_settings {
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
                        });
                    }
                    SettingsTab::Downloader => {
                        ui.label(RichText::new("Downloader behavior").strong());
                        settings_form_grid(ui, "dl_behavior", |ui| {
                            changed |= settings_checkbox(
                                ui,
                                "Auto-add pasted URLs after a short delay",
                                &mut self.settings.auto_add_pasted_urls,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Auto-start downloads when new items become ready",
                                &mut self.settings.auto_start_downloads,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Enqueue completed downloads in Video Converter queue",
                                &mut self.settings.enqueue_downloads_to_convert,
                            );
                            ui.label("Parallel downloads");
                            changed |= ui
                                .add(egui::Slider::new(&mut self.worker_count, 1..=6).integer())
                                .changed();
                            ui.end_row();
                        });
                        ui.separator();
                        ui.label(RichText::new("Downloader executables").strong());
                        ui.label("Leave empty to use PATH lookup.");
                        settings_form_grid(ui, "dl_executables", |ui| {
                            ui.label("yt-dlp");
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.yt_dlp_path)
                                    .hint_text("yt-dlp.exe or full path"),
                            );
                            changed |= resp.changed();
                            executable_paths_changed |= resp.changed();
                            ui.end_row();
                        });
                        ui.separator();
                        ui.label(RichText::new("Download profile").strong());
                        let profiles = all_profiles(&self.profile_store);
                        let active = self.settings.active_profile.clone();
                        settings_form_grid(ui, "dl_active_profile", |ui| {
                            ui.label("Active profile");
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
                            ui.end_row();
                        });
                        left_button_row(ui, |ui| {
                            button_group(ui, "profile_save_as", |g| {
                                if g.secondary(
                                    &format!("{} Save current as profile…", ui_icons::SAVE),
                                    true,
                                )
                                .clicked()
                                {
                                    self.new_profile_name_buffer = Some(String::new());
                                }
                            });
                        });
                        if self.new_profile_name_buffer.is_some() {
                            let mut name_buf = self
                                .new_profile_name_buffer
                                .take()
                                .unwrap_or_default();
                            let mut save_clicked = false;
                            settings_form_grid(ui, "dl_profile_name", |ui| {
                                ui.label("Profile name");
                                ui.horizontal(|ui| {
                                    ui.text_edit_singleline(&mut name_buf);
                                    button_group(ui, "profile_name_save", |g| {
                                        save_clicked = g.secondary(
                                            &format!("{} Save", ui_icons::SAVE),
                                            !name_buf.trim().is_empty(),
                                        )
                                        .clicked();
                                    });
                                });
                                ui.end_row();
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
                        if let Some(active) = find_profile(&self.profile_store, &self.settings.active_profile) {
                            if !active.builtin {
                                left_button_row(ui, |ui| {
                                    button_group(ui, "profile_manage", |g| {
                                        if g
                                            .secondary(
                                                &format!("{} Rename profile…", ui_icons::SAVE),
                                                true,
                                            )
                                            .clicked()
                                        {
                                            self.profile_rename_buffer = Some((
                                                active.name.clone(),
                                                active.name.clone(),
                                            ));
                                        }
                                        if g
                                            .danger(
                                                &format!("{} Delete profile", ui_icons::REMOVE),
                                                true,
                                            )
                                            .clicked()
                                        {
                                            let name = active.name.clone();
                                            if let Err(e) =
                                                delete_user_profile(&mut self.profile_store, &name)
                                            {
                                                self.append_log(&format!(
                                                    "Delete profile failed: {e:#}"
                                                ));
                                            } else {
                                                if self.settings.active_profile == name {
                                                    self.settings.active_profile =
                                                        "Best quality".to_owned();
                                                }
                                                self.append_log(&format!("Deleted profile: {name}"));
                                                changed = true;
                                            }
                                        }
                                    });
                                });
                            }
                        }
                        if let Some((old_name, mut new_name)) = self.profile_rename_buffer.take() {
                            let mut save_rename = false;
                            settings_form_grid(ui, "dl_profile_rename", |ui| {
                                ui.label("Rename to");
                                ui.horizontal(|ui| {
                                    ui.text_edit_singleline(&mut new_name);
                                    button_group(ui, "profile_rename_actions", |g| {
                                        save_rename = g
                                            .secondary(&format!("{} Save", ui_icons::SAVE), true)
                                            .clicked();
                                        if g
                                            .secondary(&format!("{} Cancel", ui_icons::DISMISS), true)
                                            .clicked()
                                        {
                                            new_name.clear();
                                        }
                                    });
                                });
                                ui.end_row();
                            });
                            if save_rename && !new_name.trim().is_empty() {
                                match rename_user_profile(
                                    &mut self.profile_store,
                                    &old_name,
                                    new_name.trim(),
                                ) {
                                    Ok(()) => {
                                        if self.settings.active_profile == old_name {
                                            self.settings.active_profile = new_name.trim().to_owned();
                                        }
                                        self.append_log(&format!(
                                            "Renamed profile \"{old_name}\" to \"{}\"",
                                            new_name.trim()
                                        ));
                                        changed = true;
                                    }
                                    Err(e) => {
                                        self.append_log(&format!("Rename profile failed: {e:#}"));
                                        self.profile_rename_buffer =
                                            Some((old_name, new_name));
                                    }
                                }
                            } else if !new_name.is_empty() || save_rename {
                                self.profile_rename_buffer = Some((old_name, new_name));
                            }
                        }
                        ui.separator();
                        ui.label(RichText::new("User profiles file").strong());
                        left_button_row(ui, |ui| {
                            let mut export_profiles = false;
                            let mut import_profiles = false;
                            button_group(ui, "profiles_io", |g| {
                                g.import_export_menu(true, |ui| {
                                    if ui
                                        .button(format!(
                                            "{} Export profiles",
                                            ui_icons::EXPORT
                                        ))
                                        .clicked()
                                    {
                                        export_profiles = true;
                                    }
                                    if ui
                                        .button(format!(
                                            "{} Import profiles",
                                            ui_icons::IMPORT_FILE
                                        ))
                                        .clicked()
                                    {
                                        import_profiles = true;
                                    }
                                });
                            });
                            if export_profiles {
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
                            if import_profiles {
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
                        settings_form_grid(ui, "dl_output_quality", |ui| {
                            ui.label("Output filename template (-o)");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(
                                        &mut self.settings.output_filename_template,
                                    )
                                    .hint_text(crate::config::DEFAULT_OUTPUT_FILENAME_TEMPLATE),
                                )
                                .changed();
                            ui.end_row();
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
                            ui.end_row();
                            if self.settings.quality_preset == "custom" {
                                ui.label("Custom format (-f)");
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(
                                            &mut self.settings.quality_format_custom,
                                        )
                                        .hint_text("bestvideo+bestaudio/best"),
                                    )
                                    .changed();
                                ui.end_row();
                            }
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
                            ui.end_row();
                            ui.label("Playlist preview limit");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.playlist_preview_cap)
                                        .range(1_usize..=500_usize),
                                )
                                .changed();
                            ui.end_row();
                        });
                        ui.separator();
                        ui.label(RichText::new("Network and archive").strong());
                        settings_form_grid(ui, "dl_network_archive", |ui| {
                            ui.label("Download archive file (--download-archive)");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.yt_download_archive)
                                        .hint_text("optional path"),
                                )
                                .changed();
                            ui.end_row();
                            ui.label("Proxy URL (--proxy)");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.yt_proxy)
                                        .hint_text("http://127.0.0.1:8080"),
                                )
                                .changed();
                            ui.end_row();
                            ui.label("Download speed limit (--limit-rate)");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.yt_limit_rate)
                                        .hint_text("50K, 4M, or empty for unlimited"),
                                )
                                .changed();
                            ui.end_row();
                            changed |= settings_checkbox(
                                ui,
                                "Remove SponsorBlock segments",
                                &mut self.settings.yt_sponsorblock_remove,
                            );
                            ui.label("SponsorBlock mark categories (--sponsorblock-mark)");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.yt_sponsorblock_mark)
                                        .hint_text("sponsor,intro (leave empty to disable)"),
                                )
                                .changed();
                            ui.end_row();
                        });
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
                        settings_form_grid(ui, "dl_retries", |ui| {
                            ui.label("Unlimited HTTP and fragment retries");
                            changed |= ui
                                .checkbox(&mut self.settings.yt_dlp_unlimited_retries, "")
                                .on_hover_text(
                                    "Maps to yt-dlp --retries and --fragment-retries (infinite or a fixed count).",
                                )
                                .changed();
                            ui.end_row();
                            ui.label("Retry count (when not unlimited)");
                            changed |= ui
                                .add_enabled(
                                    !self.settings.yt_dlp_unlimited_retries,
                                    egui::DragValue::new(&mut self.settings.yt_dlp_retry_count)
                                        .range(1_u32..=999)
                                        .speed(1),
                                )
                                .changed();
                            ui.end_row();
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
                        settings_form_grid(ui, "dl_cookies", |ui| {
                            ui.label("Cookies path or browser");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.yt_dlp_cookies)
                                        .hint_text(r"C:\Users\you\cookies.txt"),
                                )
                                .changed();
                            ui.end_row();
                        });
                        ui.label("Impersonate (optional)");
                        ui.label(
                            RichText::new(
                                "Browser TLS fingerprint for yt-dlp, e.g. chrome. Some login-gated sites need \
                                 this with cookies (without it, you may see HTTP 410 even with a valid cookies.txt).",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        settings_form_grid(ui, "dl_impersonate", |ui| {
                            ui.label("Impersonate target");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.yt_dlp_impersonate)
                                        .hint_text("chrome"),
                                )
                                .changed();
                            ui.end_row();
                        });
                        ui.separator();
                        ui.label("Extra args (space-separated) added to each download command");
                        ui.label(
                            RichText::new(
                                "Appended after the retry flags above; add --retries etc. here only if you need to override.",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        settings_form_grid(ui, "dl_extra_args", |ui| {
                            ui.label("Extra args");
                            changed |= ui
                                .add(
                                    egui::TextEdit::multiline(&mut self.settings.yt_dlp_extra_args)
                                        .desired_rows(2)
                                        .hint_text("--concurrent-fragments 4"),
                                )
                                .changed();
                            ui.end_row();
                            changed |= settings_checkbox(ui, "Embed thumbnail", &mut self.settings.embed_thumbnail);
                            changed |= settings_checkbox(ui, "Embed metadata", &mut self.settings.yt_embed_metadata);
                            changed |= settings_checkbox(ui, "Ignore errors", &mut self.settings.yt_ignore_errors);
                            changed |= settings_checkbox(
                                ui,
                                "Restrict filenames",
                                &mut self.settings.yt_restrict_filenames,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Write info JSON",
                                &mut self.settings.yt_write_info_json,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Write auto subtitles",
                                &mut self.settings.yt_write_auto_subs,
                            );
                        });
                        ui.separator();
                        ui.label(
                            RichText::new("Effective command preview")
                                .small()
                                .color(Color32::GRAY),
                        );
                        draw_effective_command_preview(ui, &command_preview);
                        ui.separator();
                        ui.label(RichText::new("Downloader post-process").strong());
                        settings_form_grid(ui, "dl_post_process", |ui| {
                            ui.label("Post-processor args (--postprocessor-args)");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.ffmpeg_post_args)
                                        .hint_text("-movflags +faststart"),
                                )
                                .changed();
                            ui.end_row();
                            changed |= settings_checkbox(
                                ui,
                                "Enable faststart (-movflags +faststart)",
                                &mut self.settings.ffmpeg_faststart,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Remux video to mp4",
                                &mut self.settings.ffmpeg_remux_mp4,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Extract audio as mp3",
                                &mut self.settings.ffmpeg_extract_audio_mp3,
                            );
                            ui.label("Verify output has video and audio (ffprobe)");
                            ui.add_enabled_ui(!self.settings.ffmpeg_extract_audio_mp3, |ui| {
                                changed |= ui
                                    .checkbox(&mut self.settings.verify_output_video_audio, "")
                                    .changed();
                            });
                            ui.end_row();
                        });
                        if self.settings.ffmpeg_extract_audio_mp3 {
                            ui.colored_label(
                                LOG_COLOR_WARN,
                                "MP3 extraction is enabled, so remux-to-mp4 is ignored for downloads.",
                            );
                        }
                        if !self.settings.ffmpeg_extract_audio_mp3 {
                            ui.label(
                                RichText::new(
                                    "Marks the item failed if the saved file has no video or no audio track.",
                                )
                                .small()
                                .color(Color32::GRAY),
                            );
                        }
                    }
                    SettingsTab::Convert => {
                        ui.label(RichText::new("Video Converter settings").strong());
                        ui.label(
                            RichText::new(
                                "FFmpeg and ffprobe paths are configured in Settings → Shared.",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        ui.separator();
                        settings_form_grid(ui, "convert_settings", |ui| {
                            ui.label("Target codec");
                            egui::ComboBox::from_id_salt("settings_convert_target_codec")
                                .selected_text(crate::transcode::target_codec_label(
                                    &self.settings.convert_target_codec,
                                ))
                                .show_ui(ui, |ui| {
                                    for (value, label) in
                                        [("av1", "AV1"), ("hevc", "H.265"), ("h264", "H.264")]
                                    {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_target_codec,
                                                value.to_owned(),
                                                label,
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            ui.label("Remember Convert queue between sessions");
                            changed |= ui
                                .checkbox(&mut self.settings.convert_remember_queue, "")
                                .on_hover_text(
                                    "When enabled, queue items stay until you click Clear. \
                                     When off, the Convert queue is cleared each time you start the app.",
                                )
                                .changed();
                            ui.end_row();
                            changed |= settings_checkbox(
                                ui,
                                "Recursive folder scan",
                                &mut self.settings.convert_recursive,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Dry run by default",
                                &mut self.settings.convert_dry_run,
                            );
                            ui.label("Automatically start batch when paths are added");
                            changed |= ui
                                .checkbox(&mut self.settings.convert_auto_start_on_add, "")
                                .on_hover_text(
                                    "After Browse, Scan inputs, drag-and-drop, or paste paths, \
                                     start encoding when new ready items are added to the queue.",
                                )
                                .changed();
                            ui.end_row();
                            changed |= settings_checkbox(
                                ui,
                                "Delete original after success",
                                &mut self.settings.convert_delete_original,
                            );
                            ui.label("Rename output to original filename");
                            changed |= ui
                                .checkbox(&mut self.settings.convert_rename_original, "")
                                .on_hover_text(
                                    "After success, rename the encoded file back to the source \
                                     filename when it shares the output folder. Typically used with \
                                     delete original for in-place replacement.",
                                )
                                .changed();
                            ui.end_row();
                            changed |= settings_checkbox(
                                ui,
                                "Overwrite output files",
                                &mut self.settings.convert_overwrite,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Re-encode files already in the target codec",
                                &mut self.settings.convert_reencode_target,
                            );
                            ui.label("Use recommended container for target codec");
                            changed |= ui
                                .checkbox(&mut self.settings.convert_use_recommended_container, "")
                                .on_hover_text(
                                    "AV1 → MKV; H.264/H.265 → MP4. When off, outputs keep the source extension.",
                                )
                                .changed();
                            ui.end_row();
                            ui.label("Target bitrate");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.convert_target_bitrate)
                                        .hint_text("auto"),
                                )
                                .changed();
                            ui.end_row();
                            ui.label("Max width");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.convert_max_width)
                                        .range(320_u32..=7680_u32)
                                        .speed(10),
                                )
                                .changed();
                            ui.end_row();
                            ui.label("Min shrink %");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.convert_min_shrink_percent)
                                        .range(0.0_f32..=95.0_f32)
                                        .speed(0.5),
                                )
                                .changed();
                            ui.end_row();
                            ui.label("Size preset");
                            egui::ComboBox::from_id_salt("settings_av1_preset")
                                .selected_text(self.settings.convert_size_preset.clone())
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.convert_size_preset,
                                            "light".to_owned(),
                                            "light",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.convert_size_preset,
                                            "balanced".to_owned(),
                                            "balanced",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.convert_size_preset,
                                            "aggressive".to_owned(),
                                            "aggressive",
                                        )
                                        .changed();
                                });
                            ui.end_row();
                            ui.label("Encoder override");
                            egui::ComboBox::from_id_salt("settings_convert_encoder")
                                .selected_text(if self.settings.convert_encoder_override.is_empty() {
                                    "Auto".to_owned()
                                } else {
                                    self.settings.convert_encoder_override.clone()
                                })
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.convert_encoder_override,
                                            String::new(),
                                            "Auto",
                                        )
                                        .changed();
                                    for enc in crate::transcode::encoders_for_target(
                                        &self.settings.convert_target_codec,
                                    ) {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_encoder_override,
                                                enc.to_owned(),
                                                enc,
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                        });
                    }
                    SettingsTab::WebUi => {
                        ui.label(RichText::new("LAN web UI").strong());
                        ui.label(
                            RichText::new(
                                "HTTP on your local network with a shared token. Not encrypted — use only on networks you trust.",
                            )
                            .color(crate::app_ui::ALERT_WARNING_TEXT),
                        );
                        settings_form_grid(ui, "web_ui_settings", |ui| {
                            changed |= settings_checkbox(ui, "Enable web UI", &mut self.settings.web_ui_enabled);
                            ui.label("Bind address");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.web_bind_address)
                                        .hint_text("0.0.0.0:8765"),
                                )
                                .changed();
                            ui.end_row();
                        });
                        {
                            let show_token_id = ui.id().with("web_token_visible");
                            let mut show_token = ui.ctx().data_mut(|d| {
                                *d.get_temp_mut_or(show_token_id, false)
                            });
                            settings_form_grid(ui, "web_ui_token", |ui| {
                                ui.label("API token");
                                ui.horizontal(|ui| {
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
                                });
                                ui.end_row();
                            });
                            left_button_row(ui, |ui| {
                                let feedback_id = ui.id().with("web_token_copy_feedback");
                                let now = ui.input(|i| i.time);
                                let copied = ui.ctx().data(|d| {
                                    d.get_temp::<f64>(feedback_id)
                                        .is_some_and(|until| now < until)
                                });
                                if copied {
                                    ui.ctx().request_repaint();
                                }
                                button_group(ui, "web_token", |g| {
                                    let can_copy = !self.settings.web_auth_token.trim().is_empty();
                                    if copied {
                                        g.success(
                                            &format!("{} Copied!", ui_icons::STATUS_DONE),
                                            can_copy,
                                        )
                                        .on_hover_text("API token copied to clipboard");
                                    } else if g
                                        .secondary(
                                            &format!("{} Copy", ui_icons::COPY_CLIPBOARD),
                                            can_copy,
                                        )
                                        .on_hover_text("Copy API token to clipboard")
                                        .clicked()
                                    {
                                        g.ui().ctx().copy_text(self.settings.web_auth_token.clone());
                                        g.ui().ctx().data_mut(|d| {
                                            d.insert_temp(
                                                feedback_id,
                                                now + WEB_TOKEN_COPY_FEEDBACK_SECS,
                                            );
                                        });
                                        g.ui().ctx().request_repaint();
                                        self.append_log("Web API token copied to clipboard.");
                                    }
                                    if g.secondary(
                                        &format!("{} Generate new API token", ui_icons::TOKEN),
                                        true,
                                    )
                                    .clicked()
                                    {
                                        self.settings.web_auth_token =
                                            crate::config::generate_web_auth_token();
                                        changed = true;
                                        self.append_log(
                                            "New web API token generated. Copy it (Copy button) and update browsers that use the web UI.",
                                        );
                                    }
                                });
                            });
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
                                        "Binding to 0.0.0.0 listens on all network interfaces — use only on a trusted home LAN (plain HTTP, token auth).",
                                    )
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                                );
                                ui.label(
                                    RichText::new(
                                        "On other devices, use this PC's IP address instead of 127.0.0.1.",
                                    )
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                                );
                            }
                            if !self.settings.web_auth_token.trim().is_empty() {
                                let qr_target = format!(
                                    "{}?token={}",
                                    url.trim_end_matches('/'),
                                    self.settings.web_auth_token.trim()
                                );
                                ui.label(
                                    RichText::new(
                                        "Scan to open the web UI on this PC (on a phone, swap 127.0.0.1 for this PC's LAN IP):",
                                    )
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                                );
                                super::web_qr::draw_qr_code(ui, &qr_target, 128.0);
                            }
                        }
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
            self.settings.convert_max_width = self.settings.convert_max_width.clamp(320, 7680);
            self.settings.convert_min_shrink_percent =
                self.settings.convert_min_shrink_percent.clamp(0.0, 95.0);
            trim_activity_log(&mut self.log_lines, self.settings.log_max_chars);
            {
                let mut core = self.shared_core.lock();
                trim_activity_log(&mut core.log_lines, self.settings.log_max_chars);
            }
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
