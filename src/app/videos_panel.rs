//! Video queue: docked under main controls or in a floating window (Downloader and AV1).

use eframe::egui::{self, Color32, RichText};

use crate::app_ui::{
    button_group, button_toolbar_wrapped, constrain_content_width, draw_status_dot,
    left_button_row, status_color, with_full_width,
};
use crate::models::ItemStatus;
use crate::theme::{canvas_bg, panel_border, BG_CANVAS, BORDER_PANEL, TEXT_MUTED};
use crate::ui_icons;

use super::PydlApp;

impl PydlApp {
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
        if !self.items.is_empty() {
            ui.add_space(4.0);
            self.draw_downloader_queue_status_row(ui);
            ui.add_space(6.0);
        }
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

    /// Dock/undock and show/hide for the queue window and activity log (same controls everywhere).
    pub(super) fn draw_queue_and_log_controls(&mut self, ui: &mut egui::Ui) {
        let window_title = self.videos_window_title();
        button_toolbar_wrapped(ui, |ui| {
            button_group(ui, "queue_videos_controls", |g| {
                if self.settings.videos_docked {
                    if g.secondary(
                        &format!("{} Undock videos", ui_icons::UNDOCK_VIDEOS),
                        true,
                    )
                    .on_hover_text(
                        "Show the queue in a separate window so the main view stays compact.",
                    )
                    .clicked()
                    {
                        self.settings.videos_docked = false;
                        self.settings.videos_open = true;
                        self.persist_settings();
                    }
                } else {
                    if g.secondary(
                        &format!("{} Dock in main window", ui_icons::DOCK_VIDEOS),
                        true,
                    )
                    .on_hover_text("Move the queue back into this window.")
                    .clicked()
                    {
                        self.settings.videos_docked = true;
                        self.persist_settings();
                    }
                    if !self.settings.videos_open {
                        if g.secondary(
                            &format!("{} Show {window_title}", ui_icons::VIDEOS),
                            true,
                        )
                        .on_hover_text("Open or focus the floating queue window")
                        .clicked()
                        {
                            self.settings.videos_open = true;
                            self.persist_settings();
                        }
                    } else if g.secondary(
                        &format!("{} Hide {window_title}", ui_icons::DISMISS),
                        true,
                    )
                    .on_hover_text("Close the floating queue window")
                    .clicked()
                    {
                        self.settings.videos_open = false;
                        self.persist_settings();
                    }
                }
            });
            button_group(ui, "queue_logs_controls", |g| {
                if !self.settings.logs_open {
                    if g.secondary(&format!("{} Show log", ui_icons::LOGS), true)
                        .on_hover_text(
                            "Open the activity log (dock under the queue or in its own window)",
                        )
                        .clicked()
                    {
                        self.settings.logs_open = true;
                        self.persist_settings();
                    }
                } else {
                    let log_dock_label = if self.settings.logs_docked {
                        format!("{} Undock log", ui_icons::UNDOCK_LOG)
                    } else {
                        format!("{} Dock log", ui_icons::DOCK_LOG)
                    };
                    if g.secondary(&log_dock_label, true)
                        .on_hover_text(
                            "Dock the log under the queue in this window, or show it in a separate window",
                        )
                        .clicked()
                    {
                        self.settings.logs_docked = !self.settings.logs_docked;
                        self.persist_settings();
                    }
                    if g.secondary(&format!("{} Hide log", ui_icons::DISMISS), true)
                        .on_hover_text("Close the activity log")
                        .clicked()
                    {
                        self.settings.logs_open = false;
                        self.settings.logs_docked = false;
                        self.persist_settings();
                    }
                }
            });
        });
    }

