use super::*;
use crate::app_ui::{
    bounded_ui_height, button_group, button_toolbar_wrapped, content_width,
    dock_panel_horizontal_frame, draw_mode_nav_bar, draw_navbar_status_badge, left_button_row,
    patch_resizable_panel_state_height, show_mode_panel, with_full_width, UNDOCKED_FOOTER_PANEL_ID,
    UNDOCKED_VIDEOS_STRIP_H, VIDEOS_DOCK_PANEL_ID,
};
impl eframe::App for PydlApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        {
            let shared = self.shared_core.clone();
            let core = shared.lock();
            core_sync::sync_core_to_app(&core, self);
        }
        #[cfg(windows)]
        {
            crate::win_icon::apply_native_window_icons(frame, &app_icon::window_icon());
            crate::win_window::maybe_restore_main_window(frame, ctx);
        }
        #[cfg(not(windows))]
        let _ = frame;
        self.ensure_main_viewport_visible(ctx);
        ctx.set_zoom_factor(self.settings.ui_scale.clamp(0.85, 1.5));
        if let Some(text) = self.deferred_menu_paste_urls.take() {
            ctx.input_mut(|inp| inp.events.push(egui::Event::Paste(text)));
        }
        if let Some(text) = self.deferred_menu_paste_output_dir.take() {
            ctx.input_mut(|inp| inp.events.push(egui::Event::Paste(text)));
        }
        if let Some(text) = self.deferred_menu_paste_convert_paths.take() {
            ctx.input_mut(|inp| inp.events.push(egui::Event::Paste(text)));
        }
        self.maybe_flush_queue_save();
        self.maybe_flush_convert_queue_save();
        self.maybe_flush_log_save();
        self.process_events(ctx);
        self.poll_watch_folders();
        #[cfg(windows)]
        {
            self.maybe_install_win_browser_drop_target(frame);
            self.drain_win_browser_url_drops(ctx);
        }
        if self.convert_mode {
            self.apply_dropped_convert_paths(ctx);
        } else {
            self.apply_dropped_shortcut_files(ctx);
        }
        self.handle_viewport_close_request(ctx);
        self.maybe_adjust_videos_dock_for_viewport(ctx);
        if self.exit_pending_after_cancel && !self.exit_work_in_progress() {
            self.exit_pending_after_cancel = false;
            self.finish_exit(ctx);
        }
        self.poll_done_file_lookup();
        self.poll_output_disk_space();
        self.poll_system_usage();
        ctx.request_repaint_after(std::time::Duration::from_millis(1500));
        if let Some(deadline) = self.auto_add_after {
            let now = ctx.input(|i| i.time);
            if !self.add_in_progress && now >= deadline {
                let valid = self.collect_valid_new_lines();
                if !valid.is_empty() {
                    self.queue_urls_for_resolve(valid);
                    self.clear_input_urls_with_summary_hold(now);
                } else {
                    self.refresh_input_line_info();
                    if input_lines::is_only_duplicate_lines(&self.input_line_info) {
                        self.clear_input_urls_with_summary_hold(now);
                    }
                }
                self.auto_add_after = None;
            }
        }
        let trigger_add = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
        let trigger_download = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::D));
        let trigger_settings =
            ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma));
        let trigger_focus_search =
            ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F));
        let trigger_toggle_log = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::L));
        let trigger_palette = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K));
        let trigger_escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if trigger_settings {
            self.settings_open = true;
        }
        if trigger_focus_search && !self.convert_mode {
            self.focus_queue_search = true;
        }
        if trigger_toggle_log {
            self.settings.logs_open = !self.settings.logs_open;
            self.persist_settings();
        }
        if trigger_palette {
            self.command_palette_open = true;
            self.command_palette_query.clear();
        }
        if trigger_escape {
            if self.command_palette_open {
                self.command_palette_open = false;
            } else {
                self.settings_open = false;
                self.about_open = false;
                if !self.settings.videos_docked {
                    self.settings.videos_open = false;
                }
                if !self.settings.logs_docked {
                    self.settings.logs_open = false;
                }
            }
        }

        if self.settings.videos_docked {
            egui::TopBottomPanel::bottom(VIDEOS_DOCK_PANEL_ID)
                .resizable(true)
                .default_height(self.settings.videos_dock_height)
                .height_range(180.0..=800.0)
                .frame(dock_panel_horizontal_frame())
                .show(ctx, |ui| {
                    self.draw_docked_videos_panel(ui);
                });
            patch_resizable_panel_state_height(ctx, VIDEOS_DOCK_PANEL_ID);
        } else {
            let log_docked = self.settings.logs_open && self.settings.logs_docked;
            let (default_h, height_range, resizable) = if log_docked {
                (self.settings.undocked_footer_height, 180.0..=600.0, true)
            } else {
                (UNDOCKED_VIDEOS_STRIP_H, 72.0..=140.0, false)
            };
            egui::TopBottomPanel::bottom(UNDOCKED_FOOTER_PANEL_ID)
                .resizable(resizable)
                .default_height(default_h)
                .height_range(height_range)
                .frame(dock_panel_horizontal_frame())
                .show(ctx, |ui| {
                    self.draw_queue_footer(ui);
                });
            patch_resizable_panel_state_height(ctx, UNDOCKED_FOOTER_PANEL_ID);
        }

        egui::CentralPanel::default()
            .frame(content_panel_frame())
            .show(ctx, |ui| {
                self.constrain_content(ui);
                self.sync_theme_if_needed(ctx);
                self.draw_main_header(ui);
                self.draw_config_load_banner(ui);
                let (dl_nav, av1_nav) = draw_mode_nav_bar(
                    ui,
                    &self.settings.theme,
                    !self.convert_mode,
                    self.convert_mode,
                    crate::theme::ModePanelColors::new(
                        &self.settings.mode_downloader_color,
                        &self.settings.mode_convert_color,
                    ),
                );
                if dl_nav {
                    self.set_app_mode(false);
                }
                if av1_nav {
                    self.set_app_mode(true);
                }
                let scroll_h = bounded_ui_height(ui, 100.0).max(100.0);
                egui::ScrollArea::vertical()
                    .id_salt("rustdl_main_body_v1")
                    .auto_shrink([false, false])
                    .max_height(scroll_h)
                    .drag_to_scroll(true)
                    .show(ui, |ui| {
                ui.label(
                    "Add URLs to load previews; start downloads to see progress on each card.",
                );
                if self.settings.show_first_run_hint {
                    alert_warning(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(
                                    "Welcome to rustdl — set your output folder, confirm yt-dlp is on PATH, \
                                     and open Settings for download presets and quality options.",
                                )
                                .color(ALERT_WARNING_TEXT),
                            );
                            ui.horizontal(|ui| {
                                left_button_row(ui, |ui| {
                                    button_group(ui, "welcome_actions", |g| {
                                        if g
                                            .warning(
                                                &format!("{} Open Settings", ui_icons::SETTINGS),
                                                true,
                                            )
                                            .clicked()
                                        {
                                            self.settings_open = true;
                                        }
                                        if g
                                            .warning(
                                                &format!("{} Dismiss", ui_icons::DISMISS),
                                                true,
                                            )
                                            .clicked()
                                        {
                                            self.settings.show_first_run_hint = false;
                                            self.persist_settings();
                                        }
                                    });
                                });
                            });
                        });
                    });
                }
                if !self.has_yt_dlp || !self.has_ffmpeg || !self.has_ffprobe {
                    ui.colored_label(
                        LOG_COLOR_WARN,
                        "Setup hint: configure missing tools in Settings -> Executables.",
                    );
                }
                #[cfg(not(windows))]
                ui.label(
                    RichText::new(
                        "Tip: browser drag-and-drop for URLs is supported on Windows only; paste URLs or drop .url/.txt files on other platforms.",
                    )
                    .small()
                    .color(crate::theme::TEXT_MUTED),
                );
                ui.separator();
                let theme = self.settings.theme.clone();
                let dl_color = self.settings.mode_downloader_color.clone();
                let convert_color = self.settings.mode_convert_color.clone();
                let mode_colors =
                    crate::theme::ModePanelColors::new(&dl_color, &convert_color);
                if self.convert_mode {
                    show_mode_panel(
                        ui,
                        &theme,
                        true,
                        mode_colors,
                        egui::Margin::same(12.0),
                        10.0,
                        |ui| {
                            self.draw_convert_panel(ui);
                        },
                    );
                } else {
                show_mode_panel(
                    ui,
                    &theme,
                    false,
                    mode_colors,
                    egui::Margin::same(12.0),
                    10.0,
                    |ui| {
                with_full_width(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Downloader").heading());
                    ui.label(
                        RichText::new("Queue and download media with yt-dlp.")
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                });
                ui.separator();
                ui.label("URLs (one per line)");
                #[cfg(not(windows))]
                {
                    left_button_row(ui, |ui| {
                        button_group(ui, "paste_urls", |g| {
                            if g.secondary(&format!("{} Paste URLs", ui_icons::ADD), true).clicked()
                            {
                                if let Ok(mut clip) = arboard::Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        if !text.trim().is_empty() {
                                            self.extend_input_urls_with_lines(
                                                parse_urls_from_text_blob(&text),
                                                Some(ctx.input(|i| i.time)),
                                            );
                                            self.refresh_input_line_info();
                                        }
                                    }
                                }
                            }
                        });
                    });
                }
                let prev_url_snapshot = self.input_urls_snapshot.clone();
                let url_edit = ui.add_sized(
                    [content_width(ui), 120.0],
                    egui::TextEdit::multiline(&mut self.input_urls)
                        .hint_text(
                            "https://... — paste, drag from browser, or drop .url / .webloc / list (.txt, .m3u)",
                        ),
                );
                attach_paste_context_menu(&url_edit, &mut self.deferred_menu_paste_urls);
                if url_edit.changed() {
                    let paste_event = ctx.input(|i| {
                        i.events
                            .iter()
                            .any(|e| matches!(e, egui::Event::Paste(_)))
                    });
                    input_lines::append_newline_after_pasted_valid_url(
                        &mut self.input_urls,
                        &prev_url_snapshot,
                        paste_event,
                        url_edit.has_focus(),
                    );
                    self.refresh_input_line_info();
                    self.input_line_info_hold_until = None;
                    if self.settings.auto_add_pasted_urls {
                        self.auto_add_after = Some(ctx.input(|i| i.time + 0.7));
                    } else {
                        self.auto_add_after = None;
                    }
                }
                let now = ctx.input(|i| i.time);
                let summary_lines = if self.input_line_info.is_empty()
                    && self
                        .input_line_info_hold_until
                        .is_some_and(|until| now < until)
                {
                    &self.input_line_info_hold
                } else {
                    &self.input_line_info
                };
                draw_input_line_summary(ui, summary_lines);
                log_panel::draw_input_line_preview(ui, summary_lines);

                left_button_row(ui, |ui| {
                    let mut import_urls = false;
                    button_group(ui, "add_urls", |g| {
                        if g.success(
                            &format!("{} Add URLs", ui_icons::ADD),
                            !self.add_in_progress,
                        )
                        .clicked()
                        {
                            self.add_urls(ctx.input(|i| i.time));
                        }
                        g.import_export_menu(!self.add_in_progress, |ui| {
                            if ui
                                .add_enabled(
                                    !self.add_in_progress,
                                    egui::Button::new(format!(
                                        "{} Import file (.txt/.csv)",
                                        ui_icons::IMPORT_FILE
                                    )),
                                )
                                .clicked()
                            {
                                import_urls = true;
                            }
                        });
                    });
                    if import_urls {
                        self.import_urls_from_file();
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if self.add_in_progress {
                        ui.spinner();
                        let mut msg = format!(
                            "Adding URLs ({}/{})",
                            self.add_processed_urls, self.add_total_urls
                        );
                        if let Some(current) = &self.add_current_url {
                            if !current.is_empty() {
                                let short = current.chars().take(56).collect::<String>();
                                let suffix = if current.chars().count() > 56 {
                                    "..."
                                } else {
                                    ""
                                };
                                msg.push_str(&format!(" - fetching metadata for {short}{suffix}"));
                            }
                        }
                        ui.label(RichText::new(msg).small().color(Color32::LIGHT_BLUE));
                    }
                });
                }); // pinned URL block

                self.constrain_content(ui);

                ui.separator();
                let has_idle_items = self
                    .items
                    .iter()
                    .any(|x| x.status == ItemStatus::Idle && x.error.is_none());

                let options_id = ui.make_persistent_id("downloader_options");
                let options_expanded = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    options_id,
                    self.settings.downloader_options_expanded,
                )
                .is_open();
                let options_header = downloader_options_collapsing_label(
                    &self.output_dir,
                    &self.settings.active_profile,
                    options_expanded,
                );
                let options_resp = egui::CollapsingHeader::new(options_header)
                    .id_salt("downloader_options")
                    .default_open(self.settings.downloader_options_expanded)
                    .show(ui, |ui| {
                        self.constrain_content(ui);
                        ui.horizontal(|ui| {
                            ui.label("Output folder");
                            let path_w = content_width(ui).max(120.0);
                            let output_dir_edit = ui.add(
                                egui::TextEdit::singleline(&mut self.output_dir)
                                    .desired_width(path_w),
                            );
                            attach_paste_context_menu(
                                &output_dir_edit,
                                &mut self.deferred_menu_paste_output_dir,
                            );
                            if output_dir_edit.changed() {
                                self.persist_settings();
                                self.last_done_lookup_poll = None;
                                self.invalidate_output_disk_space();
                            }
                        });
                        left_button_row(ui, |ui| {
                            button_group(ui, "output_dir", |g| {
                                if g.secondary(
                                    &format!("{} Use Downloads", ui_icons::USE_DOWNLOADS),
                                    true,
                                )
                                .clicked()
                                {
                                    self.output_dir =
                                        default_downloads().to_string_lossy().to_string();
                                    self.persist_settings();
                                    self.last_done_lookup_poll = None;
                                    self.invalidate_output_disk_space();
                                }
                            });
                        });
                        ui.add_space(4.0);
                        let profiles = crate::profiles::all_profiles(&self.profile_store);
                        if !profiles.is_empty() {
                            ui.horizontal(|ui| {
                                ui.label("Profile");
                                egui::ComboBox::from_id_salt("toolbar_profile")
                                    .selected_text(self.settings.active_profile.clone())
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
                                                    crate::profiles::find_profile(
                                                        &self.profile_store,
                                                        &p.name,
                                                    )
                                                {
                                                    self.apply_download_profile(&prof);
                                                }
                                            }
                                        }
                                    });
                            });
                        }
                    });
                if options_resp.header_response.changed() {
                    if let Some(state) =
                        egui::collapsing_header::CollapsingState::load(ui.ctx(), options_id)
                    {
                        let open = state.is_open();
                        if open != self.settings.downloader_options_expanded {
                            self.settings.downloader_options_expanded = open;
                            self.persist_settings();
                        }
                    }
                }

                button_toolbar_wrapped(ui, |ui| {
                    let downloads_active =
                        self.status_queued > 0 || self.status_active > 0;
                    if has_idle_items || self.downloads_paused || downloads_active {
                        button_group(ui, "dl_actions", |g| {
                            if has_idle_items
                                && !self.downloads_paused
                                && g
                                    .success(
                                        &format!("{} Start downloads", ui_icons::USE_DOWNLOADS),
                                        true,
                                    )
                                    .clicked()
                            {
                                self.start_downloads();
                            }
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
                            } else if downloads_active
                                && g
                                    .warning(
                                        &format!("{} Pause downloads", ui_icons::CANCEL_TO_READY),
                                        true,
                                    )
                                    .clicked()
                            {
                                self.pause_all_downloads();
                            }
                        });
                    }
                    if !self.selected_item_ids.is_empty() {
                        button_group(ui, "dl_sel", |g| {
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
                                && g.warning(
                                    &format!("{} Retry selected", ui_icons::RETRY),
                                    true,
                                )
                                .clicked()
                            {
                                self.retry_selected_failed();
                            }
                        });
                    }
                    if self.status_failed > 0 {
                        button_group(ui, "dl_retry_all", |g| {
                            if g.warning(
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
                        });
                    }
                });
                if trigger_add && !self.add_in_progress {
                    self.add_urls(ctx.input(|i| i.time));
                }
                if trigger_download && has_idle_items {
                    self.start_downloads();
                }
                }); // mode panel
                } // downloader mode
        });
                    }); // central panel

        self.draw_settings_window(ctx);
        self.draw_about_window(ctx);
        self.draw_command_palette(ctx);
        if !self.settings.videos_docked {
            self.draw_videos_window(ctx);
        }
        if self.settings.logs_open && !self.settings.logs_docked {
            self.draw_logs_window(ctx);
        }
        self.maybe_notify_session_complete();

        self.input_urls_snapshot = self.input_urls.clone();
        self.draw_session_restore_dialog(ctx);
        self.draw_exit_confirm_dialog(ctx);
        self.request_repaint_if_background_busy(ctx);
        {
            let shared = self.shared_core.clone();
            core_sync::push_app_to_core(self, &shared);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_queue_to_disk();
        self.flush_convert_queue_to_disk();
        self.flush_log_to_disk();
        self.persist_settings();
    }
}

