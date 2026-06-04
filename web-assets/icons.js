/** Material icon ligature names (aligned with desktop `ui_icons.rs`). */
const ICON = {
  add: "add",
  archive: "archive",
  article: "article",
  bolt: "bolt",
  clear: "clear_all",
  close: "close",
  cloudDownload: "cloud_download",
  delete: "delete",
  deleteForever: "delete_forever",
  description: "description",
  download: "download",
  exit: "exit_to_app",
  folderOpen: "folder_open",
  music: "music_note",
  pause: "pause",
  play: "play_arrow",
  playCircle: "play_circle",
  refresh: "sync",
  remove: "delete",
  removeCircleOutline: "remove_circle_outline",
  save: "save",
  search: "search",
  settings: "settings",
  star: "star",
  stop: "stop",
  tune: "tune",
  upload: "upload",
  check: "check",
  schedule: "schedule",
  hourglassEmpty: "hourglass_empty",
  block: "block",
};

function statusChipIcon(slug) {
  switch (slug) {
    case "resolving":
      return ICON.search;
    case "idle":
      return ICON.hourglassEmpty;
    case "queued":
      return ICON.schedule;
    case "downloading":
      return ICON.download;
    case "done":
      return ICON.check;
    case "failed":
      return ICON.close;
    case "skipped":
      return ICON.block;
    default:
      return ICON.hourglassEmpty;
  }
}

/** Status pill on video cards: [icon] label */
function setStatusChip(chip, slug, label) {
  chip.className = `status-chip status-${slug}`;
  const text = label || "Idle";
  chip.replaceChildren(iconSpan(statusChipIcon(slug)), document.createTextNode(` ${text}`));
}

function iconSpan(name) {
  const el = document.createElement("span");
  el.className = "material-icons";
  el.setAttribute("aria-hidden", "true");
  el.textContent = name;
  return el;
}

/** Set visible label on a button: [icon] text */
function setButtonLabel(btn, iconName, text) {
  btn.replaceChildren(iconSpan(iconName), document.createTextNode(` ${text}`));
}

function applyStaticButtonIcons() {
  document.querySelectorAll("button[data-icon]").forEach((btn) => {
    const icon = btn.dataset.icon;
    if (!icon) return;
    const text = btn.textContent.trim();
    setButtonLabel(btn, icon, text);
  });
}
