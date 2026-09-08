use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{
    apply_layout_preset, bounded_ui_height, button_group, left_button_row, main_viewport_size,
};
use crate::config::{
    bump_ui_scale, export_settings_json, import_settings_json, snap_ui_scale, trim_activity_log,
    UI_SCALE_MAX, UI_SCALE_MIN, UI_SCALE_STEP,
};
use crate::profiles::{
    all_profiles, delete_user_profile, find_profile, rename_user_profile, save_user_profile,
    DownloadProfile,
};
use crate::ui_icons;

use super::{DownloadPreset, GeneralSettingsSubTab, PydlApp, SettingsTab, LOG_COLOR_WARN};

const WEB_TOKEN_COPY_FEEDBACK_SECS: f64 = 2.0;
const SETTINGS_FORM_LABEL_WIDTH: f32 = 240.0;

const DOWNLOAD_MIN_HEIGHT_OPTIONS: &[(u32, &str)] = &[
    (0, "Any"),
    (480, "480p"),
    (720, "720p"),
    (1080, "1080p"),
    (1440, "1440p"),
    (2160, "4K (2160p)"),
];

const DOWNLOAD_MIN_FPS_OPTIONS: &[(u32, &str)] = &[
    (0, "Any"),
    (24, "24"),
    (25, "25"),
    (30, "30"),
    (50, "50"),
    (60, "60"),
];

fn download_min_height_label(value: u32) -> &'static str {
    DOWNLOAD_MIN_HEIGHT_OPTIONS
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, label)| *label)
        .unwrap_or("Any")
}

fn download_min_fps_label(value: u32) -> &'static str {
    DOWNLOAD_MIN_FPS_OPTIONS
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, label)| *label)
        .unwrap_or("Any")
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

fn settings_checkbox_tooltip(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut bool,
    tooltip: &str,
) -> bool {
    ui.label(label);
    let changed = ui.checkbox(value, "").on_hover_text(tooltip).changed();
    ui.end_row();
    changed
}

fn size_limit_kind_options() -> [(&'static str, &'static str); 4] {
    use crate::convert_size_limit::{
        KIND_MAX_OUTPUT_BYTES, KIND_MAX_PERCENT_OF_SOURCE, KIND_MIN_SHRINK_PERCENT, KIND_NONE,
    };
    [
        (KIND_NONE, "Off"),
        (KIND_MIN_SHRINK_PERCENT, "Min shrink from source (%)"),
        (KIND_MAX_PERCENT_OF_SOURCE, "Max % of source size"),
        (KIND_MAX_OUTPUT_BYTES, "Max output size"),
    ]
}

fn size_limit_kind_label(kind: &str) -> &'static str {
    size_limit_kind_options()
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, label)| *label)
        .unwrap_or("Off")
}

fn size_limit_value_hint(kind: &str) -> &'static str {
    use crate::convert_size_limit::{
        KIND_MAX_OUTPUT_BYTES, KIND_MAX_PERCENT_OF_SOURCE, KIND_MIN_SHRINK_PERCENT,
    };
    match kind {
        KIND_MIN_SHRINK_PERCENT => "e.g. 50",
        KIND_MAX_PERCENT_OF_SOURCE => "e.g. 60",
        KIND_MAX_OUTPUT_BYTES => "e.g. 500M, 1.5GiB",
        _ => "",
    }
}

fn size_limit_violation_options() -> [(&'static str, &'static str); 4] {
    use crate::convert_size_limit::{
        VIOLATION_ENCODE_DELETE, VIOLATION_FAIL, VIOLATION_KEEP, VIOLATION_SKIP,
    };
    [
        (VIOLATION_SKIP, "Skip before encode (estimate)"),
        (VIOLATION_FAIL, "Fail"),
        (VIOLATION_ENCODE_DELETE, "Encode, then delete if over"),
        (VIOLATION_KEEP, "Keep anyway (warn)"),
    ]
}

fn size_limit_violation_label(violation: &str) -> &'static str {
    size_limit_violation_options()
        .iter()
        .find(|(v, _)| *v == violation)
        .map(|(_, label)| *label)
        .unwrap_or("Skip before encode (estimate)")
}

fn organize_folder_label(value: &str) -> &'static str {
    match value {
        crate::download_organize::FOLDER_UPLOADER => "By uploader / channel",
        crate::download_organize::FOLDER_PLAYLIST => "By playlist",
        crate::download_organize::FOLDER_DATE_YM => "By year / month",
        crate::download_organize::FOLDER_CUSTOM => "Custom",
        _ => "Flat (output folder only)",
    }
}

