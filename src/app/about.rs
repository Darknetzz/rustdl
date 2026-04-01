use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{secondary_button, success_button};

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
                ui.separator();
                ui.label(RichText::new("Updates").strong());
                ui.horizontal_wrapped(|ui| {
                    if secondary_button(
                        ui,
                        &format!("{} Check for updates", ui_icons::UPDATE_CHECK),
                        !self.update_check_in_progress,
                    )
                    .clicked()
                    {
                        self.start_update_check();
                    }
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
                if self.update_has_update
                    && success_button(
                        ui,
                        &format!("{} Update now (open release page)", ui_icons::UPDATE_OPEN),
                        true,
                    )
                    .clicked()
                {
                    self.open_release_url();
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
