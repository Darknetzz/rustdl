use eframe::egui;
use eframe::egui::{Align, Color32, RichText};

use crate::app_ui::{bounded_ui_height, modal_backdrop};
use crate::config::snap_ui_scale;
use crate::ui_icons;

use super::{PydlApp, SettingsTab};

struct PaletteCommand {
    label: &'static str,
    keywords: &'static str,
    section: Option<&'static str>,
}

const COMMANDS: &[PaletteCommand] = &[
    PaletteCommand {
        label: "Open Settings",
        keywords: "settings preferences options",
        section: Some("Settings"),
    },
    PaletteCommand {
        label: "Settings → General tab",
        keywords: "general shared global theme layout ui scale appearance",
        section: None,
    },
    PaletteCommand {
        label: "Settings → Downloader tab",
        keywords: "download yt-dlp profile",
        section: None,
    },
    PaletteCommand {
        label: "Settings → Converter tab",
        keywords: "convert av1 encode video",
        section: None,
    },
    PaletteCommand {
        label: "Settings → Web UI tab",
        keywords: "web lan api token bind",
        section: None,
    },
    PaletteCommand {
        label: "Reset UI scale to 100%",
        keywords: "ui scale zoom reset default size",
        section: None,
    },
    PaletteCommand {
        label: "Add URLs from input",
        keywords: "add paste queue urls",
        section: Some("Queue"),
    },
    PaletteCommand {
        label: "Start downloads",
        keywords: "start run download ready",
        section: None,
    },
    PaletteCommand {
        label: "Pause downloads",
        keywords: "pause hold stop",
        section: None,
    },
    PaletteCommand {
        label: "Resume downloads",
        keywords: "resume continue",
        section: None,
    },
    PaletteCommand {
        label: "Pause Convert batch",
        keywords: "pause hold stop convert encode",
        section: None,
    },
    PaletteCommand {
        label: "Resume Convert batch",
        keywords: "resume continue convert encode",
        section: None,
    },
    PaletteCommand {
        label: "Start Convert batch",
        keywords: "convert encode start batch",
        section: None,
    },
    PaletteCommand {
        label: "Retry all failed",
        keywords: "retry failed download again",
        section: None,
    },
    PaletteCommand {
        label: "Remove selected",
        keywords: "delete remove selected queue rows",
        section: None,
    },
    PaletteCommand {
        label: "Clear completed downloads",
        keywords: "clear done finished remove completed",
        section: None,
    },
    PaletteCommand {
        label: "Toggle activity log",
        keywords: "log show hide activity",
        section: None,
    },
    PaletteCommand {
        label: "Focus queue search",
        keywords: "search filter find queue",
        section: None,
    },
    PaletteCommand {
        label: "Switch to Downloader mode",
        keywords: "downloader download mode",
        section: None,
    },
    PaletteCommand {
        label: "Switch to Video Converter mode",
        keywords: "av1 convert encode mode",
        section: None,
    },
    PaletteCommand {
        label: "Open output folder",
        keywords: "folder output directory explorer",
        section: None,
    },
    PaletteCommand {
        label: "Open About",
        keywords: "about version help",
        section: Some("Help"),
    },
    PaletteCommand {
        label: "Show keyboard shortcuts",
        keywords: "shortcuts keys help hotkeys",
        section: None,
    },
    PaletteCommand {
        label: "Open Download library",
        keywords: "library done history downloads",
        section: None,
    },
    PaletteCommand {
        label: "Switch to Library",
        keywords: "library done history view",
        section: None,
    },
    PaletteCommand {
        label: "Export activity log",
        keywords: "export log save file",
        section: None,
    },
    PaletteCommand {
        label: "Dock Videos panel",
        keywords: "dock videos queue panel",
        section: Some("Panels"),
    },
    PaletteCommand {
        label: "Float Videos window",
        keywords: "float undock videos window",
        section: None,
    },
    PaletteCommand {
        label: "Dock activity log",
        keywords: "dock log panel bottom",
        section: None,
    },
    PaletteCommand {
        label: "Float activity log",
        keywords: "float undock log window",
        section: None,
    },
    PaletteCommand {
        label: "Layout: Compact queue",
        keywords: "layout compact list small",
        section: Some("Layout"),
    },
    PaletteCommand {
        label: "Layout: Review mode",
        keywords: "layout review cards thumbnails",
        section: None,
    },
    PaletteCommand {
        label: "Layout: Minimal",
        keywords: "layout minimal no thumbnails",
        section: None,
    },
];