fn organize_filename_label(value: &str) -> &'static str {
    match value {
        crate::download_organize::FILENAME_DATE_TITLE_ID => "Upload date prefix",
        crate::download_organize::FILENAME_PLAYLIST_INDEX_TITLE_ID => "Playlist index prefix",
        crate::download_organize::FILENAME_TITLE_ONLY => "Title only",
        crate::download_organize::FILENAME_CUSTOM => "Custom",
        _ => "Title + video ID",
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
    pub(super) fn draw_download_retry_settings(
        &mut self,
        ui: &mut egui::Ui,
        id_salt: &str,
    ) -> bool {
        let mut changed = false;
        settings_form_grid(ui, id_salt, |ui| {
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
            ui.label("Socket timeout (seconds, 0 = yt-dlp default)");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut self.settings.yt_dlp_socket_timeout_secs)
                        .range(0_u32..=3600)
                        .speed(1),
                )
                .on_hover_text("Maps to yt-dlp --socket-timeout.")
                .changed();
            ui.end_row();
            ui.label("Sleep between retries (seconds, 0 = off)");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut self.settings.yt_dlp_retry_sleep_secs)
                        .range(0_u32..=300)
                        .speed(1),
                )
                .on_hover_text("Maps to yt-dlp --retry-sleep.")
                .changed();
            ui.end_row();
            ui.label("Auto-retry on connection errors (0–10)");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut self.settings.yt_dlp_download_auto_retries)
                        .range(0_u32..=10)
                        .speed(1),
                )
                .on_hover_text(
                    "rustdl retries the whole download on transient network errors before marking Failed, keeping partial files.",
                )
                .changed();
            ui.end_row();
        });
        ui.label(
            RichText::new(
                "Applies to each download request and to DASH/HLS fragments. \
                 Auto-retry resumes partial downloads without deleting .part files.",
            )
            .small()
            .color(Color32::GRAY),
        );
        changed
    }

    fn draw_general_appearance_settings(&mut self, ui: &mut egui::Ui, changed: &mut bool) {
        ui.label(RichText::new("Cards and queue layout").strong());
        settings_form_grid(ui, "general_appearance_cards", |ui| {
            let show_thumbnails_changed = settings_checkbox(
                ui,
                "Show thumbnails in cards",
                &mut self.settings.show_thumbnails,
            );
            *changed |= show_thumbnails_changed;
            if show_thumbnails_changed && self.settings.show_thumbnails {
                self.thumbnail_attempted.clear();
            }
            *changed |=
                settings_checkbox(ui, "Use compact cards", &mut self.settings.compact_cards);
            *changed |= settings_checkbox(
                ui,
                "Hide card subtitle/uploader",
                &mut self.settings.hide_card_subtitle,
            );
            *changed |= settings_checkbox(
                ui,
                "List layout for queue cards (denser)",
                &mut self.settings.card_list_layout,
            );
            ui.label("UI scale");
            left_button_row(ui, |ui| {
                let pct = (self.settings.ui_scale * 100.0).round() as i32;
                let at_min = self.settings.ui_scale <= UI_SCALE_MIN;
                let at_max = self.settings.ui_scale >= UI_SCALE_MAX;
                button_group(ui, "ui_scale", |g| {
                    if g.secondary("−", !at_min)
                        .on_hover_text("Decrease UI scale")
                        .clicked()
                    {
                        bump_ui_scale(&mut self.settings.ui_scale, -UI_SCALE_STEP);
                        *changed = true;
                    }
                    if g.add(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(format!("{pct:>3}%")).strong())
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_text("Reset to 100%")
                    })
                    .clicked()
                        && (self.settings.ui_scale - 1.0).abs() > f32::EPSILON
                    {
                        self.settings.ui_scale = snap_ui_scale(1.0);
                        *changed = true;
                    }
                    if g.secondary("+", !at_max)
                        .on_hover_text("Increase UI scale")
                        .clicked()
                    {
                        bump_ui_scale(&mut self.settings.ui_scale, UI_SCALE_STEP);
                        *changed = true;
                    }
                });
            });
            ui.end_row();
            ui.label("Theme");
            egui::ComboBox::from_id_salt("settings_theme")
                .selected_text(self.settings.theme.clone())
                .show_ui(ui, |ui| {
                    *changed |= ui
                        .selectable_value(&mut self.settings.theme, "dark".to_owned(), "Dark")
                        .changed();
                    *changed |= ui
                        .selectable_value(&mut self.settings.theme, "light".to_owned(), "Light")
                        .changed();
                    *changed |= ui
                        .selectable_value(&mut self.settings.theme, "system".to_owned(), "System")
                        .changed();
                });
            ui.end_row();
        });
        ui.label(RichText::new("Display limits").strong());
        settings_form_grid(ui, "general_display_limits", |ui| {
            ui.label("Max content width");
            *changed |= ui
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
        ui.label(RichText::new("Mode panel colors").strong());
        ui.label(
            RichText::new("Tint and accent stripe for Downloader and Video Converter panels.")
                .small()
                .color(crate::theme::text_hint(&self.settings.theme)),
        );
        settings_form_grid(ui, "general_mode_colors", |ui| {
            ui.label("Downloader");
            *changed |= crate::theme::draw_mode_color_controls(
                ui,
                &mut self.settings.mode_downloader_color,
                crate::theme::MODE_DOWNLOADER,
            );
            ui.end_row();
            ui.label("Video Converter");
            *changed |= crate::theme::draw_mode_color_controls(
                ui,
                &mut self.settings.mode_convert_color,
                crate::theme::MODE_CONVERT,
            );
            ui.end_row();
        });
        ui.separator();
        ui.label(RichText::new("Layout presets").strong());
        ui.label(
            RichText::new(
                "One-click display bundles (does not change download or Converter options).",
            )
            .small()
            .color(Color32::GRAY),
        );
        left_button_row(ui, |ui| {
            let vh = main_viewport_size(ui.ctx()).y;
            button_group(ui, "layout_presets", |g| {
                if g.secondary("Compact queue", true)
                    .on_hover_text("List layout, compact cards, hide subtitle")
                    .clicked()
                {
                    apply_layout_preset(&mut self.settings, "compact", Some(vh));
                    *changed = true;
                }
                if g.secondary("Review mode", true)
                    .on_hover_text("Horizontal cards with thumbnails")
                    .clicked()
                {
                    apply_layout_preset(&mut self.settings, "review", Some(vh));
                    *changed = true;
                }
                if g.secondary("Minimal", true)
                    .on_hover_text("Compact list without thumbnails")
                    .clicked()
                {
                    apply_layout_preset(&mut self.settings, "minimal", Some(vh));
                    *changed = true;
                }
            });
        });
    }

    fn draw_general_panels_log_settings(&mut self, ui: &mut egui::Ui, changed: &mut bool) {
        ui.label(RichText::new("Panel layout").strong());
        settings_form_grid(ui, "general_panel_layout", |ui| {
            let videos_docked_before = self.settings.videos_docked;
            *changed |= settings_checkbox(
                ui,
                "Dock video / Convert queue in main window",
                &mut self.settings.videos_docked,
            );
            if self.settings.videos_docked != videos_docked_before {
                self.note_videos_dock_user_choice(self.settings.videos_docked);
            }
            *changed |= settings_checkbox(
                ui,
                "Dock activity log under video queue (when queue is docked)",
                &mut self.settings.logs_docked,
            );
        });
        ui.label(RichText::new("Activity log").strong());
        settings_form_grid(ui, "general_activity_log", |ui| {
            *changed |= settings_checkbox(
                ui,
                "Autoscroll log to latest line",
                &mut self.settings.autoscroll_log,
            );
            *changed |= settings_checkbox(
                ui,
                "Relative timestamps in activity log",
                &mut self.settings.log_relative_time,
            );
            ui.label("Max log chars");
            *changed |= ui
                .add(egui::Slider::new(&mut self.settings.log_max_chars, 2_000..=200_000).integer())
                .changed();
            ui.end_row();
        });
    }

    fn draw_general_system_settings(&mut self, ui: &mut egui::Ui, changed: &mut bool) {
        ui.label(RichText::new("App behavior").strong());
        settings_form_grid(ui, "general_system_behavior", |ui| {
            *changed |= settings_checkbox_tooltip(
                ui,
                "Power save during active work",
                &mut self.settings.ui_power_save,
                "Lower UI refresh rate while downloading or converting, \
                 use denser queue rows sooner, and lighter activity-log rendering",
            );
            let tray_hover = if cfg!(target_os = "linux") {
                "Hide the window in the notification area when you minimize or \
                 click the close button; use the tray icon to show rustdl again \
                 or choose Quit to exit. On Linux this uses the \
                 StatusNotifierItem protocol (D-Bus); disabling the option may \
                 require a restart to remove the tray icon."
            } else {
                "Hide the window in the notification area when you minimize or \
                 click the close button; use the tray icon to show rustdl again \
                 or choose Quit to exit"
            };
            *changed |= settings_checkbox_tooltip(
                ui,
                "Minimize to system tray",
                &mut self.settings.minimize_to_tray,
                tray_hover,
            );
        });
        ui.label(RichText::new("Session restore").strong());
        settings_form_grid(ui, "general_session_restore", |ui| {
            ui.label("On startup");
            egui::ComboBox::from_id_salt("settings_session_restore")
                .selected_text(match self.settings.session_restore_preference.as_str() {
                    "always" => "Always restore saved queues",
                    "never" => "Never restore (start fresh)",
                    _ => "Ask each startup",
                })
                .show_ui(ui, |ui| {
                    *changed |= ui
                        .selectable_value(
                            &mut self.settings.session_restore_preference,
                            "ask".to_owned(),
                            "Ask each startup",
                        )
                        .changed();
                    *changed |= ui
                        .selectable_value(
                            &mut self.settings.session_restore_preference,
                            "always".to_owned(),
                            "Always restore saved queues",
                        )
                        .changed();
                    *changed |= ui
                        .selectable_value(
                            &mut self.settings.session_restore_preference,
                            "never".to_owned(),
                            "Never restore (start fresh)",
                        )
                        .changed();
                });
            ui.end_row();
        });
        ui.label(RichText::new("Background work priority").strong());
        ui.label(
            RichText::new(
                "Lowers yt-dlp and ffmpeg process priority so downloads and encodes \
                 are less likely to slow down other apps. Not a hard CPU/GPU cap.",
            )
            .small()
            .color(Color32::GRAY),
        );
        let mut subprocess_priority = crate::external_tools::normalize_subprocess_priority(
            &self.settings.subprocess_priority,
        );
        settings_form_grid(ui, "general_subprocess_priority", |ui| {
            ui.label("Subprocess priority");
            egui::ComboBox::from_id_salt("settings_subprocess_priority")
                .selected_text(crate::external_tools::subprocess_priority_label(
                    subprocess_priority,
                ))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (crate::external_tools::SubprocessPriority::Normal, "Normal"),
                        (
                            crate::external_tools::SubprocessPriority::BelowNormal,
                            "Below normal",
                        ),
                        (crate::external_tools::SubprocessPriority::Idle, "Idle"),
                    ] {
                        *changed |= ui
                            .selectable_value(&mut subprocess_priority, value, label)
                            .changed();
                    }
                });
            ui.end_row();
        });
        let priority_changed = subprocess_priority
            != crate::external_tools::normalize_subprocess_priority(
                &self.settings.subprocess_priority,
            );
        if priority_changed {
            self.settings.subprocess_priority =
                crate::external_tools::subprocess_priority_storage_value(subprocess_priority)
                    .to_owned();
            *changed = true;
        }
    }

    fn draw_general_tools_settings(
        &mut self,
        ui: &mut egui::Ui,
        changed: &mut bool,
        executable_paths_changed: &mut bool,
    ) {
        ui.label(RichText::new("Shared executables").strong());
        ui.label("Used by the downloader and Video Converter. Leave empty to use PATH.");
        settings_form_grid(ui, "general_executables", |ui| {
            ui.label("ffmpeg");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.settings.ffmpeg_path)
                    .hint_text("ffmpeg.exe or full path"),
            );
            *changed |= resp.changed();
            *executable_paths_changed |= resp.changed();
            ui.end_row();
            ui.label("ffprobe");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.settings.ffprobe_path)
                    .hint_text("ffprobe.exe or full path"),
            );
            *changed |= resp.changed();
            *executable_paths_changed |= resp.changed();
            ui.end_row();
        });
    }

    fn draw_general_backup_settings(&mut self, ui: &mut egui::Ui, changed: &mut bool) {
        ui.label(RichText::new("GitHub releases").strong());
        ui.label(
            RichText::new(
                "Personal access token for About → Check for updates when the repository is private. \
                 Read access to repository contents is enough. You can also set RUSTDL_GITHUB_TOKEN.",
            )
            .small()
            .color(Color32::GRAY),
        );
        settings_form_grid(ui, "general_github", |ui| {
            ui.label("GitHub token");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.settings.github_token)
                    .password(true)
                    .hint_text("ghp_… or github_pat_…"),
            );
            *changed |= resp.changed();
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
                        .button(format!("{} Export settings", ui_icons::EXPORT))
                        .clicked()
                    {
                        export_settings = true;
                    }
                    if ui
                        .button(format!("{} Import settings", ui_icons::IMPORT_FILE))
                        .clicked()
                    {
                        import_settings = true;
                    }
                });
                if g.secondary(&format!("{} Reset to defaults", ui_icons::RESET), true)
                    .clicked()
                {
                    let keep_output = self.settings.output_dir.clone();
                    self.settings = crate::config::AppSettings::default();
                    self.settings.output_dir = keep_output.clone();
                    self.output_dir = keep_output;
                    *changed = true;
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
                        Err(e) => self.append_log(&format!("Export settings failed: {e:#}")),
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
                            self.worker_count = self.settings.worker_count.clamp(1, 6);
                            self.settings_tab =
                                super::settings_tab_from_str(&self.settings.settings_tab);
                            self.general_settings_subtab = super::general_settings_subtab_from_str(
                                &self.settings.settings_general_subtab,
                            );
                            *changed = true;
                            self.append_log(&format!(
                                "Imported settings from {}",
                                path.to_string_lossy()
                            ));
                        }
                        Err(e) => self.append_log(&format!("Import settings failed: {e:#}")),
                    }
                }
            }
        });
    }

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
            .min_width(480.0)
            .min_height(400.0)
            .show(ctx, |ui| {
                let prev_settings_tab = self.settings_tab;
                left_button_row(ui, |ui| {
                    button_group(ui, "settings_tabs", |g| {
                    g.add(|ui| {
                        ui.selectable_value(
                            &mut self.settings_tab,
                            SettingsTab::General,
                            format!("{} General", ui_icons::TAB_SHARED),
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
                            format!("{} Converter", ui_icons::TAB_AV1),
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
                if self.settings_tab == SettingsTab::General {
                    let prev_general_subtab = self.general_settings_subtab;
                    left_button_row(ui, |ui| {
                        button_group(ui, "general_settings_subtabs", |g| {
                            for subtab in [
                                GeneralSettingsSubTab::Appearance,
                                GeneralSettingsSubTab::PanelsLog,
                                GeneralSettingsSubTab::System,
                                GeneralSettingsSubTab::Tools,
                                GeneralSettingsSubTab::Backup,
                            ] {
                                g.add(|ui| {
                                    ui.selectable_value(
                                        &mut self.general_settings_subtab,
                                        subtab,
                                        super::general_settings_subtab_menu_label(subtab),
                                    )
                                });
                            }
                        });
                    });
                    if self.general_settings_subtab != prev_general_subtab {
                        self.sync_general_settings_subtab_to_disk();
                    }
                }
                ui.separator();
                let scroll_id = match self.settings_tab {
                    SettingsTab::General => format!(
                        "general_{}",
                        super::general_settings_subtab_to_str(self.general_settings_subtab)
                    ),
                    _ => super::settings_tab_to_str(self.settings_tab).to_owned(),
                };
                let scroll_h = bounded_ui_height(ui, 240.0).max(240.0);
                egui::ScrollArea::vertical()
                    .id_salt(scroll_id)
                    .auto_shrink([false, false])
                    .max_height(scroll_h)
                    .drag_to_scroll(true)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match self.settings_tab {
                    SettingsTab::General => match self.general_settings_subtab {
                        GeneralSettingsSubTab::Appearance => {
                            self.draw_general_appearance_settings(ui, &mut changed);
                        }
                        GeneralSettingsSubTab::PanelsLog => {
                            self.draw_general_panels_log_settings(ui, &mut changed);
                        }
                        GeneralSettingsSubTab::System => {
                            self.draw_general_system_settings(ui, &mut changed);
                        }
                        GeneralSettingsSubTab::Tools => {
                            self.draw_general_tools_settings(
                                ui,
                                &mut changed,
                                &mut executable_paths_changed,
                            );
                        }
                        GeneralSettingsSubTab::Backup => {
                            self.draw_general_backup_settings(ui, &mut changed);
                        }
                    },
                    SettingsTab::Downloader => {
                        egui::CollapsingHeader::new("Output & behavior")
                            .default_open(true)
                            .show(ui, |ui| {
                        ui.label(RichText::new("Output folder").strong());
                        settings_form_grid(ui, "dl_output_folder", |ui| {
                            ui.label("Folder");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.output_dir)
                                        .hint_text("Downloads folder path"),
                                )
                                .changed();
                            ui.end_row();
                        });
                        left_button_row(ui, |ui| {
                            button_group(ui, "settings_dl_output", |g| {
                                if g.secondary(&format!("{} Browse…", ui_icons::OPEN_FOLDER), true)
                                    .clicked()
                                {
                                    let mut dialog =
                                        rfd::FileDialog::new().set_title("Choose output folder");
                                    let trimmed = self.output_dir.trim();
                                    if !trimmed.is_empty() {
                                        let path = std::path::Path::new(trimmed);
                                        if path.is_dir() {
                                            dialog = dialog.set_directory(path);
                                        } else if let Some(parent) =
                                            path.parent().filter(|p| p.is_dir())
                                        {
                                            dialog = dialog.set_directory(parent);
                                        }
                                    }
                                    if let Some(path) = dialog.pick_folder() {
                                        self.output_dir = path.to_string_lossy().to_string();
                                        changed = true;
                                    }
                                }
                            });
                        });
                        ui.add_space(6.0);
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
                            ui.label("Scheduled start (local HH:MM)");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(
                                        &mut self.settings.scheduled_download_start,
                                    )
                                    .hint_text("empty = disabled, e.g. 08:30"),
                                )
                                .changed();
                            ui.end_row();
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
                        });
                        ui.add_space(4.0);
                        egui::CollapsingHeader::new("Watch folder & templates")
                            .default_open(false)
                            .show(ui, |ui| {
                        ui.label(RichText::new("Watch folder").strong());
                        ui.label(
                            RichText::new(
                                "Auto-enqueue URLs from new .url / .txt files dropped in a folder (polled every 5s).",
                            )
                            .small()
                            .color(Color32::GRAY),
                        );
                        settings_form_grid(ui, "dl_watch_folder", |ui| {
                            changed |= settings_checkbox(
                                ui,
                                "Enable downloader watch folder",
                                &mut self.settings.watch_folder_enabled,
                            );
                            ui.label("Folder path");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.watch_folder_path)
                                        .hint_text(r"C:\Downloads\inbox"),
                                )
                                .changed();
                            ui.end_row();
                        });
                        ui.separator();
                        ui.label(RichText::new("Quality watchlist").strong());
                        changed |= self.draw_watchlist_settings_section(ui);
                        ui.separator();
                        ui.label(RichText::new("Queue templates").strong());
                        settings_form_grid(ui, "queue_templates", |ui| {
                            ui.label("Template name");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.queue_template_name_buf)
                                        .hint_text("my-playlist"),
                                )
                                .changed();
                            ui.end_row();
                        });
                        left_button_row(ui, |ui| {
                            button_group(ui, "queue_templates_actions", |g| {
                                let name = self.queue_template_name_buf.trim();
                                if g.secondary(
                                    &format!("{} Save current queue", ui_icons::SAVE),
                                    !self.items.is_empty() && !name.is_empty(),
                                )
                                .clicked()
                                {
                                    let template = crate::queue_templates::queue_template_from_items(
                                        name,
                                        &self.items,
                                    );
                                    match crate::queue_templates::save_queue_template(&template) {
                                        Ok(()) => self.append_log(&format!(
                                            "Saved queue template \"{name}\"."
                                        )),
                                        Err(e) => {
                                            self.append_log(&format!("Save template failed: {e:#}"))
                                        }
                                    }
                                }
                            });
                        });
                        let templates = crate::queue_templates::list_queue_templates();
                        if !templates.is_empty() {
                            ui.label("Saved templates:");
                            for name in &templates {
                                ui.horizontal(|ui| {
                                    if ui.button(format!("{} Load {name}", ui_icons::IMPORT_FILE)).clicked() {
                                        if let Ok(tpl) = crate::queue_templates::load_queue_template(name) {
                                            let urls = crate::queue_templates::template_item_urls(&tpl);
                                            self.download_core_action(|core| {
                                                let stats = core.queue_urls_for_resolve(urls);
                                                if stats.accepted == 0 {
                                                    core.append_log(&format!(
                                                        "Queue template \"{name}\": no new URLs added \
                                                         ({} duplicate(s), {} invalid).",
                                                        stats.duplicate_in_input
                                                            + stats.duplicate_existing,
                                                        stats.invalid
                                                    ));
                                                }
                                            });
                                            self.append_log(&format!("Loaded queue template \"{name}\"."));
                                        }
                                    }
                                });
                            }
                        }
                        });
                        ui.add_space(4.0);
                        egui::CollapsingHeader::new("Profiles & executables")
                            .default_open(true)
                            .show(ui, |ui| {
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
                        });
                        ui.add_space(4.0);
                        egui::CollapsingHeader::new("Organize downloads")
                            .default_open(true)
                            .show(ui, |ui| {
                        ui.label(
                            "Folder layout and filenames are passed to yt-dlp as the -o template.",
                        );
                        left_button_row(ui, |ui| {
                            button_group(ui, "organize_presets", |g| {
                                if g
                                    .secondary("Flat", true)
                                    .clicked()
                                {
                                    crate::download_organize::apply_organize_preset(&mut self.settings, "flat");
                                    changed = true;
                                }
                                if g
                                    .secondary("By uploader / channel", true)
                                    .clicked()
                                {
                                    crate::download_organize::apply_organize_preset(&mut self.settings, "uploader");
                                    changed = true;
                                }
                                if g
                                    .secondary("Playlist", true)
                                    .clicked()
                                {
                                    crate::download_organize::apply_organize_preset(&mut self.settings, "playlist");
                                    changed = true;
                                }
                                if g
                                    .secondary("By date", true)
                                    .clicked()
                                {
                                    crate::download_organize::apply_organize_preset(&mut self.settings, "date");
                                    changed = true;
                                }
                            });
                        });
                        let organize_custom = crate::download_organize::uses_custom_template(
                            &self.settings,
                        );
                        settings_form_grid(ui, "dl_organize", |ui| {
                            ui.label("Folder layout");
                            egui::ComboBox::from_id_salt("settings_organize_folder")
                                .selected_text(organize_folder_label(
                                    &self.settings.download_organize_folder,
                                ))
                                .show_ui(ui, |ui| {
                                    for (v, label) in [
                                        (
                                            crate::download_organize::FOLDER_FLAT,
                                            "Flat (output folder only)",
                                        ),
                                        (
                                            crate::download_organize::FOLDER_UPLOADER,
                                            "By uploader / channel",
                                        ),
                                        (
                                            crate::download_organize::FOLDER_PLAYLIST,
                                            "By playlist",
                                        ),
                                        (
                                            crate::download_organize::FOLDER_DATE_YM,
                                            "By year / month",
                                        ),
                                        (crate::download_organize::FOLDER_CUSTOM, "Custom"),
                                    ] {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.download_organize_folder,
                                                v.to_owned(),
                                                label,
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            ui.label("Filename style");
                            egui::ComboBox::from_id_salt("settings_organize_filename")
                                .selected_text(organize_filename_label(
                                    &self.settings.download_organize_filename,
                                ))
                                .show_ui(ui, |ui| {
                                    for (v, label) in [
                                        (
                                            crate::download_organize::FILENAME_TITLE_ID,
                                            "Title + video ID",
                                        ),
                                        (
                                            crate::download_organize::FILENAME_DATE_TITLE_ID,
                                            "Upload date prefix",
                                        ),
                                        (
                                            crate::download_organize::FILENAME_PLAYLIST_INDEX_TITLE_ID,
                                            "Playlist index prefix",
                                        ),
                                        (
                                            crate::download_organize::FILENAME_TITLE_ONLY,
                                            "Title only",
                                        ),
                                        (crate::download_organize::FILENAME_CUSTOM, "Custom"),
                                    ] {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.download_organize_filename,
                                                v.to_owned(),
                                                label,
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            if organize_custom {
                                ui.label("Custom output template (-o)");
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(
                                            &mut self.settings.output_filename_template,
                                        )
                                        .hint_text(crate::config::DEFAULT_OUTPUT_FILENAME_TEMPLATE),
                                    )
                                    .changed();
                                ui.end_row();
                            }
                            changed |= settings_checkbox(
                                ui,
                                "After download, move files into organize layout",
                                &mut self.settings.post_download_organize,
                            );
                            ui.label("Filename find (after download)")
                                .on_hover_text(
                                    "Replace text in the downloaded filename stem. Runs after organize move. \
                                     Leave empty to skip.",
                                );
                            changed |= ui
                                .add(egui::TextEdit::singleline(
                                    &mut self.settings.post_download_filename_find,
                                ))
                                .changed();
                            ui.end_row();
                            ui.label("Filename replace");
                            changed |= ui
                                .add(egui::TextEdit::singleline(
                                    &mut self.settings.post_download_filename_replace,
                                ))
                                .changed();
                            ui.end_row();
                            ui.label("Example path");
                            ui.label(
                                crate::download_organize::example_output_path(
                                    &self.settings,
                                    &self.output_dir,
                                ),
                            );
                            ui.end_row();
                        });
                        if self.settings.download_organize_filename
                            == crate::download_organize::FILENAME_TITLE_ONLY
                        {
                            ui.colored_label(
                                LOG_COLOR_WARN,
                                "Title-only filenames may make it harder to match Done downloads \
                                 after restart.",
                            );
                        }
                        });
                        ui.add_space(4.0);
                        egui::CollapsingHeader::new("Quality & network")
                            .default_open(true)
                            .show(ui, |ui| {
                        ui.label(RichText::new("Output and quality").strong());
                        settings_form_grid(ui, "dl_output_quality", |ui| {
                            ui.label("Effective -o template");
                            ui.label(crate::ytdlp_download_args::output_filename_template(
                                &self.settings,
                            ));
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
                                        .hint_text("bestvideo*+bestaudio/best"),
                                    )
                                    .changed();
                                ui.end_row();
                            }
                            ui.label("Minimum height");
                            egui::ComboBox::from_id_salt("settings_download_min_height")
                                .selected_text(download_min_height_label(
                                    self.settings.download_min_height,
                                ))
                                .show_ui(ui, |ui| {
                                    for (v, label) in DOWNLOAD_MIN_HEIGHT_OPTIONS {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.download_min_height,
                                                *v,
                                                *label,
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            ui.label("Minimum FPS");
                            egui::ComboBox::from_id_salt("settings_download_min_fps")
                                .selected_text(download_min_fps_label(
                                    self.settings.download_min_fps,
                                ))
                                .show_ui(ui, |ui| {
                                    for (v, label) in DOWNLOAD_MIN_FPS_OPTIONS {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.download_min_fps,
                                                *v,
                                                *label,
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            if self.settings.download_min_height > 0
                                || self.settings.download_min_fps > 0
                            {
                                ui.label("Effective -f filter");
                                let fmt = crate::ytdlp_download_args::quality_format_args(
                                    &self.settings,
                                )
                                .into_iter()
                                .nth(1)
                                .unwrap_or_default();
                                ui.label(
                                    RichText::new(fmt)
                                        .small()
                                        .monospace()
                                        .color(crate::theme::TEXT_MUTED),
                                );
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
                        changed |= self.draw_download_retry_settings(ui, "dl_retries");
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
                        left_button_row(ui, |ui| {
                            button_group(ui, "cookie_test", |g| {
                                if g.secondary(&format!("{} Test cookies", ui_icons::RECHECK), true)
                                    .on_hover_text("Probe yt-dlp with current cookies against YouTube")
                                    .clicked()
                                {
                                    let result = crate::ytdlp::cookie_health_probe(
                                        &self.settings.yt_dlp_path,
                                        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                                        &self.settings.yt_dlp_cookies,
                                        &self.settings.yt_dlp_impersonate,
                                    );
                                    self.append_log(&format!(
                                        "Cookie check: {}",
                                        result.message
                                    ));
                                }
                            });
                        });
                        });
                        ui.add_space(4.0);
                        egui::CollapsingHeader::new("Advanced")
                            .default_open(false)
                            .show(ui, |ui| {
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
                        ui.label(RichText::new("yt-dlp options").strong());
                        settings_form_grid(ui, "dl_ytdlp_options", |ui| {
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
                        });
                    }
                    SettingsTab::Convert => {
                        ui.label(RichText::new("Video Converter settings").strong());
                        ui.label(
                            RichText::new(
                                "FFmpeg and ffprobe paths are configured in Settings → General → Tools.",
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
                            ui.label("Output folder");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(
                                        &mut self.settings.convert_output_dir,
                                    )
                                    .hint_text("same folder as each input (default)"),
                                )
                                .on_hover_text(
                                    "When empty, encoded files are written next to each source file. \
                                     Set a folder to send all batch outputs there instead.",
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
                                    "After Add folder, Add file(s), Scan inputs, drag-and-drop, \
                                     or paste paths, start encoding when new ready items are added \
                                     to the queue.",
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
                                     filename when it shares the output folder (keeps the new container extension). \
                                     Typically used with \
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
                            ui.label("Output container");
                            {
                                let recommended = crate::transcode::recommended_container_for_target(
                                    &self.settings.convert_target_codec,
                                )
                                .to_ascii_uppercase();
                                let selected_label = match self.settings.convert_container.as_str() {
                                    "source" => "Source (keep original)".to_owned(),
                                    "mkv" => "MKV".to_owned(),
                                    "mp4" => "MP4".to_owned(),
                                    "webm" => "WebM".to_owned(),
                                    _ => format!("Auto ({recommended})"),
                                };
                                egui::ComboBox::from_id_salt("settings_convert_container")
                                    .selected_text(&selected_label)
                                    .show_ui(ui, |ui| {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_container,
                                                "auto".to_owned(),
                                                format!("Auto ({recommended})"),
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_container,
                                                "source".to_owned(),
                                                "Source (keep original)",
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_container,
                                                "mkv".to_owned(),
                                                "MKV",
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_container,
                                                "mp4".to_owned(),
                                                "MP4",
                                            )
                                            .changed();
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_container,
                                                "webm".to_owned(),
                                                "WebM",
                                            )
                                            .changed();
                                    });
                            }
                            ui.end_row();
                            ui.label("Rate control");
                            let rate_label =
                                if crate::config::convert_rate_control_is_crf(
                                    &self.settings.convert_rate_control,
                                ) {
                                    "Quality (CRF)"
                                } else {
                                    "Bitrate"
                                };
                            egui::ComboBox::from_id_salt("settings_convert_rate_control")
                                .selected_text(rate_label)
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.convert_rate_control,
                                            "bitrate".to_owned(),
                                            "Bitrate",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.convert_rate_control,
                                            "crf".to_owned(),
                                            "Quality (CRF)",
                                        )
                                        .changed();
                                })
                                .response
                                .on_hover_text(
                                    "Bitrate keeps a target -b:v. Quality uses -crf on CPU encoders, -cq on NVIDIA, and QP on AMD.",
                                );
                            ui.end_row();
                            if crate::config::convert_rate_control_is_crf(
                                &self.settings.convert_rate_control,
                            ) {
                                ui.label("CRF");
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut self.settings.convert_crf)
                                            .range(0_u32..=crate::config::CONVERT_CRF_MAX)
                                            .speed(1),
                                    )
                                    .on_hover_text(
                                        "Lower is higher quality and larger files. Software encoders use ffmpeg -crf. \
                                         NVIDIA uses -cq; AMD uses QP. Typical H.264 ~18–28, H.265 ~24–32, AV1 ~20–40.",
                                    )
                                    .changed();
                            } else {
                                ui.label("Target bitrate");
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(
                                            &mut self.settings.convert_target_bitrate,
                                        )
                                        .hint_text("auto"),
                                    )
                                    .changed();
                                ui.end_row();
                                ui.label("");
                                changed |= ui
                                    .checkbox(
                                        &mut self.settings.convert_cap_bitrate_to_source,
                                        "Cap bitrate to source",
                                    )
                                    .on_hover_text(
                                        "Never set -b:v / -maxrate above the probed source bitrate. \
                                         Ignored in Quality (CRF) mode.",
                                    )
                                    .changed();
                            }
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
                            ui.label("Output size limit");
                            egui::ComboBox::from_id_salt("settings_convert_size_limit_kind")
                                .selected_text(size_limit_kind_label(
                                    &self.settings.convert_size_limit_kind,
                                ))
                                .show_ui(ui, |ui| {
                                    for (kind, label) in size_limit_kind_options() {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_size_limit_kind,
                                                kind.to_owned(),
                                                label,
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            if self.settings.convert_size_limit_kind
                                != crate::convert_size_limit::KIND_NONE
                            {
                                ui.label("Limit value");
                                changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(
                                            &mut self.settings.convert_size_limit_value,
                                        )
                                        .hint_text(size_limit_value_hint(
                                            &self.settings.convert_size_limit_kind,
                                        )),
                                    )
                                    .changed();
                                ui.end_row();
                                ui.label("If limit exceeded");
                                egui::ComboBox::from_id_salt(
                                    "settings_convert_size_limit_violation",
                                )
                                .selected_text(size_limit_violation_label(
                                    &self.settings.convert_size_limit_violation,
                                ))
                                .show_ui(ui, |ui| {
                                    for (action, label) in size_limit_violation_options() {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_size_limit_violation,
                                                action.to_owned(),
                                                label,
                                            )
                                            .changed();
                                    }
                                });
                                ui.end_row();
                            }
                            ui.label("Size preset");
                            egui::ComboBox::from_id_salt("settings_convert_preset")
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
                            ui.label("CPU threads");
                            let max_cpus = crate::external_tools::logical_cpu_count();
                            let mut cpu_threads = self.settings.convert_cpu_threads;
                            let cpu_slider_changed = ui
                                .add(
                                    egui::Slider::new(&mut cpu_threads, 0..=max_cpus)
                                        .integer()
                                        .custom_formatter(|n, _| {
                                            if n.round() as u32 == 0 {
                                                "auto".to_owned()
                                            } else {
                                                format!("{}", n.round() as u32)
                                            }
                                        }),
                                )
                                .on_hover_text(format!(
                                    "Limit ffmpeg decode/encode threads per job (0 = auto: ~75% of cores split across parallel jobs, up to 4 per job; manual max {max_cpus})."
                                ))
                                .changed();
                            if cpu_slider_changed {
                                self.settings.convert_cpu_threads = cpu_threads;
                            }
                            changed |= cpu_slider_changed;
                            ui.end_row();
                            ui.label("Parallel conversions");
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut self.settings.convert_parallel, 1..=6)
                                        .integer(),
                                )
                                .on_hover_text(
                                    "Number of ffmpeg transcodes to run at once during a batch.",
                                )
                                .changed();
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
                        ui.separator();
                        ui.label(RichText::new("Post-encode options").strong());
                        settings_form_grid(ui, "convert_post_encode", |ui| {
                            ui.label("Subtitle handling (during encode)")
                                .on_hover_text(
                                    "During encode: copy subtitle streams into the output file or burn the first \
                                     subtitle track into the video. Sidecar copy below runs after encode.",
                                );
                            egui::ComboBox::from_id_salt("settings_convert_subtitle_mode")
                                .selected_text(match self.settings.convert_subtitle_mode.as_str() {
                                    "soft" => "Soft copy (in container)",
                                    "burn" => "Burn into video",
                                    _ => "None",
                                })
                                .show_ui(ui, |ui| {
                                    for (value, label) in [
                                        ("none", "None"),
                                        ("soft", "Soft copy (in container)"),
                                        ("burn", "Burn into video (first subtitle track)"),
                                    ] {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_subtitle_mode,
                                                value.to_owned(),
                                                label,
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            ui.label("Extract audio sidecar").on_hover_text(
                                "After a successful encode, write a separate audio-only file next to the output.",
                            );
                            egui::ComboBox::from_id_salt("settings_convert_audio_extract")
                                .selected_text(match self.settings.convert_audio_extract.as_str() {
                                    "flac" => "FLAC",
                                    "aac" => "AAC",
                                    "opus" => "Opus",
                                    _ => "None",
                                })
                                .show_ui(ui, |ui| {
                                    for (value, label) in
                                        [("none", "None"), ("flac", "FLAC"), ("aac", "AAC"), ("opus", "Opus")]
                                    {
                                        changed |= ui
                                            .selectable_value(
                                                &mut self.settings.convert_audio_extract,
                                                value.to_owned(),
                                                label,
                                            )
                                            .changed();
                                    }
                                });
                            ui.end_row();
                            changed |= settings_checkbox(
                                ui,
                                "Copy subtitle sidecars after encode",
                                &mut self.settings.convert_copy_subtitles,
                            );
                            changed |= settings_checkbox(
                                ui,
                                "Write SHA-256 checksum sidecar",
                                &mut self.settings.convert_write_checksum,
                            );
                            ui.label("Filename find (after encode)")
                                .on_hover_text(
                                    "Replace text in the encoded output filename stem. Runs before other post-encode \
                                     steps. Leave empty to skip.",
                                );
                            changed |= ui
                                .add(egui::TextEdit::singleline(
                                    &mut self.settings.convert_post_filename_find,
                                ))
                                .changed();
                            ui.end_row();
                            ui.label("Filename replace");
                            changed |= ui
                                .add(egui::TextEdit::singleline(
                                    &mut self.settings.convert_post_filename_replace,
                                ))
                                .changed();
                            ui.end_row();
                            ui.label("Move output to subfolder");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(
                                        &mut self.settings.convert_post_move_subfolder,
                                    )
                                    .hint_text("encoded (optional)"),
                                )
                                .changed();
                            ui.end_row();
                            ui.label("Max hardware encodes");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.convert_max_hw_encodes)
                                        .range(0..=6),
                                )
                                .on_hover_text(
                                    "Cap concurrent GPU encodes when parallel conversions > 1 (0 = unlimited).",
                                )
                                .changed();
                            ui.end_row();
                        });
                        ui.separator();
                        ui.label(RichText::new("Convert presets").strong());
                        left_button_row(ui, |ui| {
                            button_group(ui, "convert_presets", |g| {
                                for preset in crate::convert_presets::builtin_convert_presets() {
                                    if g.secondary(&preset.name, true).clicked() {
                                        preset.fields.apply_to(&mut self.settings);
                                        changed = true;
                                        self.append_log(&format!(
                                            "Applied convert preset \"{}\".",
                                            preset.name
                                        ));
                                    }
                                }
                            });
                        });
                        ui.separator();
                        ui.label(RichText::new("Convert watch folder").strong());
                        settings_form_grid(ui, "convert_watch_folder", |ui| {
                            changed |= settings_checkbox(
                                ui,
                                "Enable convert watch folder",
                                &mut self.settings.convert_watch_folder_enabled,
                            );
                            ui.label("Folder path");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(
                                        &mut self.settings.convert_watch_folder_path,
                                    )
                                    .hint_text(r"C:\Videos\inbox"),
                                )
                                .changed();
                            ui.end_row();
                        });
                    }
                    SettingsTab::WebUi => {
                        ui.label(RichText::new("LAN web UI").strong());
                        ui.label(
                            RichText::new(
                                "HTTP on your local network with a shared token. Optional TLS certificate paths enable HTTPS when both files exist.",
                            )
                            .color(crate::app_ui::ALERT_WARNING_TEXT),
                        );
                        if let Some(err) = &self.web_server_start_error {
                            super::alert_danger(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("Web UI is not running: {err}"))
                                        .color(crate::app_ui::ALERT_DANGER_TEXT),
                                );
                            });
                            ui.add_space(4.0);
                        }
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
                            ui.label("TLS certificate");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.web_tls_cert_path)
                                        .hint_text("fullchain.pem (optional)"),
                                )
                                .changed();
                            ui.end_row();
                            ui.label("TLS private key");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.settings.web_tls_key_path)
                                        .hint_text("privkey.pem (optional)"),
                                )
                                .changed();
                            ui.end_row();
                            left_button_row(ui, |ui| {
                                button_group(ui, "web_tls_browse", |g| {
                                    if g
                                        .secondary(&format!("{} Browse cert…", ui_icons::OPEN_FILE), true)
                                        .clicked()
                                    {
                                        if let Some(path) = rfd::FileDialog::new()
                                            .add_filter("Certificate", &["pem", "crt"])
                                            .pick_file()
                                        {
                                            self.settings.web_tls_cert_path =
                                                path.to_string_lossy().into_owned();
                                            changed = true;
                                        }
                                    }
                                    if g
                                        .secondary(&format!("{} Browse key…", ui_icons::OPEN_FILE), true)
                                        .clicked()
                                    {
                                        if let Some(path) = rfd::FileDialog::new()
                                            .add_filter("Private key", &["pem", "key"])
                                            .pick_file()
                                        {
                                            self.settings.web_tls_key_path =
                                                path.to_string_lossy().into_owned();
                                            changed = true;
                                        }
                                    }
                                });
                            });
                            if let Err(err) = crate::config::validate_web_tls_settings(&self.settings)
                            {
                                ui.label(
                                    RichText::new(err).small().color(crate::app_ui::ALERT_DANGER_TEXT),
                                );
                            } else if crate::config::web_tls_enabled(&self.settings) {
                                ui.label(
                                    RichText::new("TLS enabled — web UI serves HTTPS when running.")
                                        .small()
                                        .color(crate::theme::TEXT_MUTED),
                                );
                            }
                            ui.end_row();
                            ui.label("IP whitelist");
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(
                                        "Optional. One IP or CIDR per line; matching clients skip the API token. \
                                         Default includes 127.0.0.1 and ::1 for this PC.",
                                    )
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                                );
                                let mut whitelist_body =
                                    self.settings.web_auth_ip_whitelist.join("\n");
                                if ui
                                    .add(
                                        egui::TextEdit::multiline(&mut whitelist_body)
                                            .desired_rows(4)
                                            .hint_text("127.0.0.1\n192.168.1.0/24"),
                                    )
                                    .changed()
                                {
                                    self.settings.web_auth_ip_whitelist = whitelist_body
                                        .lines()
                                        .map(str::trim)
                                        .filter(|line| !line.is_empty())
                                        .map(str::to_owned)
                                        .collect();
                                    changed = true;
                                }
                            });
                            ui.end_row();
                            changed |= settings_checkbox(
                                ui,
                                "Browser notifications on session complete",
                                &mut self.settings.web_browser_notifications,
                            );
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
                                crate::service::web::web_ui_browser_url(&self.settings);
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
            self.settings.worker_count = self.worker_count;
            self.settings.output_dir = self.output_dir.clone();
            crate::config::normalize_settings(&mut self.settings);
            self.worker_count = self.settings.worker_count;
            self.output_dir = self.settings.output_dir.clone();
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
