//! Quality watchlist UI (downloader mode + Settings).

use eframe::egui::{self, RichText};

use crate::app_ui::{button_group, left_button_row, with_full_width};
use crate::theme::TEXT_MUTED;
use crate::ui_icons;
use crate::watchlist::format_resolution_height;

use super::PydlApp;

impl PydlApp {
    pub(super) fn draw_watchlist_collapsible(&mut self, ui: &mut egui::Ui) {
        if !self.settings.watchlist_enabled && self.watchlist.entries.is_empty() {
            return;
        }
        let id = egui::Id::new("rustdl_watchlist_collapsible");
        let count = self.watchlist.entries.len();
        let improved = self
            .watchlist
            .entries
            .iter()
            .filter(|e| e.improved_pending)
            .count();
        let heading = if improved > 0 {
            format!(
                "{} Quality watchlist ({count}) — {improved} improved",
                ui_icons::WATCHLIST
            )
        } else {
            format!("{} Quality watchlist ({count})", ui_icons::WATCHLIST)
        };
        egui::CollapsingHeader::new(RichText::new(heading).strong())
            .id_salt("rustdl_watchlist_header")
            .default_open(improved > 0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Re-check URLs on a schedule; get notified when a higher max resolution appears.",
                    )
                    .small()
                    .color(TEXT_MUTED),
                );
                ui.add_space(4.0);
                if self.watchlist.entries.is_empty() {
                    ui.label(
                        RichText::new(
                            "Add a Done download via Verify → Watch for better quality, or paste a URL in Settings → Downloader → Quality watchlist.",
                        )
                        .small()
                        .color(TEXT_MUTED),
                    );
                } else {
                    self.draw_watchlist_entry_table(ui);
                }
                ui.add_space(4.0);
                left_button_row(ui, |ui| {
                    button_group(ui, "watchlist_quick", |g| {
                        if g.secondary("Check now", self.has_yt_dlp)
                            .on_hover_text("Probe all active watchlist URLs now")
                            .on_disabled_hover_text("yt-dlp is required.")
                            .clicked()
                        {
                            self.probe_watchlist_now();
                        }
                        if g.secondary(&format!("{} Settings", ui_icons::SETTINGS), true)
                            .on_hover_text("Open Settings → Downloader → Quality watchlist")
                            .clicked()
                        {
                            self.settings_open = true;
                            self.settings_tab = super::SettingsTab::Downloader;
                        }
                    });
                });
            });
        let _ = id;
    }

    fn draw_watchlist_entry_table(&mut self, ui: &mut egui::Ui) {
        let row_h = 22.0;
        egui::ScrollArea::vertical()
            .id_salt("rustdl_watchlist_scroll")
            .max_height(180.0)
            .show(ui, |ui| {
                let mut remove_id = None;
                let mut enqueue_id = None;
                let mut toggle_pause: Option<(u64, bool)> = None;
                for entry in self.watchlist.entries.clone() {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), row_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let status = if entry.improved_pending {
                                RichText::new("↑ improved").color(egui::Color32::LIGHT_GREEN)
                            } else if entry.paused {
                                RichText::new("paused").color(TEXT_MUTED)
                            } else if entry.last_probe_error.is_some() {
                                RichText::new("error").color(egui::Color32::LIGHT_RED)
                            } else {
                                RichText::new("watching").color(TEXT_MUTED)
                            };
                            ui.label(status);
                            let title = if entry.title.len() > 48 {
                                format!("{}…", &entry.title[..45])
                            } else {
                                entry.title.clone()
                            };
                            ui.label(RichText::new(title).strong());
                            ui.label(
                                RichText::new(format!(
                                    "{} → {}",
                                    format_resolution_height(entry.baseline_height),
                                    format_resolution_height(entry.last_probe_height)
                                ))
                                .small()
                                .color(TEXT_MUTED),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button(ui_icons::REMOVE)
                                        .on_hover_text("Remove")
                                        .clicked()
                                    {
                                        remove_id = Some(entry.entry_id);
                                    }
                                    if entry.improved_pending
                                        && ui
                                            .small_button(format!("{} Queue", ui_icons::ADD))
                                            .on_hover_text("Add this URL to the download queue")
                                            .clicked()
                                    {
                                        enqueue_id = Some(entry.entry_id);
                                    }
                                    let pause_label = if entry.paused { "Resume" } else { "Pause" };
                                    if ui.small_button(pause_label).clicked() {
                                        toggle_pause = Some((entry.entry_id, !entry.paused));
                                    }
                                },
                            );
                        },
                    );
                }
                if let Some(id) = remove_id {
                    self.remove_watchlist_entry(id);
                }
                if let Some(id) = enqueue_id {
                    self.enqueue_watchlist_entry(id);
                }
                if let Some((id, paused)) = toggle_pause {
                    self.set_watchlist_entry_paused(id, paused);
                }
            });
    }

    pub(super) fn draw_watchlist_settings_section(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.label(RichText::new("Quality watchlist").strong());
        ui.label(
            RichText::new(
                "Re-probe saved URLs on a schedule; rustdl logs when max available resolution increases (e.g. a low-quality premiere replaced by HD on the same link).",
            )
            .small()
            .color(TEXT_MUTED),
        );
        ui.add_space(4.0);
        changed |= ui
            .checkbox(
                &mut self.settings.watchlist_enabled,
                "Enable quality watchlist",
            )
            .changed();
        ui.horizontal(|ui| {
            ui.label("Check every");
            let mut hours = self.settings.watchlist_poll_hours.clamp(1, 168) as i32;
            if ui
                .add(egui::DragValue::new(&mut hours).range(1..=168).suffix(" h"))
                .changed()
            {
                self.settings.watchlist_poll_hours = hours as u32;
                changed = true;
            }
            ui.add_space(12.0);
            ui.label("Min height +");
            let mut px = self.settings.watchlist_min_height_delta.clamp(1, 2160) as i32;
            if ui
                .add(egui::DragValue::new(&mut px).range(1..=2160).suffix(" px"))
                .changed()
            {
                self.settings.watchlist_min_height_delta = px as u32;
                changed = true;
            }
        });
        changed |= ui
            .checkbox(
                &mut self.settings.watchlist_auto_enqueue,
                "Auto-add improved URLs to the download queue",
            )
            .changed();
        ui.horizontal(|ui| {
            ui.label("Add URL");
            ui.add(
                egui::TextEdit::singleline(&mut self.watchlist_add_url_buf)
                    .hint_text("https://…")
                    .desired_width(320.0),
            );
            if ui
                .add_enabled(
                    self.has_yt_dlp,
                    egui::Button::new(format!("{} Add", ui_icons::ADD)),
                )
                .clicked()
                && !self.watchlist_add_url_buf.trim().is_empty()
            {
                let url = self.watchlist_add_url_buf.trim().to_owned();
                self.add_url_to_watchlist(&url);
                self.watchlist_add_url_buf.clear();
            }
        });
        ui.add_space(6.0);
        if !self.watchlist.entries.is_empty() {
            with_full_width(ui, |ui| self.draw_watchlist_entry_table(ui));
            ui.add_space(4.0);
            left_button_row(ui, |ui| {
                button_group(ui, "watchlist_settings_actions", |g| {
                    if g.secondary("Check now", self.has_yt_dlp).clicked() {
                        self.probe_watchlist_now();
                    }
                });
            });
        }
        changed
    }
}
