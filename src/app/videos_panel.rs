//! Video queue: docked under main controls or in a floating window (Downloader and AV1).

use eframe::egui::{self, Color32, RichText};

use crate::app_ui::{
    allocate_top_down_rect, bounded_ui_height, button_group, button_toolbar_wrapped,
    constrain_content_width, content_width, draw_status_dot, height_to_bottom, left_button_row,
    remaining_ui_height, show_mode_panel, status_color, with_full_width,
};
use crate::models::ItemStatus;
use crate::theme::{BG_CANVAS, BORDER_PANEL, TEXT_MUTED};
use crate::ui_icons;

use super::PydlApp;

/// Chrome below the queue list when the activity log is docked under Videos (header, slider, toolbar).
const DOCKED_LOG_UNDER_VIDEOS_CHROME: f32 = 100.0;
/// Chrome above log lines when the activity log is docked in the main column (videos undocked).
const UNDOCKED_DOCKED_LOG_CHROME: f32 = 100.0;
/// Minimum scroll height for queue cards in the docked bottom panel.
const DOCKED_QUEUE_LIST_MIN_H: f32 = 160.0;
/// Space reserved at the panel bottom for the dock/hide/action toolbar (may wrap to two rows).
const QUEUE_FOOTER_TOOLBAR_RESERVE: f32 = 100.0;

