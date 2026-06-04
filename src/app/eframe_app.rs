use super::*;
use crate::app_ui::{button_group, button_toolbar, left_button_row, prepare_scroll_content, set_row_width};

impl eframe::App for PydlApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        {
            let shared = self.shared_core.clone();
            let core = shared.lock();
            core_sync::sync_core_to_app(&core, self);
        }
        #[cfg(windows)]
        crate::win_icon::apply_native_window_icons(frame, &app_icon::window_icon());
        #[cfg(not(windows))]
        let _ = frame;
        ctx.set_zoom_factor(self.settings.ui_scale.clamp(0.85, 1.5));
        if let Some(text) = self.deferred_menu_paste_urls.take() {
            ctx.input_mut(|inp| inp.events.push(egui::Event::Paste(text)));
        }
        if let Some(text) = self.deferred_menu_paste_output_dir.take() {
            ctx.input_mut(|inp| inp.events.push(egui::Event::Paste(text)));
        }
        self.maybe_flush_queue_save();
        self.maybe_flush_log_save();
        self.process_events(ctx);
        #[cfg(windows)]
        {
            self.maybe_install_win_browser_drop_target(frame);
            self.drain_win_browser_url_drops(ctx);
        }
        if self.av1_mode {
            self.apply_dropped_av1_paths(ctx);
        } else {
            self.apply_dropped_shortcut_files(ctx);
        }
        self.handle_viewport_close_request(ctx);
        if self.exit_pending_after_cancel && !self.exit_work_in_progress() {
            self.exit_pending_after_cancel = false;
            self.exit_allowed = true;
            self.flush_queue_to_disk();
            self.flush_av1_queue_to_disk();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.poll_done_file_lookup();
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

        egui::CentralPanel::default()
            .frame(content_panel_frame())
            .show(ctx, |ui| {
                self.sync_theme_if_needed(ctx);
                ui.set_width(ui.available_width());
                self.draw_main_header(ui);
                ui.label(
                    "Add URLs to load previews; start downloads to see progress on each card.",
                );
                if self.show_restore_banner && self.restored_items_count > 0 {
                    alert_warning(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Restored {} item(s) from previous session.",
                                    self.restored_items_count
                                ))
                                .color(ALERT_WARNING_TEXT),
                            );
                            let tail_w = ui.available_width();
                            ui.allocate_ui_with_layout(
                                egui::vec2(tail_w.max(0.0), 0.0),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    button_group(ui, "restore_dismiss", |g| {
                                        if g.warning(
                                            &format!("{} Dismiss", ui_icons::DISMISS),
                                            true,
                                        )
                                        .clicked()
                                        {
                                            self.show_restore_banner = false;
                                        }
                                    });
                                },
                            );
                        });
                    });
                }
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
                let mut nav_frame = egui::Frame::group(ui.style());
                nav_frame.fill = Color32::from_rgb(28, 32, 38);
                nav_frame.stroke = egui::Stroke::new(1.0, Color32::from_rgb(64, 72, 86));
                nav_frame.rounding = egui::Rounding::same(8.0);
                nav_frame.inner_margin = egui::Margin::symmetric(12.0, 10.0);
                with_full_width(ui, |ui| {
                    nav_frame.show(ui, |ui| {
                        let row_w = ui.available_width();
                        set_row_width(ui, row_w);
                        ui.allocate_ui_with_layout(
                            egui::vec2(row_w, 34.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                let half = row_w * 0.5;
                                let btn_sz = egui::vec2(half, 34.0);
                                let dl_active = !self.av1_mode;
                                let av1_active = self.av1_mode;
                                let dl = egui::Button::new(
                                    RichText::new(format!("{} Downloader", ui_icons::NAV_DOWNLOADER))
                                        .strong()
                                        .color(if dl_active {
                                            Color32::from_rgb(10, 32, 10)
                                        } else {
                                            Color32::from_rgb(210, 220, 235)
                                        }),
                                )
                                .fill(if dl_active {
                                    Color32::from_rgb(152, 255, 152)
                                } else {
                                    Color32::from_rgb(44, 52, 64)
                                })
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    if dl_active {
                                        Color32::from_rgb(80, 190, 80)
                                    } else {
                                        Color32::from_rgb(88, 100, 116)
                                    },
                                ));
                                if ui.add_sized(btn_sz, dl).clicked() {
                                    self.set_app_mode(false);
                                }
                                let av1 = egui::Button::new(
                                    RichText::new(format!("{} AV1 Converter", ui_icons::NAV_AV1))
                                        .strong()
                                        .color(if av1_active {
                                            Color32::from_rgb(45, 27, 0)
                                        } else {
                                            Color32::from_rgb(210, 220, 235)
                                        }),
                                )
                                .fill(if av1_active {
                                    Color32::from_rgb(255, 190, 90)
                                } else {
                                    Color32::from_rgb(44, 52, 64)
                                })
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    if av1_active {
                                        Color32::from_rgb(245, 154, 35)
                                    } else {
                                        Color32::from_rgb(88, 100, 116)
                                    },
                                ));
                                if ui.add_sized(btn_sz, av1).clicked() {
                                    self.set_app_mode(true);
                                }
                            },
                        );
                    });
                });
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
                    .color(TEXT_MUTED),
                );
                ui.separator();
                if self.av1_mode {
                    self.draw_av1_panel(ui);
                    return;
                }

                let main_split = compute_main_column_split(
                    ui.available_height(),
                    self.settings.videos_docked,
                    self.settings.compact_cards,
                );

                let controls_w = ui.available_width();
                let mut dl_controls_scroll = egui::ScrollArea::vertical()
                    .id_salt("rustdl_downloader_controls")
                    .auto_shrink([false, false]);
                if let Some(max_h) = main_split.controls_scroll_max_height {
                    dl_controls_scroll = dl_controls_scroll.max_height(max_h);
                }
                dl_controls_scroll.show(ui, |ui| {
                        prepare_scroll_content(ui, controls_w);

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
                    [ui.available_width(), 120.0],
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
                    button_group(ui, "add_urls", |g| {
                        if g.success(
                            &format!("{} Add URLs", ui_icons::ADD),
                            !self.add_in_progress,
                        )
                        .clicked()
                        {
                            self.add_urls(ctx.input(|i| i.time));
                        }
                        if g
                            .secondary(
                                &format!(
                                    "{} Import file (.txt/.csv)",
                                    ui_icons::IMPORT_FILE
                                ),
                                true,
                            )
                            .clicked()
                        {
                            self.import_urls_from_file();
                        }
                    });
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
                self.draw_downloader_queue_status_row(ui);
                let total_finished = self.status_done + self.status_failed;
                let total_known =
                    self.status_ready + self.status_queued + self.status_active + total_finished;
                if total_known > 0 {
                    let session_busy =
                        self.status_active > 0 || self.queue_running > 0 || self.add_in_progress;
                    let pb = egui::ProgressBar::new(total_finished as f32 / total_known as f32)
                        .animate(session_busy)
                        .text(format!(
                            "Session progress: {}/{} done ({} failed)",
                            total_finished, total_known, self.status_failed
                        ));
                    let pb_resp = ui.add(pb);
                    if pb_resp.clicked() {
                        self.focus_queue_group("Done");
                    }
                    if self.status_ready == 0
                        && self.status_queued == 0
                        && self.status_active == 0
                        && self.status_resolving == 0
                        && total_finished > 0
                    {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                status_color(ItemStatus::Done),
                                "All downloads finished for this session.",
                            );
                            left_button_row(ui, |ui| {
                                button_group(ui, "session_open_folder", |g| {
                                    if g.secondary(
                                        &format!("{} Open output folder", ui_icons::OPEN_FOLDER),
                                        true,
                                    )
                                    .clicked()
                                    {
                                        self.open_output_folder();
                                    }
                                });
                            });
                        });
                    }
                    let totals = self.transfer_totals();
                    if totals.with_known_total > 0 && totals.known_total_bytes > 0 {
                        let pct = (totals.downloaded_bytes as f64
                            / totals.known_total_bytes as f64
                            * 100.0)
                            .clamp(0.0, 100.0);
                        ui.label(
                            RichText::new(format!(
                                "Transfer: {} / {} ({pct:.1}%)",
                                human_bytes_ui(totals.downloaded_bytes),
                                human_bytes_ui(totals.known_total_bytes),
                            ))
                            .small()
                            .color(Color32::GRAY),
                        );
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Output folder");
                    let output_dir_edit = ui.add(
                        egui::TextEdit::singleline(&mut self.output_dir)
                            .desired_width(ui.available_width().max(120.0)),
                    );
                    attach_paste_context_menu(
                        &output_dir_edit,
                        &mut self.deferred_menu_paste_output_dir,
                    );
                    if output_dir_edit.changed() {
                        self.persist_settings();
                        self.last_done_lookup_poll = None;
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
                        }
                        if g.secondary(
                            &format!("{} Open output folder", ui_icons::OPEN_FOLDER),
                            true,
                        )
                        .clicked()
                        {
                            self.open_output_folder();
                        }
                    });
                });

                ui.separator();
                let has_idle_items = self
                    .items
                    .iter()
                    .any(|x| x.status == ItemStatus::Idle && x.error.is_none());

                // Queue actions + primary download control live above the scroll so they stay
                // reachable when the window is short (CentralPanel does not scroll as a whole).
                ui.label(RichText::new("Queue").small().color(TEXT_MUTED));
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
                                            crate::profiles::find_profile(&self.profile_store, &p.name)
                                        {
                                            self.apply_download_profile(&prof);
                                        }
                                    }
                                }
                            });
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Search");
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut self.queue_search)
                            .hint_text("Title, URL, uploader…")
                            .desired_width(200.0),
                    );
                    if search.changed() {
                        self.queue_group_focus = None;
                    }
                    if !self.queue_search.is_empty()
                        && ui
                            .small_button(format!("{} Clear", ui_icons::CLEAR_SEARCH))
                            .clicked()
                    {
                        self.queue_search.clear();
                    }
                });
                button_toolbar(ui, |ui| {
                    if self.downloads_paused {
                        button_group(ui, "dl_pause", |g| {
                            if g.success(
                                &format!("{} Resume downloads", ui_icons::USE_DOWNLOADS),
                                true,
                            )
                            .clicked()
                            {
                                self.resume_all_downloads();
                            }
                        });
                    } else {
                        button_group(ui, "dl_pause", |g| {
                            if g.warning(
                                &format!("{} Pause downloads", ui_icons::CANCEL_TO_READY),
                                self.status_queued > 0 || self.status_active > 0,
                            )
                            .clicked()
                            {
                                self.pause_all_downloads();
                            }
                        });
                    }
                    button_group(ui, "dl_io", |g| {
                        if g.secondary(
                            &format!("{} Export URLs", ui_icons::EXPORT),
                            !self.items.is_empty(),
                        )
                        .clicked()
                        {
                            self.export_queue_to_file();
                        }
                        if g.secondary(
                            &format!("{} Import queue", ui_icons::IMPORT_FILE),
                            !self.add_in_progress,
                        )
                        .on_hover_text(
                            "Load URLs from a .txt file directly into the download queue",
                        )
                        .clicked()
                        {
                            self.import_queue_from_file();
                        }
                    });
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
                    if self.status_queued > 0 || self.status_active > 0 {
                        button_group(ui, "dl_cancel_all", |g| {
                            if g.warning(
                                &format!("{} Cancel all -> Ready", ui_icons::CANCEL_TO_READY),
                                true,
                            )
                            .clicked()
                            {
                                self.cancel_all_active(CancelPostAction::Ready);
                            }
                            if g.danger(
                                &format!("{} Cancel all -> Remove", ui_icons::CANCEL_TO_REMOVE),
                                true,
                            )
                            .clicked()
                            {
                                self.cancel_all_active(CancelPostAction::Remove);
                            }
                        });
                    }
                    button_group(ui, "dl_recheck", |g| {
                        let recheck = g.add(|ui| {
                            ui.add_enabled(
                                self.has_ffprobe && !self.settings.ffmpeg_extract_audio_mp3,
                                egui::Button::new(
                                    RichText::new(format!(
                                        "{} Re-check saved files",
                                        ui_icons::RECHECK
                                    ))
                                    .color(Color32::from_rgb(40, 24, 0)),
                                )
                                .fill(Color32::from_rgb(255, 167, 38))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    Color32::from_rgb(214, 120, 20),
                                )),
                            )
                            .on_hover_text(
                                "Run ffprobe on each finished download on disk; mark rows failed if video or audio is missing.",
                            )
                            .on_disabled_hover_text(
                                "Requires ffprobe. Disabled while MP3 extraction is enabled.",
                            )
                        });
                        if recheck.clicked() {
                            self.recheck_all_saved_downloads();
                        }
                    });
                    button_group(ui, "dl_clear", |g| {
                        if g.danger(
                            &format!("{} Clear list", ui_icons::CLEAR_QUEUE),
                            true,
                        )
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
                    if has_idle_items {
                        button_group(ui, "dl_start", |g| {
                            if g.success(
                                &format!("{} Start downloads", ui_icons::USE_DOWNLOADS),
                                true,
                            )
                            .clicked()
                            {
                                self.start_downloads();
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

                    }); // downloader controls scroll

                if self.settings.videos_docked {
                    self.draw_docked_videos_section(ui, main_split.videos_height);
                } else {
                    self.draw_videos_undocked_strip(ui);
                    if self.settings.logs_open && self.settings.logs_docked {
                        self.draw_docked_log_only_section(ui);
                    }
                }
        });

        self.draw_settings_window(ctx);
        self.draw_about_window(ctx);
        if !self.settings.videos_docked {
            self.draw_videos_window(ctx);
        }
        if self.settings.logs_open && !self.settings.logs_docked {
            self.draw_logs_window(ctx);
        }
        self.maybe_notify_session_complete();

        self.input_urls_snapshot = self.input_urls.clone();
        self.draw_exit_confirm_dialog(ctx);
        self.request_repaint_if_background_busy(ctx);
        {
            let shared = self.shared_core.clone();
            core_sync::push_app_to_core(self, &shared);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_queue_to_disk();
        self.flush_av1_queue_to_disk();
        self.flush_log_to_disk();
        let _ = save_settings(&self.settings);
    }
}

impl PydlApp {
    /// Title on the left; tool status and window actions on one row, top-aligned, inset from the right.
    fn draw_main_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.horizontal(|ui| {
                let sz = egui::vec2(40.0, 40.0);
                let img = ui.add(
                    egui::Image::new(egui::load::SizedTexture::new(self.logo.id(), sz))
                        .sense(egui::Sense::click()),
                );
                let title = ui.add(
                    egui::Label::new(RichText::new("rustdl").heading()).sense(egui::Sense::click()),
                );
                let header = img
                    .union(title)
                    .on_hover_text("About rustdl — click to open");
                if header.clicked() {
                    self.about_open = true;
                }
            });
            let right_w = ui.available_width();
            if right_w <= 0.0 {
                return;
            }
            ui.allocate_ui_with_layout(
                egui::vec2(right_w, 0.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.add_space(HEADER_RIGHT_INSET);
                    let videos_btn = if self.settings.videos_docked || self.settings.videos_open {
                        format!("{} Videos", ui_icons::VIDEOS)
                    } else {
                        format!("{} Videos (hidden)", ui_icons::VIDEOS)
                    };
                    button_group(ui, "hdr_nav", |g| {
                        if g.secondary(
                            &format!("{} Settings", ui_icons::SETTINGS),
                            true,
                        )
                        .on_hover_text("Ctrl/Cmd+Enter adds URLs · Ctrl/Cmd+D starts downloads")
                        .clicked()
                        {
                            self.settings_open = true;
                        }
                        if g.secondary(&format!("{} Logs", ui_icons::LOGS), true)
                            .on_hover_text(
                                "View activity log (dock under queue or separate window)",
                            )
                            .clicked()
                        {
                            self.toggle_logs_panel();
                        }
                        if g.secondary(&videos_btn, true)
                            .on_hover_text(
                                "Show video queue in main window or a separate floating window",
                            )
                            .clicked()
                        {
                            self.toggle_videos_panel();
                        }
                        if g.danger(&format!("{} Exit", ui_icons::EXIT), true).clicked() {
                            self.open_exit_confirm();
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            draw_precheck_status(
                                ui,
                                "ffprobe",
                                self.has_ffprobe,
                                &self.ffprobe_version,
                            );
                            draw_precheck_status(
                                ui,
                                "ffmpeg",
                                self.has_ffmpeg,
                                &self.ffmpeg_version,
                            );
                            draw_precheck_status(
                                ui,
                                "yt-dlp",
                                self.has_yt_dlp,
                                &self.yt_dlp_version,
                            );
                            if self.settings.web_ui_enabled && self.web_server.is_some() {
                                let url = crate::service::web::web_ui_browser_url(
                                    &self.settings.web_bind_address,
                                );
                                draw_web_ui_header_link(ui, &url);
                            }
                        });
                    });
                },
            );
        });
    }
}