impl PydlApp {
    /// Logo, title, status, tool checks, and disk on the left; log / Settings / Exit on the right.
    fn draw_main_header(&mut self, ui: &mut egui::Ui) {
        self.constrain_content(ui);
        with_full_width(ui, |ui| {
            let row_w = ui.available_width();
            const HEADER_WIDE_BREAKPOINT: f32 = 1040.0;
            if row_w >= HEADER_WIDE_BREAKPOINT {
                self.draw_main_header_wide(ui, row_w);
            } else {
                self.draw_main_header_narrow(ui);
            }
        });
    }

    fn draw_main_header_branding(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.x = 12.0;
        let sz = egui::vec2(40.0, 40.0);
        let img = ui.add(
            egui::Image::new(egui::load::SizedTexture::new(self.logo.id(), sz))
                .sense(egui::Sense::click()),
        );
        let title =
            ui.add(egui::Label::new(RichText::new("rustdl").heading()).sense(egui::Sense::click()));
        let header = img
            .union(title)
            .on_hover_text("About rustdl — click to open");
        if header.clicked() {
            self.about_open = true;
        }
    }

    fn draw_main_header_web_and_status(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        if self.settings.web_ui_enabled {
            let url = crate::service::web::web_ui_browser_url(&self.settings.web_bind_address);
            let running = self.web_server.is_some();
            if draw_web_ui_header_button(ui, running, &url) {
                self.open_web_ui_in_browser();
            }
        }
        let navbar = crate::app_ui::derive_navbar_status(self.navbar_status_inputs());
        draw_navbar_status_badge(ui, &navbar);
    }

