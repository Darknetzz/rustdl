//! Video queue: docked under main controls or in a floating window (Downloader and AV1).

use eframe::egui::{self, RichText};

use crate::app_parsing::human_bytes_ui;
use crate::app_state::compute_download_batch_progress;
use crate::app_ui::{
    allocate_bottom_up_rect, allocate_top_down_rect, button_group,
    button_toolbar_wrapped, compact_button_group, compact_convert_list_row,
    consume_remaining_ui_space, content_width, docked_log_lines_max_h, draw_batch_progress_bar,
    draw_queue_status_compact_row, fill_allocated_rect, finite_ui_span, height_to_bottom,
    left_button_row, note_resizable_panel_height, pin_allocated_rect, queue_docked_under_videos_log_fit,
    queue_footer_reserve, queue_list_height_from_layout, queue_list_min_scroll_h,
    queue_log_block_height, queue_panel_layout_heights, queue_status_compact,
    queue_undocked_strip_reserve, show_mode_panel, show_persisted_resizable_window,
    status_color, with_full_width,
    PersistedFloatWindowParams, BOTTOM_PANEL_MAX_H, BOTTOM_PANEL_MIN_H, DOCKED_LOG_HEADING_H,
    UNDOCKED_FOOTER_PANEL_ID, UNDOCKED_VIDEOS_STRIP_H, VIDEOS_DOCK_PANEL_ID,
};
use crate::models::ItemStatus;
use crate::theme::{BG_CANVAS, BORDER_PANEL, TEXT_MUTED};
use crate::ui_icons;

use super::log_panel::LogToolbarPlacement;
use super::PydlApp;

/// Shared layout parameters for docked and floating video queue panels.
struct VideosQueueLayout<'a> {
    scroll_id: &'a str,
    docked: bool,
    dock_log: bool,
    /// Bottom edge of the allocated queue body (set by docked/float shell before the mode panel).
    body_bottom: Option<f32>,
}

impl VideosQueueLayout<'_> {
    fn is_docked(&self) -> bool {
        self.docked
    }
}

const QUEUE_MODE_PANEL_MARGIN: egui::Margin = egui::Margin {
    left: 10.0,
    right: 10.0,
    top: 6.0,
    bottom: 6.0,
};

fn queue_footer_height_id(scroll_id: &str) -> egui::Id {
    egui::Id::new("queue_footer_h").with(scroll_id)
}