fn matches_query(label: &str, keywords: &str, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let hay = format!("{label} {keywords}").to_ascii_lowercase();
    q.split_whitespace().all(|token| {
        let token = token.to_ascii_lowercase();
        !token.is_empty() && hay.contains(&token)
    })
}

impl PydlApp {
    pub(super) fn draw_command_palette(&mut self, ctx: &egui::Context) {
        if !self.command_palette_open {
            return;
        }
        if modal_backdrop(ctx, egui::Id::new("command_palette_backdrop")) {
            self.command_palette_open = false;
            return;
        }
        let mut palette_open = self.command_palette_open;
        let mut close_palette = false;
        let palette_id = egui::Id::new("command_palette");
        let query_changed = ctx.data_mut(|d| {
            let prev = d
                .get_temp_mut_or_insert_with(palette_id.with("query"), String::new)
                .clone();
            if prev != self.command_palette_query {
                d.insert_temp(palette_id.with("query"), self.command_palette_query.clone());
                true
            } else {
                false
            }
        });
        if query_changed {
            self.command_palette_selection = 0;
        }

        let filtered: Vec<&PaletteCommand> = COMMANDS
            .iter()
            .filter(|cmd| matches_query(cmd.label, cmd.keywords, &self.command_palette_query))
            .collect();
        if !filtered.is_empty() && self.command_palette_selection >= filtered.len() {
            self.command_palette_selection = filtered.len() - 1;
        }

        egui::Window::new(format!("{} Command palette", ui_icons::RECHECK))
            .open(&mut palette_open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .min_width(360.0)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("↑↓ navigate · Enter run · Esc close")
                        .small()
                        .color(Color32::GRAY),
                );
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.command_palette_query)
                        .hint_text("Search commands…")
                        .desired_width(f32::INFINITY),
                );
                resp.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close_palette = true;
                }
                if resp.has_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !filtered.is_empty() {
                        self.command_palette_selection =
                            (self.command_palette_selection + 1).min(filtered.len() - 1);
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        self.command_palette_selection =
                            self.command_palette_selection.saturating_sub(1);
                    }
                }

                let mut run: Option<&'static str> = None;
                let scroll_h = bounded_ui_height(ui, 120.0).clamp(120.0, 320.0);
                egui::ScrollArea::vertical()
                    .max_height(scroll_h)
                    .show(ui, |ui| {
                        if filtered.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "No commands match \"{}\"",
                                    self.command_palette_query.trim()
                                ))
                                .small()
                                .color(Color32::GRAY),
                            );
                            return;
                        }
                        let mut last_section: Option<&str> = None;
                        for (idx, cmd) in filtered.iter().enumerate() {
                            if let Some(section) = cmd.section {
                                if last_section != Some(section) {
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(section).small().strong());
                                    last_section = Some(section);
                                }
                            }
                            let selected = idx == self.command_palette_selection;
                            let row = ui.selectable_label(selected, cmd.label);
                            if selected {
                                row.scroll_to_me(Some(Align::Center));
                            }
                            if row.clicked() {
                                run = Some(cmd.label);
                            }
                        }
                    });

                if resp.has_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && run.is_none()
                    && !filtered.is_empty()
                {
                    run = Some(filtered[self.command_palette_selection].label);
                }

                if let Some(label) = run {
                    self.run_command_palette_action(label, ctx);
                    close_palette = true;
                    self.command_palette_query.clear();
                    self.command_palette_selection = 0;
                }
            });
        if close_palette {
            palette_open = false;
        }
        self.command_palette_open = palette_open;
    }

    fn run_command_palette_action(&mut self, label: &str, ctx: &egui::Context) {
        match label {
            "Open Settings" => self.settings_open = true,
            "Settings → General tab" => {
                self.settings_tab = SettingsTab::General;
                self.settings_open = true;
                self.sync_settings_tab_to_disk();
            }
            "Settings → Downloader tab" => {
                self.settings_tab = SettingsTab::Downloader;
                self.settings_open = true;
                self.sync_settings_tab_to_disk();
            }
            "Settings → Converter tab" => {
                self.settings_tab = SettingsTab::Convert;
                self.settings_open = true;
                self.sync_settings_tab_to_disk();
            }
            "Settings → Web UI tab" => {
                self.settings_tab = SettingsTab::WebUi;
                self.settings_open = true;
                self.sync_settings_tab_to_disk();
            }
            "Reset UI scale to 100%" => {
                self.settings.ui_scale = snap_ui_scale(1.0);
                self.settings_dirty = true;
                self.persist_settings();
            }
            "Add URLs from input" => {
                let now = ctx.input(|i| i.time);
                self.add_urls(now);
            }
            "Start downloads" => self.start_downloads(),
            "Pause downloads" => self.pause_all_downloads(),
            "Resume downloads" => self.resume_all_downloads(),
            "Pause Convert batch" => {
                self.convert_core_action(|core| core.pause_convert_batch());
            }
            "Resume Convert batch" => {
                self.convert_core_action(|core| core.resume_convert_batch());
            }
            "Start Convert batch" => {
                self.persist_settings();
                self.convert_core_action(|core| {
                    let _ = core.start_convert_batch();
                });
            }
            "Retry all failed" => self.retry_failed_items(),
            "Remove selected" => self.remove_selected_items(),
            "Clear completed downloads" => self.clear_completed_downloads(),
            "Toggle activity log" => {
                self.settings.logs_open = !self.settings.logs_open;
                self.persist_settings();
            }
            "Focus queue search" => self.focus_queue_search = true,
            "Switch to Downloader mode" => self.set_app_mode(false),
            "Switch to Video Converter mode" => self.set_app_mode(true),
            "Open output folder" => self.open_output_folder(),
            "Open About" => {
                self.about_scroll_to_shortcuts = false;
                self.about_open = true;
            }
            "Show keyboard shortcuts" => {
                self.about_scroll_to_shortcuts = true;
                self.about_open = true;
            }
            "Open Download library" => self.library_open = true,
            "Switch to Library" => self.library_open = true,
            "Export activity log" => self.export_activity_log(),
            "Dock Videos panel" => {
                self.note_videos_dock_user_choice(true);
                self.settings.videos_docked = true;
                self.persist_settings();
            }
            "Float Videos window" => {
                self.note_videos_dock_user_choice(false);
                self.settings.videos_docked = false;
                self.persist_settings();
            }
            "Dock activity log" => {
                self.settings.logs_docked = true;
                self.settings.logs_open = true;
                self.persist_settings();
            }
            "Float activity log" => {
                self.settings.logs_docked = false;
                self.settings.logs_open = true;
                self.persist_settings();
            }
            "Layout: Compact queue" => {
                let vh = crate::app_ui::main_viewport_size(ctx).y;
                crate::app_ui::apply_layout_preset(&mut self.settings, "compact", Some(vh));
                self.settings_dirty = true;
                self.persist_settings();
            }
            "Layout: Review mode" => {
                let vh = crate::app_ui::main_viewport_size(ctx).y;
                crate::app_ui::apply_layout_preset(&mut self.settings, "review", Some(vh));
                self.settings_dirty = true;
                self.persist_settings();
            }
            "Layout: Minimal" => {
                let vh = crate::app_ui::main_viewport_size(ctx).y;
                crate::app_ui::apply_layout_preset(&mut self.settings, "minimal", Some(vh));
                self.settings_dirty = true;
                self.persist_settings();
            }
            _ => {}
        }
    }
}