    fn draw_main_header_tool_checks(&mut self, ui: &mut egui::Ui, compact: bool) {
        ui.spacing_mut().item_spacing.x = 10.0;
        draw_precheck_status(
            ui,
            "ffprobe",
            self.has_ffprobe,
            &self.ffprobe_version,
            compact,
        );
        draw_precheck_status(ui, "ffmpeg", self.has_ffmpeg, &self.ffmpeg_version, compact);
        draw_precheck_status(ui, "yt-dlp", self.has_yt_dlp, &self.yt_dlp_version, compact);
        self.draw_system_usage(ui);
        self.draw_output_disk_space(ui);
    }

    fn draw_main_header_actions(&mut self, ui: &mut egui::Ui) {
        button_toolbar_wrapped(ui, |ui| {
            button_group(ui, "hdr_actions", |g| {
                if self.settings.logs_open {
                    if g.secondary(&format!("{} Hide log", ui_icons::DISMISS), true)
                        .on_hover_text("Close the activity log")
                        .clicked()
                    {
                        self.settings.logs_open = false;
                        self.persist_settings();
                    }
                } else if g
                    .secondary(&format!("{} Show log", ui_icons::LOGS), true)
                    .on_hover_text(
                        "Open the activity log (dock under the queue or in its own window)",
                    )
                    .clicked()
                {
                    self.settings.logs_open = true;
                    self.persist_settings();
                }
                if g.secondary(&format!("{} Settings", ui_icons::SETTINGS), true)
                    .on_hover_text(
                        "Ctrl/Cmd+Enter adds URLs · Ctrl/Cmd+D starts · Ctrl/Cmd+K command palette",
                    )
                    .clicked()
                {
                    self.settings_open = true;
                }
                if g.danger(&format!("{} Exit", ui_icons::EXIT), true)
                    .clicked()
                {
                    self.open_exit_confirm();
                }
            });
        });
    }

