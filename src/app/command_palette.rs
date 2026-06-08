use eframe::egui;
use eframe::egui::{Color32, RichText};

use crate::app_ui::modal_backdrop;
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
        label: "Settings → AV1 tab",
        keywords: "av1 encode converter",
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
        label: "Switch to AV1 Converter mode",
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
            .resizable(false)
            .default_width(480.0)
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
                egui::ScrollArea::vertical()
                    .max_height(280.0)
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
            "Settings → AV1 tab" => {
                self.settings_tab = SettingsTab::Av1;
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
            "Toggle activity log" => {
                self.settings.logs_open = !self.settings.logs_open;
                self.persist_settings();
            }
            "Focus queue search" => self.focus_queue_search = true,
            "Switch to Downloader mode" => self.set_app_mode(false),
            "Switch to AV1 Converter mode" => self.set_app_mode(true),
            "Open output folder" => self.open_output_folder(),
            "Open About" => self.about_open = true,
            _ => {}
        }
    }
}