fn queue_undocked_strip_height_id() -> egui::Id {
    egui::Id::new("queue_undocked_strip_h")
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
        show_mode_panel(ui, theme, av1, colors, inner_margin, 8.0, true, add_contents).inner
    }

    /// Scrollable card list in a fixed-height region (cards align from the top).
    fn draw_queue_list_body(
        &mut self,
        ui: &mut egui::Ui,
        scroll_h: f32,
        scroll_id: &str,
        docked: bool,
        outer_scroll_h: f32,
    ) {
        let compact_convert = self.convert_mode
            && compact_convert_list_row(self.settings.compact_cards, outer_scroll_h);
        let min_h = queue_list_min_scroll_h(docked, self.convert_mode, compact_convert);
        let scroll_h = finite_ui_span(scroll_h, min_h).max(min_h);
        if scroll_h < 1.0 {
            return;
        }
        let w = crate::app_ui::clip_bounded_width(ui);
        allocate_top_down_rect(ui, egui::vec2(w, scroll_h), |ui| {
            ui.set_min_height(scroll_h);
            self.constrain_panel_content(ui);
            let inner_h = finite_ui_span(ui.max_rect().height(), scroll_h).clamp(1.0, scroll_h);
            if self.convert_mode {
                self.draw_convert_queue_list_scroll(ui, inner_h, min_h, scroll_h);
            } else {
                self.draw_downloader_queue_list_scroll(ui, inner_h, scroll_id, docked, scroll_h);
            }
        });
    }

    fn clear_inactive_downloader_queue(&mut self) {
        use crate::service::core::QueueClearFilter;
        self.download_core_action(|core| {
            core.clear_queue(QueueClearFilter::Inactive);
        });
    }

    fn draw_dl_queue_transport_group(
        &mut self,
        g: &mut crate::app_ui::ButtonGroup<'_>,
        has_idle_ready: bool,
        can_cancel_all: bool,
        cancel_all_ready: &mut bool,
        cancel_all_remove: &mut bool,
    ) {
        if has_idle_ready
            && g.success(
                &format!("{} Start downloads", ui_icons::USE_DOWNLOADS),
                true,
            )
            .clicked()
        {
            self.start_downloads();
        }
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
        g.cancel_all_menu(can_cancel_all, cancel_all_ready, cancel_all_remove);
    }

    fn draw_dl_queue_io_group(
        &mut self,
        g: &mut crate::app_ui::ButtonGroup<'_>,
        export_queue: &mut bool,
        import_queue: &mut bool,
    ) {
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
                *export_queue = true;
            }
            if ui
                .add_enabled(
                    !self.add_in_progress,
                    egui::Button::new(format!("{} Import queue", ui_icons::IMPORT_FILE)),
                )
                .on_hover_text("Load URLs from a .txt file directly into the download queue")
                .clicked()
            {
                *import_queue = true;
            }
        });
    }

    fn draw_dl_queue_maint_group(&mut self, g: &mut crate::app_ui::ButtonGroup<'_>) {
        if !self.selected_item_ids.is_empty() {
            if g.danger(
                &format!(
                    "{} Remove selected ({})",
                    ui_icons::REMOVE,
                    self.selected_item_ids.len()
                ),
                true,
            )
            .clicked()
            {
                self.remove_selected_items();
            }
            if self.status_failed > 0
                && g.warning(&format!("{} Retry selected", ui_icons::RETRY), true)
                    .clicked()
            {
                self.retry_selected_failed();
            }
        }
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
        if self.status_failed > 0
            && !self.add_in_progress
            && g.secondary(&format!("{} Refetch all failed", ui_icons::RETRY), true)
                .on_hover_text(
                    "Run yt-dlp metadata again for every failed row that still has a source URL \
                     (use when a stale format/title, not the download itself, caused the failure).",
                )
                .clicked()
        {
            self.refetch_failed_items();
        }
        if g
            .warning(
                &format!("{} Re-check saved files", ui_icons::RECHECK),
                self.has_ffprobe && !self.settings.ffmpeg_extract_audio_mp3,
            )
            .on_hover_text(
                "Run ffprobe on each finished download on disk; mark rows failed if video or audio is missing.",
            )
            .on_disabled_hover_text("Requires ffprobe. Disabled while MP3 extraction is enabled.")
            .clicked()
        {
            self.recheck_all_saved_downloads();
        }
        if g.danger(&format!("{} Clear list", ui_icons::CLEAR_QUEUE), true)
            .clicked()
        {
            self.clear_inactive_downloader_queue();
        }
    }

    /// Pause/import-export/recheck/clear — split or fused groups for narrow footers.
    fn draw_downloader_queue_actions(&mut self, ui: &mut egui::Ui, compact: bool, fused: bool) {
        let mut export_queue = false;
        let mut import_queue = false;
        let mut cancel_all_ready = false;
        let mut cancel_all_remove = false;
        let can_cancel_all = self.status_queued > 0 || self.status_active > 0;
        let has_idle_ready = self
            .items
            .iter()
            .any(|x| x.status == ItemStatus::Idle && x.error.is_none());
        let draw = |ui: &mut egui::Ui,
                    id: &str,
                    add: &mut dyn FnMut(&mut crate::app_ui::ButtonGroup<'_>)| {
            if compact {
                compact_button_group(ui, id, |g| add(g));
            } else {
                button_group(ui, id, |g| add(g));
            }
        };
        if fused {
            draw(ui, "dl_queue_actions", &mut |g| {
                self.draw_dl_queue_transport_group(
                    g,
                    has_idle_ready,
                    can_cancel_all,
                    &mut cancel_all_ready,
                    &mut cancel_all_remove,
                );
                self.draw_dl_queue_io_group(g, &mut export_queue, &mut import_queue);
                self.draw_dl_queue_maint_group(g);
            });
        } else {
            draw(ui, "dl_queue_transport", &mut |g| {
                self.draw_dl_queue_transport_group(
                    g,
                    has_idle_ready,
                    can_cancel_all,
                    &mut cancel_all_ready,
                    &mut cancel_all_remove,
                );
            });
            draw(ui, "dl_queue_io", &mut |g| {
                self.draw_dl_queue_io_group(g, &mut export_queue, &mut import_queue);
            });
            draw(ui, "dl_queue_maint", &mut |g| {
                self.draw_dl_queue_maint_group(g);
            });
        }
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
        self.draw_downloader_queue_actions(ui, compact, true);
    }

    fn draw_downloader_queue_action_groups(&mut self, ui: &mut egui::Ui, compact: bool) {
        self.draw_downloader_queue_actions(ui, compact, false);
    }

    fn draw_downloader_queue_list_scroll(
        &mut self,
        ui: &mut egui::Ui,
        scroll_h: f32,
        scroll_id: &str,
        docked: bool,
        outer_scroll_h: f32,
    ) {
        let min_h = queue_list_min_scroll_h(docked, false, false);
        let scroll_h = finite_ui_span(scroll_h, min_h).max(1.0);
        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .animated(false)
            .drag_to_scroll(true)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                ui.set_width(crate::app_ui::clip_bounded_width(ui));
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
                    self.draw_grouped_cards(ui, outer_scroll_h);
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
            }
        });
    }

    pub(super) fn draw_video_queue_controls(&mut self, ui: &mut egui::Ui) {
        button_toolbar_wrapped(ui, |ui| self.draw_video_queue_controls_inner(ui, false));
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
                self.draw_video_queue_controls_inner(ui, false);
                if self.convert_mode {
                    self.draw_convert_queue_action_groups(ui, false);
                } else {
                    self.draw_downloader_queue_action_groups(ui, false);
                }
            });
        } else {
            left_button_row(ui, |ui| {
                ui.label(RichText::new(heading).strong());
                self.draw_video_queue_controls_inner(ui, false);
            });
            left_button_row(ui, |ui| {
                if self.convert_mode {
                    self.draw_convert_queue_action_groups(ui, false);
                } else {
                    self.draw_downloader_queue_action_fused(ui, false);
                }
            });
        }
    }

    fn draw_queue_search_row(&mut self, ui: &mut egui::Ui) {
        let hint = if self.convert_mode {
            "Filename, path…"
        } else {
            "Title, URL, uploader…"
        };
        let cw = crate::app_ui::clip_bounded_width(ui).max(1.0);
        let search_w = (cw * 0.35).clamp(160.0, 420.0);
        ui.horizontal(|ui| {
            ui.label("Search");
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.queue_search)
                    .hint_text(hint)
                    .desired_width(search_w),
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

    /// Status row, scrollable cards, toolbar, optional docked log — bottom stack pinned to body bottom.
    fn draw_videos_queue_body(&mut self, ui: &mut egui::Ui, layout: VideosQueueLayout<'_>) {
        self.constrain_panel_content(ui);
        ui.spacing_mut().item_spacing.y = 3.0;
        let body_bottom = layout
            .body_bottom
            .filter(|y| y.is_finite())
            .unwrap_or_else(|| {
                if ui.max_rect().bottom().is_finite() {
                    ui.max_rect().bottom()
                } else {
                    ui.clip_rect().bottom()
                }
            });

        self.draw_queue_search_row(ui);

        let content_top_after_search = ui.cursor().min.y;
        let cw = crate::app_ui::clip_bounded_width(ui).max(1.0);
        let ui_scale = crate::config::snap_ui_scale(self.settings.ui_scale);
        let resizing = ui.ctx().input(|i| i.pointer.any_down());
        let measured_footer = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(queue_footer_height_id(layout.scroll_id)));
        let footer_h = queue_footer_reserve(
            cw,
            self.convert_mode,
            layout.is_docked(),
            measured_footer,
            ui_scale,
            resizing,
        );
        // Prefer at least one queue row; shrink the docked log before collapsing the list.
        let min_list_h = queue_list_min_scroll_h(layout.is_docked(), self.convert_mode, false);
        let (_, log_block_est) = if layout.dock_log {
            queue_docked_under_videos_log_fit(
                true,
                self.settings.log_dock_height,
                body_bottom,
                content_top_after_search,
                footer_h,
                min_list_h,
            )
        } else {
            (0.0, 0.0)
        };
        let predicted_list_h = queue_list_height_from_layout(
            content_top_after_search,
            body_bottom,
            footer_h,
            log_block_est,
        );
        let status_compact = queue_status_compact(predicted_list_h, self.convert_mode);

        if self.convert_mode {
            if !self.convert_items.is_empty() {
                if status_compact {
                    self.draw_convert_queue_status_row_compact(ui);
                } else {
                    self.draw_convert_queue_status_row(ui);
                    self.draw_convert_batch_progress_row(ui);
                    self.draw_convert_batch_summary_row(ui);
                }
            }
        } else if !self.items.is_empty() {
            if status_compact {
                self.draw_downloader_queue_status_row_compact(ui);
            } else {
                self.draw_downloader_queue_status_row(ui);
                self.draw_download_batch_progress_row(ui);
            }
        }

        let content_top = ui.cursor().min.y;
        // Re-fit after status rows so list vs log still honor min_list_h.
        let (log_lines, log_block_est) = if layout.dock_log {
            queue_docked_under_videos_log_fit(
                true,
                self.settings.log_dock_height,
                body_bottom,
                content_top,
                footer_h,
                min_list_h,
            )
        } else {
            (0.0, 0.0)
        };
        let show_dock_log = layout.dock_log && log_block_est > 1.0;
        let (list_h, stack_h) =
            queue_panel_layout_heights(content_top, body_bottom, footer_h, log_block_est);
        self.draw_queue_list_body(ui, list_h, layout.scroll_id, layout.docked, list_h);

        allocate_bottom_up_rect(ui, body_bottom, cw, stack_h, |ui| {
            ui.add_space(2.0);
            let footer_top = ui.cursor().min.y;
            self.constrain_panel_content(ui);
            self.draw_videos_footer_toolbar(ui, layout.is_docked() || self.convert_mode);
            let footer_measured = (ui.min_rect().max.y - footer_top).max(0.0) + 2.0;
            ui.ctx().data_mut(|d| {
                d.insert_temp(queue_footer_height_id(layout.scroll_id), footer_measured);
            });
            if show_dock_log {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);
                let remaining = height_to_bottom(ui, body_bottom) - DOCKED_LOG_HEADING_H;
                let max_log = docked_log_lines_max_h(remaining.max(0.0)).max(log_lines);
                self.draw_docked_log_under_videos(ui, max_log);
            }
        });
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
        let (heading, parts) = self.downloader_queue_status_parts();
        match crate::app_ui::draw_queue_status_row(
            ui,
            &heading,
            &parts,
            self.queue_group_focus.is_some(),
        ) {
            Some(crate::app_ui::QueueStatusRowAction::ShowAll) => self.queue_group_focus = None,
            Some(crate::app_ui::QueueStatusRowAction::Focus(group)) => {
                self.focus_queue_group(group);
            }
            None => {}
        }
    }

    fn downloader_queue_status_parts(&self) -> (String, Vec<crate::app_ui::QueueStatusPart>) {
        let mut parts: Vec<crate::app_ui::QueueStatusPart> = Vec::new();
        if self.status_resolving > 0 {
            parts.push(crate::app_ui::QueueStatusPart {
                name: "resolving",
                count: self.status_resolving,
                color: status_color(ItemStatus::Resolving),
                group: "Resolving",
            });
        }
        if self.status_ready > 0 {
            parts.push(crate::app_ui::QueueStatusPart {
                name: "ready",
                count: self.status_ready,
                color: status_color(ItemStatus::Idle),
                group: "Ready",
            });
        }
        if self.status_queued > 0 {
            parts.push(crate::app_ui::QueueStatusPart {
                name: "queued",
                count: self.status_queued,
                color: status_color(ItemStatus::Queued),
                group: "Active",
            });
        }
        if self.status_active > 0 {
            parts.push(crate::app_ui::QueueStatusPart {
                name: "active",
                count: self.status_active,
                color: status_color(ItemStatus::Downloading),
                group: "Active",
            });
        }
        if self.status_done > 0 {
            parts.push(crate::app_ui::QueueStatusPart {
                name: "done",
                count: self.status_done,
                color: status_color(ItemStatus::Done),
                group: "Done",
            });
        }
        if self.status_failed > 0 {
            parts.push(crate::app_ui::QueueStatusPart {
                name: "failed",
                count: self.status_failed,
                color: status_color(ItemStatus::Failed),
                group: "Issues",
            });
        }
        let heading = if self.items.is_empty() {
            "Downloads:".to_owned()
        } else {
            format!("Downloads ({}):", self.items.len())
        };
        (heading, parts)
    }

    pub(super) fn draw_downloader_queue_status_row_compact(&mut self, ui: &mut egui::Ui) {
        let (heading, parts) = self.downloader_queue_status_parts();
        draw_queue_status_compact_row(ui, &heading, &parts);
    }

    pub(super) fn draw_download_batch_progress_row(&mut self, ui: &mut egui::Ui) {
        crate::app_ui::with_full_width(ui, |ui| {
            self.draw_download_batch_progress_row_inner(ui);
        });
    }

    fn draw_download_batch_progress_row_inner(&mut self, ui: &mut egui::Ui) {
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
        crate::app_ui::with_full_width(ui, |ui| {
            self.draw_convert_batch_progress_row_inner(ui);
        });
    }

    fn draw_convert_batch_progress_row_inner(&self, ui: &mut egui::Ui) {
        let progress = self.convert_batch_progress;
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
            .stroke(egui::Stroke::new(1.0_f32, BORDER_PANEL))
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
                let max_log = docked_log_lines_max_h((budget - DOCKED_LOG_HEADING_H).max(0.0));
                self.draw_docked_activity_log_body(ui, max_log, LogToolbarPlacement::DockFooter);
            });
    }

    /// Pinned footer when the queue is undocked (`TopBottomPanel` body).
    pub(super) fn draw_queue_footer(&mut self, ui: &mut egui::Ui) {
        let log_docked = self.settings.logs_open && self.settings.logs_docked;
        if log_docked {
            let panel_h = finite_ui_span(
                ui.clip_rect().height(),
                self.settings.undocked_footer_height,
            )
            .max(crate::app_ui::BOTTOM_PANEL_MIN_H_WITH_DOCKED_LOG);
            let panel_w = finite_ui_span(ui.clip_rect().width(), 800.0).max(1.0);
            allocate_top_down_rect(ui, egui::vec2(panel_w, panel_h), |ui| {
                fill_allocated_rect(ui);
                let body_bottom = ui.max_rect().bottom();
                let cw = content_width(ui).max(1.0);
                let measured_strip = ui
                    .ctx()
                    .data(|d| d.get_temp::<f32>(queue_undocked_strip_height_id()));
                let strip_h = queue_undocked_strip_reserve(measured_strip);
                let log_block = queue_log_block_height(true, self.settings.log_dock_height, false);
                let stack_h = strip_h + log_block;
                allocate_bottom_up_rect(ui, body_bottom, cw, stack_h, |ui| {
                    let strip_top = ui.cursor().min.y;
                    self.draw_videos_undocked_strip(ui);
                    let strip_measured =
                        (ui.min_rect().max.y - strip_top).max(UNDOCKED_VIDEOS_STRIP_H);
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(queue_undocked_strip_height_id(), strip_measured);
                    });
                    self.draw_docked_log_only_section(ui);
                });
                consume_remaining_ui_space(ui);
            });
            consume_remaining_ui_space(ui);
            note_resizable_panel_height(ui.ctx(), UNDOCKED_FOOTER_PANEL_ID, panel_h);
            if !ui.ctx().input(|i| i.pointer.any_down())
                && (panel_h - self.settings.undocked_footer_height).abs() > 1.0
            {
                self.settings.undocked_footer_height = panel_h.clamp(
                    crate::app_ui::BOTTOM_PANEL_MIN_H_WITH_DOCKED_LOG,
                    BOTTOM_PANEL_MAX_H,
                );
                self.schedule_settings_save();
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
        let dock_log = self.settings.logs_open && self.settings.logs_docked;
        let panel_min = if dock_log {
            crate::app_ui::BOTTOM_PANEL_MIN_H_WITH_DOCKED_LOG
        } else {
            BOTTOM_PANEL_MIN_H
        };
        let panel_h = finite_ui_span(ui.clip_rect().height(), 360.0).max(panel_min);
        let panel_w = finite_ui_span(ui.clip_rect().width(), 800.0).max(1.0);
        let theme = self.settings.theme.clone();
        let av1 = self.convert_mode;
        let dl_color = self.settings.mode_downloader_color.clone();
        let convert_color = self.settings.mode_convert_color.clone();
        let mode_colors = crate::theme::ModePanelColors::new(&dl_color, &convert_color);
        allocate_top_down_rect(ui, egui::vec2(panel_w, panel_h), |ui| {
            fill_allocated_rect(ui);
            pin_allocated_rect(ui);
            self.constrain_panel_content(ui);
            let body_bottom = ui.max_rect().bottom();
            let layout = VideosQueueLayout {
                scroll_id: "rustdl_videos_dock_scroll",
                docked: true,
                dock_log,
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
            self.settings.videos_dock_height = panel_h.clamp(panel_min, BOTTOM_PANEL_MAX_H);
            self.schedule_settings_save();
        }
    }

    pub(super) fn draw_videos_window(&mut self, ctx: &egui::Context) {
        if !self.settings.videos_open {
            return;
        }
        let mut open = true;
        let title = self.videos_window_title().to_owned();
        let theme = self.settings.theme.clone();
        let av1 = self.convert_mode;
        let dl_color = self.settings.mode_downloader_color.clone();
        let convert_color = self.settings.mode_convert_color.clone();
        let mode_colors = crate::theme::ModePanelColors::new(&dl_color, &convert_color);
        let params = PersistedFloatWindowParams {
            title,
            window_id: egui::Id::new("rustdl_videos_float_v8"),
            min_size: egui::vec2(480.0, 320.0),
            max_size: egui::vec2(2400.0, 1600.0),
            default_size: egui::vec2(
                self.settings.video_float_width,
                self.settings.video_float_height,
            ),
            stored_size: (
                self.settings.video_float_width,
                self.settings.video_float_height,
            ),
            item_spacing_y: 6.0,
        };
        let outcome = show_persisted_resizable_window(ctx, &mut open, &params, |ui| {
            pin_allocated_rect(ui);
            let body_bottom = ui.max_rect().bottom();
            let layout = VideosQueueLayout {
                scroll_id: "rustdl_videos_float_v8",
                docked: false,
                dock_log: false,
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
        });
        if let Some((w, h)) = outcome.size {
            let prev_w = self.settings.video_float_width;
            let prev_h = self.settings.video_float_height;
            if (prev_w - w).abs() > 0.5 || (prev_h - h).abs() > 0.5 {
                self.settings.video_float_width = w;
                self.settings.video_float_height = h;
                self.schedule_settings_save();
            }
        }
        if !outcome.open {
            self.settings.videos_open = false;
            self.persist_settings();
        }
    }
}
