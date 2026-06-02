//! Video queue: docked under main controls or in a floating window (Downloader and AV1).

use eframe::egui::{self, RichText};

use crate::app_ui::{secondary_button, status_color};
use crate::models::ItemStatus;
use crate::theme::{canvas_bg, panel_border, BG_CANVAS, BORDER_PANEL, TEXT_MUTED};
use crate::ui_icons;

use super::av1_panel::av1_item_is_skipped;
use super::PydlApp;

impl PydlApp {
    pub(super) fn toggle_videos_panel(&mut self) {
        if self.settings.videos_docked {
            self.settings.videos_docked = false;
            self.settings.videos_open = true;
        } else {
            self.settings.videos_open = !self.settings.videos_open;
        }
        self.persist_settings();
    }

    pub(super) fn ensure_videos_window_open(&mut self) {
        if !self.settings.videos_docked {
            self.settings.videos_open = true;
        }
    }

    fn videos_window_title(&self) -> &'static str {
        if self.av1_mode {
            "AV1 queue"
        } else {
            "Videos"
        }
    }

    /// Scrollable card grid (shared by docked panel and floating window).
    pub(super) fn draw_queue_cards(&mut self, ui: &mut egui::Ui, max_height: f32) {
        if self.av1_mode {
            self.draw_av1_queue_cards(ui, max_height);
        } else {
            self.draw_downloader_queue_cards(ui, max_height);
        }
    }

    fn draw_downloader_queue_cards(&mut self, ui: &mut egui::Ui, max_height: f32) {
        egui::ScrollArea::vertical()
            .id_salt("rustdl_videos_scroll")
            .auto_shrink([false, false])
            .max_height(max_height.max(120.0))
            .animated(true)
            .drag_to_scroll(true)
            .show(ui, |ui| {
                if self.items.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(32.0);
                        ui.label(RichText::new("Nothing here yet").color(TEXT_MUTED));
                        ui.label(
                            RichText::new(
                                "Paste URL(s) above and click Add URLs to fetch previews.",
                            )
                            .small(),
                        );
                    });
                } else {
                    let profile = std::env::var("RUSTDL_PROFILE").ok().as_deref() == Some("1");
                    let t0 = profile.then(std::time::Instant::now);
                    self.draw_grouped_cards(ui);
                    if let Some(t0) = t0 {
                        let ms = t0.elapsed().as_secs_f64() * 1000.0;
                        if ms > 8.0 {
                            eprintln!(
                                "rustdl profile: draw_grouped_cards {} items in {ms:.1}ms",
                                self.items.len()
                            );
                        }
                    }
                }
            });
    }

    fn draw_videos_header_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let heading = if self.av1_mode {
                "AV1 queue"
            } else {
                "Videos"
            };
            ui.label(RichText::new(heading).strong());
            let tail_w = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(tail_w.max(0.0), 0.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let video_dock_label = if self.settings.videos_docked {
                        format!("{} Undock videos", ui_icons::UNDOCK_VIDEOS)
                    } else {
                        format!("{} Dock videos", ui_icons::DOCK_VIDEOS)
                    };
                    if secondary_button(ui, &video_dock_label, true)
                        .on_hover_text(
                            "Show the queue in a separate window so the main view stays compact.",
                        )
                        .clicked()
                    {
                        self.settings.videos_docked = !self.settings.videos_docked;
                        if !self.settings.videos_docked {
                            self.settings.videos_open = true;
                        }
                        self.persist_settings();
                    }
                    if self.settings.videos_docked {
                        let log_dock_label = if self.settings.logs_docked {
                            format!("{} Undock log", ui_icons::UNDOCK_LOG)
                        } else {
                            format!("{} Dock log", ui_icons::DOCK_LOG)
                        };
                        if secondary_button(ui, &log_dock_label, true).clicked() {
                            self.settings.logs_docked = !self.settings.logs_docked;
                            self.persist_settings();
                        }
                    }
                },
            );
        });
    }

    /// Compact strip when the queue lives in a floating window.
    pub(super) fn draw_videos_undocked_strip(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let strip_label = if self.av1_mode {
                "AV1 queue is in a separate window."
            } else {
                "Video queue is in a separate window."
            };
            ui.label(RichText::new(strip_label).color(TEXT_MUTED));
            if !self.settings.videos_open
                && secondary_button(
                    ui,
                    &format!("{} Show videos", ui_icons::VIDEOS),
                    true,
                )
                .clicked()
            {
                self.settings.videos_open = true;
                self.persist_settings();
            }
            if self.av1_mode {
                self.draw_av1_undocked_strip_counts(ui);
            } else {
                self.draw_downloader_undocked_strip_counts(ui);
            }
        });
    }

    fn draw_downloader_undocked_strip_counts(&mut self, ui: &mut egui::Ui) {
        if self.status_done > 0 {
            let label = format!("{} done", self.status_done);
            if ui
                .add(
                    egui::Label::new(RichText::new(label).color(status_color(ItemStatus::Done)))
                        .sense(egui::Sense::click()),
                )
                .clicked()
            {
                self.focus_queue_group("Done");
            }
        }
        if self.status_ready > 0 {
            let label = format!("{} ready", self.status_ready);
            if ui
                .add(
                    egui::Label::new(RichText::new(label).color(status_color(ItemStatus::Idle)))
                        .sense(egui::Sense::click()),
                )
                .clicked()
            {
                self.focus_queue_group("Ready");
            }
        }
    }

    fn draw_av1_undocked_strip_counts(&mut self, ui: &mut egui::Ui) {
        let ready = self
            .av1_items
            .iter()
            .filter(|i| i.status == ItemStatus::Idle)
            .count();
        let done = self
            .av1_items
            .iter()
            .filter(|i| i.status == ItemStatus::Done && !av1_item_is_skipped(i))
            .count();
        if done > 0 {
            let label = format!("{done} done");
            if ui
                .add(
                    egui::Label::new(RichText::new(label).color(status_color(ItemStatus::Done)))
                        .sense(egui::Sense::click()),
                )
                .clicked()
            {
                self.focus_queue_group("Done");
            }
        }
        if ready > 0 {
            let label = format!("{ready} ready");
            if ui
                .add(
                    egui::Label::new(RichText::new(label).color(status_color(ItemStatus::Idle)))
                        .sense(egui::Sense::click()),
                )
                .clicked()
            {
                self.focus_queue_group("Ready");
            }
        }
    }

    /// Activity log docked in the main panel when the video queue is undocked.
    pub(super) fn draw_docked_log_only_section(&mut self, ui: &mut egui::Ui) {
        let log_h = self.settings.log_dock_height.clamp(80.0, 480.0);
        egui::Frame::dark_canvas(ui.style())
            .fill(BG_CANVAS)
            .stroke(egui::Stroke::new(1.0, BORDER_PANEL))
            .inner_margin(egui::Margin::same(10.0))
            .rounding(egui::Rounding::same(8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Activity log").small().strong());
                    let tail_w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(tail_w.max(0.0), 0.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if secondary_button(
                                ui,
                                &format!("{} Undock log", ui_icons::UNDOCK_LOG),
                                true,
                            )
                            .clicked()
                            {
                                self.settings.logs_docked = false;
                                self.persist_settings();
                            }
                        },
                    );
                });
                if ui
                    .add(egui::Slider::new(
                        &mut self.settings.log_dock_height,
                        80.0..=480.0,
                    ))
                    .changed()
                {
                    self.persist_settings();
                }
                egui::ScrollArea::vertical()
                    .id_salt("rustdl_log_docked_only")
                    .max_height(log_h)
                    .show(ui, |ui| {
                        self.draw_activity_log_panel(ui);
                    });
            });
    }

    /// Docked video frame (cards + optional docked log below).
    pub(super) fn draw_docked_videos_section(
        &mut self,
        ui: &mut egui::Ui,
        video_scroll_h: f32,
    ) {
        let theme = self.settings.theme.clone();
        let fill = if self.av1_mode {
            canvas_bg(&theme)
        } else {
            BG_CANVAS
        };
        let border = if self.av1_mode {
            panel_border(&theme)
        } else {
            BORDER_PANEL
        };

        egui::Frame::dark_canvas(ui.style())
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, border))
            .inner_margin(egui::Margin::same(10.0))
            .rounding(egui::Rounding::same(8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.draw_videos_header_toolbar(ui);
                let dock_log = self.settings.logs_open && self.settings.logs_docked;
                let log_h = if dock_log {
                    self.settings.log_dock_height.clamp(80.0, 480.0)
                } else {
                    0.0
                };
                let cards_h = if dock_log {
                    (video_scroll_h - log_h - 12.0).max(120.0)
                } else {
                    video_scroll_h
                };
                self.draw_queue_cards(ui, cards_h);
                if dock_log {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Activity log").small().strong());
                    if ui
                        .add(egui::Slider::new(
                            &mut self.settings.log_dock_height,
                            80.0..=480.0,
                        ))
                        .changed()
                    {
                        self.persist_settings();
                    }
                    self.draw_activity_log_panel(ui);
                }
            });
    }

    pub(super) fn draw_videos_window(&mut self, ctx: &egui::Context) {
        if !self.settings.videos_open {
            return;
        }
        let mut open = true;
        let default_size = egui::vec2(
            self.settings.video_float_width,
            self.settings.video_float_height,
        );
        let title = self.videos_window_title().to_owned();
        let response = egui::Window::new(title)
            .open(&mut open)
            .default_size(default_size)
            .min_width(480.0)
            .min_height(320.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            self.settings.videos_open = false;
                            self.persist_settings();
                        }
                        if secondary_button(
                            ui,
                            &format!("{} Dock in main window", ui_icons::DOCK_VIDEOS),
                            true,
                        )
                        .clicked()
                        {
                            self.settings.videos_docked = true;
                            self.persist_settings();
                        }
                    });
                });
                let h = ui.available_height().max(200.0);
                self.draw_queue_cards(ui, h);
            });
        if let Some(inner) = response {
            let size = inner.response.rect.size();
            if size.x >= 480.0 && size.y >= 320.0 {
                self.settings.video_float_width = size.x;
                self.settings.video_float_height = size.y;
            }
        }
        if !open {
            self.settings.videos_open = false;
            self.persist_settings();
        }
    }
}
