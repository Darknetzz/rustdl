//! Command palette manifest for the LAN web UI.

use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::Serialize;

use super::api::ApiState;

#[derive(Clone, Serialize)]
pub struct PaletteCommandJson {
    pub id: &'static str,
    pub label: &'static str,
    pub keywords: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<&'static str>,
}

const WEB_PALETTE_COMMANDS: &[PaletteCommandJson] = &[
    PaletteCommandJson {
        id: "open_settings",
        label: "Open Settings",
        keywords: "settings preferences options",
        section: Some("Settings"),
    },
    PaletteCommandJson {
        id: "settings_general",
        label: "Settings → General tab",
        keywords: "general shared global theme layout",
        section: None,
    },
    PaletteCommandJson {
        id: "settings_shared",
        label: "Settings → General tab",
        keywords: "shared general global theme layout",
        section: None,
    },
    PaletteCommandJson {
        id: "settings_downloader",
        label: "Settings → Downloader tab",
        keywords: "download yt-dlp profile",
        section: None,
    },
    PaletteCommandJson {
        id: "settings_convert",
        label: "Settings → Converter tab",
        keywords: "convert av1 encode video",
        section: None,
    },
    PaletteCommandJson {
        id: "settings_webui",
        label: "Settings → Web UI tab",
        keywords: "web lan api token bind",
        section: None,
    },
    PaletteCommandJson {
        id: "reset_ui_scale",
        label: "Reset UI scale to 100% (host app)",
        keywords: "ui scale zoom reset desktop host",
        section: None,
    },
    PaletteCommandJson {
        id: "start_downloads",
        label: "Start downloads",
        keywords: "start run download ready",
        section: Some("Queue"),
    },
    PaletteCommandJson {
        id: "pause_downloads",
        label: "Pause downloads",
        keywords: "pause hold stop",
        section: None,
    },
    PaletteCommandJson {
        id: "resume_downloads",
        label: "Resume downloads",
        keywords: "resume continue",
        section: None,
    },
    PaletteCommandJson {
        id: "retry_failed",
        label: "Retry all failed",
        keywords: "retry failed download again",
        section: None,
    },
    PaletteCommandJson {
        id: "remove_selected",
        label: "Remove selected",
        keywords: "remove delete selected queue bulk",
        section: None,
    },
    PaletteCommandJson {
        id: "clear_done",
        label: "Clear completed downloads",
        keywords: "clear done finished remove completed",
        section: None,
    },
    PaletteCommandJson {
        id: "convert_start",
        label: "Start Convert batch",
        keywords: "convert encode start",
        section: None,
    },
    PaletteCommandJson {
        id: "convert_pause",
        label: "Pause Convert batch",
        keywords: "convert pause hold",
        section: None,
    },
    PaletteCommandJson {
        id: "convert_resume",
        label: "Resume Convert batch",
        keywords: "convert resume continue",
        section: None,
    },
    PaletteCommandJson {
        id: "mode_downloader",
        label: "Switch to Downloader",
        keywords: "mode download",
        section: None,
    },
    PaletteCommandJson {
        id: "mode_convert",
        label: "Switch to Video Converter",
        keywords: "mode convert av1",
        section: None,
    },
    PaletteCommandJson {
        id: "mode_library",
        label: "Switch to Library",
        keywords: "library done history",
        section: None,
    },
    PaletteCommandJson {
        id: "focus_search",
        label: "Focus queue search",
        keywords: "search find filter queue",
        section: None,
    },
    PaletteCommandJson {
        id: "toggle_log",
        label: "Toggle activity log",
        keywords: "log show hide expand activity",
        section: None,
    },
    PaletteCommandJson {
        id: "export_log",
        label: "Export activity log",
        keywords: "export log save file",
        section: None,
    },
    PaletteCommandJson {
        id: "layout_compact",
        label: "Layout: Compact queue",
        keywords: "layout compact list small",
        section: Some("Layout"),
    },
    PaletteCommandJson {
        id: "layout_review",
        label: "Layout: Review mode",
        keywords: "layout review cards thumbnails",
        section: None,
    },
    PaletteCommandJson {
        id: "layout_minimal",
        label: "Layout: Minimal",
        keywords: "layout minimal no thumbnails",
        section: None,
    },
    PaletteCommandJson {
        id: "dock_videos",
        label: "Dock Videos panel (host app)",
        keywords: "dock videos queue panel desktop host",
        section: Some("Panels"),
    },
    PaletteCommandJson {
        id: "float_videos",
        label: "Float Videos window (host app)",
        keywords: "float undock videos window desktop host",
        section: None,
    },
    PaletteCommandJson {
        id: "dock_log",
        label: "Dock activity log (host app)",
        keywords: "dock log panel bottom desktop host",
        section: None,
    },
    PaletteCommandJson {
        id: "float_log",
        label: "Float activity log (host app)",
        keywords: "float undock log window desktop host",
        section: None,
    },
    PaletteCommandJson {
        id: "open_about",
        label: "Open About",
        keywords: "about version help",
        section: Some("Help"),
    },
    PaletteCommandJson {
        id: "refresh_all",
        label: "Refresh page data",
        keywords: "refresh reload sync",
        section: None,
    },
];

#[derive(Serialize)]
struct PaletteCommandsResponse {
    commands: &'static [PaletteCommandJson],
}

async fn palette_commands() -> Json<PaletteCommandsResponse> {
    Json(PaletteCommandsResponse {
        commands: WEB_PALETTE_COMMANDS,
    })
}

pub(super) fn register(router: Router<ApiState>) -> Router<ApiState> {
    router.route("/api/palette/commands", get(palette_commands))
}