    fn draw_videos_header_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let heading = if self.av1_mode { "AV1 queue" } else { "Videos" };
            ui.label(RichText::new(heading).strong());
            let tail_w = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(tail_w.max(0.0), 0.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    self.draw_queue_and_log_controls(ui);
                },
            );
        });
    }

    /// Compact strip when the queue lives in a floating window.
    pub(super) fn draw_videos_undocked_strip(&mut self, ui: &mut egui::Ui) {
        let theme = self.settings.theme.clone();
        let heading = if self.av1_mode { "AV1 queue" } else { "Videos" };
        let window_title = self.videos_window_title();
        with_full_width(ui, |ui| {
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
            egui::Frame::none()
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, border))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .rounding(egui::Rounding::same(8.0))
                .show(ui, |ui| {
                    constrain_content_width(ui);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(heading).strong());
                        ui.label(
                            RichText::new(format!(
                                "Showing in separate \"{window_title}\" window"
                            ))
                            .small()
                            .color(TEXT_MUTED),
                        );
                    });
                    left_button_row(ui, |ui| {
                        self.draw_queue_and_log_controls(ui);
                    });
                    let show_status = if self.av1_mode {
                        !self.av1_items.is_empty()
                    } else {
                        !self.items.is_empty()
                    };
                    if show_status {
                        ui.add_space(4.0);
                        if self.av1_mode {
                            self.draw_av1_queue_status_row(ui);
                        } else {
                            self.draw_downloader_queue_status_row(ui);
                        }
                    }
                });
        });
    }

    /// Colored per-status counts for the downloader queue (main panel, undocked strip, videos panel).
    pub(super) fn draw_downloader_queue_status_row(&mut self, ui: &mut egui::Ui) {
        let mut parts: Vec<(&str, usize, Color32)> = Vec::new();
        if self.status_resolving > 0 {
            parts.push((
                "resolving",
                self.status_resolving,
                status_color(ItemStatus::Resolving),
            ));
        }
        if self.status_ready > 0 {
            parts.push(("ready", self.status_ready, status_color(ItemStatus::Idle)));
        }
        if self.status_queued > 0 {
            parts.push((
                "queued",
                self.status_queued,
                status_color(ItemStatus::Queued),
            ));
        }
        if self.status_active > 0 {
            parts.push((
                "active",
                self.status_active,
                status_color(ItemStatus::Downloading),
            ));
        }
        if self.status_done > 0 {
            parts.push(("done", self.status_done, status_color(ItemStatus::Done)));
        }
        if self.status_failed > 0 {
            parts.push((
                "failed",
                self.status_failed,
                status_color(ItemStatus::Failed),
            ));
        }
        if parts.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let heading = if self.items.is_empty() {
                "Downloads:".to_owned()
            } else {
                format!("Downloads ({}):", self.items.len())
            };
            ui.label(RichText::new(heading).color(TEXT_MUTED));
            if self.queue_group_focus.is_some()
                && ui
                    .small_button(format!("{} Show all", ui_icons::SHOW_ALL))
                    .clicked()
            {
                self.queue_group_focus = None;
            }
            for (idx, (name, count, color)) in parts.iter().enumerate() {
                let suffix = if idx + 1 == parts.len() { "" } else { "," };
                let group = match *name {
                    "ready" => "Ready",
                    "queued" | "active" => "Active",
                    "done" => "Done",
                    "failed" => "Issues",
                    "resolving" => "Resolving",
                    _ => "Active",
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    draw_status_dot(ui, *color);
                    let label = format!("{count} {name}{suffix}");
                    let r = ui.add(
                        egui::Label::new(RichText::new(label).color(*color))
                            .sense(egui::Sense::click()),
                    );
                    if r.clicked() {
                        self.focus_queue_group(group);
                    }
                    r.on_hover_text(format!("Show {group} items"));
                });
            }
        });
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
                constrain_content_width(ui);
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
                egui::ScrollArea::vertical()
                    .id_salt("rustdl_log_docked_only")
                    .max_height(log_h)
                    .show(ui, |ui| {
                        self.draw_activity_log_panel(ui);
                    });
            });
    }

    /// Docked video frame (cards + optional docked log below).
    pub(super) fn draw_docked_videos_section(&mut self, ui: &mut egui::Ui, video_scroll_h: f32) {
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
                constrain_content_width(ui);
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
                left_button_row(ui, |ui| {
                    self.draw_queue_and_log_controls(ui);
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
