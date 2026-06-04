use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{button_group, left_button_row};

use crate::pkg_version;
use crate::ui_icons;

use super::PydlApp;

impl PydlApp {
    pub(super) fn draw_about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let mut about_open = self.about_open;
        egui::Window::new("About rustdl")
            .open(&mut about_open)
            .resizable(false)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("rustdl").strong());
                ui.label(format!("Version: {}", pkg_version::VERSION));
                ui.label(
                    RichText::new(format!("Build: {}", pkg_version::BUILD_DATE))
                        .small()
                        .color(Color32::LIGHT_GRAY),
                );
                ui.separator();
                ui.label(RichText::new("Keyboard shortcuts").strong());
                ui.label(
                    RichText::new("Ctrl+Enter (Cmd+Enter on macOS): Add URLs from input")
                        .small()
                        .color(Color32::GRAY),
                );
                ui.label(
                    RichText::new("Ctrl+D (Cmd+D on macOS): Start downloads for ready items")
                        .small()
                        .color(Color32::GRAY),
                );
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
                                !self.update_check_in_progress,
                            )
                            .clicked()
                            {
                                self.start_update_check();
                            }
                            if self.update_has_update
                                && g.success(
                                    &format!(
                                        "{} Update now (open release page)",
                                        ui_icons::UPDATE_OPEN
                                    ),
                                    true,
                                )
                                .clicked()
                            {
                                self.open_release_url();
                            }
                        });
                    });
                    if self.update_check_in_progress {
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
                    ui.label(format!("Latest release: {latest}"));
                }
                ui.label(
                    RichText::new(
                        "Updater behavior: opens the latest release page for safe manual install.",
                    )
                    .small()
                    .color(Color32::GRAY),
                );
            });
        self.about_open = about_open;
    }
}
