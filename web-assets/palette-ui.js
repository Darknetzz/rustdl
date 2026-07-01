/** Server-driven command palette for the LAN web UI. */

let PALETTE_COMMANDS = [];

const PALETTE_ACTIONS = {
  open_settings: () => openSettingsDialog(),
  settings_shared: () => openSettingsDialog("shared"),
  settings_downloader: () => openSettingsDialog("downloader"),
  settings_convert: () => openSettingsDialog("convert"),
  settings_webui: () => openSettingsDialog("webui"),
  reset_ui_scale: () => patchHostSettings({ ui_scale: 1.0 }),
  start_downloads: () =>
    postAction("/api/downloads/start", "Downloads could not start.")
      .then(refreshAll)
      .catch(() => {}),
  pause_downloads: () => api("/api/downloads/pause", { method: "POST" }).then(refreshAll),
  resume_downloads: () => api("/api/downloads/resume", { method: "POST" }).then(refreshAll),
  retry_failed: () =>
    postAction("/api/downloads/retry-failed", "Could not retry failed downloads.")
      .then(refreshAll)
      .catch(() => {}),
  remove_selected: () => {
    if (currentView === "convert") {
      bulkRemoveConvertSelected().catch((e) => notifyError(e.message || String(e)));
    } else if (currentView === "downloader") {
      bulkRemoveSelected().catch((e) => notifyError(e.message || String(e)));
    } else {
      showToast("Switch to Downloader or Video Converter to remove selected queue items.");
    }
  },
  clear_done: () => clearQueue("done"),
  convert_start: () => convertStart(),
  convert_pause: () => convertPause(),
  convert_resume: () => convertResume(),
  mode_downloader: () => setView("downloader"),
  mode_convert: () => setView("convert"),
  mode_library: () => setView("library"),
  focus_search: () => focusActiveSearch(),
  toggle_log: () => toggleActivityLogExpanded(),
  export_log: () => exportActivityLog(),
  layout_compact: () => applyLayoutPresetViaApi("compact"),
  layout_review: () => applyLayoutPresetViaApi("review"),
  layout_minimal: () => applyLayoutPresetViaApi("minimal"),
  dock_videos: () => patchHostSettings({ videos_docked: true, videos_open: true }),
  float_videos: () => patchHostSettings({ videos_docked: false, videos_open: true }),
  dock_log: () => patchHostSettings({ logs_docked: true, logs_open: true }),
  float_log: () => patchHostSettings({ logs_open: true, logs_docked: false }),
  open_about: () => document.getElementById("about-dialog")?.showModal(),
  refresh_all: () => refreshAll(),
};

async function loadPaletteCommands() {
  try {
    const res = await fetch(apiUrlWithAuth("/api/palette/commands"), {
      headers: imageFetchHeaders(),
    });
    if (!res.ok) return;
    const data = await res.json();
    PALETTE_COMMANDS = (data.commands || []).map((cmd) => ({
      label: cmd.label,
      keywords: cmd.keywords,
      section: cmd.section,
      run: () => {
        const action = PALETTE_ACTIONS[cmd.id];
        if (action) action();
        else showToast(`Not available on web: ${cmd.label}`);
      },
    }));
  } catch (e) {
    console.error("palette manifest", e);
  }
}
