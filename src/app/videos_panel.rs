//! Video queue: docked under main controls or in a floating window (Downloader and AV1).

use eframe::egui::{self, Color32, RichText};

use crate::app_parsing::human_bytes_ui;
use crate::app_state::compute_download_batch_progress;
use crate::app_ui::{
    allocate_top_down_rect, bounded_ui_height, button_group, button_toolbar_wrapped,
    compact_button_group, consume_remaining_ui_space, content_width, draw_batch_progress_bar,
    draw_status_dot, fill_allocated_rect, height_to_bottom, left_button_row,
    finite_ui_span, note_resizable_panel_height, persist_resizable_window_size,
    queue_footer_toolbar_reserve, show_mode_panel, status_color, with_full_width,
    UNDOCKED_FOOTER_PANEL_ID, UNDOCKED_VIDEOS_STRIP_H, VIDEOS_DOCK_PANEL_ID,
};
use crate::convert_state::compute_convert_batch_progress;
use crate::models::ItemStatus;
use crate::theme::{BG_CANVAS, BORDER_PANEL, TEXT_MUTED};
use crate::ui_icons;

use super::PydlApp;

/// Chrome below the queue list when the activity log is docked under Videos (placement + slider + filter rows).
const DOCKED_LOG_UNDER_VIDEOS_CHROME: f32 = 100.0;
/// Chrome above log lines when the activity log is docked in the main column (videos undocked).
const UNDOCKED_DOCKED_LOG_CHROME: f32 = 100.0;
/// Minimum scroll height for queue cards in the docked bottom panel.
const DOCKED_QUEUE_LIST_MIN_H: f32 = 48.0;
const DOCKED_LOG_MIN_LINES_H: f32 = 48.0;
const QUEUE_MODE_PANEL_MARGIN: egui::Margin = egui::Margin {
    left: 10.0,
    right: 10.0,
    top: 6.0,
    bottom: 6.0,
};

/// Shared layout parameters for docked and floating video queue panels.
struct VideosQueueLayout<'a> {
    scroll_id: &'a str,
    dock_log: bool,
    log_dock_height: f32,
    /// Bottom edge of the allocated queue body (set by docked/float shell before the mode panel).
    body_bottom: Option<f32>,
}

impl VideosQueueLayout<'_> {
    fn is_docked(&self) -> bool {
        self.scroll_id.contains("dock")
    }
}

fn queue_list_min_scroll_h(scroll_id: &str) -> f32 {
    if scroll_id.contains("dock") {
        DOCKED_QUEUE_LIST_MIN_H
    } else {
        80.0
    }
}

impl PydlApp {
    pub(super) fn ensure_videos_window_open(&mut self) {
        if !self.settings.videos_docked {
            self.settings.videos_open = true;
        }
    }

