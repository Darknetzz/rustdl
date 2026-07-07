use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{button_group, draw_keyboard_shortcut_row, left_button_row};

use crate::pkg_version;
use crate::ui_icons;

use super::PydlApp;

enum AboutLinkIcon<'a> {
    Texture(&'a egui::TextureHandle),
    Material(&'static str),
}

impl PydlApp {
    fn draw_about_link(ui: &mut egui::Ui, icon: AboutLinkIcon<'_>, label: &str, url: &str) {
        ui.spacing_mut().item_spacing.x = 4.0;
        let tint = ui.visuals().hyperlink_color;
        let icon_clicked = match icon {
            AboutLinkIcon::Texture(mark) => {
                let icon_size = egui::vec2(16.0, 16.0);
                ui.add(
                    egui::Image::new(egui::load::SizedTexture::new(mark.id(), icon_size))
                        .sense(egui::Sense::click()),
                )
                .clicked()
            }
            AboutLinkIcon::Material(glyph) => ui
                .add(
                    egui::Label::new(RichText::new(glyph).color(tint).size(16.0))
                        .sense(egui::Sense::click()),
                )
                .clicked(),
        };
        ui.hyperlink_to(label, url);
        if icon_clicked {
            if let Err(e) = crate::app_actions::open_browser(url) {
                eprintln!("rustdl: failed to open URL: {e}");
            }
        }
    }

    pub(super) fn draw_about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let mut about_open = self.about_open;
        egui::Window::new("About rustdl")
            .open(&mut about_open)
            .resizable(true)
            .default_width(480.0)
            .min_width(420.0)
            .min_height(320.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("rustdl").strong());
                ui.label(format!("Version: {}", pkg_version::VERSION));
                ui.label(
                    RichText::new(format!("Build: {}", pkg_version::build_date_local()))
                        .small()
                        .color(Color32::LIGHT_GRAY),
                );
                ui.horizontal(|ui| {
                    Self::draw_about_link(
                        ui,
                        AboutLinkIcon::Texture(&self.github_mark),
                        "Source on GitHub",
                        pkg_version::GITHUB_REPOSITORY,
                    );
                    ui.label("·");
                    Self::draw_about_link(
                        ui,
                        AboutLinkIcon::Material(ui_icons::RELEASES),
                        "Releases",
                        pkg_version::GITHUB_RELEASES,
                    );
                });
                ui.separator();
                let shortcuts_heading = ui.label(RichText::new("Keyboard shortcuts").strong());
                if self.about_scroll_to_shortcuts {
                    shortcuts_heading.scroll_to_me(Some(egui::Align::TOP));
                    self.about_scroll_to_shortcuts = false;
                }
                ui.add_space(2.0);
                let theme = self.settings.theme.clone();
                const SHORTCUTS: [(&[&str], Option<&[&str]>, &str); 7] = [
                    (&["Ctrl", "Enter"], Some(&["Cmd", "Enter"]), "Add URLs from input"),
                    (
                        &["Ctrl", "D"],
                        Some(&["Cmd", "D"]),
                        "Start downloads for ready items",
                    ),
                    (&["Ctrl", ","], Some(&["Cmd", ","]), "Open Settings"),
                    (&["Ctrl", "F"], Some(&["Cmd", "F"]), "Focus queue search"),
                    (
                        &["Ctrl", "L"],
                        Some(&["Cmd", "L"]),
                        "Show or hide activity log",
                    ),
                    (
                        &["Ctrl", "K"],
                        Some(&["Cmd", "K"]),
                        "Command palette (pause/resume, retry all, mode switch)",
                    ),
                    (&["Escape"], None, "Close dialogs and floating panels"),
                ];
                for (i, (win, mac, desc)) in SHORTCUTS.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(2.0);
                    }
                    draw_keyboard_shortcut_row(ui, &theme, win, *mac, desc);
                }
                ui.separator();
                left_button_row(ui, |ui| {
                    button_group(ui, "about_paths", |g| {
                        if g.secondary(
                            &format!("{} Open config folder", ui_icons::OPEN_FOLDER),
                            true,
                        )
                        .clicked()
                        {
                            self.open_config_folder();
                        }
                        if g.secondary(
                            &format!("{} Open activity log file", ui_icons::OPEN_FILE),
                            true,
                        )
                        .clicked()
                        {
                            self.open_activity_log_file();
                        }
                    });
                });
                ui.separator();
                ui.label(RichText::new("Updates").strong());
                ui.horizontal(|ui| {
                    left_button_row(ui, |ui| {
                        button_group(ui, "about_updates", |g| {
                            if g.secondary(
                                &format!("{} Check for updates", ui_icons::UPDATE_CHECK),
                                !self.update_check_in_progress && !self.update_download_in_progress,
                            )
                            .clicked()
                            {
                                self.start_update_check();
                            }
                            if self.update_pending_path.is_some()
                                && g.success(
                                    &format!(
                                        "{} Restart to apply update",
                                        ui_icons::UPDATE_OPEN
                                    ),
                                    !self.update_download_in_progress,
                                )
                                .clicked()
                            {
                                self.apply_pending_update_and_exit();
                            }
                            else if self.update_has_update
                                && self.update_download_asset.is_some()
                                && g.success(
                                    &format!("{} Download update", ui_icons::UPDATE_OPEN),
                                    !self.update_download_in_progress,
                                )
                                .clicked()
                            {
                                self.start_update_download();
                            }
                            else if self.update_has_update
                                && g.success(
                                    &format!(
                                        "{} Open release page",
                                        ui_icons::UPDATE_OPEN
                                    ),
                                    true,
                                )
                                .clicked()
                            {
                                self.open_release_url();
                            }
                        });
                        #[cfg(not(windows))]
                        if self.update_has_update {
                            ui.label(
                                RichText::new(
                                    "In-app download is Windows-only. Use Open release page, then download rustdl for your platform from GitHub Releases.",
                                )
                                .small()
                                .color(Color32::GRAY),
                            );
                        }
                    });
                    if self.update_check_in_progress || self.update_download_in_progress {
                        ui.spinner();
                    }
                });
                if !self.update_status_text.is_empty() {
                    ui.label(
                        RichText::new(self.update_status_text.clone())
                            .small()
                            .color(Color32::GRAY),
                    );
                }
                if let Some(latest) = &self.update_latest_version {
                    ui.label(format!("Latest GitHub release: {latest}"));
                }
                ui.label(
                    RichText::new(
                        "Checks GitHub releases for Darknetzz/rustdl. Private repos need a token in \
                         Settings → General → Backup. On Windows, Download update fetches rustdl.exe and Restart \
                         replaces the running binary.",
                    )
                    .small()
                    .color(Color32::GRAY),
                );
            });
        self.about_open = about_open;
    }
}
