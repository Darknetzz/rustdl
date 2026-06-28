use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::{bounded_ui_height, modal_backdrop};
use crate::ui_icons;

use super::{PydlApp, SettingsTab};

struct PaletteCommand {
    label: &'static str,
    keywords: &'static str,
}

const COMMANDS: &[PaletteCommand] = &[
    PaletteCommand {
        label: "Open Settings",
        keywords: "settings preferences options",
    },
    PaletteCommand {
        label: "Settings → Shared tab",
        keywords: "shared global theme layout",
    },
    PaletteCommand {
        label: "Settings → Downloader tab",
        keywords: "download yt-dlp profile",
    },
    PaletteCommand {
        label: "Settings → Converter tab",
        keywords: "convert av1 encode video",
    },
    PaletteCommand {
        label: "Settings → Web UI tab",
        keywords: "web lan api token bind",
    },
    PaletteCommand {
        label: "Add URLs from input",
        keywords: "add paste queue urls",
    },
    PaletteCommand {
        label: "Start downloads",
        keywords: "start run download ready",
    },
    PaletteCommand {
        label: "Pause downloads",
        keywords: "pause hold stop",
    },
    PaletteCommand {
        label: "Resume downloads",
        keywords: "resume continue",
    },
    PaletteCommand {
        label: "Pause Convert batch",
        keywords: "pause hold stop convert encode",
    },
    PaletteCommand {
        label: "Resume Convert batch",
        keywords: "resume continue convert encode",
    },
    PaletteCommand {
        label: "Retry all failed",
        keywords: "retry failed download again",
    },
    PaletteCommand {
        label: "Toggle activity log",
        keywords: "log show hide activity",
    },
    PaletteCommand {
        label: "Focus queue search",
        keywords: "search filter find queue",
    },
    PaletteCommand {
        label: "Switch to Downloader mode",
        keywords: "downloader download mode",
    },
    PaletteCommand {
        label: "Switch to Video Converter mode",
        keywords: "av1 convert encode mode",
    },
    PaletteCommand {
        label: "Open output folder",
        keywords: "folder output directory explorer",
    },
    PaletteCommand {
        label: "Open About",
        keywords: "about version help",
    },
    PaletteCommand {
        label: "Show keyboard shortcuts",
        keywords: "shortcuts keys help hotkeys",
    },
    PaletteCommand {
        label: "Open Download library",
        keywords: "library done history downloads",
    },
    PaletteCommand {
        label: "Export activity log",
        keywords: "export log save file",
    },
    PaletteCommand {
        label: "Dock Videos panel",
        keywords: "dock videos queue panel",
    },
    PaletteCommand {
        label: "Float Videos window",
        keywords: "float undock videos window",
    },
    PaletteCommand {
        label: "Dock activity log",
        keywords: "dock log panel bottom",
    },
    PaletteCommand {
        label: "Float activity log",
        keywords: "float undock log window",
    },
    PaletteCommand {
        label: "Layout: Compact queue",
        keywords: "layout compact list small",
    },
    PaletteCommand {
        label: "Layout: Review mode",
        keywords: "layout review cards thumbnails",
    },
    PaletteCommand {
        label: "Layout: Minimal",
        keywords: "layout minimal no thumbnails",
    },
];

fn matches_query(label: &str, keywords: &str, query: &str) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    label.to_ascii_lowercase().contains(&q) || keywords.to_ascii_lowercase().contains(&q)
}

impl PydlApp {
    pub(super) fn draw_command_palette(&mut self, ctx: &egui::Context) {
        if !self.command_palette_open {
            return;
        }
        let _ = modal_backdrop(ctx, egui::Id::new("command_palette_backdrop"));
        let mut palette_open = self.command_palette_open;
        let mut close_palette = false;
        egui::Window::new(format!("{} Command palette", ui_icons::RECHECK))
            .open(&mut palette_open)
            .collapsible(false)
            .resizable(true)
            .default_width(480.0)
            .min_width(360.0)
            .min_height(120.0)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Type to filter · Enter to run · Esc to close")
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

                let mut run: Option<&'static str> = None;
                let scroll_h = bounded_ui_height(ui, 120.0).max(120.0).min(280.0);
                egui::ScrollArea::vertical()
                    .max_height(scroll_h)
                    .show(ui, |ui| {
                        for cmd in COMMANDS {
                            if !matches_query(cmd.label, cmd.keywords, &self.command_palette_query)
                            {
                                continue;
                            }
                            if ui.button(cmd.label).clicked()
                                || (resp.has_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                    && run.is_none())
                            {
                                run = Some(cmd.label);
                            }
                        }
                    });

                if let Some(label) = run {
                    self.run_command_palette_action(label, ctx);
                    close_palette = true;
                    self.command_palette_query.clear();
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
            "Settings → Shared tab" => {
                self.settings_tab = SettingsTab::Shared;
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
            "Retry all failed" => self.retry_failed_items(),
            "Toggle activity log" => {
                self.settings.logs_open = !self.settings.logs_open;
                self.persist_settings();
            }
            "Focus queue search" => self.focus_queue_search = true,
            "Switch to Downloader mode" => self.set_app_mode(false),
            "Switch to Video Converter mode" => self.set_app_mode(true),
            "Open output folder" => self.open_output_folder(),
            "Open About" => self.about_open = true,
            "Show keyboard shortcuts" => self.about_open = true,
            "Open Download library" => self.library_open = true,
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
                crate::app::settings_panel::apply_layout_preset(&mut self.settings, "compact");
                self.settings_dirty = true;
                self.persist_settings();
            }
            "Layout: Review mode" => {
                crate::app::settings_panel::apply_layout_preset(&mut self.settings, "review");
                self.settings_dirty = true;
                self.persist_settings();
            }
            "Layout: Minimal" => {
                crate::app::settings_panel::apply_layout_preset(&mut self.settings, "minimal");
                self.settings_dirty = true;
                self.persist_settings();
            }
            _ => {}
        }
    }
}