    fn videos_window_title(&self) -> &'static str {
        if self.convert_mode {
            "Convert queue"
        } else {
            "Videos"
        }
    }

    fn draw_mode_queue_panel<R>(
        ui: &mut egui::Ui,
        theme: &str,
        av1: bool,
        colors: crate::theme::ModePanelColors<'_>,
        inner_margin: egui::Margin,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        show_mode_panel(ui, theme, av1, colors, inner_margin, 8.0, add_contents).inner
    }

    /// Scrollable card list in a fixed-height region (cards align from the top).
    fn draw_queue_list_body(&mut self, ui: &mut egui::Ui, scroll_h: f32, scroll_id: &str) {
        let min_h = queue_list_min_scroll_h(scroll_id);
        let cap = bounded_ui_height(ui, min_h).max(min_h);
        let scroll_h = finite_ui_span(scroll_h, min_h)
            .clamp(min_h, cap);
        let w = content_width(ui).max(1.0);
        allocate_top_down_rect(ui, egui::vec2(w, scroll_h), |ui| {
            ui.set_min_height(scroll_h);
            self.constrain_content(ui);
            let inner_h = finite_ui_span(ui.max_rect().height(), scroll_h).clamp(min_h, scroll_h);
            if self.convert_mode {
                self.draw_convert_queue_list_scroll(ui, inner_h, min_h);
            } else {
                self.draw_downloader_queue_list_scroll(ui, inner_h, scroll_id);
            }
        });
    }

    /// Pause/import-export/recheck/clear — split into wrap-friendly groups for narrow footers.
    fn draw_downloader_queue_action_groups(&mut self, ui: &mut egui::Ui, compact: bool) {
        let mut export_queue = false;
        let mut import_queue = false;
        let mut cancel_all_ready = false;
        let mut cancel_all_remove = false;
        let can_cancel_all = self.status_queued > 0 || self.status_active > 0;
        let draw = |ui: &mut egui::Ui,
                    id: &str,
                    add: &mut dyn FnMut(&mut crate::app_ui::ButtonGroup<'_>)| {
            if compact {
                compact_button_group(ui, id, |g| add(g));
            } else {
                button_group(ui, id, |g| add(g));
            }
        };
        draw(ui, "dl_queue_transport", &mut |g| {
            if self.downloads_paused {
                if g.success(
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
            g.cancel_all_menu(
                can_cancel_all,
                &mut cancel_all_ready,
                &mut cancel_all_remove,
            );
        });
        draw(ui, "dl_queue_io", &mut |g| {
            if g.secondary(
                &format!("{} Open output folder", ui_icons::OPEN_FOLDER),
                true,
            )
            .clicked()
            {
                self.open_output_folder();
            }
            g.import_export_menu(!self.items.is_empty() || !self.add_in_progress, |ui| {
                if ui
                    .add_enabled(
                        !self.items.is_empty(),
                        egui::Button::new(format!("{} Export URLs", ui_icons::EXPORT)),
                    )
                    .clicked()
                {
                    export_queue = true;
                }
                if ui
                    .add_enabled(
                        !self.add_in_progress,
                        egui::Button::new(format!("{} Import queue", ui_icons::IMPORT_FILE)),
                    )
                    .on_hover_text("Load URLs from a .txt file directly into the download queue")
                    .clicked()
                {
                    import_queue = true;
                }
            });
        });
        draw(ui, "dl_queue_maint", &mut |g| {
            if self.status_failed > 0
                && g
                    .warning(
                        &format!("{} Retry all failed", ui_icons::RETRY),
                        true,
                    )
                    .on_hover_text(
                        "Retry every failed download that still has a URL (same as each card's Retry download).",
                    )
                    .clicked()
            {
                self.retry_failed_items();
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
            if g.danger(&format!("{} Clear list", ui_icons::CLEAR_QUEUE), true)
                .clicked()
            {
                self.items
                    .retain(|x| matches!(x.status, ItemStatus::Queued | ItemStatus::Downloading));
                self.pending_resolve_ids
                    .retain(|_, iid| self.items.iter().any(|x| x.item_id == *iid));
                self.update_status();
                self.refresh_input_line_info();
                self.schedule_queue_save();
                self.mark_queue_dirty();
            }
        });
        if export_queue {
            self.export_queue_to_file();
        }
        if import_queue {
            self.import_queue_from_file();
        }
        if cancel_all_ready {
            self.cancel_all_active(super::CancelPostAction::Ready);
        }
        if cancel_all_remove {
            self.cancel_all_active(super::CancelPostAction::Remove);
        }
    }

    /// Single fused action row for the docked queue footer (stable height for panel resize).
    fn draw_downloader_queue_action_fused(&mut self, ui: &mut egui::Ui, compact: bool) {
        let mut export_queue = false;
        let mut import_queue = false;
        let mut cancel_all_ready = false;
        let mut cancel_all_remove = false;
        let can_cancel_all = self.status_queued > 0 || self.status_active > 0;
        let draw = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut crate::app_ui::ButtonGroup<'_>)| {
            if compact {
                compact_button_group(ui, "dl_queue_actions", |g| add(g));
            } else {
                button_group(ui, "dl_queue_actions", |g| add(g));
            }
        };
        draw(ui, &mut |g| {
            if self.downloads_paused {
                if g.success(
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
            g.cancel_all_menu(
                can_cancel_all,
                &mut cancel_all_ready,
                &mut cancel_all_remove,
            );
            if g.secondary(
                &format!("{} Open output folder", ui_icons::OPEN_FOLDER),
                true,
            )
            .clicked()
            {
                self.open_output_folder();
            }
            g.import_export_menu(!self.items.is_empty() || !self.add_in_progress, |ui| {
                if ui
                    .add_enabled(
                        !self.items.is_empty(),
                        egui::Button::new(format!("{} Export URLs", ui_icons::EXPORT)),
                    )
                    .clicked()
                {
                    export_queue = true;
                }
                if ui
                    .add_enabled(
                        !self.add_in_progress,
                        egui::Button::new(format!("{} Import queue", ui_icons::IMPORT_FILE)),
                    )
                    .on_hover_text("Load URLs from a .txt file directly into the download queue")
                    .clicked()
                {
                    import_queue = true;
                }
            });
            if self.status_failed > 0
                && g
                    .warning(
                        &format!("{} Retry all failed", ui_icons::RETRY),
                        true,
                    )
                    .on_hover_text(
                        "Retry every failed download that still has a URL (same as each card's Retry download).",
                    )
                    .clicked()
            {
                self.retry_failed_items();
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
            if g.danger(&format!("{} Clear list", ui_icons::CLEAR_QUEUE), true)
                .clicked()
            {
                self.items
                    .retain(|x| matches!(x.status, ItemStatus::Queued | ItemStatus::Downloading));
                self.pending_resolve_ids
                    .retain(|_, iid| self.items.iter().any(|x| x.item_id == *iid));
                self.update_status();
                self.refresh_input_line_info();
                self.schedule_queue_save();
                self.mark_queue_dirty();
            }
        });
        if export_queue {
            self.export_queue_to_file();
        }
        if import_queue {
            self.import_queue_from_file();
        }
        if cancel_all_ready {
            self.cancel_all_active(super::CancelPostAction::Ready);
        }
        if cancel_all_remove {
            self.cancel_all_active(super::CancelPostAction::Remove);
        }
    }

    fn draw_downloader_queue_list_scroll(
        &mut self,
        ui: &mut egui::Ui,
        scroll_h: f32,
        scroll_id: &str,
    ) {
        let min_h = queue_list_min_scroll_h(scroll_id);
        let scroll_h = finite_ui_span(scroll_h, min_h).max(min_h);
        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .animated(true)
            .drag_to_scroll(true)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                ui.set_width(content_width(ui).max(1.0));
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
    fn draw_video_queue_controls_inner(&mut self, ui: &mut egui::Ui, compact: bool) {
        let window_title = self.videos_window_title();
        let draw = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut crate::app_ui::ButtonGroup<'_>)| {
            if compact {
                compact_button_group(ui, "queue_videos_controls", |g| add(g));
            } else {
                button_group(ui, "queue_videos_controls", |g| add(g));
            }
        };
        draw(ui, &mut |g| {
            if self.settings.videos_docked {
                if g.secondary(&format!("{} Undock videos", ui_icons::UNDOCK_VIDEOS), true)
                    .on_hover_text(
                        "Show the queue in a separate window so the main view stays compact.",
                    )
                    .clicked()
                {
                    self.note_videos_dock_user_choice(false);
                    self.settings.videos_docked = false;
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
                    self.note_videos_dock_user_choice(true);
                    self.settings.videos_docked = true;
                    self.persist_settings();
                }
                if !self.settings.videos_open {
                    if g.secondary(&format!("{} Show {window_title}", ui_icons::VIDEOS), true)
                        .on_hover_text("Open or focus the floating queue window")
                        .clicked()
                    {
                        self.settings.videos_open = true;
                        self.persist_settings();
                    }
                } else if g
                    .secondary(&format!("{} Hide {window_title}", ui_icons::DISMISS), true)
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
        button_toolbar_wrapped(ui, |ui| self.draw_video_queue_controls_inner(ui, false));
    }

    /// Compact dock/undock control for the queue footer (single row with action buttons).
    fn draw_video_queue_controls_compact(&mut self, ui: &mut egui::Ui) {
        self.draw_video_queue_controls_inner(ui, true);
    }

    /// Window/panel chrome (dock, hide) and queue batch actions.
    fn draw_videos_footer_toolbar(&mut self, ui: &mut egui::Ui, wrapped: bool) {
        let heading = if self.convert_mode {
            "Convert queue"
        } else {
            "Videos"
        };
        if wrapped {
            button_toolbar_wrapped(ui, |ui| {
                ui.label(RichText::new(heading).strong());
                self.draw_video_queue_controls_compact(ui);
                if self.convert_mode {
                    self.draw_convert_queue_action_groups(ui, true);
                } else {
                    self.draw_downloader_queue_action_groups(ui, true);
                }
            });
        } else {
            left_button_row(ui, |ui| {
                ui.label(RichText::new(heading).strong());
                self.draw_video_queue_controls_compact(ui);
            });
            left_button_row(ui, |ui| {
                if self.convert_mode {
                    self.draw_convert_queue_action_groups(ui, true);
                } else {
                    self.draw_downloader_queue_action_fused(ui, true);
                }
            });
        }
    }

    fn draw_queue_search_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Search");
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.queue_search)
                    .hint_text("Title, URL, uploader…")
                    .desired_width(220.0),
            );
            if self.focus_queue_search {
                search.request_focus();
                self.focus_queue_search = false;
            }
            if search.changed() {
                self.queue_group_focus = None;
                self.persist_ui_prefs();
            }
            if !self.queue_search.is_empty()
                && ui
                    .small_button(format!("{} Clear", ui_icons::CLEAR_SEARCH))
                    .clicked()
            {
                self.queue_search.clear();
            }
        });
        ui.add_space(2.0);
    }

    /// Status row, scrollable cards, toolbar, optional docked log — top-down with a pinned body bottom.
    fn draw_videos_queue_body(&mut self, ui: &mut egui::Ui, layout: VideosQueueLayout<'_>) {
        self.constrain_content(ui);
        ui.spacing_mut().item_spacing.y = 3.0;
        let body_bottom = layout.body_bottom.filter(|y| y.is_finite()).unwrap_or_else(|| {
            if ui.max_rect().bottom().is_finite() {
                ui.max_rect().bottom()
            } else {
                ui.clip_rect().bottom()
            }
        });

        if !self.convert_mode {
            self.draw_queue_search_row(ui);
        }

        if self.convert_mode {
            if !self.convert_items.is_empty() {
                self.draw_convert_queue_status_row(ui);
                self.draw_convert_batch_progress_row(ui);
                self.draw_convert_batch_summary_row(ui);
            }
        } else if !self.items.is_empty() {
            self.draw_downloader_queue_status_row(ui);
            self.draw_download_batch_progress_row(ui);
        }

        let log_bar = if layout.dock_log {
            DOCKED_LOG_UNDER_VIDEOS_CHROME
        } else {
            0.0
        };
        let log_lines_reserve = if layout.dock_log {
            layout.log_dock_height.clamp(80.0, 480.0)
        } else {
            0.0
        };
        let bottom_reserve = queue_footer_toolbar_reserve(content_width(ui))
            + log_bar
            + log_lines_reserve
            + 4.0;
        let min_list = queue_list_min_scroll_h(layout.scroll_id);
        let list_h = (height_to_bottom(ui, body_bottom) - bottom_reserve).max(min_list);
        self.draw_queue_list_body(ui, list_h, layout.scroll_id);

        ui.add_space(2.0);
        self.draw_videos_footer_toolbar(ui, !layout.is_docked());

        if layout.dock_log {
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            let log_lines_h = height_to_bottom(ui, body_bottom).clamp(
                DOCKED_LOG_MIN_LINES_H,
                layout.log_dock_height.clamp(80.0, 480.0),
            );
            self.draw_docked_log_under_videos(ui, log_lines_h);
        }
    }

    /// Compact strip when the queue lives in a floating window.
    pub(super) fn draw_videos_undocked_strip(&mut self, ui: &mut egui::Ui) {
        let heading = if self.convert_mode {
            "Convert queue"
        } else {
            "Videos"
        };
        let window_title = self.videos_window_title();
        let theme = self.settings.theme.clone();
        let av1 = self.convert_mode;
        let dl_color = self.settings.mode_downloader_color.clone();
        let convert_color = self.settings.mode_convert_color.clone();
        let mode_colors = crate::theme::ModePanelColors::new(&dl_color, &convert_color);
        with_full_width(ui, |ui| {
            Self::draw_mode_queue_panel(
                ui,
                &theme,
                av1,
                mode_colors,
                egui::Margin::symmetric(12.0, 10.0),
                |ui| {
                    self.constrain_content(ui);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(heading).strong());
                        ui.label(
                            RichText::new(format!("Showing in separate \"{window_title}\" window"))
                                .small()
                                .color(TEXT_MUTED),
                        );
                    });
                    left_button_row(ui, |ui| {
                        self.draw_video_queue_controls(ui);
                    });
                },
            );
        });
    }

    /// Colored per-status counts for the downloader queue (videos panel / floating window).
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

    pub(super) fn draw_download_batch_progress_row(&mut self, ui: &mut egui::Ui) {
        let progress = compute_download_batch_progress(&self.items);
        if progress.is_empty() {
            return;
        }
        let busy = self.status_active > 0 || self.queue_running > 0 || self.add_in_progress;
        let mut caption = format!(
            "Batch progress: {:.1}% · {}/{} done",
            progress.percent(),
            progress.finished,
            progress.total,
        );
        if progress.active > 0 {
            caption.push_str(&format!(" · {} active", progress.active));
        }
        if self.status_failed > 0 {
            caption.push_str(&format!(" · {} failed", self.status_failed));
        }
        let resp = draw_batch_progress_bar(
            ui,
            progress.fraction,
            caption,
            status_color(ItemStatus::Downloading),
            busy,
        );
        if resp.clicked() {
            self.focus_queue_group("Done");
        }

        let totals = self.transfer_totals();
        if totals.with_known_total > 0 && totals.known_total_bytes > 0 {
            let frac = totals.downloaded_bytes as f32 / totals.known_total_bytes.max(1) as f32;
            let pct = (frac * 100.0).clamp(0.0, 100.0);
            draw_batch_progress_bar(
                ui,
                frac,
                format!(
                    "Transfer: {} / {} ({pct:.1}%)",
                    human_bytes_ui(totals.downloaded_bytes),
                    human_bytes_ui(totals.known_total_bytes),
                ),
                status_color(ItemStatus::Downloading),
                busy,
            );
        }

        if self.status_ready == 0
            && self.status_queued == 0
            && self.status_active == 0
            && self.status_resolving == 0
            && progress.finished > 0
            && progress.finished == progress.total
        {
            ui.colored_label(
                status_color(ItemStatus::Done),
                "All downloads finished for this session.",
            );
        }
    }

    pub(super) fn draw_convert_batch_progress_row(&self, ui: &mut egui::Ui) {
        let progress = compute_convert_batch_progress(&self.convert_items);
        if progress.is_empty() {
            return;
        }
        let mut caption = format!(
            "Batch progress: {:.1}% · {}/{} processed",
            progress.percent(),
            progress.finished,
            progress.total,
        );
        if progress.active > 0 {
            caption.push_str(&format!(" · {} active", progress.active));
        }
        draw_batch_progress_bar(
            ui,
            progress.fraction,
            caption,
            status_color(ItemStatus::Downloading),
            self.convert_running,
        );
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
                fill_allocated_rect(ui);
                self.constrain_content(ui);
                let body_bottom = ui.max_rect().bottom();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Activity log").small().strong());
                    let tail_w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(tail_w.max(0.0), 0.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            self.draw_log_dock_controls(ui);
                        },
                    );
                });
                let budget = height_to_bottom(ui, body_bottom).max(80.0);
                let max_log = (budget - UNDOCKED_DOCKED_LOG_CHROME).max(80.0);
                self.draw_log_height_slider(ui, max_log);
                self.draw_activity_log_toolbar(ui);
                let log_h = height_to_bottom(ui, body_bottom).max(60.0);
                self.draw_activity_log_lines_scroll(ui, log_h);
            });
    }

    /// Pinned footer when the queue is undocked (`TopBottomPanel` body).
    pub(super) fn draw_queue_footer(&mut self, ui: &mut egui::Ui) {
        let log_docked = self.settings.logs_open && self.settings.logs_docked;
        if log_docked {
            let panel_h =
                finite_ui_span(ui.clip_rect().height(), self.settings.undocked_footer_height)
                    .max(180.0);
            let panel_w = finite_ui_span(ui.clip_rect().width(), 800.0).max(1.0);
            allocate_top_down_rect(ui, egui::vec2(panel_w, panel_h), |ui| {
                fill_allocated_rect(ui);
                self.draw_videos_undocked_strip(ui);
                self.draw_docked_log_only_section(ui);
                consume_remaining_ui_space(ui);
            });
            consume_remaining_ui_space(ui);
            note_resizable_panel_height(ui.ctx(), UNDOCKED_FOOTER_PANEL_ID, panel_h);
            if !ui.ctx().input(|i| i.pointer.any_down())
                && (panel_h - self.settings.undocked_footer_height).abs() > 1.0
            {
                self.settings.undocked_footer_height = panel_h.clamp(180.0, 600.0);
                self.persist_settings();
            }
        } else {
            with_full_width(ui, |ui| {
                self.draw_videos_undocked_strip(ui);
            });
            let strip_h = ui.min_rect().height().max(UNDOCKED_VIDEOS_STRIP_H);
            note_resizable_panel_height(ui.ctx(), UNDOCKED_FOOTER_PANEL_ID, strip_h);
        }
    }

    /// Pinned bottom panel when the video queue is docked.
    pub(super) fn draw_docked_videos_panel(&mut self, ui: &mut egui::Ui) {
        // Capture before any shrink-wrapped children run (egui uses this for PanelState).
        let panel_h = finite_ui_span(ui.clip_rect().height(), 360.0).max(180.0);
        let panel_w = finite_ui_span(ui.clip_rect().width(), 800.0).max(1.0);
        let dock_log = self.settings.logs_open && self.settings.logs_docked;
        let theme = self.settings.theme.clone();
        let av1 = self.convert_mode;
        let dl_color = self.settings.mode_downloader_color.clone();
        let convert_color = self.settings.mode_convert_color.clone();
        let mode_colors = crate::theme::ModePanelColors::new(&dl_color, &convert_color);
        allocate_top_down_rect(ui, egui::vec2(panel_w, panel_h), |ui| {
            fill_allocated_rect(ui);
            let body_bottom = ui.max_rect().bottom();
            let layout = VideosQueueLayout {
                scroll_id: "rustdl_videos_dock_scroll",
                dock_log,
                log_dock_height: self.settings.log_dock_height,
                body_bottom: Some(body_bottom),
            };
            Self::draw_mode_queue_panel(
                ui,
                &theme,
                av1,
                mode_colors,
                QUEUE_MODE_PANEL_MARGIN,
                |ui| {
                    self.draw_videos_queue_body(ui, layout);
                },
            );
            consume_remaining_ui_space(ui);
        });
        consume_remaining_ui_space(ui);
        note_resizable_panel_height(ui.ctx(), VIDEOS_DOCK_PANEL_ID, panel_h);
        if !ui.ctx().input(|i| i.pointer.any_down())
            && (panel_h - self.settings.videos_dock_height).abs() > 1.0
        {
            self.settings.videos_dock_height = panel_h.clamp(180.0, 800.0);
            self.persist_settings();
        }
    }

    pub(super) fn draw_videos_window(&mut self, ctx: &egui::Context) {
        if !self.settings.videos_open {
            return;
        }
        let mut open = true;
        let window_id = egui::Id::new("rustdl_videos_float_v8");
        let init_id = window_id.with("size_init");
        let needs_default = ctx.data(|d| d.get_temp::<egui::Vec2>(init_id).is_none());
        let title = self.videos_window_title().to_owned();
        let theme = self.settings.theme.clone();
        let av1 = self.convert_mode;
        let dl_color = self.settings.mode_downloader_color.clone();
        let convert_color = self.settings.mode_convert_color.clone();
        let mode_colors = crate::theme::ModePanelColors::new(&dl_color, &convert_color);
        let mut window = egui::Window::new(title)
            .id(window_id)
            .open(&mut open)
            .min_width(480.0)
            .min_height(320.0)
            .resizable(true);
        if needs_default {
            window = window.default_size(egui::vec2(
                self.settings.video_float_width,
                self.settings.video_float_height,
            ));
            ctx.data_mut(|d| {
                d.insert_temp(init_id, egui::vec2(1.0, 1.0));
            });
        }
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        let response = window.show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            // Size from the window body (`max_rect`), not viewport `clip_rect`.
            let body_h = finite_ui_span(ui.max_rect().height(), self.settings.video_float_height)
                .max(320.0);
            let body_w = finite_ui_span(ui.max_rect().width(), self.settings.video_float_width)
                .max(480.0);
            allocate_top_down_rect(ui, egui::vec2(body_w, body_h), |ui| {
                fill_allocated_rect(ui);
                let body_bottom = ui.max_rect().bottom();
                let layout = VideosQueueLayout {
                    scroll_id: "rustdl_videos_float_v8",
                    dock_log: false,
                    log_dock_height: self.settings.log_dock_height,
                    body_bottom: Some(body_bottom),
                };
                Self::draw_mode_queue_panel(
                    ui,
                    &theme,
                    av1,
                    mode_colors,
                    QUEUE_MODE_PANEL_MARGIN,
                    |ui| {
                        self.draw_videos_queue_body(ui, layout);
                    },
                );
                consume_remaining_ui_space(ui);
            });
            consume_remaining_ui_space(ui);
        });
        if let Some(inner) = &response {
            if let Some((w, h)) = persist_resizable_window_size(
                pointer_down,
                inner.response.rect.size(),
                egui::vec2(480.0, 320.0),
                egui::vec2(2400.0, 1600.0),
                (
                    self.settings.video_float_width,
                    self.settings.video_float_height,
                ),
            ) {
                self.settings.video_float_width = w;
                self.settings.video_float_height = h;
                self.persist_settings();
            }
        }
        if !open {
            self.settings.videos_open = false;
            self.persist_settings();
        }
    }
}