impl PydlApp {
    fn draw_log_height_slider(&mut self, ui: &mut egui::Ui, max_log: f32) -> bool {
        let max_log = max_log.max(80.0).round();
        let mut px = self.settings.log_dock_height.round().clamp(80.0, max_log) as i32;
        let max_i = max_log as i32;
        let changed = ui
            .add(egui::Slider::new(&mut px, 80..=max_i.max(80)).text("px"))
            .changed();
        if changed {
            self.settings.log_dock_height = px as f32;
            self.persist_settings();
        }
        changed
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

    fn draw_mode_queue_panel<R>(
        ui: &mut egui::Ui,
        theme: &str,
        av1: bool,
        inner_margin: egui::Margin,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        show_mode_panel(ui, theme, av1, inner_margin, 8.0, add_contents).inner
    }

    /// Scrollable card list in a fixed-height region (cards align from the top).
    fn draw_queue_list_body(&mut self, ui: &mut egui::Ui, scroll_h: f32, scroll_id: &str) {
        let mut scroll_h = scroll_h.max(80.0);
        if !scroll_h.is_finite() {
            scroll_h = bounded_ui_height(ui, 80.0);
        }
        let w = content_width(ui).max(1.0);
        allocate_top_down_rect(ui, egui::vec2(w, scroll_h), |ui| {
            constrain_content_width(ui);
            let inner_h = ui.max_rect().height();
            if self.av1_mode {
                self.draw_av1_queue_list_scroll(ui, inner_h);
            } else {
                self.draw_downloader_queue_list_scroll(ui, inner_h, scroll_id);
            }
        });
    }

    /// Pause/export/import/recheck/clear — lives in the video queue card or floating window.
    fn draw_downloader_queue_action_toolbar_inner(&mut self, ui: &mut egui::Ui) {
        button_group(ui, "dl_queue_actions", |g| {
            if self.downloads_paused {
                if g
                    .success(
                        &format!("{} Resume downloads", ui_icons::USE_DOWNLOADS),
                        true,
                    )
                    .clicked()
                {
                    self.resume_all_downloads();
                }
            } else if g
                .warning(
                    &format!("{} Pause downloads", ui_icons::CANCEL_TO_READY),
                    self.status_queued > 0 || self.status_active > 0,
                )
                .clicked()
            {
                self.pause_all_downloads();
            }
            if g
                .secondary(
                    &format!("{} Export URLs", ui_icons::EXPORT),
                    !self.items.is_empty(),
                )
                .clicked()
            {
                self.export_queue_to_file();
            }
            if g
                .secondary(
                    &format!("{} Import queue", ui_icons::IMPORT_FILE),
                    !self.add_in_progress,
                )
                .on_hover_text("Load URLs from a .txt file directly into the download queue")
                .clicked()
            {
                self.import_queue_from_file();
            }
            if g
                .warning(
                    &format!("{} Re-check saved files", ui_icons::RECHECK),
                    self.has_ffprobe && !self.settings.ffmpeg_extract_audio_mp3,
                )
                .on_hover_text(
                    "Run ffprobe on each finished download on disk; mark rows failed if video or audio is missing.",
                )
                .on_disabled_hover_text(
                    "Requires ffprobe. Disabled while MP3 extraction is enabled.",
                )
                .clicked()
            {
                self.recheck_all_saved_downloads();
            }
            if g
                .danger(&format!("{} Clear list", ui_icons::CLEAR_QUEUE), true)
                .clicked()
            {
                self.items.retain(|x| {
                    matches!(x.status, ItemStatus::Queued | ItemStatus::Downloading)
                });
                self.pending_resolve_ids
                    .retain(|_, iid| self.items.iter().any(|x| x.item_id == *iid));
                self.update_status();
                self.refresh_input_line_info();
                self.schedule_queue_save();
                self.mark_queue_dirty();
            }
        });
    }

    fn draw_downloader_queue_list_scroll(
        &mut self,
        ui: &mut egui::Ui,
        scroll_h: f32,
        scroll_id: &str,
    ) {
        let scroll_h = scroll_h.max(80.0);
        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .animated(true)
            .drag_to_scroll(true)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                if self.items.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
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

    /// Dock/undock and show/hide for the video queue (videos window and docked panel only).
    fn draw_video_queue_controls_inner(&mut self, ui: &mut egui::Ui) {
        let window_title = self.videos_window_title();
        button_group(ui, "queue_videos_controls", |g| {
            if self.settings.videos_docked {
                if g
                    .secondary(
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
                if g
                    .secondary(
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
                    if g
                        .secondary(
                            &format!("{} Show {window_title}", ui_icons::VIDEOS),
                            true,
                        )
                        .on_hover_text("Open or focus the floating queue window")
                        .clicked()
                    {
                        self.settings.videos_open = true;
                        self.persist_settings();
                    }
                } else if g
                    .secondary(
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
    }

    pub(super) fn draw_video_queue_controls(&mut self, ui: &mut egui::Ui) {
        button_toolbar_wrapped(ui, |ui| self.draw_video_queue_controls_inner(ui));
    }

    /// Dock/undock and show/hide for the activity log (log window and docked log sections only).
    pub(super) fn draw_log_controls(&mut self, ui: &mut egui::Ui) {
        button_toolbar_wrapped(ui, |ui| {
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
                            "Dock the log under the queue in the main window, or show it in a separate window",
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

    /// Dock/hide and queue actions — pinned below the card list.
    fn draw_videos_footer_toolbar(&mut self, ui: &mut egui::Ui) {
        button_toolbar_wrapped(ui, |ui| {
            let heading = if self.av1_mode {
                "AV1 queue"
            } else {
                "Videos"
            };
            ui.label(RichText::new(heading).strong());
            self.draw_video_queue_controls_inner(ui);
            if self.av1_mode {
                self.draw_av1_queue_action_toolbar_inner(ui);
            } else {
                self.draw_downloader_queue_action_toolbar_inner(ui);
            }
        });
    }

    /// Status row, scrollable cards (top), toolbar (bottom); optional log under the toolbar when docked.
    fn draw_videos_queue_body(
        &mut self,
        ui: &mut egui::Ui,
        panel_bottom: f32,
        scroll_id: &str,
        dock_log: bool,
    ) {
        constrain_content_width(ui);
        ui.spacing_mut().item_spacing.y = 4.0;

        if self.av1_mode {
            if !self.av1_items.is_empty() {
                self.draw_av1_queue_status_row(ui);
                self.draw_av1_batch_summary_row(ui);
            }
        } else if !self.items.is_empty() {
            self.draw_downloader_queue_status_row(ui);
        }

        let log_chrome = DOCKED_LOG_UNDER_VIDEOS_CHROME;
        let log_pref = if dock_log {
            let below_status = height_to_bottom(ui, panel_bottom);
            self.docked_log_height_budget(
                below_status - QUEUE_FOOTER_TOOLBAR_RESERVE - log_chrome,
            )
        } else {
            0.0
        };
        let mut bottom_reserve = QUEUE_FOOTER_TOOLBAR_RESERVE;
        if dock_log {
            bottom_reserve += log_pref + log_chrome + 8.0;
        }
        let min_list = if scroll_id.contains("dock") {
            DOCKED_QUEUE_LIST_MIN_H
        } else {
            120.0
        };
        let list_h = (panel_bottom - ui.cursor().min.y - bottom_reserve).max(min_list);
        self.draw_queue_list_body(ui, list_h, scroll_id);

        ui.add_space(4.0);
        self.draw_videos_footer_toolbar(ui);

        if dock_log {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Activity log").small().strong());
                let tail_w = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(tail_w.max(0.0), 0.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        self.draw_log_controls(ui);
                    },
                );
            });
            let below_toolbar = height_to_bottom(ui, panel_bottom);
            let max_log =
                (below_toolbar - log_chrome).max(80.0);
            self.draw_log_height_slider(ui, max_log);
            self.draw_activity_log_toolbar(ui);
            let log_lines_h = height_to_bottom(ui, panel_bottom).max(60.0);
            self.draw_activity_log_lines_scroll(ui, log_lines_h);
        }
    }

    /// Compact strip when the queue lives in a floating window.
    pub(super) fn draw_videos_undocked_strip(&mut self, ui: &mut egui::Ui) {
        let heading = if self.av1_mode { "AV1 queue" } else { "Videos" };
        let window_title = self.videos_window_title();
        let theme = self.settings.theme.clone();
        let av1 = self.av1_mode;
        with_full_width(ui, |ui| {
            Self::draw_mode_queue_panel(
                ui,
                &theme,
                av1,
                egui::Margin::symmetric(12.0, 10.0),
                |ui| {
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
                        self.draw_video_queue_controls(ui);
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
                },
            );
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
        ui.add_space(4.0);
        egui::Frame::dark_canvas(ui.style())
            .fill(BG_CANVAS)
            .stroke(egui::Stroke::new(1.0, BORDER_PANEL))
            .inner_margin(egui::Margin::same(10.0))
            .rounding(egui::Rounding::same(8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                constrain_content_width(ui);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Activity log").small().strong());
                    let tail_w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(tail_w.max(0.0), 0.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            self.draw_log_controls(ui);
                        },
                    );
                });
                let budget = remaining_ui_height(ui).max(80.0);
                let max_log = (budget - UNDOCKED_DOCKED_LOG_CHROME).max(80.0);
                self.draw_log_height_slider(ui, max_log);
                self.draw_activity_log_toolbar(ui);
                let log_h = remaining_ui_height(ui).max(60.0);
                self.draw_activity_log_lines_scroll(ui, log_h);
            });
    }

    /// Pinned footer when the queue is undocked (`TopBottomPanel` body).
    pub(super) fn draw_queue_footer(&mut self, ui: &mut egui::Ui) {
        self.draw_videos_undocked_strip(ui);
        if self.settings.logs_open && self.settings.logs_docked {
            self.draw_docked_log_only_section(ui);
        }
    }

    fn docked_log_height_budget(&self, remaining: f32) -> f32 {
        let max_log = (remaining - DOCKED_QUEUE_LIST_MIN_H - DOCKED_LOG_UNDER_VIDEOS_CHROME)
            .max(80.0);
        self.settings
            .log_dock_height
            .clamp(80.0, 480.0)
            .min(max_log)
    }

    /// Docked video queue (`TopBottomPanel` body).
    pub(super) fn draw_docked_videos_panel(&mut self, ui: &mut egui::Ui) {
        let panel_h = remaining_ui_height(ui).max(180.0);
        let panel_w = content_width(ui).max(1.0);
        // egui persists panel height from the content rect; fill the panel so resize sticks.
        ui.set_min_size(egui::vec2(panel_w, panel_h));
        let dock_log = self.settings.logs_open && self.settings.logs_docked;
        let theme = self.settings.theme.clone();
        let av1 = self.av1_mode;

        Self::draw_mode_queue_panel(ui, &theme, av1, egui::Margin::symmetric(10.0, 8.0), |ui| {
            let inner_h = remaining_ui_height(ui).max(180.0);
            let inner_w = content_width(ui).max(1.0);
            allocate_top_down_rect(ui, egui::vec2(inner_w, inner_h), |ui| {
                self.draw_videos_queue_body(
                    ui,
                    ui.max_rect().bottom(),
                    "rustdl_videos_dock_scroll",
                    dock_log,
                );
            });
        });
        if (panel_h - self.settings.videos_dock_height).abs() > 0.5 {
            self.settings.videos_dock_height = panel_h.clamp(180.0, 800.0);
            self.persist_settings();
        }
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
        let theme = self.settings.theme.clone();
        let av1 = self.av1_mode;
        let response = egui::Window::new(title)
            .id(egui::Id::new("rustdl_videos_float_v4"))
            .open(&mut open)
            .default_size(default_size)
            .min_width(480.0)
            .min_height(320.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let panel_h = remaining_ui_height(ui).max(320.0);
                let panel_w = content_width(ui).max(480.0);
                // Fill the window body so the resize grip changes the window (not just shrink-wrapped content).
                ui.set_min_size(egui::vec2(panel_w, panel_h));
                Self::draw_mode_queue_panel(
                    ui,
                    &theme,
                    av1,
                    egui::Margin::symmetric(10.0, 8.0),
                    |ui| {
                    let inner_h = remaining_ui_height(ui).max(120.0);
                    let inner_w = content_width(ui).max(480.0);
                    allocate_top_down_rect(ui, egui::vec2(inner_w, inner_h), |ui| {
                        self.draw_videos_queue_body(
                            ui,
                            ui.max_rect().bottom(),
                            "rustdl_videos_float_v4",
                            false,
                        );
                    });
                });
            });
        if let Some(inner) = response {
            let size = inner.response.rect.size();
            if size.x.is_finite()
                && size.y.is_finite()
                && size.x >= 480.0
                && size.y >= 320.0
                && size.x <= 2400.0
                && size.y <= 1600.0
                && ((self.settings.video_float_width - size.x).abs() > 0.5
                    || (self.settings.video_float_height - size.y).abs() > 0.5)
            {
                self.settings.video_float_width = size.x;
                self.settings.video_float_height = size.y;
                self.persist_settings();
            }
        }
        if !open {
            self.settings.videos_open = false;
            self.persist_settings();
        }
    }
}
