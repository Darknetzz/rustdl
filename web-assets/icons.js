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
  save: "save",
  search: "search",
  settings: "settings",
  star: "star",
  stop: "stop",
  tune: "tune",
  upload: "upload",
};

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