    fn draw_main_header_wide(&mut self, ui: &mut egui::Ui, row_w: f32) {
        ui.horizontal(|ui| {
            let actions_w = (row_w * 0.34).clamp(220.0, 340.0);
            let left_w = (row_w - actions_w).max(120.0);

            ui.allocate_ui_with_layout(
                egui::vec2(left_w, 0.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_max_width(left_w);
                    ui.horizontal_wrapped(|ui| {
                        self.draw_main_header_branding(ui);
                        self.draw_main_header_web_and_status(ui);
                        ui.add_space(6.0);
                        self.draw_main_header_tool_checks(ui, false);
                    });
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(actions_w, 0.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.set_max_width(actions_w);
                    self.draw_main_header_actions(ui);
                },
            );
        });
    }

    fn draw_main_header_narrow(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                self.draw_main_header_branding(ui);
                self.draw_main_header_web_and_status(ui);
            });
            ui.horizontal(|ui| {
                let left_w = (ui.available_width() * 0.55).max(120.0);
                let right_w = (ui.available_width() - left_w).max(120.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, 0.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_max_width(left_w);
                        ui.horizontal_wrapped(|ui| {
                            self.draw_main_header_tool_checks(ui, true);
                        });
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, 0.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.set_max_width(right_w);
                        self.draw_main_header_actions(ui);
                    },
                );
            });
        });
    }
}

fn downloader_options_collapsing_label(output_dir: &str, profile: &str, expanded: bool) -> String {
    if expanded {
        return "Download options".to_owned();
    }
    let folder = std::path::Path::new(output_dir)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(output_dir);
    let folder = if folder.chars().count() > 36 {
        let short: String = folder.chars().take(33).collect();
        format!("{short}…")
    } else {
        folder.to_owned()
    };
    format!("Download options — {folder} · {profile}")
}
