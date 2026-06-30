const TOKEN_KEY = "rustdl_web_token";
const WEB_THEME_KEY = "rustdl_web_theme";
const QUEUE_GROUP_COLLAPSED_KEY = "rustdl_web_queue_groups";
const WEB_LAYOUT_HEIGHTS_KEY = "rustdl_web_layout_heights";
const DONE_HISTORY_FILTER_KEY = "rustdl_web_done_history_days";
// Layout tiers mirror desktop app_ui.rs (1040 / 900 / 600).
const LAYOUT_FOOTER_WIDE_BREAKPOINT_PX = 900;
const LAYOUT_DL_SHORT_PANEL_LIST_THRESHOLD = 220;
const LAYOUT_CONVERT_SHORT_PANEL_LIST_THRESHOLD = 280;
const WEB_LOG_MIN_HEIGHT_PX = 80;
const WEB_QUEUE_MIN_HEIGHT_PX = 48;
const DOWNLOAD_QUEUE_GROUPS = ["Active", "Ready", "Issues", "Done", "Resolving"];
const CONVERT_QUEUE_GROUPS = ["Active", "Ready", "Failed", "Skipped", "Done"];

let cachedSettings = null;
/** True when this browser was accepted without an API token (IP whitelist). */
let ipAuthBypass = false;
let logLinesCache = [];
/** @type {string | null} */
let queueStatusFilter = null;
/** @type {Set<number>} */
let selectedQueueIds = new Set();
let selectedConvertIds = new Set();
/** @type {{ error_keywords: string[], important_keywords: string[] } | null} */
let logFilterRules = null;
let sessionRestorePromptOpen = false;
let sessionRestoreHandled = false;
/** @type {number | null} done history filter in days; null = all time */
let doneHistoryFilterDays = loadDoneHistoryFilter();
let queueSearchSaveTimer = null;
let logExpanded = false;

let toastTimer = null;
function showToast(message, kind = "info") {
  const el = document.getElementById("web-toast");
  if (!el) return;
  el.textContent = message;
  el.className = `web-toast toast-${kind}`;
  el.classList.remove("hidden");
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.add("hidden"), 5000);
}

function notifyError(message) {
  showToast(message, "error");
}

const VIEW_STORAGE_KEY = "rustdl-web-view";

function showConfirmDialog(message, title = "Confirm") {
  return new Promise((resolve) => {
    const dlg = document.getElementById("confirm-dialog");
    const titleEl = document.getElementById("confirm-title");
    const msgEl = document.getElementById("confirm-message");
    if (!dlg || !titleEl || !msgEl) {
      resolve(window.confirm(message));
      return;
    }
    titleEl.textContent = title;
    msgEl.textContent = message;
    const onClose = (ok) => {
      dlg.removeEventListener("close", onCloseHandler);
      resolve(ok);
    };
    const onCloseHandler = () => onClose(dlg.returnValue === "confirm");
    dlg.addEventListener("close", onCloseHandler);
    document.getElementById("btn-confirm-cancel").onclick = () => {
      dlg.close("cancel");
    };
    document.getElementById("confirm-form").onsubmit = (e) => {
      e.preventDefault();
      dlg.close("confirm");
    };
    dlg.showModal();
  });
}

function showPromptDialog(message, defaultValue = "", title = "Input") {
  return new Promise((resolve) => {
    const dlg = document.getElementById("prompt-dialog");
    const titleEl = document.getElementById("prompt-title");
    const msgEl = document.getElementById("prompt-message");
    const input = document.getElementById("prompt-input");
    if (!dlg || !titleEl || !msgEl || !input) {
      resolve(window.prompt(message, defaultValue));
      return;
    }
    titleEl.textContent = title;
    msgEl.textContent = message;
    input.value = defaultValue || "";
    const onClose = () => {
      dlg.removeEventListener("close", onCloseHandler);
      resolve(dlg.returnValue === "ok" ? input.value : null);
    };
    const onCloseHandler = onClose;
    dlg.addEventListener("close", onCloseHandler);
    document.getElementById("btn-prompt-cancel").onclick = () => dlg.close("cancel");
    document.getElementById("prompt-form").onsubmit = (e) => {
      e.preventDefault();
      dlg.close("ok");
    };
    dlg.showModal();
    input.focus();
  });
}

function maskWebToken(token) {
  const t = String(token || "").trim();
  if (!t) return "";
  if (t.length <= 8) return t;
  return `${t.slice(0, 4)}…${t.slice(-4)}`;
}

function randomWebToken() {
  const bytes = new Uint8Array(24);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

// Route legacy alert() calls through the toast bar (LAN/mobile friendly).
window.alert = (message) => showToast(String(message), "error");

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function splitLogLine(line) {
  if (!line.startsWith("[")) return { ts: "", body: line };
  const rest = line.slice(1);
  const sep = rest.indexOf("] ");
  if (sep !== 19) return { ts: "", body: line };
  const ts = rest.slice(0, 19);
  if (!/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(ts)) return { ts: "", body: line };
  return { ts, body: rest.slice(sep + 2) };
}

function formatRelativeAgo(date) {
  const sec = Math.floor((Date.now() - date.getTime()) / 1000);
  if (sec < 10) return "just now";
  if (sec < 60) return `${sec} sec ago`;
  if (sec < 3600) {
    const m = Math.floor(sec / 60);
    return m === 1 ? "1 min ago" : `${m} min ago`;
  }
  if (sec < 86400) {
    const h = Math.floor(sec / 3600);
    return h === 1 ? "1 hr ago" : `${h} hr ago`;
  }
  if (sec < 604800) {
    const d = Math.floor(sec / 86400);
    return d === 1 ? "1 day ago" : `${d} days ago`;
  }
  const y = date.getFullYear();
  const nowY = new Date().getFullYear();
  if (y === nowY) {
    return date.toLocaleString(undefined, { month: "short", day: "numeric" });
  }
  return date.toLocaleDateString();
}

function formatRelativeTime(unixSecs) {
  if (!unixSecs) return "";
  return formatRelativeAgo(new Date(unixSecs * 1000));
}

function formatLogLineDisplay(line, relative) {
  const { ts, body } = splitLogLine(line);
  if (!ts) return line;
  if (!relative) return `[${ts}] ${body}`;
  const parsed = new Date(ts.replace(" ", "T"));
  if (Number.isNaN(parsed.getTime())) return `[${ts}] ${body}`;
  return `[${formatRelativeAgo(parsed)}] ${body}`;
}

function shouldAutoscrollLog() {
  return (cachedSettings || {}).autoscroll_log !== false;
}

function logMessageBody(line) {
  const { body } = splitLogLine(line);
  return body || line;
}

function isLogErrorLine(body) {
  const lower = body.toLowerCase();
  const keywords = logFilterRules?.error_keywords || [
    "error",
    "failed",
    "failure",
    "not found",
    "invalid",
    "missing",
    "denied",
  ];
  return keywords.some((kw) => lower.includes(kw));
}

function logFilterAccepts(line, filter) {
  const body = logMessageBody(line);
  if (filter === "errors") return isLogErrorLine(body);
  if (filter === "important") {
    const lower = body.toLowerCase();
    const keywords = logFilterRules?.important_keywords || [
      "metadata fetch failed",
      "download failed",
      "starting",
      "started",
      "completed",
      "done",
      "queue",
      "convert",
      "skipped",
      "skip_reason",
    ];
    return isLogErrorLine(body) || keywords.some((kw) => lower.includes(kw));
  }
  return true;
}

function currentLogFilter() {
  const sel = document.getElementById("log-filter");
  if (sel && sel.value) return sel.value;
  return (cachedSettings || {}).log_filter || "all";
}

function renderLogView() {
  const log = document.getElementById("log-view");
  if (!log) return;
  const relative = !!(cachedSettings || {}).log_relative_time;
  const filter = currentLogFilter();
  const filtered = logLinesCache.filter((l) => logFilterAccepts(l, filter));
  const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 24;
  if (!logLinesCache.length) {
    log.textContent = "Activity from rustdl will appear here (downloads, converts, settings changes).";
    log.classList.add("log-empty");
  } else if (!filtered.length) {
    log.textContent = `No log lines match the "${filter}" filter.`;
    log.classList.add("log-empty");
  } else {
    log.classList.remove("log-empty");
    log.textContent = filtered.map((l) => formatLogLineDisplay(l, relative)).join("\n");
  }
  if (shouldAutoscrollLog() || atBottom) {
    log.scrollTop = log.scrollHeight;
  }
}

function loadDoneHistoryFilter() {
  try {
    const raw = localStorage.getItem(DONE_HISTORY_FILTER_KEY);
    if (raw === "all" || raw === null || raw === "") return null;
    const n = parseInt(raw, 10);
    return Number.isFinite(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

function saveDoneHistoryFilter(days) {
  doneHistoryFilterDays = days;
  try {
    if (days == null) localStorage.removeItem(DONE_HISTORY_FILTER_KEY);
    else localStorage.setItem(DONE_HISTORY_FILTER_KEY, String(days));
  } catch {
    /* ignore */
  }
}

function itemMatchesDoneHistory(item) {
  if (item.status !== "Done") return true;
  if (doneHistoryFilterDays == null) return true;
  const completedAt = item.completed_at || 0;
  if (!completedAt) return true;
  const cutoff = Math.floor(Date.now() / 1000) - doneHistoryFilterDays * 86400;
  return completedAt >= cutoff;
}

function applyWebTheme(theme) {
  const t = theme === "light" ? "light" : "dark";
  document.body.classList.toggle("theme-light", t === "light");
  localStorage.setItem(WEB_THEME_KEY, t);
  const btn = document.getElementById("btn-theme-toggle");
  if (btn) btn.textContent = t === "light" ? "Dark theme" : "Light theme";
}

const DEFAULT_MODE_DOWNLOADER = "#42a5f5";
const DEFAULT_MODE_CONVERT = "#ab47bc";

function normalizeModeHex(raw, fallback) {
  let s = String(raw || "").trim().toLowerCase();
  if (!s) return fallback;
  if (!s.startsWith("#")) s = `#${s}`;
  if (/^#[0-9a-f]{6}$/.test(s)) return s;
  if (/^#[0-9a-f]{3}$/.test(s)) {
    return `#${s[1]}${s[1]}${s[2]}${s[2]}${s[3]}${s[3]}`;
  }
  return fallback;
}

function hexToRgb(hex) {
  const h = normalizeModeHex(hex, "#000000").slice(1);
  return {
    r: parseInt(h.slice(0, 2), 16),
    g: parseInt(h.slice(2, 4), 16),
    b: parseInt(h.slice(4, 6), 16),
  };
}

function applyModeColors(settings) {
  const dl = normalizeModeHex(settings?.mode_downloader_color, DEFAULT_MODE_DOWNLOADER);
  const cv = normalizeModeHex(settings?.mode_convert_color, DEFAULT_MODE_CONVERT);
  const dlRgb = hexToRgb(dl);
  const cvRgb = hexToRgb(cv);
  const root = document.documentElement;
  root.style.setProperty("--mode-downloader", dl);
  root.style.setProperty(
    "--mode-downloader-soft",
    `rgba(${dlRgb.r}, ${dlRgb.g}, ${dlRgb.b}, 0.1)`,
  );
  root.style.setProperty(
    "--mode-downloader-border",
    `rgba(${dlRgb.r}, ${dlRgb.g}, ${dlRgb.b}, 0.32)`,
  );
  root.style.setProperty("--mode-convert", cv);
  root.style.setProperty(
    "--mode-convert-soft",
    `rgba(${cvRgb.r}, ${cvRgb.g}, ${cvRgb.b}, 0.1)`,
  );
  root.style.setProperty(
    "--mode-convert-border",
    `rgba(${cvRgb.r}, ${cvRgb.g}, ${cvRgb.b}, 0.32)`,
  );
}

function readModeColorField(pickerId, hexId) {
  const hexEl = document.getElementById(hexId);
  const pickerEl = document.getElementById(pickerId);
  const typed = hexEl?.value?.trim() || "";
  if (typed) return typed;
  return pickerEl?.value || "";
}

function syncModeColorControls(pickerId, hexId, storedHex, fallback) {
  const picker = document.getElementById(pickerId);
  const hex = document.getElementById(hexId);
  if (!picker || !hex) return;
  const effective = normalizeModeHex(storedHex, fallback);
  picker.value = effective;
  hex.value = storedHex?.trim() || "";
}

function wireModeColorControls() {
  const rows = [
    [
      "set-mode-downloader-color",
      "set-mode-downloader-hex",
      "btn-mode-downloader-default",
      DEFAULT_MODE_DOWNLOADER,
    ],
    [
      "set-mode-convert-color",
      "set-mode-convert-hex",
      "btn-mode-convert-default",
      DEFAULT_MODE_CONVERT,
    ],
  ];
  const previewFromForm = () =>
    applyModeColors({
      mode_downloader_color: readModeColorField(
        "set-mode-downloader-color",
        "set-mode-downloader-hex",
      ),
      mode_convert_color: readModeColorField("set-mode-convert-color", "set-mode-convert-hex"),
    });
  for (const [pickerId, hexId, resetId, fallback] of rows) {
    const picker = document.getElementById(pickerId);
    const hex = document.getElementById(hexId);
    const reset = document.getElementById(resetId);
    if (!picker || !hex) continue;
    picker.addEventListener("input", () => {
      hex.value = picker.value;
      previewFromForm();
    });
    hex.addEventListener("input", () => {
      const normalized = normalizeModeHex(hex.value, fallback);
      if (/^#[0-9a-f]{6}$/.test(normalized)) {
        picker.value = normalized;
      }
      previewFromForm();
    });
    reset?.addEventListener("click", () => {
      hex.value = "";
      picker.value = fallback;
      previewFromForm();
    });
  }
}

function initWebTheme() {
  const stored = localStorage.getItem(WEB_THEME_KEY);
  if (stored === "light" || stored === "dark") {
    applyWebTheme(stored);
    return;
  }
  if (cachedSettings?.theme === "light") applyWebTheme("light");
  else applyWebTheme("dark");
}

const ORGANIZE_FOLDER_PREFIX = {
  flat: "",
  uploader: "%(uploader)s/",
  playlist: "%(playlist_title)s/",
  date_ym: "%(upload_date>%Y)s/%(upload_date>%m)s/",
};

const ORGANIZE_FILENAME_SUFFIX = {
  title_id: "%(title)s [%(id)s].%(ext)s",
  date_title_id: "%(upload_date)s - %(title)s [%(id)s].%(ext)s",
  playlist_index_title_id: "%(playlist_index)03d - %(title)s [%(id)s].%(ext)s",
  title_only: "%(title)s.%(ext)s",
};

function usesCustomOrganizeTemplate(settings) {
  return (
    settings.download_organize_folder === "custom" ||
    settings.download_organize_filename === "custom"
  );
}

function composeOutputTemplate(settings) {
  if (usesCustomOrganizeTemplate(settings)) {
    const t = (settings.output_filename_template || "").trim();
    return t || "%(title)s [%(id)s].%(ext)s";
  }
  const folder = settings.download_organize_folder || "flat";
  const filename = settings.download_organize_filename || "title_id";
  const prefix = ORGANIZE_FOLDER_PREFIX[folder] || "";
  const suffix = ORGANIZE_FILENAME_SUFFIX[filename] || ORGANIZE_FILENAME_SUFFIX.title_id;
  return `${prefix}${suffix}`;
}

function exampleOrganizePath(settings) {
  const dir = (settings.output_dir || "").trim() || "Downloads";
  if (usesCustomOrganizeTemplate(settings)) {
    const rel = composeOutputTemplate(settings)
      .replace("%(title)s", "Example Video")
      .replace("%(id)s", "abc123")
      .replace("%(ext)s", "mp4")
      .replace("%(uploader)s", "Example Channel")
      .replace("%(playlist_title)s", "My Playlist")
      .replace("%(playlist_index)03d", "001")
      .replace("%(upload_date)s", "20240115")
      .replace("%(upload_date>%Y)s", "2024")
      .replace("%(upload_date>%m)s", "01");
    return `${dir}/${rel}`;
  }
  const parts = [];
  const folder = settings.download_organize_folder || "flat";
  if (folder === "uploader") parts.push("Example Channel");
  else if (folder === "playlist") parts.push("My Playlist");
  else if (folder === "date_ym") parts.push("2024", "01");
  const filename = settings.download_organize_filename || "title_id";
  let name = "Example Video [abc123].mp4";
  if (filename === "date_title_id") name = "20240115 - Example Video [abc123].mp4";
  else if (filename === "playlist_index_title_id") name = "001 - Example Video [abc123].mp4";
  else if (filename === "title_only") name = "Example Video.mp4";
  parts.push(name);
  return `${dir}/${parts.join("/")}`;
}

function applyOrganizePreset(settings, preset) {
  if (preset === "flat") {
    settings.download_organize_folder = "flat";
    settings.download_organize_filename = "title_id";
  } else if (preset === "uploader") {
    settings.download_organize_folder = "uploader";
    settings.download_organize_filename = "title_id";
  } else if (preset === "playlist") {
    settings.download_organize_folder = "playlist";
    settings.download_organize_filename = "playlist_index_title_id";
  } else if (preset === "date") {
    settings.download_organize_folder = "date_ym";
    settings.download_organize_filename = "date_title_id";
  }
}

function updateOrganizeUi(settings) {
  const custom = usesCustomOrganizeTemplate(settings || {});
  const wrap = document.getElementById("wrap-output-template");
  if (wrap) wrap.classList.toggle("hidden", !custom);
  const ex = document.getElementById("organize-example-path");
  if (ex && settings) ex.textContent = `Example: ${exampleOrganizePath(settings)}`;
  const eff = document.getElementById("effective-output-template");
  if (eff && settings) eff.textContent = composeOutputTemplate(settings);
  const warn = document.getElementById("organize-title-only-warn");
  if (warn && settings) {
    warn.classList.toggle(
      "hidden",
      settings.download_organize_filename !== "title_only",
    );
  }
}

function loadWebLayoutHeights() {
  try {
    const raw = localStorage.getItem(WEB_LAYOUT_HEIGHTS_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function saveWebLayoutHeights(patch) {
  const cur = loadWebLayoutHeights();
  localStorage.setItem(WEB_LAYOUT_HEIGHTS_KEY, JSON.stringify({ ...cur, ...patch }));
}

function applyWebLayoutHeights() {
  const h = loadWebLayoutHeights();
  const logView = document.getElementById("log-view");
  const queue = document.getElementById("queue");
  const convertQueue = document.getElementById("convert-queue");
  if (logView && h.logHeightPx >= WEB_LOG_MIN_HEIGHT_PX) {
    logView.style.height = `${h.logHeightPx}px`;
    logView.style.maxHeight = `${Math.max(h.logHeightPx, 240)}px`;
  }
  const queueMin = h.queueMinHeightPx;
  if (queueMin >= WEB_QUEUE_MIN_HEIGHT_PX) {
    if (queue) queue.style.minHeight = `${queueMin}px`;
    if (convertQueue) convertQueue.style.minHeight = `${queueMin}px`;
  }
}

function initWebLayoutHeightPersistence() {
  applyWebLayoutHeights();
  const logView = document.getElementById("log-view");
  if (!logView || typeof ResizeObserver === "undefined") return;
  let saveTimer = null;
  const ro = new ResizeObserver(() => {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      const h = logView.offsetHeight;
      if (h >= WEB_LOG_MIN_HEIGHT_PX) {
        saveWebLayoutHeights({ logHeightPx: h });
      }
    }, 250);
  });
  ro.observe(logView);
}

function applyLayoutPreset(settings, preset) {
  if (preset === "compact") {
    settings.card_list_layout = true;
    settings.compact_cards = true;
    settings.hide_card_subtitle = true;
    settings.show_thumbnails = true;
  } else if (preset === "review") {
    settings.card_list_layout = false;
    settings.compact_cards = false;
    settings.hide_card_subtitle = false;
    settings.show_thumbnails = true;
  } else if (preset === "minimal") {
    settings.card_list_layout = true;
    settings.compact_cards = true;
    settings.hide_card_subtitle = true;
    settings.show_thumbnails = false;
  }
}

function itemMatchesSearch(item, query) {
  const q = String(query || "").trim().toLowerCase();
  if (!q) return true;
  const hay = [
    item.title,
    item.source_line,
    item.webpage_url,
    item.uploader,
    item.video_id,
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return hay.includes(q);
}

function convertItemMatchesSearch(item, query) {
  const q = String(query || "").trim().toLowerCase();
  if (!q) return true;
  const hay = [item.source_path, item.output_path, item.detail]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return hay.includes(q);
}

function libraryItemMatchesSearch(item, query) {
  const q = String(query || "").trim().toLowerCase();
  if (!q) return true;
  const hay = [item.title, item.webpage_url, item.uploader, item.video_id, item.local_path]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  return hay.includes(q);
}

function libraryItemWithinHistory(item, days) {
  if (days == null) return true;
  if (!item.completed_at) return false;
  const cutoff = Math.floor(Date.now() / 1000) - days * 86400;
  return item.completed_at >= cutoff;
}

function getConvertSearchQuery() {
  const el = document.getElementById("convert-search");
  const settings = cachedSettings || {};
  return el ? el.value : settings.queue_search || "";
}

function browserNotificationsEnabled() {
  return cachedSettings?.web_browser_notifications !== false;
}

let notificationPermissionRequested = false;

async function ensureNotificationPermission() {
  if (!("Notification" in window)) return false;
  if (Notification.permission === "granted") return true;
  if (Notification.permission === "denied") return false;
  const result = await Notification.requestPermission();
  return result === "granted";
}

function maybeRequestNotificationPermissionOnGesture() {
  if (notificationPermissionRequested || !browserNotificationsEnabled()) return;
  if (!("Notification" in window) || Notification.permission !== "default") return;
  notificationPermissionRequested = true;
  ensureNotificationPermission().catch(() => {});
}

function showSessionNotification(body) {
  if (!browserNotificationsEnabled()) return;
  if (!("Notification" in window) || Notification.permission !== "granted") return;
  try {
    new Notification("rustdl", { body, icon: "/favicon.png" });
  } catch {
    /* ignore */
  }
}

function notifyConvertBatchComplete() {
  const data = lastConvertPayload;
  if (!data?.items?.length) return;
  let done = 0;
  let failed = 0;
  let skipped = 0;
  for (const it of data.items) {
    if (it.skipped) skipped += 1;
    else if (it.status === "Done") done += 1;
    else if (it.status === "Failed") failed += 1;
  }
  showSessionNotification(
    `Convert batch finished: ${done} done, ${failed} failed, ${skipped} skipped`,
  );
}

function itemMatchesStatusFilter(item, slug) {
  if (!slug) return true;
  const st = statusSlug(item.status);
  if (slug === "ready") return st === "idle";
  if (slug === "active") return st === "downloading";
  return st === slug;
}

function renderConfigWarnings(warnings) {
  const el = document.getElementById("config-warnings");
  if (!el) return;
  if (!warnings?.length) {
    el.classList.add("hidden");
    el.innerHTML = "";
    return;
  }
  el.classList.remove("hidden");
  el.innerHTML = warnings.map((w) => `<p>${escapeHtml(w)}</p>`).join("");
}

function sessionRestoreBodyText(restore) {
  const parts = [];
  if (restore.downloader_count > 0) {
    parts.push(`${restore.downloader_count} downloader item(s)`);
  }
  if (restore.convert_count > 0) {
    parts.push(`${restore.convert_count} convert item(s)`);
  }
  if (!parts.length) return "Restore saved queue state from the last session?";
  return `Restore saved queue from the last session? (${parts.join(", ")})`;
}

async function maybePromptSessionRestore(statusData) {
  const restore = statusData?.session_restore;
  if (!restore?.pending) {
    sessionRestoreHandled = false;
    return;
  }
  if (sessionRestoreHandled || sessionRestorePromptOpen) return;
  sessionRestoreHandled = true;
  const pref = (cachedSettings || {}).session_restore_preference || "ask";
  if (pref.toLowerCase() === "never") {
    await api("/api/session-restore/discard", { method: "POST" }).catch(() => {});
    return;
  }
  if (pref.toLowerCase() === "always") {
    await api("/api/session-restore/apply", { method: "POST" }).catch(() => {});
    await refreshAll();
    return;
  }
  sessionRestorePromptOpen = true;
  const ok = await showConfirmDialog(sessionRestoreBodyText(restore), "Restore session");
  sessionRestorePromptOpen = false;
  try {
    if (ok) {
      await api("/api/session-restore/apply", { method: "POST" });
    } else {
      await api("/api/session-restore/discard", { method: "POST" });
    }
    await refreshAll();
  } catch (e) {
    notifyError(e.message || String(e));
  }
}

async function saveQueueSearchSetting(value) {
  if (!cachedSettings) return;
  const patch = { ...cachedSettings, queue_search: value };
  try {
    const res = await api("/api/settings", {
      method: "POST",
      body: JSON.stringify({ settings: patch }),
    });
    const data = await res.json();
    cachedSettings = data.settings;
  } catch {
    /* ignore */
  }
}

const AUTO_ADD_MS = 700;
let autoAddTimer = null;
const statusFlags = {
  auto_add_pasted_urls: false,
  add_in_progress: false,
  shutdown_pending: false,
};

let shuttingDown = false;
/** @type {number | null} */
let refreshIntervalId = null;
/** Debounce SSE/poll rebuilds so in-flight thumbnails are not aborted every tick. */
let refreshAllTimer = null;
let statusRefreshTimer = null;

function scheduleStatusRefresh(delayMs = 300) {
  if (statusRefreshTimer) clearTimeout(statusRefreshTimer);
  statusRefreshTimer = setTimeout(() => {
    statusRefreshTimer = null;
    refreshStatus().catch(() => {});
    refreshLogs().catch(() => {});
  }, delayMs);
}

let statusPollIntervalId = null;

function startStatusPoll() {
  if (statusPollIntervalId != null) return;
  statusPollIntervalId = setInterval(() => {
    if (shuttingDown) return;
    if (sseConnected) return;
    refreshStatus().catch(() => {});
    refreshLogs().catch(() => {});
  }, 5000);
}
/** @type {object | null} */
let lastStatusPayload = null;
/** @type {object | null} */
let lastConvertPayload = null;

let cachedHasYtDlp = false;

/** Last queue generation from `/api/queue` (skip rebuild when unchanged). */
let lastQueueGeneration = 0;
let lastQueueStructureKey = "";
/** Last status generation from `/api/status`. */
let lastStatusGeneration = 0;
let sseConnected = false;

const THUMB_MAX_RETRIES = 3;
/** Per-cache-key failed fetch/decode attempts (cleared on success or metadata change). */
const thumbRetryCounts = new Map();
/** Blob URLs survive queue DOM rebuilds (SSE / polling used to abort direct img src loads). */
const thumbBlobCache = new Map();
/** @type {Map<string, Promise<string|null>>} */
const thumbInflight = new Map();

/** @type {HTMLMediaElement | null} */
let activeMediaEl = null;

function token() {
  return localStorage.getItem(TOKEN_KEY) || "";
}

function apiAuthOptional() {
  return !!token() || ipAuthBypass;
}

/** Append token query param when the client uses token auth. */
function apiUrlWithAuth(path) {
  const t = token();
  if (!t) return path;
  const sep = path.includes("?") ? "&" : "?";
  return `${path}${sep}token=${encodeURIComponent(t)}`;
}

function headers() {
  const h = { "Content-Type": "application/json" };
  const t = token();
  if (t) h["X-Rustdl-Token"] = t;
  return h;
}

/** Auth headers for binary GETs (no JSON Content-Type). */
function imageFetchHeaders() {
  const h = {};
  const t = token();
  if (t) h["X-Rustdl-Token"] = t;
  return h;
}

async function blobFromImageResponse(res) {
  const ct =
    res.headers.get("Content-Type")?.split(";")[0]?.trim() || "image/jpeg";
  const buf = await res.arrayBuffer();
  return new Blob([buf], { type: ct });
}

async function api(path, options = {}) {
  const res = await fetch(path, {
    ...options,
    headers: { ...headers(), ...(options.headers || {}) },
  });
  if (res.status === 401) {
    const msg =
      "Token rejected. Copy the current API token from rustdl Settings → Web UI, paste it below, then click Save token.";
    showAuthPanel(msg);
    throw new Error(msg);
  }
  if (!res.ok) {
    throw new Error(await readApiError(res, `Request failed (${res.status})`));
  }
  return res;
}

function showAuthPanel(statusText) {
  document.getElementById("auth-panel").classList.remove("hidden");
  document.getElementById("app-main").classList.add("hidden");
  if (statusText) {
    document.getElementById("auth-status").textContent = statusText;
  }
}

function thumbFailurePlaceholder(reason, cacheKey) {
  if ((reason === "no_token" || !token()) && !ipAuthBypass) {
    return "Save API token to load thumbnails";
  }
  if (reason === "unauthorized") {
    return "Token rejected — re-save below";
  }
  if (reason === "unavailable" || thumbFailuresExhausted(cacheKey)) {
    return "Thumbnail unavailable";
  }
  return "Fetching thumbnail…";
}

function convertThumbFailurePlaceholder(reason, key) {
  if ((reason === "no_token" || !token()) && !ipAuthBypass) {
    return "Save API token to load thumbnails";
  }
  if (reason === "unauthorized") {
    return "Token rejected — re-save below";
  }
  if (reason === "unavailable" || convertThumbFailedKeys.has(key)) {
    return "No preview available";
  }
  return "Loading preview…";
}

function showApp() {
  document.getElementById("auth-panel").classList.add("hidden");
  document.getElementById("app-main").classList.remove("hidden");
  initWebLayoutHeightPersistence();
}

function renderTools(tools) {
  const root = document.getElementById("tools-status");
  if (!root || !tools) return;
  root.innerHTML = "";
  for (const key of ["yt_dlp", "ffmpeg", "ffprobe"]) {
    const t = tools[key];
    if (!t) continue;
    const el = document.createElement("span");
    el.className = "tool-badge " + (t.ok ? "ok" : "missing");
    const short = t.version_short ? ` · ${t.version_short}` : "";
    const pathHint = t.configured_path ? ` · ${t.configured_path}` : "";
    el.textContent = `${t.ok ? "✔" : "✖"} ${t.name} ${t.status}${short}`;
    if (t.version) el.title = t.version + pathHint;
    else if (pathHint) el.title = pathHint.trim();
    root.appendChild(el);
  }
}

async function refreshToolsOnly() {
  const res = await api("/api/tools/refresh", { method: "POST" });
  const tools = await res.json();
  renderTools(tools);
  clearThumbnailCaches();
}

async function refreshStatus() {
  const res = await api("/api/status");
  const data = await res.json();
  lastStatusPayload = data;
  const s = data.status;
  statusFlags.auto_add_pasted_urls = !!data.auto_add_pasted_urls;
  statusFlags.add_in_progress = !!data.add_in_progress;
  statusFlags.shutdown_pending = !!data.shutdown_pending;
  if (data.shutdown_pending) shuttingDown = true;
  renderStatusSummary(data);
  renderNavbarStatus();
  renderNavbarSystemUsage(data.system_usage);
  renderNavbarDiskSpace(data.output_disk_space);
  updateTopbarVersion(data);
  updateSettingsOutputDiskHint(data.output_disk_space);
  updateQuitButtonState();
  renderTools(data.tools);
  renderConfigWarnings(data.config_warnings);
  if (data.log_filter_rules) logFilterRules = data.log_filter_rules;
  cachedHasYtDlp = data.tools?.yt_dlp?.ok === true;
  updateDownloadControlButtons(data);
  await maybePromptSessionRestore(data);
  if (typeof data.generation === "number") {
    lastStatusGeneration = data.generation;
  }
}

/**
 * @returns {{ slug: string, label: string, pulse: boolean, title: string }}
 */
function deriveNavbarStatus(statusData, convertData) {
  const s = statusData?.status || {};
  const resolving = s.resolving || 0;
  const queued = s.queued || 0;
  const active = s.active || 0;
  const ready = s.ready || 0;
  const paused = !!statusData?.downloads_paused;
  const queueRunning = statusData?.queue_running ?? 0;
  const convertRunning = !!statusData?.convert_running || !!convertData?.running;
  const convertResolving =
    convertData?.items?.some(
      (it) => it.status === "Resolving" || it.probing
    ) ?? false;

  if (statusFlags.shutdown_pending || shuttingDown || statusData?.shutdown_pending) {
    return {
      slug: "shutdown",
      label: "Shutting down",
      pulse: true,
      title: "rustdl is saving state and exiting",
    };
  }
  if (statusFlags.add_in_progress) {
    return {
      slug: "adding",
      label: "Adding URLs",
      pulse: true,
      title: "Fetching metadata for new URLs",
    };
  }
  if (convertRunning) {
    return {
      slug: "converting",
      label: "Converting",
      pulse: true,
      title: "Convert batch encode in progress",
    };
  }
  if (active > 0 || (queueRunning > 0 && !paused)) {
    return {
      slug: "downloading",
      label: "Downloading",
      pulse: true,
      title: `${active} active · ${queueRunning} worker slot(s)`,
    };
  }
  if (resolving > 0 || convertResolving) {
    return {
      slug: "resolving",
      label: "Resolving",
      pulse: true,
      title: "Probing media metadata",
    };
  }
  if (paused && (queued > 0 || active > 0 || ready > 0)) {
    return {
      slug: "paused",
      label: "Paused",
      pulse: false,
      title: "Downloads paused — resume to continue",
    };
  }
  if (queued > 0) {
    return {
      slug: "queued",
      label: "Queued",
      pulse: true,
      title: `${queued} item(s) waiting to download`,
    };
  }
  return {
    slug: "idle",
    label: "Idle",
    pulse: false,
    title: "No active downloads or conversions",
  };
}

function renderNavbarStatus() {
  const root = document.getElementById("navbar-status");
  if (!root) return;
  const info = deriveNavbarStatus(lastStatusPayload, lastConvertPayload);
  const shouldPulse = info.slug !== "idle" && info.slug !== "paused";
  root.className =
    "navbar-status navbar-status-" +
    info.slug +
    (shouldPulse ? " navbar-status-pulse" : "");
  root.title = info.title;
  const label = root.querySelector(".navbar-status-label");
  if (label) label.textContent = info.label;
}

function updateTopbarVersion(data) {
  const el = document.getElementById("topbar-version");
  if (!el || !data?.version) return;
  el.textContent = `v${data.version}`;
}

function populateAboutDialog(data) {
  const versionEl = document.getElementById("about-version");
  const buildEl = document.getElementById("about-build");
  if (versionEl) {
    versionEl.textContent = data?.version
      ? `Version ${data.version}`
      : "Version unknown (connect to rustdl to load)";
  }
  if (buildEl) {
    buildEl.textContent = data?.build_date
      ? `Build ${data.build_date}`
      : "";
    buildEl.classList.toggle("hidden", !data?.build_date);
  }
}

function openAboutDialog() {
  populateAboutDialog(lastStatusPayload);
  const dlg = document.getElementById("about-dialog");
  if (dlg) dlg.showModal();
}

function updateDownloadControlButtons(data) {
  const pauseBtn = document.getElementById("btn-pause");
  const resumeBtn = document.getElementById("btn-resume");
  const startBtn = document.getElementById("btn-start");
  if (!pauseBtn || !resumeBtn || !startBtn) return;

  const s = data?.status || {};
  const paused = !!data?.downloads_paused;
  const queued = s.queued || 0;
  const active = s.active || 0;
  const ready = s.ready || 0;
  const isShuttingDown = statusFlags.shutdown_pending || shuttingDown;

  const canPause = !paused && (queued > 0 || active > 0);
  const canResume = paused;
  const canStart = !paused && ready > 0 && cachedHasYtDlp;

  pauseBtn.disabled = isShuttingDown || !canPause;
  resumeBtn.disabled = isShuttingDown || !canResume;
  startBtn.disabled = isShuttingDown || !canStart;

  pauseBtn.title = isShuttingDown
    ? "Unavailable while shutting down"
    : canPause
      ? "Pause active and queued downloads"
      : paused
        ? "Downloads are already paused"
        : "No queued or active downloads to pause";

  resumeBtn.title = isShuttingDown
    ? "Unavailable while shutting down"
    : canResume
      ? "Resume downloads and start ready items"
      : "Downloads are not paused";

  startBtn.title = isShuttingDown
    ? "Unavailable while shutting down"
    : paused
      ? "Resume downloads first"
      : !cachedHasYtDlp
        ? "yt-dlp not available (check Settings or Refresh tools)"
        : canStart
          ? `Start ${ready} ready download(s)`
          : "No ready items to download";

  const retryFailedBtn = document.getElementById("btn-retry-failed");
  if (retryFailedBtn) {
    const failed = s.failed || 0;
    const canRetryFailed = !isShuttingDown && failed > 0 && cachedHasYtDlp;
    retryFailedBtn.disabled = !canRetryFailed;
    retryFailedBtn.title = isShuttingDown
      ? "Unavailable while shutting down"
      : failed === 0
        ? "No failed downloads"
        : !cachedHasYtDlp
          ? "yt-dlp not available (check Settings or Refresh tools)"
          : `Retry ${failed} failed download(s) that still have a URL`;
  }
}

function updateQuitButtonState() {
  const btn = document.getElementById("btn-quit");
  if (!btn) return;
  const busy = shuttingDown || statusFlags.shutdown_pending;
  btn.disabled = busy;
  btn.title = busy
    ? "Shutting down rustdl…"
    : "Quit rustdl (saves queue, cancels active jobs)";
}

function showShutdownNotice(message) {
  shuttingDown = true;
  updateQuitButtonState();
  if (refreshIntervalId != null) {
    clearInterval(refreshIntervalId);
    refreshIntervalId = null;
  }
  document.getElementById("settings-dialog")?.close();
  document.body.classList.add("shutdown-mode");
  const msgEl = document.getElementById("shutdown-message");
  if (msgEl) msgEl.textContent = message;
}

async function requestAppShutdown() {
  if (shuttingDown || statusFlags.shutdown_pending) return;
  const st = lastStatusPayload?.status;
  const workActive =
    statusFlags.add_in_progress ||
    (lastStatusPayload?.queue_running ?? 0) > 0 ||
    (st &&
      (st.resolving + st.queued + st.active > 0));
  let msg =
    "Quit rustdl? Your queue and settings will be saved.";
  if (workActive) {
    msg +=
      " Active downloads will be cancelled first, then rustdl will exit.";
  }
  if (!(await showConfirmDialog(msg, "Quit rustdl?"))) return;
  shuttingDown = true;
  updateQuitButtonState();
  showShutdownNotice("Shutting down rustdl…");
  await api("/api/shutdown", { method: "POST" });
}

function renderBatchProgressBar(root, progress, caption, animate) {
  if (!root) return;
  root.innerHTML = "";
  if (!progress || !progress.total) {
    root.classList.add("hidden");
    return;
  }
  root.classList.remove("hidden");
  const pct = Math.min(100, Math.max(0, Number(progress.percent) || progress.fraction * 100 || 0));
  const wrap = document.createElement("div");
  wrap.className = "batch-progress" + (animate ? " batch-progress-live" : "");
  wrap.setAttribute("role", "progressbar");
  wrap.setAttribute("aria-valuemin", "0");
  wrap.setAttribute("aria-valuemax", "100");
  wrap.setAttribute("aria-valuenow", String(Math.round(pct)));
  const fill = document.createElement("div");
  fill.className = "batch-progress-fill";
  fill.style.width = `${pct}%`;
  const label = document.createElement("span");
  label.className = "batch-progress-label";
  label.textContent = caption;
  wrap.appendChild(fill);
  wrap.appendChild(label);
  root.appendChild(wrap);
}

function renderDownloadBatchProgress(statusData) {
  const root = document.getElementById("download-batch-progress");
  const batch = statusData?.download_batch;
  if (!batch || !batch.total) {
    if (root) {
      root.innerHTML = "";
      root.classList.add("hidden");
    }
    return;
  }
  const failed = statusData?.status?.failed || 0;
  let caption = `Batch progress: ${batch.percent.toFixed(1)}% · ${batch.finished}/${batch.total} done`;
  if (batch.active > 0) caption += ` · ${batch.active} active`;
  if (failed > 0) caption += ` · ${failed} failed`;
  const animate =
    !!statusData?.add_in_progress ||
    (batch.active || 0) > 0 ||
    (statusData?.queue_running || 0) > 0;
  renderBatchProgressBar(root, batch, caption, animate);
}

function renderConvertBatchProgress(convertData) {
  const root = document.getElementById("convert-batch-progress");
  const batch = convertData?.batch_progress;
  if (!batch || !batch.total) {
    if (root) {
      root.innerHTML = "";
      root.classList.add("hidden");
    }
    return;
  }
  let caption = `Batch progress: ${batch.percent.toFixed(1)}% · ${batch.finished}/${batch.total} processed`;
  if (batch.active > 0) caption += ` · ${batch.active} active`;
  renderBatchProgressBar(root, batch, caption, !!convertData?.running);
}

function renderStatusSummary(data) {
  const root = document.getElementById("status-summary");
  if (!root) return;
  root.innerHTML = "";
  root.className = "status-summary";

  if (data.shutdown_pending || shuttingDown) {
    const el = document.createElement("span");
    el.className = "status-badge status-paused";
    el.innerHTML =
      '<span class="status-dot" aria-hidden="true"></span>Shutting down…';
    root.appendChild(el);
  }

  const runningCount = data.queue_running ?? 0;
  const paused = document.createElement("span");
  paused.className =
    "status-badge " + (data.downloads_paused ? "status-paused" : "status-live");
  paused.innerHTML = `<span class="status-dot" aria-hidden="true"></span>${runningCount} ${
    data.downloads_paused ? "Paused" : "Running"
  }`;
  root.appendChild(paused);

  const s = data.status;
  const parts = [
    ["resolving", s.resolving, "Resolving"],
    ["ready", s.ready, "Ready"],
    ["queued", s.queued, "Queued"],
    ["active", s.active, "Active"],
    ["done", s.done, "Done"],
    ["failed", s.failed, "Failed"],
  ];
  for (const [slug, count, label] of parts) {
    const el = document.createElement("span");
    el.className = `status-badge status-${slug} status-chip-filter`;
    if (count > 0) {
      el.title = `Filter queue to ${label}`;
    }
    if (queueStatusFilter === slug) {
      el.classList.add("active");
    }
    el.innerHTML = `<span class="status-dot" aria-hidden="true"></span>${count} ${label}`;
    if (count > 0) {
      el.onclick = () => {
        queueStatusFilter = queueStatusFilter === slug ? null : slug;
        refreshQueue(true).catch(console.error);
        renderStatusSummary(data);
      };
    }
    root.appendChild(el);
  }
  renderDownloadBatchProgress(data);
}

function diskSpaceLevel(disk) {
  return disk?.level || "ok";
}

function diskSpaceBarLevel(disk) {
  const used = diskSpacePercentUsed(disk);
  if (used == null) return "ok";
  if (used >= 90) return "critical";
  if (used >= 75) return "low";
  return "ok";
}

function diskSpaceFreeHtml(disk) {
  const level = diskSpaceLevel(disk);
  const free = formatBytes(disk.available_bytes);
  return `<span class="disk-space-free disk-space-free-${level}">${free} free</span>`;
}

function diskSpacePercentUsed(disk) {
  if (disk?.percent_free == null || !isFinite(disk.percent_free)) return null;
  return Math.max(0, Math.min(100, Math.round(100 - disk.percent_free)));
}

function diskSpaceBarHtml(disk) {
  const pct = diskSpacePercentUsed(disk);
  if (pct == null) return "";
  const level = diskSpaceBarLevel(disk);
  return `<div class="disk-space-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${pct}" aria-label="Used disk space">
    <div class="disk-space-bar-fill disk-space-bar-fill-${level}" style="width:${pct}%">${pct}%</div>
  </div>`;
}

function destinationDiskLabelHtml(suffix = "") {
  return `<span class="material-icons disk-space-label-icon" aria-hidden="true">${ICON.storage}</span> Destination disk${suffix}`;
}

function updateSettingsOutputDiskHint(disk) {
  const el = document.getElementById("settings-output-disk");
  if (!el) return;
  if (!disk || disk.total_bytes == null) {
    el.classList.add("hidden");
    el.innerHTML = "";
    return;
  }
  const vol = disk.volume_label ? ` (${disk.volume_label})` : "";
  el.innerHTML = `${destinationDiskLabelHtml(vol)}: ${diskSpaceFreeHtml(disk)} / ${formatBytes(
    disk.total_bytes
  )} total${diskSpaceBarHtml(disk)}`;
  el.classList.remove("hidden");
}

function formatUsagePercent(value) {
  if (value == null || !isFinite(value)) return "…";
  return `${Math.round(Math.max(0, Math.min(100, value)))}%`;
}

function usageLevelClass(percent) {
  if (percent == null || !isFinite(percent)) return "usage-badge-unknown";
  if (percent >= 90) return "usage-badge-critical";
  if (percent >= 75) return "usage-badge-warn";
  return "usage-badge-ok";
}

function usageBadgeIcon(name) {
  switch (name) {
    case "CPU":
      return ICON.speed;
    case "RAM":
      return ICON.memory;
    case "GPU":
      return ICON.monitor;
    default:
      return ICON.speed;
  }
}

function renderUsageBadge(name, percent) {
  const el = document.createElement("span");
  const level = usageLevelClass(percent);
  el.className = `usage-badge ${level}`;
  const icon = document.createElement("span");
  icon.className = "material-icons usage-badge-icon";
  icon.setAttribute("aria-hidden", "true");
  icon.textContent = usageBadgeIcon(name);
  el.appendChild(icon);
  el.appendChild(
    document.createTextNode(` ${name} ${formatUsagePercent(percent)}`)
  );
  el.title =
    percent != null && isFinite(percent)
      ? `${name} utilization: ${percent.toFixed(1)}%`
      : `${name} utilization: measuring…`;
  return el;
}

function renderNavbarSystemUsage(usage) {
  const root = document.getElementById("navbar-system-usage");
  if (!root) return;
  root.innerHTML = "";
  if (!usage) return;
  root.appendChild(renderUsageBadge("CPU", usage.cpu_percent));
  root.appendChild(renderUsageBadge("RAM", usage.ram_percent));
  if (usage.show_gpu) {
    root.appendChild(renderUsageBadge("GPU", usage.gpu_percent));
  }
}

function renderNavbarDiskSpace(disk) {
  const root = document.getElementById("navbar-disk-space");
  if (!root) return;
  root.innerHTML = "";
  if (!disk || disk.total_bytes == null) return;
  const level = diskSpaceLevel(disk);
  const el = document.createElement("span");
  el.className = `status-badge disk-space disk-space-${level}`;
  const vol = disk.volume_label ? ` (${disk.volume_label})` : "";
  const pctUsed = diskSpacePercentUsed(disk);
  const pct = pctUsed != null ? ` · ${pctUsed}% used` : "";
  el.title = "Used and free space on the output folder volume";
  el.innerHTML = `${destinationDiskLabelHtml(vol)}: ${diskSpaceFreeHtml(
    disk
  )} / ${formatBytes(disk.total_bytes)}${pct}`;
  root.appendChild(el);
}

function collectUrlsFromInput() {
  const text = document.getElementById("url-input").value;
  return text.split(/\n+/).map((s) => s.trim()).filter(Boolean);
}

function scheduleAutoAddFromInput() {
  if (!statusFlags.auto_add_pasted_urls) return;
  clearTimeout(autoAddTimer);
  autoAddTimer = setTimeout(() => {
    flushAutoAddFromInput().catch(() => {});
  }, AUTO_ADD_MS);
}

function showAddFeedback(result) {
  const el = document.getElementById("add-feedback");
  if (!el || !result) return;
  const accepted = result.accepted || 0;
  const dup = result.skipped_duplicates || 0;
  const invalid = result.skipped_invalid || 0;
  if (accepted === 0 && dup === 0 && invalid === 0) {
    el.classList.add("hidden");
    return;
  }
  const parts = [];
  if (accepted > 0) parts.push(`Added ${accepted} URL(s).`);
  if (dup > 0) parts.push(`Skipped ${dup} duplicate(s).`);
  if (invalid > 0) parts.push(`Skipped ${invalid} invalid line(s).`);
  el.textContent = parts.join(" ");
  el.classList.remove("hidden");
  el.classList.toggle("ok", accepted > 0 && dup === 0 && invalid === 0);
  if (accepted === 0) {
    showToast(parts.join(" ") || "No new URLs were added.", "warning");
  }
}

async function postQueueUrls(urls) {
  const res = await api("/api/queue", {
    method: "POST",
    body: JSON.stringify({ urls }),
  });
  return res.json();
}

async function flushAutoAddFromInput() {
  if (!statusFlags.auto_add_pasted_urls || statusFlags.add_in_progress) return;
  const urls = collectUrlsFromInput();
  if (!urls.length) return;
  const result = await postQueueUrls(urls);
  if ((result.accepted || 0) > 0) {
    document.getElementById("url-input").value = "";
  }
  showAddFeedback(result);
  await refreshAll();
}

function statusSlug(status) {
  return String(status || "Idle").toLowerCase();
}

function formatDuration(sec) {
  if (sec == null || sec < 0) return null;
  const s = Math.floor(sec % 60);
  const m = Math.floor((sec / 60) % 60);
  const h = Math.floor(sec / 3600);
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  return `${m}:${String(s).padStart(2, "0")}`;
}

function formatSubtitle(item, hideSubtitle) {
  if (hideSubtitle) return "";
  const parts = [];
  const dur = formatDuration(item.duration);
  if (dur) parts.push(dur);
  if (item.uploader && String(item.uploader).trim()) parts.push(String(item.uploader).trim());
  return parts.join(" · ");
}

function formatResolution(w, h) {
  if (!w || !h) return null;
  return `${w}×${h}`;
}

function thumbPlaceholderText(item, showThumbnails) {
  if (!showThumbnails) return "Thumbnails off";
  if (statusSlug(item.status) === "resolving") return "Fetching metadata…";
  if (itemHasThumbnailSource(item)) return "Fetching thumbnail…";
  return "No preview available";
}

function itemHasThumbnailSource(item) {
  if (item.thumbnail_url) return true;
  if (item.video_id && String(item.video_id).trim()) return true;
  if (item.playable || item.can_delete_file) return true;
  const line = item.source_line || item.webpage_url || "";
  return /youtu\.be\/|youtube\.com\/watch|youtube\.com\/shorts/i.test(line);
}

function isQueueableHttpUrl(line) {
  const s = String(line || "").trim();
  if (!s) return false;
  try {
    const u = new URL(s);
    if (u.protocol !== "http:" && u.protocol !== "https:") return false;
    return Boolean(u.hostname);
  } catch {
    return false;
  }
}

/** Page URL for copy/open (matches desktop `resolve_item_download_url`). */
function resolveItemPageUrl(item) {
  const web = String(item.webpage_url || "").trim();
  if (isQueueableHttpUrl(web)) return web;
  const src = String(item.source_line || "").trim();
  if (isQueueableHttpUrl(src)) return src;
  const vid = String(item.video_id || "").trim();
  if (vid) return `https://www.youtube.com/watch?v=${encodeURIComponent(vid)}`;
  return null;
}

function appendUrlMenuButton(group, item) {
  const url = resolveItemPageUrl(item);
  if (!url) return;

  const menu = document.createElement("details");
  menu.className = "btn-menu";

  const trigger = document.createElement("summary");
  trigger.className = "btn-menu-trigger secondary";
  trigger.title = url;
  setButtonLabel(trigger, ICON.link, "URL...");
  menu.appendChild(trigger);

  const panel = document.createElement("div");
  panel.className = "btn-menu-panel";
  panel.setAttribute("role", "menu");

  const copyBtn = document.createElement("button");
  copyBtn.type = "button";
  copyBtn.className = "btn-menu-item";
  setButtonLabel(copyBtn, ICON.contentCopy, "Copy URL");
  copyBtn.title = url;
  copyBtn.onclick = (e) => {
    e.preventDefault();
    menu.open = false;
    navigator.clipboard
      .writeText(url)
      .then(() => showToast("URL copied to clipboard."))
      .catch(() => {
        showPromptDialog("Copy this URL:", url, "Copy URL");
      });
  };
  panel.appendChild(copyBtn);

  const openBtn = document.createElement("button");
  openBtn.type = "button";
  openBtn.className = "btn-menu-item";
  setButtonLabel(openBtn, ICON.openInNew, "Open URL");
  openBtn.title = "Open in your default browser";
  openBtn.onclick = (e) => {
    e.preventDefault();
    menu.open = false;
    window.open(url, "_blank", "noopener,noreferrer");
  };
  panel.appendChild(openBtn);

  menu.appendChild(panel);
  group.appendChild(menu);
}

function thumbCacheKey(item) {
  return [
    item.item_id,
    item.video_id || "",
    item.thumbnail_url || "",
    item.local_path || "",
    item.media_filename || "",
    item.source_line || "",
    item.webpage_url || "",
  ].join("|");
}

/** Extensions the built-in video/audio element can decode in typical browsers. */
function browserCanPlayMediaFilename(name) {
  const ext = String(name || "")
    .split(".")
    .pop()
    ?.toLowerCase();
  if (!ext) return true;
  return [
    "mp4",
    "m4v",
    "webm",
    "mp3",
    "m4a",
    "opus",
    "ogg",
    "wav",
    "aac",
  ].includes(ext);
}

function revokeThumbBlob(cacheKey) {
  const url = thumbBlobCache.get(cacheKey);
  if (url) {
    URL.revokeObjectURL(url);
    thumbBlobCache.delete(cacheKey);
  }
}

function thumbFailuresExhausted(cacheKey) {
  return (thumbRetryCounts.get(cacheKey) || 0) >= THUMB_MAX_RETRIES;
}

function noteThumbFailure(cacheKey) {
  thumbRetryCounts.set(cacheKey, (thumbRetryCounts.get(cacheKey) || 0) + 1);
}

function pruneThumbFailedKeys(activeItems) {
  const active = new Set(activeItems.map((item) => thumbCacheKey(item)));
  for (const key of thumbRetryCounts.keys()) {
    if (!active.has(key)) thumbRetryCounts.delete(key);
  }
  for (const key of thumbBlobCache.keys()) {
    if (!active.has(key)) revokeThumbBlob(key);
  }
  for (const key of thumbInflight.keys()) {
    if (!active.has(key)) thumbInflight.delete(key);
  }
}

async function fetchQueueThumbnailBlob(item, options = {}) {
  const cacheKey = thumbCacheKey(item);
  if (thumbBlobCache.has(cacheKey)) {
    return { url: thumbBlobCache.get(cacheKey), reason: null };
  }
  if (!options.force && thumbFailuresExhausted(cacheKey)) {
    return { url: null, reason: "unavailable" };
  }
  if (thumbInflight.has(cacheKey)) {
    return thumbInflight.get(cacheKey);
  }
  const apiUrl = thumbnailApiUrl(item.item_id);
  if (!apiUrl) {
    return { url: null, reason: "no_token" };
  }
  const work = (async () => {
    try {
      const res = await fetch(apiUrl, { headers: imageFetchHeaders() });
      if (!res.ok) {
        if (res.status === 401) {
          showAuthPanel(
            "Token rejected. Copy the current API token from rustdl Settings → Web UI, paste it below, then click Save token."
          );
          return { url: null, reason: "unauthorized" };
        }
        noteThumbFailure(cacheKey);
        return { url: null, reason: "unavailable" };
      }
      const blob = await blobFromImageResponse(res);
      if (blob.size < 32) {
        noteThumbFailure(cacheKey);
        return { url: null, reason: "unavailable" };
      }
      const objUrl = URL.createObjectURL(blob);
      thumbBlobCache.set(cacheKey, objUrl);
      thumbRetryCounts.delete(cacheKey);
      return { url: objUrl, reason: null };
    } catch {
      noteThumbFailure(cacheKey);
      return { url: null, reason: "unavailable" };
    }
  })();
  thumbInflight.set(cacheKey, work);
  try {
    return await work;
  } finally {
    thumbInflight.delete(cacheKey);
  }
}

function stopActiveMedia() {
  if (!activeMediaEl) return;
  activeMediaEl.pause();
  const thumb = activeMediaEl.closest(".card-thumb");
  if (thumb) {
    thumb.querySelector("img")?.classList.remove("hidden");
    thumb.querySelector(".card-thumb-placeholder")?.classList.remove("hidden");
  }
  activeMediaEl.remove();
  activeMediaEl = null;
}

function mediaStreamUrl(itemId) {
  return apiUrlWithAuth(`/api/media/${itemId}`);
}

function toggleCardMedia(item, thumb) {
  const existing = thumb.querySelector(".card-media");
  if (existing) {
    stopActiveMedia();
    return;
  }
  stopActiveMedia();
  if (
    item.media_filename &&
    !browserCanPlayMediaFilename(item.media_filename)
  ) {
    const ph = thumb.querySelector(".card-thumb-placeholder");
    if (ph) {
      ph.textContent =
        "In-browser playback is not supported for this file type (e.g. MKV). Open the file on the PC running rustdl.";
      ph.classList.remove("hidden");
    }
    thumb.querySelector("img")?.classList.add("hidden");
    return;
  }
  const tag = item.media_kind === "audio" ? "audio" : "video";
  const el = document.createElement(tag);
  el.className = "card-media";
  el.controls = true;
  el.playsInline = true;
  el.preload = "metadata";
  el.src = mediaStreamUrl(item.item_id);
  el.addEventListener("error", () => {
    stopActiveMedia();
    const ph = thumb.querySelector(".card-thumb-placeholder");
    if (ph) {
      ph.textContent = item.playable
        ? "Playback failed (file missing or blocked)"
        : "No local file for this item";
      ph.classList.remove("hidden");
    }
  });
  thumb.querySelector("img")?.classList.add("hidden");
  thumb.querySelector(".card-thumb-placeholder")?.classList.add("hidden");
  thumb.appendChild(el);
  activeMediaEl = el;
  el.play().catch(() => {});
}

function createCardActionBar() {
  const bar = document.createElement("div");
  bar.className = "card-actions";
  const group = document.createElement("div");
  group.className = "btn-group";
  group.setAttribute("role", "group");
  bar.appendChild(group);
  return { bar, group };
}

async function reorderQueueItem(draggedId, targetId) {
  const res = await api("/api/queue/reorder", {
    method: "POST",
    body: JSON.stringify({ dragged_id: draggedId, target_id: targetId }),
  });
  if (!res.ok) {
    throw new Error(await readApiError(res, "Could not reorder queue item."));
  }
  await refreshQueue(true);
}

async function reorderConvertItem(draggedId, targetId) {
  const res = await api("/api/convert/reorder", {
    method: "POST",
    body: JSON.stringify({ dragged_id: draggedId, target_id: targetId }),
  });
  if (!res.ok) {
    throw new Error(await readApiError(res, "Could not reorder convert item."));
  }
  await refreshConvert();
}

async function requeueDoneItems(itemIds) {
  const res = await api("/api/queue/requeue", {
    method: "POST",
    body: JSON.stringify({ item_ids: itemIds }),
  });
  if (!res.ok) throw new Error("Re-queue failed.");
  const data = await res.json();
  await refreshAll();
  return data.requeued || 0;
}

async function refetchQueueItem(id) {
  const res = await api(`/api/queue/refetch/${id}`, { method: "POST" });
  if (!res.ok) {
    throw new Error(await readApiError(res, "Metadata refetch failed."));
  }
  await refreshAll();
}

async function cancelItemWithAction(id, postAction) {
  const res = await api(`/api/downloads/cancel/${id}`, {
    method: "POST",
    body: JSON.stringify({ post_action: postAction }),
  });
  if (!res.ok) throw new Error("Cancel failed.");
  await refreshAll();
}

async function setItemOverrides(id, formatOverride, profileOverride) {
  const res = await api(`/api/queue/${id}/overrides`, {
    method: "POST",
    body: JSON.stringify({
      format_override: formatOverride || null,
      profile_override: profileOverride || null,
    }),
  });
  if (!res.ok) throw new Error(await readApiError(res, "Could not update overrides."));
  await refreshQueue(true);
}

function appendReadyReorderButtons(group, item, readyItems, reorderFn) {
  const reorder = reorderFn || reorderQueueItem;
  const idx = readyItems.findIndex((it) => it.item_id === item.item_id);
  if (idx < 0) return;
  if (idx > 0) {
    const up = document.createElement("button");
    up.type = "button";
    up.className = "secondary icon-only";
    up.title = "Move up";
    setButtonLabel(up, ICON.arrowUpward, "↑");
    up.onclick = () =>
      reorder(item.item_id, readyItems[idx - 1].item_id).catch((e) =>
        alert(e.message || String(e))
      );
    group.appendChild(up);
  }
  if (idx < readyItems.length - 1) {
    const down = document.createElement("button");
    down.type = "button";
    down.className = "secondary icon-only";
    down.title = "Move down";
    setButtonLabel(down, ICON.arrowDownward, "↓");
    down.onclick = () =>
      reorder(item.item_id, readyItems[idx + 1].item_id).catch((e) =>
        alert(e.message || String(e))
      );
    group.appendChild(down);
  }
}

function attachReadyRowDragDrop(card, item, readyItems, reorderFn) {
  if (!card || readyItems.length < 2) return;
  const reorder = reorderFn || reorderQueueItem;
  card.draggable = true;
  card.classList.add("ready-draggable");
  card.addEventListener("dragstart", (e) => {
    card.classList.add("dragging");
    e.dataTransfer?.setData("text/plain", String(item.item_id));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  });
  card.addEventListener("dragend", () => {
    card.classList.remove("dragging");
    document.querySelectorAll(".ready-drop-target").forEach((el) => {
      el.classList.remove("ready-drop-target");
    });
  });
  card.addEventListener("dragover", (e) => {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    card.classList.add("ready-drop-target");
  });
  card.addEventListener("dragleave", () => {
    card.classList.remove("ready-drop-target");
  });
  card.addEventListener("drop", (e) => {
    e.preventDefault();
    card.classList.remove("ready-drop-target");
    const draggedRaw = e.dataTransfer?.getData("text/plain");
    const draggedId = parseInt(draggedRaw || "", 10);
    if (!draggedId || draggedId === item.item_id) return;
    reorder(draggedId, item.item_id).catch((err) => notifyError(err.message || String(err)));
  });
}

function appendVerifyStreamsButton(group, item) {
  if (statusSlug(item.status) !== "done") return;
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "secondary";
  setButtonLabel(btn, ICON.check, "Verify");
  btn.onclick = () =>
    api(`/api/queue/${item.item_id}/verify`, { method: "POST" })
      .then(async (res) => {
        const data = await res.json();
        showToast(data.message || "Verify complete.");
      })
      .catch((e) => notifyError(e.message || "Verify failed."));
  group.appendChild(btn);
}

function appendRefetchButton(group, item) {
  const slug = statusSlug(item.status);
  if (slug !== "idle" || !item.error) return;
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "secondary";
  setButtonLabel(btn, ICON.refresh, "Refetch");
  btn.onclick = () => refetchQueueItem(item.item_id).catch((e) => alert(e.message || String(e)));
  group.appendChild(btn);
}

function appendCancelMenuButton(group, item) {
  if (!canCancel(item)) return;
  const menu = document.createElement("details");
  menu.className = "btn-menu";
  const trigger = document.createElement("summary");
  trigger.className = "btn-menu-trigger warning";
  setButtonLabel(trigger, ICON.stop, "Cancel…");
  menu.appendChild(trigger);
  const panel = document.createElement("div");
  panel.className = "btn-menu-panel";
  const readyBtn = document.createElement("button");
  readyBtn.type = "button";
  readyBtn.className = "btn-menu-item";
  setButtonLabel(readyBtn, ICON.replay, "Cancel → Ready");
  readyBtn.onclick = (e) => {
    e.preventDefault();
    menu.open = false;
    cancelItemWithAction(item.item_id, "ready").catch((err) => alert(err.message || String(err)));
  };
  panel.appendChild(readyBtn);
  const removeBtn = document.createElement("button");
  removeBtn.type = "button";
  removeBtn.className = "btn-menu-item danger";
  setButtonLabel(removeBtn, ICON.remove, "Cancel → Remove");
  removeBtn.onclick = (e) => {
    e.preventDefault();
    menu.open = false;
    cancelItemWithAction(item.item_id, "remove").catch((err) => alert(err.message || String(err)));
  };
  panel.appendChild(removeBtn);
  menu.appendChild(panel);
  group.appendChild(menu);
}

function appendSelectionCheckbox(card, item, selectedSet, updateUiFn) {
  const ids = selectedSet || selectedQueueIds;
  const updateUi = updateUiFn || updateBulkSelectionUi;
  const wrap = document.createElement("label");
  wrap.className = "card-select";
  const cb = document.createElement("input");
  cb.type = "checkbox";
  cb.checked = ids.has(item.item_id);
  cb.onchange = () => {
    if (cb.checked) ids.add(item.item_id);
    else ids.delete(item.item_id);
    updateUi();
  };
  wrap.appendChild(cb);
  card.insertBefore(wrap, card.firstChild);
}

function appendPlayButton(actions, item, thumb) {
  if (!item.playable) return;
  const play = document.createElement("button");
  play.type = "button";
  play.className = "primary";
  setButtonLabel(play, ICON.playCircle, "Play");
  play.onclick = () => toggleCardMedia(item, thumb);
  actions.appendChild(play);
}

function thumbnailApiUrl(itemId) {
  if (!apiAuthOptional()) return null;
  return apiUrlWithAuth(`/api/thumbnail/${itemId}`);
}

function revealThumbImage(img, placeholder, cacheKey) {
  img.classList.remove("hidden");
  placeholder.classList.add("hidden");
  thumbRetryCounts.delete(cacheKey);
}

function applyThumbBlobToImg(img, placeholder, cacheKey, objUrl, item, showThumbnails) {
  img.onload = () => {
    if (!img.isConnected) return;
    revealThumbImage(img, placeholder, cacheKey);
  };
  img.onerror = () => {
    if (!img.isConnected) return;
    revokeThumbBlob(cacheKey);
    img.classList.add("hidden");
    img.removeAttribute("src");
    noteThumbFailure(cacheKey);
    if (item && !thumbFailuresExhausted(cacheKey)) {
      placeholder.textContent = "Fetching thumbnail…";
      placeholder.classList.remove("hidden");
      fetchQueueThumbnailBlob(item, { force: true }).then((result) => {
        if (!img.isConnected) return;
        if (result.url) {
          applyThumbBlobToImg(img, placeholder, cacheKey, result.url, item, showThumbnails);
        } else {
          placeholder.textContent = thumbFailurePlaceholder(result.reason, cacheKey);
          placeholder.classList.remove("hidden");
        }
      });
      return;
    }
    placeholder.textContent = "Thumbnail unavailable";
    placeholder.classList.remove("hidden");
  };
  img.src = objUrl;
  if (img.complete && img.naturalWidth > 0) {
    revealThumbImage(img, placeholder, cacheKey);
  }
}

function attachCardThumbnail(img, placeholder, item, showThumbnails) {
  img.classList.add("hidden");
  placeholder.classList.remove("hidden");
  placeholder.textContent = thumbPlaceholderText(item, showThumbnails);

  if (!showThumbnails || statusSlug(item.status) === "resolving" || !itemHasThumbnailSource(item)) {
    return;
  }
  const cacheKey = thumbCacheKey(item);
  if (!thumbnailApiUrl(item.item_id)) {
    placeholder.textContent = thumbFailurePlaceholder("no_token", cacheKey);
    return;
  }
  if (thumbFailuresExhausted(cacheKey)) {
    placeholder.textContent = "Thumbnail unavailable";
    return;
  }

  const cached = thumbBlobCache.get(cacheKey);
  if (cached) {
    applyThumbBlobToImg(img, placeholder, cacheKey, cached, item, showThumbnails);
    return;
  }

  placeholder.textContent = "Fetching thumbnail…";
  fetchQueueThumbnailBlob(item).then((result) => {
    if (!img.isConnected) return;
    if (result.url) {
      applyThumbBlobToImg(img, placeholder, cacheKey, result.url, item, showThumbnails);
    } else {
      placeholder.textContent = thumbFailurePlaceholder(result.reason, cacheKey);
      placeholder.classList.remove("hidden");
      img.classList.add("hidden");
    }
  });
}

function footerStatusText(item) {
  const slug = statusSlug(item.status);
  if (slug === "resolving") return "Fetching metadata…";
  if (slug === "idle" || slug === "queued") {
    const parts = [`${item.percent.toFixed(1)}%`];
    if (item.size_text && item.size_text !== "-") parts.push(item.size_text);
    if (item.speed_text && item.speed_text !== "-") parts.push(item.speed_text);
    if (item.eta_text && item.eta_text !== "-") parts.push(item.eta_text);
    return parts.join(" · ");
  }
  if (slug === "done") {
    return `${item.percent.toFixed(1)}% · ${item.size_text || "-"} · ${item.speed_text || "-"} · ${item.eta_text || "-"}`;
  }
  return `${item.percent.toFixed(1)}% · ${item.size_text || "-"} · ${item.speed_text || "-"} · ${item.eta_text || "-"}`;
}

function progressPercent(item) {
  const slug = statusSlug(item.status);
  if (slug === "resolving") return 0;
  if (slug === "done") return 100;
  return Math.min(100, Math.max(0, Number(item.percent) || 0));
}

function progressLabel(item) {
  const slug = statusSlug(item.status);
  if (slug === "resolving") return "Fetching metadata…";
  if (slug === "done") return `${item.percent.toFixed(0)}%`;
  if (slug === "downloading" || slug === "queued") {
    return `${item.percent.toFixed(0)}%`;
  }
  return "";
}

function canCancel(item) {
  const slug = statusSlug(item.status);
  return slug === "queued" || slug === "downloading";
}

function canRedownload(item) {
  if (!item.can_redownload || !cachedHasYtDlp) return false;
  const slug = statusSlug(item.status);
  return slug === "done" || slug === "failed";
}

function appendRemoveMenuButton(group, item) {
  const menu = document.createElement("details");
  menu.className = "btn-menu";

  const trigger = document.createElement("summary");
  trigger.className = "btn-menu-trigger danger";
  trigger.title = "Remove from queue or delete the saved file";
  setButtonLabel(trigger, ICON.remove, "Remove...");
  menu.appendChild(trigger);

  const panel = document.createElement("div");
  panel.className = "btn-menu-panel";
  panel.setAttribute("role", "menu");

  const removeBtn = document.createElement("button");
  removeBtn.type = "button";
  removeBtn.className = "btn-menu-item";
  setButtonLabel(removeBtn, ICON.removeCircleOutline, "Remove from queue");
  removeBtn.title =
    "Remove this row from the queue (does not delete the file on disk).";
  removeBtn.onclick = (e) => {
    e.preventDefault();
    menu.open = false;
    removeQueueItem(item.item_id).catch((err) =>
      alert(err.message || String(err))
    );
  };
  panel.appendChild(removeBtn);

  if (item.can_delete_file) {
    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "btn-menu-item danger";
    setButtonLabel(deleteBtn, ICON.deleteForever, "Delete file");
    deleteBtn.title =
      "Delete the downloaded file on disk. The queue row stays until you remove it.";
    deleteBtn.onclick = (e) => {
      e.preventDefault();
      menu.open = false;
      const name = item.media_filename || "this file";
      showConfirmDialog(`Delete ${name} from the output folder?`, "Delete file").then((ok) => {
        if (!ok) return;
        deleteQueueItemFile(item.item_id).catch((err) =>
          notifyError(err.message || String(err))
        );
      });
    };
    panel.appendChild(deleteBtn);
  }

  menu.appendChild(panel);
  group.appendChild(menu);
}

function appendRedownloadButton(actions, item) {
  if (!canRedownload(item)) return;
  const slug = statusSlug(item.status);
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "secondary";
  const label = slug === "failed" ? "Retry" : "Redo";
  setButtonLabel(btn, ICON.refresh, label);
  btn.title =
    "Deletes the matched file in the output folder (if found), then downloads this URL again with current quality settings.";
  btn.onclick = () =>
    redownloadItem(item.item_id).catch((e) => alert(e.message || String(e)));
  actions.appendChild(btn);
}

function renderQueueCard(item, settings, ctx) {
  const s = settings || {};
  const showThumbnails = s.show_thumbnails !== false;
  const compact = !!s.compact_cards;
  const hideSubtitle = !!s.hide_card_subtitle;
  const slug = statusSlug(item.status);
  const highlightDone = slug === "done" && !item.error;
  const readyItems = (ctx && ctx.readyItems) || [];

  const card = document.createElement("article");
  card.className = "card" + (compact ? " compact" : "") + (highlightDone ? " card-done-highlight" : "");
  card.dataset.itemId = String(item.item_id);
  appendSelectionCheckbox(card, item);

  const thumb = document.createElement("div");
  thumb.className = "card-thumb";
  const placeholder = document.createElement("span");
  placeholder.className = "card-thumb-placeholder";
  placeholder.textContent = thumbPlaceholderText(item, showThumbnails);

  const img = document.createElement("img");
  img.alt = "";
  img.className = "hidden";
  thumb.appendChild(img);
  attachCardThumbnail(img, placeholder, item, showThumbnails);
  thumb.appendChild(placeholder);
  card.appendChild(thumb);

  const body = document.createElement("div");
  body.className = "card-body";

  const title = document.createElement("h3");
  title.className = "card-title";
  title.textContent = item.title || item.source_line || "(no title)";
  body.appendChild(title);

  const subtitleText = formatSubtitle(item, hideSubtitle);
  const subtitle = document.createElement("p");
  subtitle.className = "card-subtitle" + (subtitleText ? "" : " hidden");
  subtitle.textContent = subtitleText || "";
  body.appendChild(subtitle);

  const progressWrap = document.createElement("div");
  progressWrap.className = "card-progress";
  const progressFill = document.createElement("div");
  progressFill.className = `card-progress-fill status-${slug}`;
  progressFill.style.width = `${progressPercent(item)}%`;
  progressWrap.appendChild(progressFill);
  body.appendChild(progressWrap);

  const progressLabelEl = document.createElement("div");
  progressLabelEl.className = "card-progress-label";
  progressLabelEl.textContent = progressLabel(item);
  body.appendChild(progressLabelEl);

  const detail = (item.detail || "").trim();
  const detailEl = document.createElement("p");
  detailEl.className = "card-detail" + (detail && slug !== "resolving" ? "" : " hidden");
  detailEl.textContent = detail;
  body.appendChild(detailEl);

  if (item.error) {
    const errEl = document.createElement("p");
    errEl.className = "card-error";
    errEl.textContent = item.error;
    body.appendChild(errEl);
  }

  const badges = document.createElement("div");
  badges.className = "card-badges";
  const res = formatResolution(item.width, item.height);
  if (res) {
    const resBadge = document.createElement("span");
    resBadge.className = "meta-badge";
    resBadge.textContent = res;
    badges.appendChild(resBadge);
  }
  if (
    (slug === "idle" || slug === "queued") &&
    item.size_text &&
    item.size_text !== "-"
  ) {
    const sizeBadge = document.createElement("span");
    sizeBadge.className = "meta-badge";
    sizeBadge.textContent = item.size_text.startsWith("~")
      ? item.size_text
      : `~${item.size_text}`;
    badges.appendChild(sizeBadge);
  }
  const chip = document.createElement("span");
  setStatusChip(chip, slug, item.status || "Idle");
  badges.appendChild(chip);
  body.appendChild(badges);

  const footer = document.createElement("p");
  footer.className = `card-footer status-${slug}`;
  footer.textContent = footerStatusText(item);
  body.appendChild(footer);

  card.appendChild(body);

  const { bar: actions, group } = createCardActionBar();
  appendPlayButton(group, item, thumb);
  appendUrlMenuButton(group, item);
  appendRefetchButton(group, item);
  if (slug === "idle" && !item.error) {
    appendReadyReorderButtons(group, item, readyItems);
    if (readyItems.length > 1) {
      attachReadyRowDragDrop(card, item, readyItems, reorderQueueItem);
    }
  }
  appendCancelMenuButton(group, item);
  appendRedownloadButton(group, item);
  appendVerifyStreamsButton(group, item);
  appendRemoveMenuButton(group, item);
  if (group.childElementCount > 0) {
    card.appendChild(actions);
  }

  return card;
}

function renderQueueCardListRow(item, settings, ctx) {
  const s = settings || {};
  const showThumbnails = s.show_thumbnails !== false;
  const slug = statusSlug(item.status);
  const readyItems = (ctx && ctx.readyItems) || [];
  const card = document.createElement("article");
  card.className = "card";
  card.dataset.itemId = String(item.item_id);
  appendSelectionCheckbox(card, item);

  const thumb = document.createElement("div");
  thumb.className = "card-thumb";
  const placeholder = document.createElement("span");
  placeholder.className = "card-thumb-placeholder";
  placeholder.textContent = "…";
  const img = document.createElement("img");
  img.alt = "";
  img.className = "hidden";
  thumb.appendChild(img);
  attachCardThumbnail(img, placeholder, item, showThumbnails);
  thumb.appendChild(placeholder);
  card.appendChild(thumb);

  const body = document.createElement("div");
  body.className = "card-body";
  const chip = document.createElement("span");
  setStatusChip(chip, slug, item.status);
  const title = document.createElement("span");
  title.className = "card-title";
  title.style.display = "inline";
  title.textContent = " " + (item.title || item.source_line);
  body.appendChild(chip);
  body.appendChild(title);
  if (s.card_list_layout) {
    const fmt = document.createElement("input");
    fmt.type = "text";
    fmt.className = "format-override-input";
    fmt.placeholder = "yt-dlp -f override (optional)";
    fmt.value = item.format_override || "";
    fmt.title = "Per-item format override";
    fmt.addEventListener("change", () => {
      setItemOverrides(item.item_id, fmt.value.trim() || null, item.profile_override || null).catch(
        (e) => notifyError(String(e))
      );
    });
    body.appendChild(fmt);
  }
  if (slug === "downloading" || slug === "queued") {
    const pct = document.createElement("span");
    pct.className = "card-footer";
    pct.textContent = ` ${item.percent.toFixed(0)}%`;
    body.appendChild(pct);
  }
  card.appendChild(body);

  const { bar: actions, group } = createCardActionBar();
  appendPlayButton(group, item, thumb);
  appendUrlMenuButton(group, item);
  appendRefetchButton(group, item);
  if (slug === "idle" && !item.error) {
    appendReadyReorderButtons(group, item, readyItems);
    if (readyItems.length > 1) {
      attachReadyRowDragDrop(card, item, readyItems, reorderQueueItem);
    }
  }
  appendCancelMenuButton(group, item);
  appendRedownloadButton(group, item);
  appendVerifyStreamsButton(group, item);
  appendRemoveMenuButton(group, item);
  if (group.childElementCount > 0) {
    card.appendChild(actions);
  }

  return card;
}

function loadQueueGroupCollapsed() {
  try {
    const raw = localStorage.getItem(QUEUE_GROUP_COLLAPSED_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function saveQueueGroupCollapsed(state) {
  try {
    localStorage.setItem(QUEUE_GROUP_COLLAPSED_KEY, JSON.stringify(state));
  } catch {
    /* ignore */
  }
}

function queueGroupStorageKey(mode, label) {
  return `${mode}:${label}`;
}

function queueGroupSlug(label) {
  switch (label) {
    case "Active":
      return "downloading";
    case "Ready":
      return "idle";
    case "Issues":
    case "Failed":
      return "failed";
    case "Done":
      return "done";
    case "Resolving":
      return "resolving";
    case "Skipped":
      return "skipped";
    default:
      return "idle";
  }
}

function downloadQueueGroup(item) {
  switch (item.status) {
    case "Queued":
    case "Downloading":
      return "Active";
    case "Failed":
      return "Issues";
    case "Idle":
      return item.error ? "Issues" : "Ready";
    case "Done":
      return "Done";
    case "Resolving":
      return "Resolving";
    default:
      return "Ready";
  }
}

function downloadQueueGroupDefaultOpen(label, ctx) {
  if (ctx.statusFilter) return true;
  const s = ctx.status || {};
  if (label === "Done") {
    if (ctx.totalItems > 30) return false;
    return (
      (s.done || 0) > 0 &&
      (s.active || 0) === 0 &&
      (s.queued || 0) === 0 &&
      (s.ready || 0) === 0 &&
      (s.resolving || 0) === 0
    );
  }
  if (label === "Ready") return ctx.totalItems <= 12;
  if (label === "Issues") return true;
  if (label === "Active" || label === "Resolving") return !ctx.searchQuery;
  return true;
}

function convertQueueGroupDefaultOpen(label) {
  if (label === "Done" || label === "Ready") return false;
  if (label === "Failed" || label === "Skipped") return true;
  return true;
}

function sortDownloadGroupItems(label, items) {
  if (label === "Ready") {
    return [...items].sort(
      (a, b) => (a.sort_order || a.item_id || 0) - (b.sort_order || b.item_id || 0)
    );
  }
  if (label === "Done") {
    return [...items].sort(
      (a, b) => (b.completed_at || 0) - (a.completed_at || 0)
    );
  }
  return items;
}

function appendCollapsibleQueueGroup(root, label, items, options) {
  const { settings, renderItem, defaultOpen, collapsedState, listLayout, mode, groupExtras } =
    options;
  const details = document.createElement("details");
  details.className = "queue-group";
  details.dataset.group = label;

  const storageKey = queueGroupStorageKey(mode, label);
  const persisted = collapsedState[storageKey];
  details.open = persisted === undefined ? defaultOpen : !persisted;

  const summary = document.createElement("summary");
  summary.className = `queue-group-summary status-${queueGroupSlug(label)}`;
  summary.innerHTML = `<span class="status-dot" aria-hidden="true"></span><span class="queue-group-label">${escapeHtml(
    label
  )} (${items.length})</span>`;

  if (label === "Done" && groupExtras) {
    const tools = document.createElement("span");
    tools.className = "queue-group-tools";
    const histLabel = document.createElement("label");
    histLabel.className = "queue-group-history";
    histLabel.textContent = "History: ";
    const histSel = document.createElement("select");
    histSel.title = "Filter Done items by completion time";
    for (const [val, text] of [
      ["all", "All time"],
      ["1", "Last 24h"],
      ["7", "Last 7 days"],
      ["30", "Last 30 days"],
    ]) {
      const opt = document.createElement("option");
      opt.value = val;
      opt.textContent = text;
      if (
        (val === "all" && doneHistoryFilterDays == null) ||
        (val !== "all" && doneHistoryFilterDays === parseInt(val, 10))
      ) {
        opt.selected = true;
      }
      histSel.appendChild(opt);
    }
    histSel.onchange = (e) => {
      e.stopPropagation();
      const v = histSel.value;
      saveDoneHistoryFilter(v === "all" ? null : parseInt(v, 10));
      refreshQueue(true);
    };
    histSel.onclick = (e) => e.stopPropagation();
    histLabel.appendChild(histSel);
    tools.appendChild(histLabel);
    const requeueBtn = document.createElement("button");
    requeueBtn.type = "button";
    requeueBtn.className = "secondary queue-group-action";
    setButtonLabel(requeueBtn, ICON.replay, "Re-queue visible");
    requeueBtn.onclick = (e) => {
      e.preventDefault();
      e.stopPropagation();
      const ids = items.map((it) => it.item_id);
      requeueDoneItems(ids).then((n) => {
        if (n > 0) appendLogLine(`Re-queued ${n} done item(s) from web UI.`);
      }).catch((err) => alert(err.message || String(err)));
    };
    tools.appendChild(requeueBtn);
    summary.appendChild(tools);
  }

  details.appendChild(summary);

  const body = document.createElement("div");
  body.className = "queue-group-body" + (listLayout ? " list-layout" : "");
  for (const item of items) {
    body.appendChild(renderItem(item, settings));
  }
  details.appendChild(body);

  details.addEventListener("toggle", () => {
    if (details.open) {
      delete collapsedState[storageKey];
    } else {
      collapsedState[storageKey] = true;
    }
    saveQueueGroupCollapsed(collapsedState);
  });

  root.appendChild(details);
}

function renderGroupedQueue(root, items, options) {
  const {
    settings,
    groupFn,
    groupOrder,
    renderItem,
    defaultOpenCtx,
    mode,
    defaultOpenFn,
    sortGroupItems,
    groupExtras,
  } = options;
  const collapsedState = loadQueueGroupCollapsed();
  const outerH = options.outerScrollPx ?? 0;
  const listLayout = effectiveListLayout(
    settings,
    items.length,
    outerH,
    mode === "cv",
  );
  const buckets = new Map();
  for (const item of items) {
    const label = groupFn(item);
    if (!buckets.has(label)) buckets.set(label, []);
    buckets.get(label).push(item);
  }
  for (const label of groupOrder) {
    const group = buckets.get(label);
    if (!group?.length) continue;
    const sorted = sortGroupItems ? sortGroupItems(label, group) : group;
    const defaultOpen = defaultOpenFn(label, {
      ...defaultOpenCtx,
      totalItems: items.length,
    });
    appendCollapsibleQueueGroup(root, label, sorted, {
      settings,
      renderItem,
      defaultOpen,
      collapsedState,
      listLayout,
      mode,
      groupExtras,
    });
  }
}

function findQueueCard(itemId) {
  return document.querySelector(`#queue [data-item-id="${itemId}"]`);
}

function patchQueueCardProgress(item) {
  const card = findQueueCard(item.item_id);
  if (!card) return false;
  const slug = statusSlug(item.status);
  const fill = card.querySelector(".card-progress-fill");
  if (fill) {
    fill.style.width = `${Math.min(100, Math.max(0, Number(item.percent) || 0))}%`;
    fill.className = `card-progress-fill status-${slug}`;
  }
  patchQueueCardDownloadLine(item.item_id, item.detail || item.speed_text || "");
  const chip = card.querySelector(".status-chip");
  if (chip) setStatusChip(chip, slug, item.status || "");
  return true;
}

function tryPatchDownloaderQueue(items) {
  if (!items?.length) return false;
  let patched = 0;
  for (const item of items) {
    if (patchQueueCardProgress(item)) patched += 1;
  }
  return patched === items.length;
}

function patchQueueCardDownloadLine(itemId, line) {
  const card = findQueueCard(itemId);
  if (!card) return;
  const detail = card.querySelector(".card-detail");
  if (detail && line) {
    detail.textContent = line;
    detail.classList.remove("hidden");
  }
  const footer = card.querySelector(".card-footer");
  if (footer && line) {
    footer.textContent = line;
  }
}

function appendLogLine(line) {
  if (!line) return;
  logLinesCache.push(line);
  renderLogView();
}

async function refreshLogs() {
  const res = await api("/api/logs");
  const data = await res.json();
  logLinesCache = data.lines || [];
  renderLogView();
}

async function refreshQueue(force = false) {
  const res = await api("/api/queue");
  const data = await res.json();
  if (!force && typeof data.generation === "number" && data.generation === lastQueueGeneration) {
    return;
  }
  if (typeof data.generation === "number") {
    lastQueueGeneration = data.generation;
  }
  const root = document.getElementById("queue");
  const settings = cachedSettings || {};
  pruneThumbFailedKeys(data.items);
  stopActiveMedia();
  const allItems = data.items || [];
  const structureKey = allItems.map((it) => it.item_id).join(",");
  const searchInput = document.getElementById("queue-search");
  const searchQuery = searchInput ? searchInput.value : settings.queue_search || "";
  const items = allItems.filter(
    (item) =>
      itemMatchesSearch(item, searchQuery) &&
      itemMatchesStatusFilter(item, queueStatusFilter) &&
      itemMatchesDoneHistory(item)
  );
  if (
    !force &&
    structureKey === lastQueueStructureKey &&
    items.length > 0 &&
    tryPatchDownloaderQueue(items)
  ) {
    return;
  }
  lastQueueStructureKey = structureKey;
  const outerH = queueOuterScrollPx("queue");
  const listLayout = effectiveListLayout(settings, allItems.length, outerH, false);
  root.className = "queue" + (listLayout ? " list-layout" : "");
  root.innerHTML = "";
  const readyItems = (data.items || [])
    .filter((item) => downloadQueueGroup(item) === "Ready" && itemMatchesSearch(item, searchQuery))
    .sort((a, b) => (a.sort_order || a.item_id || 0) - (b.sort_order || b.item_id || 0));
  const cardCtx = { readyItems };
  if (!items.length) {
    const empty = document.createElement("p");
    empty.className = "hint";
    empty.textContent = searchQuery || queueStatusFilter
      ? "No queue items match the current search or filter."
      : "Queue is empty. Add URLs above.";
    root.appendChild(empty);
    return;
  }
  renderGroupedQueue(root, items, {
    settings: {
      ...settings,
      card_list_layout: listLayout,
    },
    groupFn: downloadQueueGroup,
    groupOrder: DOWNLOAD_QUEUE_GROUPS,
    outerScrollPx: outerH,
    renderItem: (item, s) =>
      s.card_list_layout
        ? renderQueueCardListRow(item, s, cardCtx)
        : renderQueueCard(item, s, cardCtx),
    defaultOpenCtx: {
      status: lastStatusPayload?.status,
      searchQuery,
      statusFilter: queueStatusFilter,
    },
    mode: "dl",
    defaultOpenFn: downloadQueueGroupDefaultOpen,
    sortGroupItems: sortDownloadGroupItems,
    groupExtras: true,
  });
  updateBulkSelectionUi();
}

async function cancelItem(id) {
  await api(`/api/downloads/cancel/${id}`, { method: "POST" });
  await refreshAll();
}

async function readApiError(res, fallback) {
  try {
    const body = await res.clone().json();
    if (body && typeof body.error === "string" && body.error.trim()) {
      return body.error.trim();
    }
  } catch {
    /* ignore */
  }
  try {
    const text = (await res.text()).trim();
    if (text) return text;
  } catch {
    /* ignore */
  }
  if (res.status === 503) {
    return "Web UI is unavailable (API token not configured in rustdl Settings).";
  }
  return fallback;
}

/** POST to an action endpoint; shows an error toast and throws when the response is not OK. */
async function postAction(path, fallback, options = {}) {
  const res = await api(path, { method: "POST", ...options });
  if (!res.ok) {
    const msg = await readApiError(res, fallback);
    showToast(msg, "error");
    throw new Error(msg);
  }
  return res;
}

async function redownloadItem(id) {
  const res = await api(`/api/downloads/redownload/${id}`, { method: "POST" });
  if (!res.ok) {
    const fallback =
      "Re-download could not start (missing URL, invalid output folder, or yt-dlp unavailable).";
    throw new Error(await readApiError(res, fallback));
  }
  await refreshAll();
}

async function removeQueueItem(id) {
  const res = await api(`/api/queue/${id}`, { method: "DELETE" });
  if (!res.ok) {
    const fallback =
      res.status === 404
        ? "Item not found in the queue."
        : `Could not remove this item (HTTP ${res.status}).`;
    throw new Error(await readApiError(res, fallback));
  }
  await refreshAll();
}

async function deleteQueueItemFile(id) {
  const res = await api(`/api/queue/${id}/file`, { method: "DELETE" });
  if (!res.ok && res.status !== 204) {
    throw new Error("Could not delete the file (not found on disk or permission denied).");
  }
  await refreshAll();
}

async function clearQueue(filter, confirmMessage) {
  if (confirmMessage && !(await showConfirmDialog(confirmMessage))) return;
  const res = await api("/api/queue/clear", {
    method: "POST",
    body: JSON.stringify({ filter }),
  });
  if (!res.ok) {
    throw new Error("Queue clear failed.");
  }
  const data = await res.json();
  if (data.removed === 0) {
    alert("Nothing to remove for that filter.");
  }
  await refreshAll();
}

const QUEUE_CLEAR_ACTIONS = [
  {
    filter: "finished",
    icon: ICON.clear,
    label: "Clear finished",
    title: "Remove done and failed rows",
    confirm: "Remove all done and failed items from the queue?",
  },
  {
    filter: "done",
    icon: ICON.delete,
    label: "Clear done",
    title: "Remove done rows only",
    confirm: "Remove all done items from the queue?",
  },
  {
    filter: "failed",
    icon: ICON.delete,
    label: "Clear failed",
    title: "Remove failed rows only",
    confirm: "Remove all failed items from the queue?",
  },
  {
    filter: "inactive",
    icon: ICON.removeCircleOutline,
    label: "Clear inactive",
    title: "Remove all rows except queued or downloading",
    confirm:
      "Remove all items except those queued or downloading? Active downloads are not cancelled.",
  },
  {
    filter: "all",
    icon: ICON.deleteForever,
    label: "Clear all",
    title: "Remove every row (cancels active downloads)",
    confirm:
      "Remove every item from the queue? Downloads in progress will be cancelled.",
    danger: true,
  },
];

function mountClearQueueMenu(container) {
  const menu = document.createElement("details");
  menu.className = "btn-menu queue-clear-menu";

  const trigger = document.createElement("summary");
  trigger.className = "btn-menu-trigger secondary";
  trigger.title = "Remove queue rows by status";
  setButtonLabel(trigger, ICON.clear, "Clear...");
  menu.appendChild(trigger);

  const panel = document.createElement("div");
  panel.className = "btn-menu-panel btn-menu-panel-down";
  panel.setAttribute("role", "menu");

  for (const action of QUEUE_CLEAR_ACTIONS) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "btn-menu-item" + (action.danger ? " danger" : "");
    setButtonLabel(btn, action.icon, action.label);
    btn.title = action.title;
    btn.onclick = (e) => {
      e.preventDefault();
      menu.open = false;
      clearQueue(action.filter, action.confirm).catch((err) =>
        alert(err.message || String(err))
      );
    };
    panel.appendChild(btn);
  }

  menu.appendChild(panel);
  container.appendChild(menu);
}

async function exportQueueUrls() {
  const res = await api("/api/queue/export");
  if (!res.ok) {
    throw new Error("Could not export queue URLs.");
  }
  const text = await res.text();
  const blob = new Blob([text], { type: "text/plain;charset=utf-8" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "rustdl-queue.txt";
  a.click();
  URL.revokeObjectURL(a.href);
}

async function importQueueUrlsFromPrompt() {
  const dlg = document.getElementById("import-urls-dialog");
  const input = document.getElementById("import-urls-input");
  if (!dlg || !input) return;
  input.value = "";
  dlg.showModal();
}

async function submitImportUrlsFromDialog() {
  const input = document.getElementById("import-urls-input");
  const raw = input?.value || "";
  if (!raw.trim()) return;
  const urls = raw
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"));
  if (!urls.length) {
    notifyError("No URLs to import.");
    return;
  }
  await api("/api/queue/import", {
    method: "POST",
    body: JSON.stringify({ urls }),
  });
  await refreshAll();
  showToast(`Imported ${urls.length} URL(s).`);
}

function mountQueueImportExport(container) {
  const exportBtn = document.createElement("button");
  exportBtn.type = "button";
  exportBtn.className = "secondary";
  setButtonLabel(exportBtn, ICON.download, "Export URLs");
  exportBtn.title = "Download queue URLs as a .txt file";
  exportBtn.onclick = () =>
    exportQueueUrls().catch((err) => alert(err.message || String(err)));

  const importBtn = document.createElement("button");
  importBtn.type = "button";
  importBtn.className = "secondary";
  setButtonLabel(importBtn, ICON.add, "Import URLs");
  importBtn.title = "Paste URLs to append to the queue";
  importBtn.onclick = () =>
    importQueueUrlsFromPrompt().catch((err) => alert(err.message || String(err)));

  container.appendChild(exportBtn);
  container.appendChild(importBtn);
}

async function clearActivityLog() {
  await api("/api/logs/clear", { method: "POST" });
  await refreshLogs();
}

function clearUrlInput() {
  document.getElementById("url-input").value = "";
}

async function refreshSettingsCache() {
  try {
    const res = await api("/api/settings");
    const data = await res.json();
    cachedSettings = data.settings;
    applyModeColors(cachedSettings);
  } catch {
    /* settings optional until connected */
  }
}

async function refreshAll() {
  await Promise.all([
    refreshStatus(),
    refreshSettingsCache(),
    refreshQueue(),
    refreshLogs(),
    refreshConvert(),
  ]);
}

function scheduleRefreshAll(delayMs = 400) {
  if (refreshAllTimer) clearTimeout(refreshAllTimer);
  refreshAllTimer = setTimeout(() => {
    refreshAllTimer = null;
    refreshAll().catch(() => {});
  }, delayMs);
}

function handleSseEvent(data) {
  if (!data || typeof data !== "object") {
    scheduleRefreshAll();
    return;
  }
  switch (data.type) {
    case "shutdown":
      showShutdownNotice("rustdl has shut down. You can close this tab.");
      return;
    case "download_line":
      if (data.item_id != null) {
        patchQueueCardDownloadLine(data.item_id, data.line || "");
        scheduleStatusRefresh();
      }
      return;
    case "log":
      appendLogLine(data.line || "");
      return;
    case "download_done":
      refreshStatus().catch(() => {});
      refreshQueue(true).catch(() => {});
      return;
    case "add_progress":
    case "add_resolved":
    case "add_done":
      refreshStatus().catch(() => {});
      refreshQueue(true).catch(() => {});
      return;
    case "convert_line":
      scheduleRefreshAll(800);
      return;
    case "convert_done":
    case "convert_batch_done":
      refreshConvert()
        .then(() => {
          if (data.type === "convert_batch_done") {
            notifyConvertBatchComplete();
          }
        })
        .catch(() => {});
      refreshStatus().catch(() => {});
      return;
    case "download_session_complete":
      showSessionNotification(
        `Downloads finished: ${data.done ?? 0} done, ${data.failed ?? 0} failed`,
      );
      refreshStatus().catch(() => {});
      refreshQueue(true).catch(() => {});
      return;
    case "convert_duration":
    case "convert_media_probed":
      refreshConvert().catch(() => {});
      refreshStatus().catch(() => {});
      return;
    default:
      scheduleRefreshAll();
  }
}

function connectSse() {
  const t = token();
  const url = t ? `/api/events?token=${encodeURIComponent(t)}` : "/api/events";
  const es = new EventSource(url);
  es.onopen = () => {
    sseConnected = true;
    if (refreshIntervalId != null) {
      clearInterval(refreshIntervalId);
      refreshIntervalId = null;
    }
  };
  es.onmessage = (ev) => {
    try {
      const data = JSON.parse(ev.data);
      if (data?.type === "shutdown") {
        showShutdownNotice("rustdl has shut down. You can close this tab.");
        es.close();
        sseConnected = false;
        startFallbackPolling();
        return;
      }
      handleSseEvent(data);
    } catch {
      scheduleRefreshAll();
    }
  };
  es.onerror = () => {
    es.close();
    sseConnected = false;
    startFallbackPolling();
    if (shuttingDown) {
      showShutdownNotice("rustdl has shut down. You can close this tab.");
      return;
    }
    setTimeout(connectSse, 3000);
  };
}

function startFallbackPolling() {
  if (refreshIntervalId != null || shuttingDown) return;
  refreshIntervalId = setInterval(() => {
    if (shuttingDown) return;
    scheduleRefreshAll(0);
  }, 5000);
}

function setCheck(id, v) {
  const el = document.getElementById(id);
  if (el) el.checked = !!v;
}

function setVal(id, v) {
  const el = document.getElementById(id);
  if (el) el.value = v ?? "";
}

function updateQualityCustomVisibility() {
  const sel = document.getElementById("set-quality");
  const wrap = document.getElementById("wrap-quality-custom");
  if (!sel || !wrap) return;
  wrap.classList.toggle("hidden", sel.value !== "custom");
}

function populateProfiles(profilesResp) {
  const sel = document.getElementById("set-active-profile");
  if (!sel) return;
  sel.innerHTML = "";
  for (const name of profilesResp.profiles || []) {
    const opt = document.createElement("option");
    opt.value = name;
    opt.textContent = name;
    if (name === profilesResp.active) opt.selected = true;
    sel.appendChild(opt);
  }
}

function updateConvertSizeLimitFieldsVisibility() {
  const kind = document.getElementById("set-convert-size-limit-kind")?.value || "none";
  const on = kind !== "none";
  for (const id of [
    "set-convert-size-limit-value-wrap",
    "set-convert-size-limit-violation-wrap",
  ]) {
    const el = document.getElementById(id);
    if (el) el.hidden = !on;
  }
  const valueEl = document.getElementById("set-convert-size-limit-value");
  if (valueEl) {
    valueEl.placeholder =
      kind === "max_output_bytes" ? "e.g. 500M, 1.5GiB" : "e.g. 50";
  }
}

function populateSettingsForm(s, commandPreview) {
  setCheck("set-show-thumbnails", s.show_thumbnails);
  setCheck("set-compact-cards", s.compact_cards);
  setCheck("set-hide-subtitle", s.hide_card_subtitle);
  setCheck("set-card-list", s.card_list_layout);
  setCheck("set-autoscroll-log", s.autoscroll_log);
  setCheck("set-log-relative", s.log_relative_time);
  setVal("set-log-max", s.log_max_chars);
  const logFilterEl = document.getElementById("log-filter");
  if (logFilterEl) logFilterEl.value = s.log_filter || "all";
  syncModeColorControls(
    "set-mode-downloader-color",
    "set-mode-downloader-hex",
    s.mode_downloader_color,
    DEFAULT_MODE_DOWNLOADER,
  );
  syncModeColorControls(
    "set-mode-convert-color",
    "set-mode-convert-hex",
    s.mode_convert_color,
    DEFAULT_MODE_CONVERT,
  );
  setVal("set-subprocess-priority", s.subprocess_priority || "normal");
  setVal("set-ffmpeg-path", s.ffmpeg_path);
  setVal("set-ffprobe-path", s.ffprobe_path);
  const qs = document.getElementById("queue-search");
  if (qs && document.activeElement !== qs) {
    qs.value = s.queue_search || "";
  }
  const cs = document.getElementById("convert-search");
  if (cs && document.activeElement !== cs) {
    cs.value = s.queue_search || "";
  }

  setCheck("set-auto-add", s.auto_add_pasted_urls);
  setCheck("set-auto-start", s.auto_start_downloads);
  setVal("set-scheduled-start", s.scheduled_download_start || "");
  setCheck("set-enqueue-convert", s.enqueue_downloads_to_convert);
  setVal("set-workers", s.worker_count);
  setVal("set-output-dir", s.output_dir);
  setVal("set-yt-dlp-path", s.yt_dlp_path);

  setVal("set-organize-folder", s.download_organize_folder || "flat");
  setVal("set-organize-filename", s.download_organize_filename || "title_id");
  setVal("set-output-template", s.output_filename_template);
  setCheck("set-post-organize", s.post_download_organize);
  setVal("set-quality", s.quality_preset);
  setVal("set-quality-custom", s.quality_format_custom);
  setVal("set-download-min-height", s.download_min_height ?? 0);
  setVal("set-download-min-fps", s.download_min_fps ?? 0);
  setVal("set-merge-container", s.merge_container);
  setVal("set-playlist-cap", s.playlist_preview_cap);
  setVal("set-download-archive", s.yt_download_archive);
  setVal("set-proxy", s.yt_proxy);
  setVal("set-limit-rate", s.yt_limit_rate);
  setCheck("set-sponsorblock-remove", s.yt_sponsorblock_remove);
  setVal("set-sponsorblock-mark", s.yt_sponsorblock_mark);

  setCheck("set-unlimited-retries", s.yt_dlp_unlimited_retries);
  setVal("set-retry-count", s.yt_dlp_retry_count);
  setVal("set-socket-timeout", s.yt_dlp_socket_timeout_secs);
  setVal("set-retry-sleep", s.yt_dlp_retry_sleep_secs);
  setVal("set-download-auto-retries", s.yt_dlp_download_auto_retries);
  setVal("set-cookies", s.yt_dlp_cookies);
  setVal("set-impersonate", s.yt_dlp_impersonate);
  setVal("set-extra-args", s.yt_dlp_extra_args);
  setCheck("set-embed-thumbnail", s.embed_thumbnail);
  setCheck("set-embed-metadata", s.yt_embed_metadata);
  setCheck("set-ignore-errors", s.yt_ignore_errors);
  setCheck("set-restrict-filenames", s.yt_restrict_filenames);
  setCheck("set-write-info-json", s.yt_write_info_json);
  setCheck("set-write-auto-subs", s.yt_write_auto_subs);

  setVal("set-ffmpeg-post-args", s.ffmpeg_post_args);
  setCheck("set-ffmpeg-faststart", s.ffmpeg_faststart);
  setCheck("set-ffmpeg-remux-mp4", s.ffmpeg_remux_mp4);
  setCheck("set-ffmpeg-mp3", s.ffmpeg_extract_audio_mp3);
  setCheck("set-verify-streams", s.verify_output_video_audio);

  setVal("set-convert-target-codec", s.convert_target_codec || "av1");
  setVal("set-convert-bitrate", s.convert_target_bitrate);
  setVal("set-convert-max-width", s.convert_max_width);
  setVal("set-convert-preset", s.convert_size_preset);
  let sizeLimitKind = s.convert_size_limit_kind || "none";
  if (sizeLimitKind === "none" && (s.convert_min_shrink_percent || 0) > 0) {
    sizeLimitKind = "min_shrink_percent";
    s.convert_size_limit_value = String(s.convert_min_shrink_percent);
  }
  setVal("set-convert-size-limit-kind", sizeLimitKind);
  setVal("set-convert-size-limit-value", s.convert_size_limit_value || "");
  setVal("set-convert-size-limit-violation", s.convert_size_limit_violation || "skip");
  updateConvertSizeLimitFieldsVisibility();
  setVal("set-convert-cpu-threads", s.convert_cpu_threads ?? 0);
  setVal("set-convert-parallel", s.convert_parallel ?? 1);
  setVal("set-convert-encoder-override", s.convert_encoder_override);
  setCheck("set-convert-recursive", s.convert_recursive);
  setCheck("set-convert-dry-run", s.convert_dry_run);
  setCheck("set-convert-auto-start", s.convert_auto_start_on_add);
  setCheck("set-convert-overwrite", s.convert_overwrite);
  setCheck("set-convert-reencode", s.convert_reencode_target);
  setCheck("set-convert-recommended-container", s.convert_use_recommended_container);
  setCheck("set-convert-delete-original", s.convert_delete_original);
  setCheck("set-convert-rename-original", s.convert_rename_original);
  setCheck("set-convert-remember-queue", s.convert_remember_queue);
  setVal("set-convert-subtitle-mode", s.convert_subtitle_mode || "none");
  setVal("set-convert-audio-extract", s.convert_audio_extract || "none");
  setCheck("set-convert-copy-subtitles", s.convert_copy_subtitles);
  setCheck("set-convert-write-checksum", s.convert_write_checksum);
  setVal("set-convert-post-move-subfolder", s.convert_post_move_subfolder);
  setVal("set-convert-max-hw-encodes", s.convert_max_hw_encodes ?? 0);

  setCheck("set-web-ui-enabled", s.web_ui_enabled);
  setVal("set-web-bind-address", s.web_bind_address || "0.0.0.0:8765");
  setVal("set-web-tls-cert", s.web_tls_cert_path);
  setVal("set-web-tls-key", s.web_tls_key_path);
  setCheck("set-watch-folder-enabled", s.watch_folder_enabled);
  setVal("set-watch-folder-path", s.watch_folder_path);
  setCheck("set-convert-watch-enabled", s.convert_watch_folder_enabled);
  setVal("set-convert-watch-path", s.convert_watch_folder_path);
  const tokenEl = document.getElementById("set-web-auth-token");
  if (tokenEl) tokenEl.value = maskWebToken(s.web_auth_token);
  const whitelistEl = document.getElementById("set-web-ip-whitelist");
  if (whitelistEl) {
    whitelistEl.value = Array.isArray(s.web_auth_ip_whitelist)
      ? s.web_auth_ip_whitelist.join("\n")
      : "";
  }

  document.getElementById("command-preview").textContent = commandPreview || "";
  updateQualityCustomVisibility();
  updateOrganizeUi(s);
}

function collectSettingsForm(base) {
  const s = { ...base };
  s.show_thumbnails = document.getElementById("set-show-thumbnails").checked;
  s.compact_cards = document.getElementById("set-compact-cards").checked;
  s.hide_card_subtitle = document.getElementById("set-hide-subtitle").checked;
  s.card_list_layout = document.getElementById("set-card-list").checked;
  s.autoscroll_log = document.getElementById("set-autoscroll-log").checked;
  s.log_relative_time = document.getElementById("set-log-relative").checked;
  s.log_max_chars = parseInt(document.getElementById("set-log-max").value, 10) || 28000;
  s.log_filter = document.getElementById("log-filter")?.value || "all";
  s.mode_downloader_color = readModeColorField(
    "set-mode-downloader-color",
    "set-mode-downloader-hex",
  );
  s.mode_convert_color = readModeColorField("set-mode-convert-color", "set-mode-convert-hex");
  s.subprocess_priority =
    document.getElementById("set-subprocess-priority").value || "normal";
  s.ffmpeg_path = document.getElementById("set-ffmpeg-path").value;
  s.ffprobe_path = document.getElementById("set-ffprobe-path").value;

  s.auto_add_pasted_urls = document.getElementById("set-auto-add").checked;
  s.auto_start_downloads = document.getElementById("set-auto-start").checked;
  s.scheduled_download_start =
    document.getElementById("set-scheduled-start")?.value?.trim() || "";
  s.enqueue_downloads_to_convert = document.getElementById("set-enqueue-convert").checked;
  s.worker_count = parseInt(document.getElementById("set-workers").value, 10) || 3;
  s.output_dir = document.getElementById("set-output-dir").value;
  s.yt_dlp_path = document.getElementById("set-yt-dlp-path").value;
  s.active_profile = document.getElementById("set-active-profile").value;

  s.download_organize_folder = document.getElementById("set-organize-folder").value;
  s.download_organize_filename = document.getElementById("set-organize-filename").value;
  s.output_filename_template = document.getElementById("set-output-template").value;
  s.post_download_organize = document.getElementById("set-post-organize").checked;
  s.quality_preset = document.getElementById("set-quality").value;
  s.quality_format_custom = document.getElementById("set-quality-custom").value;
  s.download_min_height =
    parseInt(document.getElementById("set-download-min-height").value, 10) || 0;
  s.download_min_fps =
    parseInt(document.getElementById("set-download-min-fps").value, 10) || 0;
  s.merge_container = document.getElementById("set-merge-container").value;
  s.playlist_preview_cap =
    parseInt(document.getElementById("set-playlist-cap").value, 10) || 20;
  s.yt_download_archive = document.getElementById("set-download-archive").value;
  s.yt_proxy = document.getElementById("set-proxy").value;
  s.yt_limit_rate = document.getElementById("set-limit-rate").value;
  s.yt_sponsorblock_remove = document.getElementById("set-sponsorblock-remove").checked;
  s.yt_sponsorblock_mark = document.getElementById("set-sponsorblock-mark").value;

  s.yt_dlp_unlimited_retries = document.getElementById("set-unlimited-retries").checked;
  s.yt_dlp_retry_count = parseInt(document.getElementById("set-retry-count").value, 10) || 10;
  s.yt_dlp_socket_timeout_secs = parseInt(document.getElementById("set-socket-timeout").value, 10) || 0;
  s.yt_dlp_retry_sleep_secs = parseInt(document.getElementById("set-retry-sleep").value, 10) || 0;
  s.yt_dlp_download_auto_retries = parseInt(document.getElementById("set-download-auto-retries").value, 10) || 0;
  s.yt_dlp_cookies = document.getElementById("set-cookies").value;
  s.yt_dlp_impersonate = document.getElementById("set-impersonate").value;
  s.yt_dlp_extra_args = document.getElementById("set-extra-args").value;
  s.embed_thumbnail = document.getElementById("set-embed-thumbnail").checked;
  s.yt_embed_metadata = document.getElementById("set-embed-metadata").checked;
  s.yt_ignore_errors = document.getElementById("set-ignore-errors").checked;
  s.yt_restrict_filenames = document.getElementById("set-restrict-filenames").checked;
  s.yt_write_info_json = document.getElementById("set-write-info-json").checked;
  s.yt_write_auto_subs = document.getElementById("set-write-auto-subs").checked;

  s.ffmpeg_post_args = document.getElementById("set-ffmpeg-post-args").value;
  s.ffmpeg_faststart = document.getElementById("set-ffmpeg-faststart").checked;
  s.ffmpeg_remux_mp4 = document.getElementById("set-ffmpeg-remux-mp4").checked;
  s.ffmpeg_extract_audio_mp3 = document.getElementById("set-ffmpeg-mp3").checked;
  s.verify_output_video_audio = document.getElementById("set-verify-streams").checked;

  s.convert_target_codec = document.getElementById("set-convert-target-codec").value || "av1";
  s.convert_target_bitrate = document.getElementById("set-convert-bitrate").value;
  s.convert_max_width = parseInt(document.getElementById("set-convert-max-width").value, 10) || 1920;
  s.convert_size_preset = document.getElementById("set-convert-preset").value;
  s.convert_size_limit_kind =
    document.getElementById("set-convert-size-limit-kind").value || "none";
  s.convert_size_limit_value = document
    .getElementById("set-convert-size-limit-value")
    .value.trim();
  s.convert_size_limit_violation =
    document.getElementById("set-convert-size-limit-violation").value || "skip";
  if (s.convert_size_limit_kind === "min_shrink_percent") {
    s.convert_min_shrink_percent =
      parseFloat(s.convert_size_limit_value) || 0;
  } else {
    s.convert_min_shrink_percent = 0;
  }
  s.convert_cpu_threads =
    parseInt(document.getElementById("set-convert-cpu-threads").value, 10) || 0;
  s.convert_parallel =
    parseInt(document.getElementById("set-convert-parallel").value, 10) || 1;
  s.convert_encoder_override = document.getElementById("set-convert-encoder-override").value;
  s.convert_recursive = document.getElementById("set-convert-recursive").checked;
  s.convert_dry_run = document.getElementById("set-convert-dry-run").checked;
  s.convert_auto_start_on_add = document.getElementById("set-convert-auto-start").checked;
  s.convert_overwrite = document.getElementById("set-convert-overwrite").checked;
  s.convert_reencode_target = document.getElementById("set-convert-reencode").checked;
  s.convert_use_recommended_container = document.getElementById(
    "set-convert-recommended-container",
  ).checked;
  s.convert_delete_original = document.getElementById("set-convert-delete-original").checked;
  s.convert_rename_original = document.getElementById("set-convert-rename-original").checked;
  s.convert_remember_queue = document.getElementById("set-convert-remember-queue").checked;
  s.convert_subtitle_mode =
    document.getElementById("set-convert-subtitle-mode")?.value || "none";
  s.convert_audio_extract =
    document.getElementById("set-convert-audio-extract")?.value || "none";
  s.convert_copy_subtitles =
    document.getElementById("set-convert-copy-subtitles")?.checked ?? false;
  s.convert_write_checksum =
    document.getElementById("set-convert-write-checksum")?.checked ?? false;
  s.convert_post_move_subfolder =
    document.getElementById("set-convert-post-move-subfolder")?.value || "";
  s.convert_max_hw_encodes =
    parseInt(document.getElementById("set-convert-max-hw-encodes")?.value, 10) || 0;

  s.web_ui_enabled = document.getElementById("set-web-ui-enabled").checked;
  s.web_bind_address =
    document.getElementById("set-web-bind-address").value.trim() || "0.0.0.0:8765";
  s.web_tls_cert_path = document.getElementById("set-web-tls-cert")?.value.trim() || "";
  s.web_tls_key_path = document.getElementById("set-web-tls-key")?.value.trim() || "";
  s.web_auth_ip_whitelist = (document.getElementById("set-web-ip-whitelist")?.value || "")
    .split(/\n+/)
    .map((line) => line.trim())
    .filter(Boolean);
  s.web_browser_notifications =
    document.getElementById("set-web-browser-notifications")?.checked ?? true;
  s.watch_folder_enabled = document.getElementById("set-watch-folder-enabled")?.checked ?? false;
  s.watch_folder_path = document.getElementById("set-watch-folder-path")?.value || "";
  s.convert_watch_folder_enabled =
    document.getElementById("set-convert-watch-enabled")?.checked ?? false;
  s.convert_watch_folder_path = document.getElementById("set-convert-watch-path")?.value || "";

  if (s.ffmpeg_extract_audio_mp3) s.ffmpeg_remux_mp4 = false;
  s.convert_max_width = Math.min(7680, Math.max(320, s.convert_max_width));
  if (s.convert_size_limit_kind === "min_shrink_percent") {
    s.convert_min_shrink_percent = Math.min(
      95,
      Math.max(0, parseFloat(s.convert_size_limit_value) || 0),
    );
  } else {
    s.convert_min_shrink_percent = 0;
  }
  s.convert_cpu_threads = Math.max(0, s.convert_cpu_threads);
  s.convert_parallel = Math.min(6, Math.max(1, s.convert_parallel));
  s.convert_max_hw_encodes = Math.min(6, Math.max(0, s.convert_max_hw_encodes ?? 0));
  const allowedPriority = new Set(["normal", "below_normal", "idle"]);
  if (!allowedPriority.has(s.subprocess_priority)) s.subprocess_priority = "normal";
  s.worker_count = Math.min(6, Math.max(1, s.worker_count));
  s.playlist_preview_cap = Math.min(500, Math.max(1, s.playlist_preview_cap));
  return s;
}

function switchSettingsTab(name) {
  document.querySelectorAll(".settings-tab").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.tab === name);
  });
  document.getElementById("settings-tab-shared").hidden = name !== "shared";
  document.getElementById("settings-tab-downloader").hidden = name !== "downloader";
  document.getElementById("settings-tab-convert").hidden = name !== "convert";
  document.getElementById("settings-tab-webui").hidden = name !== "webui";
}

async function refreshQueueTemplatesList() {
  const list = document.getElementById("queue-template-list");
  if (!list) return;
  try {
    const res = await api("/api/queue/templates");
    const data = await res.json();
    const names = data.templates || [];
    if (!names.length) {
      list.innerHTML = "<li>No saved templates yet.</li>";
      return;
    }
    list.innerHTML = names.map((n) => `<li>${escapeHtml(n)}</li>`).join("");
  } catch {
    list.innerHTML = "<li>Could not load templates.</li>";
  }
}

async function loadConvertPresetsRow() {
  const root = document.getElementById("convert-presets-row");
  if (!root) return;
  try {
    const res = await api("/api/convert/presets");
    const data = await res.json();
    const presets = data.presets || [];
    root.innerHTML = "";
    for (const name of presets) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "secondary";
      btn.textContent = name;
      btn.onclick = () =>
        api("/api/convert/presets/apply", {
          method: "POST",
          body: JSON.stringify({ name }),
        })
          .then(() => showToast(`Applied preset “${name}”.`))
          .catch((e) => notifyError(e.message || "Could not apply preset."));
      root.appendChild(btn);
    }
  } catch {
    root.innerHTML = "<span class=\"hint\">Presets unavailable.</span>";
  }
}

async function openSettingsDialog(tab) {
  const [settingsRes, profilesRes] = await Promise.all([
    api("/api/settings"),
    api("/api/profiles"),
  ]);
  const settingsData = await settingsRes.json();
  const profilesData = await profilesRes.json();
  cachedSettings = settingsData.settings;
  populateSettingsForm(cachedSettings, settingsData.command_preview);
  populateProfiles(profilesData);
  refreshQueueTemplatesList().catch(() => {});
  document.getElementById("settings-dialog").showModal();
  if (tab) switchSettingsTab(tab);
}

async function patchHostSettings(patch) {
  const res = await api("/api/settings", {
    method: "POST",
    body: JSON.stringify({ patch }),
  });
  if (!res.ok) {
    throw new Error(await readApiError(res, "Could not update settings."));
  }
  const data = await res.json();
  cachedSettings = data.settings;
  return data;
}

async function applyLayoutPresetViaApi(preset) {
  if (!cachedSettings) {
    const res = await api("/api/settings");
    cachedSettings = (await res.json()).settings;
  }
  const patch = { ...cachedSettings };
  applyLayoutPreset(patch, preset);
  const res = await api("/api/settings", {
    method: "POST",
    body: JSON.stringify({ settings: patch }),
  });
  if (!res.ok) {
    throw new Error(await readApiError(res, "Could not apply layout preset."));
  }
  const data = await res.json();
  cachedSettings = data.settings;
  await refreshAll();
}

function toggleActivityLogExpanded() {
  logExpanded = !logExpanded;
  document.getElementById("log-view")?.classList.toggle("log-expanded", logExpanded);
  const btn = document.getElementById("btn-expand-log");
  if (btn) btn.textContent = logExpanded ? "Collapse log" : "Expand log";
}

async function applyProfile(name) {
  await api("/api/profiles/apply", {
    method: "POST",
    body: JSON.stringify({ name }),
  });
  const res = await api("/api/settings");
  const data = await res.json();
  cachedSettings = data.settings;
  populateSettingsForm(cachedSettings, data.command_preview);
  const profilesRes = await api("/api/profiles");
  populateProfiles(await profilesRes.json());
  await refreshToolsOnly();
}

function clearThumbnailCaches() {
  thumbRetryCounts.clear();
  for (const key of thumbBlobCache.keys()) {
    revokeThumbBlob(key);
  }
  thumbInflight.clear();
  convertThumbFailedKeys.clear();
  for (const key of convertThumbBlobCache.keys()) {
    const url = convertThumbBlobCache.get(key);
    if (url) URL.revokeObjectURL(url);
    convertThumbBlobCache.delete(key);
  }
  convertThumbInflight.clear();
}

async function validateAuthToken(candidate) {
  const res = await fetch("/api/status", {
    headers: { "X-Rustdl-Token": candidate },
  });
  if (res.status === 401) {
    throw new Error(
      "Token rejected. Copy the current API token from rustdl Settings → Web UI on the host PC."
    );
  }
  if (res.status === 503) {
    throw new Error(
      "Web UI has no API token configured on the host. Set one in rustdl Settings → Web UI."
    );
  }
  if (!res.ok) {
    throw new Error(await readApiError(res, `Could not connect (${res.status})`));
  }
}

function tokenFromPageUrl() {
  return new URLSearchParams(window.location.search).get("token")?.trim() || "";
}

function stripTokenFromPageUrl() {
  const url = new URL(window.location.href);
  if (!url.searchParams.has("token")) return;
  url.searchParams.delete("token");
  window.history.replaceState({}, "", url.pathname + url.search + url.hash);
}

/** @returns {Promise<boolean>} true when the token was accepted and the app is connected */
async function saveTokenFromForm() {
  const input = document.getElementById("token-input");
  const statusEl = document.getElementById("auth-status");
  const saveBtn = document.getElementById("btn-save-token");
  const v = input?.value?.trim() ?? "";
  if (!v) {
    if (statusEl) {
      statusEl.textContent =
        "Enter the API token from rustdl Settings → Web UI, then click Save token.";
    }
    input?.focus();
    return false;
  }
  if (saveBtn) saveBtn.disabled = true;
  if (statusEl) statusEl.textContent = "Checking token…";
  try {
    await validateAuthToken(v);
    localStorage.setItem(TOKEN_KEY, v);
    ipAuthBypass = false;
    clearThumbnailCaches();
    if (statusEl) statusEl.textContent = "Token saved.";
    showApp();
    await refreshAll();
    connectSse();
    startStatusPoll();
    startFallbackPolling();
    return true;
  } catch (e) {
    localStorage.removeItem(TOKEN_KEY);
    showAuthPanel();
    const msg = e instanceof Error ? e.message : String(e);
    if (statusEl) statusEl.textContent = msg;
    notifyError(msg);
    return false;
  } finally {
    if (saveBtn) saveBtn.disabled = false;
  }
}

const authForm = document.getElementById("auth-form");
if (authForm) {
  authForm.addEventListener("submit", (e) => {
    e.preventDefault();
    saveTokenFromForm().catch((err) =>
      notifyError(err instanceof Error ? err.message : String(err))
    );
  });
}

document.getElementById("btn-refresh-tools").onclick = () => refreshToolsOnly().catch(() => {});

document.getElementById("btn-quit").onclick = () =>
  requestAppShutdown().catch((e) => {
    shuttingDown = false;
    updateQuitButtonState();
    alert(e instanceof Error ? e.message : String(e));
  });

document.getElementById("btn-add").onclick = async () => {
  clearTimeout(autoAddTimer);
  await flushAutoAddFromInput();
};

let pendingPlaylistUrls = [];

document.getElementById("btn-playlist-preview")?.addEventListener("click", async () => {
  const input = document.getElementById("url-input");
  const lines = (input?.value || "")
    .split(/\n+/)
    .map((s) => s.trim())
    .filter(Boolean);
  const url = lines[0];
  if (!url) {
    notifyError("Paste a playlist or channel URL first.");
    return;
  }
  try {
    const res = await api("/api/queue/playlist-preview", {
      method: "POST",
      body: JSON.stringify({ url }),
    });
    const data = await res.json();
    const title = data.title ? `"${data.title}"` : "This playlist";
    if (!data.count) {
      notifyError("No entries found (single video or empty playlist).");
      return;
    }
    pendingPlaylistUrls = data.urls || [];
    const summary = `${title} has ${data.count} video(s) (up to cap). Add all to the queue?`;
    document.getElementById("playlist-preview-summary").textContent = summary;
    document.getElementById("playlist-preview-dialog").showModal();
  } catch (e) {
    notifyError(e.message || String(e));
  }
});

document.getElementById("playlist-preview-form")?.addEventListener("submit", async (e) => {
  e.preventDefault();
  document.getElementById("playlist-preview-dialog").close();
  if (!pendingPlaylistUrls.length) return;
  try {
    await api("/api/queue", {
      method: "POST",
      body: JSON.stringify({ urls: pendingPlaylistUrls }),
    });
    pendingPlaylistUrls = [];
    await refreshAll();
    showToast("Playlist URLs added to the queue.");
  } catch (err) {
    notifyError(err.message || "Could not add playlist URLs.");
  }
});

document.getElementById("btn-playlist-preview-cancel")?.addEventListener("click", () => {
  document.getElementById("playlist-preview-dialog")?.close();
});

document.getElementById("btn-clear-url-input").onclick = () => clearUrlInput();

const queueClearMount = document.getElementById("queue-clear-menu");
if (queueClearMount) {
  mountQueueImportExport(queueClearMount);
  mountClearQueueMenu(queueClearMount);
}

document.getElementById("btn-clear-log").onclick = () =>
  clearActivityLog().catch((e) => alert(e.message || String(e)));

document.getElementById("url-input").addEventListener("input", scheduleAutoAddFromInput);

document.getElementById("btn-start").onclick = () =>
  postAction("/api/downloads/start", "Downloads could not start.")
    .then(() => refreshAll())
    .catch(() => {});
document.getElementById("btn-pause").onclick = () =>
  api("/api/downloads/pause", { method: "POST" })
    .then(() => refreshAll())
    .catch(console.error);
document.getElementById("btn-resume").onclick = () =>
  api("/api/downloads/resume", { method: "POST" })
    .then(() => refreshAll())
    .catch(console.error);

document.getElementById("btn-retry-failed")?.addEventListener("click", () =>
  postAction("/api/downloads/retry-failed", "Could not retry failed downloads.")
    .then(() => refreshAll())
    .catch(() => {})
);

document.getElementById("btn-about-brand")?.addEventListener("click", () => openAboutDialog());
document.getElementById("btn-about-close")?.addEventListener("click", () => {
  document.getElementById("about-dialog")?.close();
});
document.getElementById("about-dialog")?.addEventListener("click", (e) => {
  if (e.target === e.currentTarget) e.currentTarget.close();
});
document.getElementById("btn-settings").onclick = () => openSettingsDialog().catch(console.error);

document.getElementById("btn-settings-cancel").onclick = () => {
  document.getElementById("settings-dialog").close();
};

document.getElementById("settings-form").onsubmit = async (e) => {
  e.preventDefault();
  if (!cachedSettings) {
    try {
      const res = await api("/api/settings");
      const data = await res.json();
      cachedSettings = data.settings;
    } catch (err) {
      notifyError(err.message || "Could not load settings.");
      return;
    }
  }
  const wasThumbnails = cachedSettings.show_thumbnails !== false;
  const patch = collectSettingsForm(cachedSettings);
  try {
    const res = await api("/api/settings", { method: "POST", body: JSON.stringify({ settings: patch }) });
    const data = await res.json();
    cachedSettings = data.settings;
    statusFlags.auto_add_pasted_urls = !!cachedSettings.auto_add_pasted_urls;
    if (cachedSettings.show_thumbnails && !wasThumbnails) {
      clearThumbnailCaches();
    }
    renderLogView();
    document.getElementById("settings-dialog").close();
    await refreshAll();
  } catch (err) {
    notifyError(err.message || "Could not save settings.");
  }
};

document.querySelectorAll(".settings-tab").forEach((btn) => {
  btn.onclick = () => switchSettingsTab(btn.dataset.tab);
});

document.getElementById("set-quality").onchange = updateQualityCustomVisibility;

function onOrganizeFieldChange() {
  if (!cachedSettings) return;
  const draft = collectSettingsForm(cachedSettings);
  updateOrganizeUi(draft);
}

["set-organize-folder", "set-organize-filename", "set-output-template", "set-output-dir"].forEach(
  (id) => {
    const el = document.getElementById(id);
    if (el) el.addEventListener("input", onOrganizeFieldChange);
    if (el) el.addEventListener("change", onOrganizeFieldChange);
  },
);

document.querySelectorAll(".organize-preset-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    if (!cachedSettings) return;
    const draft = collectSettingsForm(cachedSettings);
    applyOrganizePreset(draft, btn.dataset.organize);
    populateSettingsForm(draft, document.getElementById("command-preview")?.textContent || "");
  });
});

document.getElementById("btn-apply-profile").onclick = () => {
  const name = document.getElementById("set-active-profile").value;
  if (name) applyProfile(name).catch(console.error);
};

document.querySelectorAll(".preset-btn").forEach((btn) => {
  btn.onclick = () => applyProfile(btn.dataset.profile).catch(console.error);
});

/* ----------------------------- Video Converter ----------------------------- */

let currentView = "downloader";
/** Skip re-fetching AV1 thumbnails that already failed until the source changes. */
const convertThumbFailedKeys = new Set();
const convertThumbBlobCache = new Map();
/** @type {Map<string, Promise<string|null>>} */
const convertThumbInflight = new Map();

function formatBytes(n) {
  if (n == null) return "";
  let v = Number(n);
  if (!isFinite(v)) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  const digits = i === 0 || v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${units[i]}`;
}

function baseName(p) {
  if (!p) return "";
  const parts = String(p).split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : String(p);
}

function setView(view) {
  currentView =
    view === "convert" ? "convert" : view === "library" ? "library" : "downloader";
  document.body.classList.remove("view-downloader", "view-convert", "view-library");
  document.body.classList.add(`view-${currentView}`);
  try {
    localStorage.setItem(VIEW_STORAGE_KEY, currentView);
  } catch {
    /* ignore */
  }
  document.querySelectorAll(".nav-btn").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.view === currentView);
  });
  document.getElementById("downloader-main").classList.toggle("hidden", currentView !== "downloader");
  document.getElementById("convert-main").classList.toggle("hidden", currentView !== "convert");
  document.getElementById("library-main")?.classList.toggle("hidden", currentView !== "library");
  const dlActions = document.getElementById("downloader-only-actions");
  if (dlActions) dlActions.classList.toggle("hidden", currentView !== "downloader");
  if (currentView === "convert") refreshConvert().catch(() => {});
  if (currentView === "library") refreshLibrary().catch(() => {});
}

async function refreshLibrary() {
  const root = document.getElementById("library-list");
  if (!root) return;
  const searchEl = document.getElementById("library-search");
  const historyEl = document.getElementById("library-history-filter");
  const searchQuery = searchEl ? searchEl.value : "";
  const historyVal = historyEl ? historyEl.value : "all";
  const historyDays = historyVal === "all" ? null : parseInt(historyVal, 10);
  try {
    const res = await api("/api/library");
    const data = await res.json();
    const allItems = data.items || [];
    const items = allItems.filter(
      (it) =>
        libraryItemMatchesSearch(it, searchQuery) &&
        libraryItemWithinHistory(it, historyDays),
    );
    if (!allItems.length) {
      root.innerHTML = "<p class=\"hint\">No completed downloads.</p>";
      return;
    }
    if (!items.length) {
      root.innerHTML = "<p class=\"hint\">No library items match the current search or filter.</p>";
      return;
    }
    root.innerHTML = items
      .map((it) => {
        const meta = [
          it.uploader ? escapeHtml(it.uploader) : "",
          it.completed_at ? formatRelativeTime(it.completed_at) : "",
          it.local_path ? escapeHtml(it.local_path) : "",
        ]
          .filter(Boolean)
          .join(" · ");
        const openBtn = it.local_path
          ? `<button type="button" class="secondary" data-open="${it.item_id}">Open</button>`
          : "";
        return `<div class="queue-card library-card">
        <div class="card-title">${escapeHtml(it.title || it.video_id || "Untitled")}</div>
        ${meta ? `<div class="card-meta hint">${meta}</div>` : ""}
        <div class="card-actions btn-group">
          <button type="button" class="secondary" data-requeue="${it.item_id}">Re-queue</button>
          ${openBtn}
        </div>
      </div>`;
      })
      .join("");
    root.querySelectorAll("[data-requeue]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const item = items.find((x) => String(x.item_id) === btn.dataset.requeue);
        if (!item) return;
        const url = item.webpage_url;
        if (!url) {
          notifyError("No URL to re-queue.");
          return;
        }
        try {
          await api("/api/queue", { method: "POST", body: JSON.stringify({ urls: [url] }) });
          showToast("Re-queued download.");
        } catch (err) {
          notifyError(err.message || "Re-queue failed.");
        }
      });
    });
    root.querySelectorAll("[data-open]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        try {
          await api(`/api/library/${btn.dataset.open}/open`, {
            method: "POST",
            body: JSON.stringify({ target: "file" }),
          });
        } catch (err) {
          notifyError(err.message || "Could not open file.");
        }
      });
    });
  } catch (err) {
    root.innerHTML = `<p class="hint">${escapeHtml(err.message || "Could not load library.")}</p>`;
  }
}

function convertSlug(item) {
  if (item.skipped) return "skipped";
  switch (item.status) {
    case "Idle":
      return "idle";
    case "Queued":
      return "queued";
    case "Downloading":
      return "downloading";
    case "Done":
      return "done";
    case "Failed":
      return "failed";
    case "Resolving":
      return "resolving";
    default:
      return "idle";
  }
}

function convertGroup(item) {
  if (item.skipped) return "Skipped";
  switch (item.status) {
    case "Queued":
    case "Downloading":
      return "Active";
    case "Failed":
      return "Failed";
    case "Done":
      return "Done";
    case "Idle":
    default:
      return "Ready";
  }
}

function convertThumbKey(item) {
  return `${item.item_id}|${item.source_path || ""}`;
}

function convertThumbnailUrl(itemId) {
  if (!apiAuthOptional()) return null;
  return apiUrlWithAuth(`/api/convert/thumbnail/${itemId}`);
}

function revealconvertThumbImage(img, placeholder, key) {
  img.classList.remove("hidden");
  placeholder.classList.add("hidden");
  convertThumbFailedKeys.delete(key);
}

function applyconvertThumbBlobToImg(img, placeholder, key, objUrl) {
  img.onload = () => {
    if (!img.isConnected) return;
    revealconvertThumbImage(img, placeholder, key);
  };
  img.onerror = () => {
    if (!img.isConnected) return;
    revokeconvertThumbBlob(key);
    img.classList.add("hidden");
    img.removeAttribute("src");
    placeholder.textContent = "No preview available";
    placeholder.classList.remove("hidden");
  };
  img.src = objUrl;
  if (img.complete && img.naturalWidth > 0) {
    revealconvertThumbImage(img, placeholder, key);
  }
}

function attachconvertThumbnail(img, placeholder, item, showThumbnails) {
  img.classList.add("hidden");
  placeholder.classList.remove("hidden");
  if (!showThumbnails) {
    placeholder.textContent = "Thumbnails off";
    return;
  }
  if (!convertThumbnailUrl(item.item_id)) {
    placeholder.textContent = convertThumbFailurePlaceholder("no_token", convertThumbKey(item));
    return;
  }
  const key = convertThumbKey(item);
  if (convertThumbFailedKeys.has(key)) {
    placeholder.textContent = "No preview available";
    return;
  }
  const cached = convertThumbBlobCache.get(key);
  if (cached) {
    applyconvertThumbBlobToImg(img, placeholder, key, cached);
    return;
  }
  placeholder.textContent = "Loading preview…";
  fetchconvertThumbnailBlob(item).then((result) => {
    if (!img.isConnected) return;
    if (result.url) {
      applyconvertThumbBlobToImg(img, placeholder, key, result.url);
    } else {
      placeholder.textContent = convertThumbFailurePlaceholder(result.reason, key);
      placeholder.classList.remove("hidden");
      img.classList.add("hidden");
    }
  });
}

function parseResolutionHeight(label) {
  const s = String(label);
  const x = s.split("x");
  if (x.length === 2) {
    const h = parseInt(x[1].trim(), 10);
    if (!Number.isNaN(h)) return h;
  }
  const p = s.trim().match(/^(\d+)p$/i);
  if (p) return parseInt(p[1], 10);
  const w = s.trim().match(/^(\d+)w$/i);
  if (w) return parseInt(w[1], 10);
  return 0;
}

function parseFpsValue(label) {
  const n = parseFloat(String(label).split(/\s+/)[0]);
  return Number.isFinite(n) ? n : 0;
}

function metaBadgeClasses(kind, label) {
  const classes = ["meta-badge", `meta-badge-${kind}`];
  const text = String(label || "");
  if (kind === "codec") {
    const c = text.toLowerCase().replace(/[.\- _]/g, "");
    if (c.includes("av1")) classes.push("meta-badge-codec-av1");
    else if (c.includes("hevc") || c.includes("h265") || c.includes("265"))
      classes.push("meta-badge-codec-hevc");
    else if (c.includes("h264") || c.includes("avc") || c.includes("264"))
      classes.push("meta-badge-codec-h264");
    else if (c.includes("vp9")) classes.push("meta-badge-codec-vp9");
    else classes.push("meta-badge-codec-other");
  } else if (kind === "resolution") {
    const h = parseResolutionHeight(text);
    if (h >= 2160) classes.push("meta-badge-res-4k");
    else if (h >= 1080) classes.push("meta-badge-res-1080");
    else if (h >= 720) classes.push("meta-badge-res-720");
    else if (h >= 480) classes.push("meta-badge-res-480");
    else classes.push("meta-badge-res-other");
  } else if (kind === "fps") {
    const fps = parseFpsValue(text);
    if (fps >= 50) classes.push("meta-badge-fps-high");
    else if (fps >= 28) classes.push("meta-badge-fps-mid");
    else if (fps >= 23) classes.push("meta-badge-fps-cine");
    else classes.push("meta-badge-fps-other");
  } else if (kind === "size") {
    classes.push("meta-badge-size");
  } else if (kind === "bitrate") {
    classes.push("meta-badge-bitrate");
  } else if (kind === "skip") {
    classes.push("meta-badge-skip");
  }
  return classes.join(" ");
}

function appendMetaBadge(container, kind, text) {
  if (!text) return;
  const b = document.createElement("span");
  b.className = metaBadgeClasses(kind, text);
  b.textContent = text;
  container.appendChild(b);
}

function ConvertWillSkipNotice() {
  const el = document.createElement("p");
  el.className = "convert-will-skip-notice";
  el.textContent = "Will skip · already at target codec (re-encode disabled)";
  return el;
}

function ConvertMediaBadges(item) {
  const badges = document.createElement("div");
  badges.className = "card-badges";
  if (item.probing) {
    appendMetaBadge(badges, "other", "Probing…");
    return badges;
  }
  if (item.video_codec) appendMetaBadge(badges, "codec", String(item.video_codec).toUpperCase());
  if (item.width && item.height)
    appendMetaBadge(badges, "resolution", `${item.width}×${item.height}`);
  if (item.fps) appendMetaBadge(badges, "fps", `${Number(item.fps).toFixed(2)} fps`);
  if (item.input_bytes) appendMetaBadge(badges, "size", formatBytes(item.input_bytes));
  if (item.bitrate_bps) {
    const bps = Number(item.bitrate_bps);
    appendMetaBadge(
      badges,
      "bitrate",
      bps >= 1_000_000
        ? `${(bps / 1_000_000).toFixed(2)} Mbps`
        : `${Math.round(bps / 1000)} kbps`
    );
  }
  return badges;
}

async function openConvertItem(id, target) {
  const res = await api(`/api/convert/${id}/open`, {
    method: "POST",
    body: JSON.stringify({ target }),
  });
  if (!res.ok) {
    throw new Error(
      await readApiError(res, "Could not open on the PC running rustdl.")
    );
  }
}

function convertMediaStreamUrl(itemId) {
  return apiUrlWithAuth(`/api/convert/media/${itemId}`);
}

function toggleConvertCardMedia(item, thumb) {
  const existing = thumb.querySelector(".card-media");
  if (existing) {
    stopActiveMedia();
    return;
  }
  stopActiveMedia();
  const name = item.media_filename || baseName(item.source_path) || "";
  if (!browserCanPlayMediaFilename(name)) {
    const ph = thumb.querySelector(".card-thumb-placeholder");
    if (ph) {
      ph.textContent =
        "In-browser playback is not supported for this file type (e.g. MKV). Use Open to play on the PC running rustdl.";
      ph.classList.remove("hidden");
    }
    thumb.querySelector("img")?.classList.add("hidden");
    return;
  }
  const tag = item.media_kind === "audio" ? "audio" : "video";
  const el = document.createElement(tag);
  el.className = "card-media";
  el.controls = true;
  el.playsInline = true;
  el.preload = "metadata";
  el.src = convertMediaStreamUrl(item.item_id);
  el.addEventListener("error", () => {
    stopActiveMedia();
    const ph = thumb.querySelector(".card-thumb-placeholder");
    if (ph) {
      ph.textContent = item.playable
        ? "Playback failed (file missing or blocked)"
        : "No local file for this row";
      ph.classList.remove("hidden");
    }
  });
  thumb.querySelector("img")?.classList.add("hidden");
  thumb.querySelector(".card-thumb-placeholder")?.classList.add("hidden");
  thumb.appendChild(el);
  activeMediaEl = el;
  el.play().catch(() => {});
}

function appendConvertPlayButton(group, item, thumb) {
  if (!item.playable) return;
  const play = document.createElement("button");
  play.type = "button";
  play.className = "primary";
  setButtonLabel(play, ICON.playCircle, "Play");
  play.onclick = () => toggleConvertCardMedia(item, thumb);
  group.appendChild(play);
}

function appendConvertOpenMenuButton(group, item) {
  if (!item.can_open_file && !item.can_open_folder) return;

  const menu = document.createElement("details");
  menu.className = "btn-menu";

  const trigger = document.createElement("summary");
  trigger.className = "btn-menu-trigger secondary";
  setButtonLabel(trigger, ICON.folderOpen, "Open...");
  trigger.title = "Open on the PC running rustdl";
  menu.appendChild(trigger);

  const panel = document.createElement("div");
  panel.className = "btn-menu-panel";
  panel.setAttribute("role", "menu");

  if (item.can_open_file) {
    const openBtn = document.createElement("button");
    openBtn.type = "button";
    openBtn.className = "btn-menu-item";
    setButtonLabel(openBtn, ICON.playCircle, "Open file");
    openBtn.title = "Launch with the default app on the PC running rustdl";
    openBtn.onclick = (e) => {
      e.preventDefault();
      menu.open = false;
      openConvertItem(item.item_id, "file").catch((err) =>
        alert(err.message || String(err))
      );
    };
    panel.appendChild(openBtn);
  }

  if (item.can_open_folder) {
    const folderBtn = document.createElement("button");
    folderBtn.type = "button";
    folderBtn.className = "btn-menu-item";
    setButtonLabel(folderBtn, ICON.folderOpen, "Show in folder");
    folderBtn.title = "Reveal in Explorer / file manager on the PC running rustdl";
    folderBtn.onclick = (e) => {
      e.preventDefault();
      menu.open = false;
      openConvertItem(item.item_id, "folder").catch((err) =>
        alert(err.message || String(err))
      );
    };
    panel.appendChild(folderBtn);
  }

  menu.appendChild(panel);
  group.appendChild(menu);
}

function renderConvertCard(item, settings, ctx) {
  const slug = convertSlug(item);
  const active = slug === "downloading" || slug === "queued";
  const readyItems = (ctx && ctx.readyItems) || [];
  const card = document.createElement("article");
  card.className = "card" + (item.will_skip_target ? " convert-will-skip" : "");
  card.dataset.itemId = String(item.item_id);

  const showThumbnails = (settings || {}).show_thumbnails !== false;
  const thumb = document.createElement("div");
  thumb.className = "card-thumb";
  const img = document.createElement("img");
  img.alt = "";
  img.className = "hidden";
  const placeholder = document.createElement("span");
  placeholder.className = "card-thumb-placeholder";
  thumb.appendChild(img);
  attachconvertThumbnail(img, placeholder, item, showThumbnails);
  thumb.appendChild(placeholder);
  card.appendChild(thumb);

  const body = document.createElement("div");
  body.className = "card-body";

  const title = document.createElement("h3");
  title.className = "card-title";
  title.textContent = baseName(item.source_path) || "(unknown)";
  title.title = item.source_path || "";
  body.appendChild(title);

  if (item.will_skip_target) {
    body.appendChild(ConvertWillSkipNotice());
  }

  const badges = ConvertMediaBadges(item);
  const chip = document.createElement("span");
  setStatusChip(chip, slug, item.status_label || item.status || "");
  badges.appendChild(chip);
  body.appendChild(badges);

  if (active) {
    const wrap = document.createElement("div");
    wrap.className = "card-progress";
    const fill = document.createElement("div");
    fill.className = `card-progress-fill status-${slug}`;
    fill.style.width = `${Math.min(100, Math.max(0, Number(item.percent) || 0))}%`;
    wrap.appendChild(fill);
    body.appendChild(wrap);
  }

  const detail = (item.detail || "").trim();
  if (detail) {
    const detailEl = document.createElement("p");
    detailEl.className = `card-footer status-${slug}`;
    detailEl.textContent = detail;
    body.appendChild(detailEl);
  }

  const pathsEl = document.createElement("p");
  pathsEl.className = "convert-paths";
  pathsEl.textContent = `→ ${item.output_path || ""}`;
  pathsEl.title = item.output_path || "";
  body.appendChild(pathsEl);

  card.appendChild(body);

  const { bar: actions, group } = createCardActionBar();
  appendConvertPlayButton(group, item, thumb);
  appendConvertOpenMenuButton(group, item);
  if (slug === "idle" && readyItems.length > 1) {
    appendReadyReorderButtons(group, item, readyItems, reorderConvertItem);
    attachReadyRowDragDrop(card, item, readyItems, reorderConvertItem);
  }
  if (group.childElementCount > 0) {
    card.appendChild(actions);
  }

  appendSelectionCheckbox(card, item, selectedConvertIds, updateConvertBulkSelectionUi);

  return card;
}

function renderConvertSummary(data) {
  const root = document.getElementById("convert-summary");
  if (!root) return;
  root.innerHTML = "";

  const running = document.createElement("span");
  running.className = "status-badge " + (data.running ? "status-live" : "status-paused");
  running.innerHTML = `<span class="status-dot" aria-hidden="true"></span>${data.running ? "Converting" : "Idle"}`;
  root.appendChild(running);

  const counts = {};
  for (const it of data.items) {
    const g = convertGroup(it);
    counts[g] = (counts[g] || 0) + 1;
  }
  for (const [label, slug] of [
    ["Active", "downloading"],
    ["Ready", "idle"],
    ["Failed", "failed"],
    ["Skipped", "skipped"],
    ["Done", "done"],
  ]) {
    if (!counts[label]) continue;
    const el = document.createElement("span");
    el.className = `status-badge status-${slug}`;
    el.innerHTML = `<span class="status-dot" aria-hidden="true"></span>${counts[label]} ${label}`;
    root.appendChild(el);
  }

  const sum = data.summary || {};
  if (sum.completed > 0) {
    const inB = sum.completed_input_bytes || 0;
    const outB = sum.completed_output_bytes || 0;
    const el = document.createElement("span");
    el.className =
      "status-badge " + (outB > inB && inB > 0 ? "status-skipped" : "status-done");
    if (outB > inB && inB > 0) {
      const growth = outB - inB;
      const pct = ((growth / inB) * 100).toFixed(1);
      el.textContent = `Output +${formatBytes(growth)} (+${pct}%) across ${sum.completed} file(s)`;
    } else {
      const saved = Math.max(0, inB - outB);
      const pct = inB > 0 ? ((saved / inB) * 100).toFixed(1) : "0.0";
      el.textContent = `Saved ${formatBytes(saved)} (${pct}%) across ${sum.completed} file(s)`;
    }
    root.appendChild(el);
  }
  if (sum.pending_count > 0) {
    const el = document.createElement("span");
    el.className = "status-badge status-queued";
    el.textContent = `${sum.pending_count} pending · ${formatBytes(sum.pending_input_bytes || 0)}`;
    root.appendChild(el);
  }
}

function renderConvertEncoder(data) {
  const el = document.getElementById("convert-encoder");
  if (!el) return;
  const parts = [];
  if (data.encoder) parts.push(`Encoder: ${data.encoder.label}`);
  else if (!data.has_ffmpeg) parts.push("Encoder: ffmpeg not found (set the path in Settings → Shared)");
  if (!data.has_ffprobe) parts.push("ffprobe not found — metadata and start are disabled");
  el.textContent = parts.join(" · ");
}

function revokeconvertThumbBlob(key) {
  const url = convertThumbBlobCache.get(key);
  if (url) {
    URL.revokeObjectURL(url);
    convertThumbBlobCache.delete(key);
  }
}

function pruneconvertThumbKeys(items) {
  const active = new Set(items.map((it) => convertThumbKey(it)));
  for (const key of convertThumbFailedKeys) {
    if (!active.has(key)) convertThumbFailedKeys.delete(key);
  }
  for (const key of convertThumbBlobCache.keys()) {
    if (!active.has(key)) revokeconvertThumbBlob(key);
  }
  for (const key of convertThumbInflight.keys()) {
    if (!active.has(key)) convertThumbInflight.delete(key);
  }
}

async function fetchconvertThumbnailBlob(item) {
  const cacheKey = convertThumbKey(item);
  if (convertThumbBlobCache.has(cacheKey)) {
    return { url: convertThumbBlobCache.get(cacheKey), reason: null };
  }
  if (convertThumbFailedKeys.has(cacheKey)) {
    return { url: null, reason: "unavailable" };
  }
  if (convertThumbInflight.has(cacheKey)) {
    return convertThumbInflight.get(cacheKey);
  }
  const apiUrl = convertThumbnailUrl(item.item_id);
  if (!apiUrl) {
    return { url: null, reason: "no_token" };
  }
  const work = (async () => {
    try {
      const res = await fetch(apiUrl, { headers: imageFetchHeaders() });
      if (!res.ok) {
        if (res.status === 401) {
          showAuthPanel(
            "Token rejected. Copy the current API token from rustdl Settings → Web UI, paste it below, then click Save token."
          );
          return { url: null, reason: "unauthorized" };
        }
        convertThumbFailedKeys.add(cacheKey);
        return { url: null, reason: "unavailable" };
      }
      const blob = await blobFromImageResponse(res);
      if (blob.size < 32) {
        convertThumbFailedKeys.add(cacheKey);
        return { url: null, reason: "unavailable" };
      }
      const objUrl = URL.createObjectURL(blob);
      convertThumbBlobCache.set(cacheKey, objUrl);
      convertThumbFailedKeys.delete(cacheKey);
      return { url: objUrl, reason: null };
    } catch {
      convertThumbFailedKeys.add(cacheKey);
      return { url: null, reason: "unavailable" };
    }
  })();
  convertThumbInflight.set(cacheKey, work);
  try {
    return await work;
  } finally {
    convertThumbInflight.delete(cacheKey);
  }
}

const AUTO_LIST_LAYOUT_THRESHOLD = 50;

function effectiveListLayout(settings, itemCount, outerScrollPx = null, convertMode = false) {
  const s = settings || {};
  if (s.card_list_layout || itemCount > AUTO_LIST_LAYOUT_THRESHOLD) return true;
  if (outerScrollPx == null || outerScrollPx < 1) return false;
  const threshold = convertMode
    ? LAYOUT_CONVERT_SHORT_PANEL_LIST_THRESHOLD
    : LAYOUT_DL_SHORT_PANEL_LIST_THRESHOLD;
  return outerScrollPx < threshold;
}

function queueOuterScrollPx(elementId) {
  const el = document.getElementById(elementId);
  return el?.clientHeight || 0;
}

function updateConvertBulkSelectionUi() {
  const n = selectedConvertIds.size;
  const removeBtn = document.getElementById("btn-convert-bulk-remove");
  if (removeBtn) removeBtn.disabled = n === 0;
}

async function bulkRemoveConvertSelected() {
  if (!selectedConvertIds.size) return;
  if (
    !(await showConfirmDialog(
      `Remove ${selectedConvertIds.size} selected convert item(s) from the queue?`,
      "Remove selected"
    ))
  )
    return;
  const res = await api("/api/convert/bulk-remove", {
    method: "POST",
    body: JSON.stringify({ item_ids: [...selectedConvertIds] }),
  });
  if (!res.ok) throw new Error(await readApiError(res, "Bulk remove failed."));
  selectedConvertIds.clear();
  updateConvertBulkSelectionUi();
  await refreshConvert();
}

async function refreshConvert() {
  let data;
  try {
    const res = await api("/api/convert/queue");
    if (!res.ok) return;
    data = await res.json();
  } catch {
    return;
  }
  lastConvertPayload = data;
  // Keep the textarea in sync with the server unless the user is editing it.
  const input = document.getElementById("convert-input");
  if (input && document.activeElement !== input) {
    input.value = data.input_paths || "";
  }
  renderConvertEncoder(data);
  renderConvertSummary(data);
  renderConvertBatchProgress(data);
  renderNavbarStatus();

  const startBtn = document.getElementById("btn-convert-start");
  const pauseBtn = document.getElementById("btn-convert-pause");
  const resumeBtn = document.getElementById("btn-convert-resume");
  const cancelBtn = document.getElementById("btn-convert-cancel");
  const retrySkippedBtn = document.getElementById("btn-convert-retry-skipped");
  const readyCount = data.items.filter((it) => it.status === "Idle").length;
  const skippedCount = data.items.filter((it) => it.skipped).length;
  if (startBtn) startBtn.disabled = data.running || !data.has_ffmpeg || !data.has_ffprobe || readyCount === 0;
  if (pauseBtn) {
    pauseBtn.disabled = !data.running || data.paused;
    pauseBtn.title = data.running && !data.paused ? "Pause the running Convert batch" : "Convert batch is not running or already paused";
  }
  if (resumeBtn) {
    resumeBtn.disabled = !data.running || !data.paused;
    resumeBtn.title = data.running && data.paused ? "Resume the paused Convert batch" : "Convert batch is not paused";
  }
  if (cancelBtn) {
    cancelBtn.disabled = !data.running;
    cancelBtn.title = data.running
      ? "Cancel the running Convert batch"
      : "No Convert batch is running";
  }
  if (retrySkippedBtn) {
    retrySkippedBtn.disabled = data.running || skippedCount === 0;
    retrySkippedBtn.title =
      skippedCount > 0
        ? `Reset ${skippedCount} skipped item(s) to ready (adjust size limit settings first if needed)`
        : "No skipped items";
  }

  const root = document.getElementById("convert-queue");
  if (!root) return;
  const settings = cachedSettings || {};
  const showThumbnails = settings.show_thumbnails !== false;
  const searchQuery = getConvertSearchQuery();
  const filteredItems = (data.items || []).filter((it) =>
    convertItemMatchesSearch(it, searchQuery),
  );
  const outerH = queueOuterScrollPx("convert-queue");
  const listLayout = effectiveListLayout(settings, data.items.length, outerH, true);
  pruneconvertThumbKeys(data.items);
  root.className = "queue" + (listLayout ? " list-layout" : "");
  root.innerHTML = "";
  if (!data.items.length) {
    const empty = document.createElement("p");
    empty.className = "hint convert-empty";
    empty.textContent = "Nothing here yet. Add file or folder paths above, then Scan inputs.";
    root.appendChild(empty);
    updateConvertBulkSelectionUi();
    return;
  }
  if (!filteredItems.length) {
    const empty = document.createElement("p");
    empty.className = "hint convert-empty";
    empty.textContent = "No convert items match the current search.";
    root.appendChild(empty);
    updateConvertBulkSelectionUi();
    return;
  }
  const readyItems = filteredItems
    .filter((it) => convertGroup(it) === "Ready")
    .sort((a, b) => (a.sort_order || a.item_id || 0) - (b.sort_order || b.item_id || 0));
  const cardCtx = { readyItems };
  renderGroupedQueue(root, filteredItems, {
    settings: { ...settings, card_list_layout: listLayout, show_thumbnails: showThumbnails },
    groupFn: convertGroup,
    groupOrder: CONVERT_QUEUE_GROUPS,
    outerScrollPx: outerH,
    renderItem: (item, s) => renderConvertCard(item, s, cardCtx),
    defaultOpenCtx: {},
    mode: "cv",
    defaultOpenFn: (label) => convertQueueGroupDefaultOpen(label),
    sortGroupItems: null,
  });
  updateConvertBulkSelectionUi();
}

async function convertScan() {
  const input = document.getElementById("convert-input");
  const paths = (input ? input.value : "")
    .split(/\n+/)
    .map((s) => s.trim())
    .filter(Boolean);
  if (!paths.length) return;
  await postAction("/api/convert/scan", "Convert scan failed.", {
    body: JSON.stringify({ paths }),
  });
  await refreshConvert();
}

async function convertStart() {
  await postAction("/api/convert/start", "Convert batch could not start.");
  await refreshConvert();
}

async function convertCancel() {
  await api("/api/convert/cancel", { method: "POST" });
  await refreshConvert();
}

async function convertPause() {
  await api("/api/convert/pause", { method: "POST" });
  await refreshConvert();
}

async function convertResume() {
  await api("/api/convert/resume", { method: "POST" });
  await refreshConvert();
}

async function convertClear() {
  if (!(await showConfirmDialog("Clear the entire Convert queue?", "Clear Convert queue"))) return;
  await api("/api/convert/clear", { method: "POST" });
  await refreshConvert();
}

async function convertRetrySkipped() {
  await api("/api/convert/retry-skipped", { method: "POST" });
  await refreshConvert();
}

async function profileDelete() {
  const name = document.getElementById("set-active-profile")?.value;
  if (!name) return;
  if (!(await showConfirmDialog(`Delete profile "${name}"?`, "Delete profile"))) return;
  const res = await api("/api/profiles/delete", {
    method: "POST",
    body: JSON.stringify({ name }),
  });
  if (!res.ok) throw new Error(await readApiError(res, "Could not delete profile."));
  await reloadProfilesAndSettings();
}

async function profileRename() {
  const oldName = document.getElementById("set-active-profile")?.value;
  const newName = await showPromptDialog("Enter a new name for this profile.", oldName, "Rename profile");
  if (!newName || !oldName || newName.trim() === oldName) return;
  const res = await api("/api/profiles/rename", {
    method: "POST",
    body: JSON.stringify({ old_name: oldName, new_name: newName.trim() }),
  });
  if (!res.ok) throw new Error(await readApiError(res, "Could not rename profile."));
  await reloadProfilesAndSettings();
}

async function profileSaveAs() {
  const name = await showPromptDialog("Save current settings as a new profile.", "", "Save profile");
  if (!name || !name.trim()) return;
  const res = await api("/api/profiles/save", {
    method: "POST",
    body: JSON.stringify({ name: name.trim() }),
  });
  if (!res.ok) throw new Error(await readApiError(res, "Could not save profile."));
  await reloadProfilesAndSettings();
}

async function profileExport() {
  const res = await api("/api/profiles/export");
  if (!res.ok) throw new Error("Export failed.");
  const blob = await res.blob();
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = "rustdl_profiles.json";
  a.click();
  URL.revokeObjectURL(url);
}

async function profileImport() {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = "application/json,.json";
  input.onchange = async () => {
    const file = input.files?.[0];
    if (!file) return;
    const text = await file.text();
    const res = await api("/api/profiles/import", {
      method: "POST",
      body: text,
      headers: { "Content-Type": "application/json" },
    });
    if (!res.ok) throw new Error(await readApiError(res, "Import failed."));
    await reloadProfilesAndSettings();
  };
  input.click();
}

async function reloadProfilesAndSettings() {
  const [settingsRes, profilesRes] = await Promise.all([
    api("/api/settings"),
    api("/api/profiles"),
  ]);
  const settingsData = await settingsRes.json();
  cachedSettings = settingsData.settings;
  populateSettingsForm(settingsData.settings, settingsData.command_preview);
  populateProfiles(await profilesRes.json());
  await refreshAll();
}

function updateBulkSelectionUi() {
  const n = selectedQueueIds.size;
  const removeBtn = document.getElementById("btn-bulk-remove");
  const retryBtn = document.getElementById("btn-bulk-retry");
  if (removeBtn) removeBtn.disabled = n === 0;
  if (retryBtn) retryBtn.disabled = n === 0;
}

async function bulkRemoveSelected() {
  if (!selectedQueueIds.size) return;
  if (
    !(await showConfirmDialog(
      `Remove ${selectedQueueIds.size} selected item(s) from the queue?`,
      "Remove selected"
    ))
  )
    return;
  for (const id of [...selectedQueueIds]) {
    await api(`/api/queue/${id}`, { method: "DELETE" });
  }
  selectedQueueIds.clear();
  updateBulkSelectionUi();
  await refreshAll();
}

async function bulkRetrySelected() {
  if (!selectedQueueIds.size) return;
  const res = await api("/api/queue/bulk-retry", {
    method: "POST",
    body: JSON.stringify({ item_ids: [...selectedQueueIds] }),
  });
  if (!res.ok) throw new Error("Bulk retry failed.");
  selectedQueueIds.clear();
  updateBulkSelectionUi();
  await refreshAll();
}

async function recheckAllSavedFiles() {
  const res = await api("/api/queue/recheck-saved", { method: "POST" });
  if (!res.ok) throw new Error(await readApiError(res, "Re-check failed."));
  await refreshAll();
}

function exportActivityLog() {
  const filter = currentLogFilter();
  const lines = logLinesCache.filter((l) => logFilterAccepts(l, filter));
  const blob = new Blob([lines.join("\n")], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = "rustdl_activity_log.txt";
  a.click();
  URL.revokeObjectURL(url);
}

document.querySelectorAll(".nav-btn").forEach((btn) => {
  btn.onclick = () => setView(btn.dataset.view);
});
document.getElementById("btn-convert-scan").onclick = () => convertScan().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-start").onclick = () => convertStart().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-pause").onclick = () => convertPause().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-resume").onclick = () => convertResume().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-cancel").onclick = () => convertCancel().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-clear").onclick = () => convertClear().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-export")?.addEventListener("click", async () => {
  const token = localStorage.getItem(TOKEN_KEY) || "";
  const res = await fetch("/api/convert/export-summary", {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
  });
  if (!res.ok) {
    notifyError("Export failed.");
    return;
  }
  const blob = await res.blob();
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "rustdl-convert-summary.csv";
  a.click();
  URL.revokeObjectURL(a.href);
  showToast("Convert summary exported.");
});
document.getElementById("btn-convert-fallback")?.addEventListener("click", () =>
  api("/api/convert/fallback-software", { method: "POST" })
    .then(() => showToast("Switched to software encoder."))
    .catch((e) => notifyError(e.message || String(e)))
);
document.getElementById("btn-convert-bulk-remove")?.addEventListener("click", () =>
  bulkRemoveConvertSelected().catch((e) => notifyError(e.message || String(e)))
);
document.getElementById("btn-convert-retry-skipped").onclick = () =>
  convertRetrySkipped().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-settings").onclick = () =>
  openSettingsDialog().then(() => switchSettingsTab("convert")).catch(console.error);
document.getElementById("btn-profile-delete")?.addEventListener("click", () =>
  profileDelete().catch((e) => alert(e.message || String(e)))
);
document.getElementById("btn-profile-rename")?.addEventListener("click", () =>
  profileRename().catch((e) => alert(e.message || String(e)))
);
document.getElementById("btn-profile-save")?.addEventListener("click", () =>
  profileSaveAs().catch((e) => alert(e.message || String(e)))
);
document.getElementById("btn-profile-export")?.addEventListener("click", () =>
  profileExport().catch((e) => alert(e.message || String(e)))
);
document.getElementById("btn-profile-import")?.addEventListener("click", () =>
  profileImport().catch((e) => alert(e.message || String(e)))
);
document.getElementById("btn-bulk-remove")?.addEventListener("click", () =>
  bulkRemoveSelected().catch((e) => alert(e.message || String(e)))
);
document.getElementById("btn-bulk-retry")?.addEventListener("click", () =>
  bulkRetrySelected().catch((e) => alert(e.message || String(e)))
);
document.getElementById("btn-recheck-files")?.addEventListener("click", () =>
  recheckAllSavedFiles().catch((e) => alert(e.message || String(e)))
);
document.getElementById("btn-export-log")?.addEventListener("click", exportActivityLog);
document.getElementById("log-filter")?.addEventListener("change", async (e) => {
  renderLogView();
  if (!cachedSettings) return;
  const patch = { ...cachedSettings, log_filter: e.target.value };
  const res = await api("/api/settings", {
    method: "POST",
    body: JSON.stringify({ patch: { log_filter: e.target.value } }),
  });
  if (res.ok) cachedSettings = patch;
});

applyStaticButtonIcons();

initWebTheme();

loadConvertPresetsRow().catch(() => {});

document.getElementById("btn-topbar-menu")?.addEventListener("click", () => {
  const menu = document.getElementById("topbar-overflow-menu");
  const btn = document.getElementById("btn-topbar-menu");
  if (!menu) return;
  menu.classList.toggle("hidden");
  if (btn) btn.setAttribute("aria-expanded", menu.classList.contains("hidden") ? "false" : "true");
});
document.getElementById("btn-refresh-tools-menu")?.addEventListener("click", () => {
  refreshToolsOnly().catch((e) => notifyError(e.message));
  document.getElementById("topbar-overflow-menu")?.classList.add("hidden");
});
document.getElementById("btn-theme-toggle-menu")?.addEventListener("click", () => {
  document.getElementById("btn-theme-toggle")?.click();
  document.getElementById("topbar-overflow-menu")?.classList.add("hidden");
});
document.getElementById("btn-settings-menu")?.addEventListener("click", () => {
  openSettingsDialog().catch(console.error);
  document.getElementById("topbar-overflow-menu")?.classList.add("hidden");
});
document.getElementById("btn-quit-menu")?.addEventListener("click", () => {
  requestAppShutdown().catch((e) => notifyError(e.message));
});

document.getElementById("btn-test-cookies")?.addEventListener("click", async () => {
  try {
    const res = await api("/api/tools/cookie-check", { method: "POST" });
    const data = await res.json();
    showToast(data.message || data.ok ? "Cookies OK." : "Cookie check finished.");
  } catch (e) {
    notifyError(e.message || "Cookie check failed.");
  }
});

document.getElementById("btn-queue-template-save")?.addEventListener("click", async () => {
  const name = document.getElementById("queue-template-name")?.value?.trim();
  if (!name) {
    notifyError("Enter a template name.");
    return;
  }
  try {
    await api("/api/queue/templates", {
      method: "POST",
      body: JSON.stringify({ name }),
    });
    showToast(`Saved template “${name}”.`);
    refreshQueueTemplatesList().catch(() => {});
  } catch (e) {
    notifyError(e.message || "Could not save template.");
  }
});

document.getElementById("btn-queue-template-load")?.addEventListener("click", async () => {
  const name = document.getElementById("queue-template-name")?.value?.trim();
  if (!name) {
    notifyError("Enter a template name to load.");
    return;
  }
  try {
    const res = await api("/api/queue/templates/load", {
      method: "POST",
      body: JSON.stringify({ name }),
    });
    const data = await res.json();
    showToast(`Loaded ${data.accepted || 0} URL(s) from template.`);
    await refreshAll();
  } catch (e) {
    notifyError(e.message || "Could not load template.");
  }
});

document.getElementById("btn-generate-web-token")?.addEventListener("click", async () => {
  if (!cachedSettings) return;
  const token = randomWebToken();
  try {
    const res = await api("/api/settings", {
      method: "POST",
      body: JSON.stringify({ patch: { web_auth_token: token } }),
    });
    const data = await res.json();
    cachedSettings = data.settings;
    const tokenEl = document.getElementById("set-web-auth-token");
    if (tokenEl) tokenEl.value = maskWebToken(cachedSettings.web_auth_token);
    showToast("New API token generated and saved.");
  } catch (e) {
    notifyError(e.message || "Could not generate token.");
  }
});

document.getElementById("btn-copy-web-token")?.addEventListener("click", async () => {
  const token = cachedSettings?.web_auth_token?.trim();
  if (!token) {
    notifyError("No token to copy.");
    return;
  }
  try {
    await navigator.clipboard.writeText(token);
    showToast("Token copied to clipboard.");
  } catch {
    notifyError("Could not copy token.");
  }
});

wireModeColorControls();
applyModeColors(cachedSettings);

document.getElementById("btn-theme-toggle")?.addEventListener("click", () => {
  const next = document.body.classList.contains("theme-light") ? "dark" : "light";
  applyWebTheme(next);
});

document.getElementById("queue-search")?.addEventListener("input", (e) => {
  refreshQueue(true).catch(console.error);
  clearTimeout(queueSearchSaveTimer);
  queueSearchSaveTimer = setTimeout(() => saveQueueSearchSetting(e.target.value), 400);
});

document.getElementById("convert-search")?.addEventListener("input", (e) => {
  refreshConvert().catch(console.error);
  clearTimeout(queueSearchSaveTimer);
  queueSearchSaveTimer = setTimeout(() => saveQueueSearchSetting(e.target.value), 400);
});

document.getElementById("library-search")?.addEventListener("input", () => {
  refreshLibrary().catch(console.error);
});

document.getElementById("library-history-filter")?.addEventListener("change", () => {
  refreshLibrary().catch(console.error);
});

const PALETTE_COMMANDS = [
  { label: "Open Settings", keywords: "settings preferences options", section: "Settings", run: () => openSettingsDialog() },
  { label: "Settings → Shared tab", keywords: "shared global theme layout", run: () => openSettingsDialog("shared") },
  { label: "Settings → Downloader tab", keywords: "download yt-dlp profile", run: () => openSettingsDialog("downloader") },
  { label: "Settings → Converter tab", keywords: "convert av1 encode video", run: () => openSettingsDialog("convert") },
  { label: "Settings → Web UI tab", keywords: "web lan api token bind", run: () => openSettingsDialog("webui") },
  { label: "Reset UI scale to 100% (host app)", keywords: "ui scale zoom reset desktop host", run: () => patchHostSettings({ ui_scale: 1.0 }) },
  { label: "Start downloads", keywords: "start run download ready", section: "Queue", run: () => postAction("/api/downloads/start", "Downloads could not start.").then(refreshAll).catch(() => {}) },
  { label: "Pause downloads", keywords: "pause hold stop", run: () => api("/api/downloads/pause", { method: "POST" }).then(refreshAll) },
  { label: "Resume downloads", keywords: "resume continue", run: () => api("/api/downloads/resume", { method: "POST" }).then(refreshAll) },
  { label: "Retry all failed", keywords: "retry failed download again", run: () => postAction("/api/downloads/retry-failed", "Could not retry failed downloads.").then(refreshAll).catch(() => {}) },
  {
    label: "Remove selected",
    keywords: "remove delete selected queue bulk",
    run: () => {
      if (currentView === "convert") {
        bulkRemoveConvertSelected().catch((e) => notifyError(e.message || String(e)));
      } else if (currentView === "downloader") {
        bulkRemoveSelected().catch((e) => notifyError(e.message || String(e)));
      } else {
        showToast("Switch to Downloader or Video Converter to remove selected queue items.");
      }
    },
  },
  { label: "Clear completed downloads", keywords: "clear done finished remove completed", run: () => clearQueue("done") },
  { label: "Start Convert batch", keywords: "convert encode start", run: () => convertStart() },
  { label: "Pause Convert batch", keywords: "convert pause hold", run: () => convertPause() },
  { label: "Resume Convert batch", keywords: "convert resume continue", run: () => convertResume() },
  { label: "Switch to Downloader", keywords: "mode download", run: () => setView("downloader") },
  { label: "Switch to Video Converter", keywords: "mode convert av1", run: () => setView("convert") },
  { label: "Switch to Library", keywords: "library done history", run: () => setView("library") },
  { label: "Focus queue search", keywords: "search find filter queue", run: () => focusActiveSearch() },
  { label: "Toggle activity log", keywords: "log show hide expand activity", run: () => toggleActivityLogExpanded() },
  { label: "Export activity log", keywords: "export log save file", run: () => exportActivityLog() },
  { label: "Layout: Compact queue", keywords: "layout compact list small", section: "Layout", run: () => applyLayoutPresetViaApi("compact") },
  { label: "Layout: Review mode", keywords: "layout review cards thumbnails", run: () => applyLayoutPresetViaApi("review") },
  { label: "Layout: Minimal", keywords: "layout minimal no thumbnails", run: () => applyLayoutPresetViaApi("minimal") },
  { label: "Dock Videos panel (host app)", keywords: "dock videos queue panel desktop host", section: "Panels", run: () => patchHostSettings({ videos_docked: true, videos_open: true }) },
  { label: "Float Videos window (host app)", keywords: "float undock videos window desktop host", run: () => patchHostSettings({ videos_docked: false, videos_open: true }) },
  { label: "Dock activity log (host app)", keywords: "dock log panel bottom desktop host", run: () => patchHostSettings({ logs_docked: true, logs_open: true }) },
  { label: "Float activity log (host app)", keywords: "float undock log window desktop host", run: () => patchHostSettings({ logs_open: true, logs_docked: false }) },
  { label: "Open About", keywords: "about version help", section: "Help", run: () => document.getElementById("about-dialog")?.showModal() },
  { label: "Refresh page data", keywords: "refresh reload sync", run: () => refreshAll() },
];

let paletteActiveIndex = 0;

function focusActiveSearch() {
  if (currentView === "convert") {
    document.getElementById("convert-search")?.focus();
  } else if (currentView === "library") {
    document.getElementById("library-search")?.focus();
  } else {
    document.getElementById("queue-search")?.focus();
  }
}

function filterPaletteCommands(query) {
  const tokens = String(query || "")
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  if (!tokens.length) return PALETTE_COMMANDS;
  return PALETTE_COMMANDS.filter((cmd) => {
    const hay = `${cmd.label} ${cmd.keywords}`.toLowerCase();
    return tokens.every((token) => hay.includes(token));
  });
}

function scrollPaletteActiveIntoView() {
  const list = document.getElementById("command-palette-list");
  list?.querySelector("li.active")?.scrollIntoView({ block: "nearest" });
}

function renderCommandPaletteList(commands, query = "") {
  const list = document.getElementById("command-palette-list");
  if (!list) return;
  list.innerHTML = "";
  if (!commands.length) {
    const li = document.createElement("li");
    li.className = "command-palette-empty";
    li.textContent = query.trim()
      ? `No commands match "${query.trim()}"`
      : "No commands available";
    list.appendChild(li);
    return;
  }
  let lastSection = null;
  commands.forEach((cmd, idx) => {
    if (cmd.section && cmd.section !== lastSection) {
      const heading = document.createElement("li");
      heading.className = "command-palette-section";
      heading.textContent = cmd.section;
      list.appendChild(heading);
      lastSection = cmd.section;
    }
    const li = document.createElement("li");
    li.textContent = cmd.label;
    li.dataset.index = String(idx);
    li.classList.toggle("active", idx === paletteActiveIndex);
    li.addEventListener("mousedown", (e) => {
      e.preventDefault();
      runPaletteCommand(cmd);
    });
    list.appendChild(li);
  });
  scrollPaletteActiveIntoView();
}

function openCommandPalette() {
  const root = document.getElementById("command-palette");
  const input = document.getElementById("command-palette-input");
  if (!root || !input) return;
  paletteActiveIndex = 0;
  input.value = "";
  renderCommandPaletteList(PALETTE_COMMANDS);
  root.classList.remove("hidden");
  input.focus();
}

function closeCommandPalette() {
  document.getElementById("command-palette")?.classList.add("hidden");
}

async function runPaletteCommand(cmd) {
  closeCommandPalette();
  try {
    await cmd.run();
  } catch (err) {
    notifyError(err.message || String(err));
  }
}

document.getElementById("command-palette-backdrop")?.addEventListener("click", closeCommandPalette);

document.getElementById("command-palette-input")?.addEventListener("input", (e) => {
  paletteActiveIndex = 0;
  renderCommandPaletteList(filterPaletteCommands(e.target.value), e.target.value);
});

document.getElementById("command-palette-input")?.addEventListener("keydown", (e) => {
  const commands = filterPaletteCommands(e.target.value);
  if (e.key === "Escape") {
    e.preventDefault();
    closeCommandPalette();
    return;
  }
  if (e.key === "ArrowDown") {
    e.preventDefault();
    paletteActiveIndex = Math.min(paletteActiveIndex + 1, Math.max(0, commands.length - 1));
    renderCommandPaletteList(commands, e.target.value);
    return;
  }
  if (e.key === "ArrowUp") {
    e.preventDefault();
    paletteActiveIndex = Math.max(paletteActiveIndex - 1, 0);
    renderCommandPaletteList(commands, e.target.value);
    return;
  }
  if (e.key === "Enter") {
    e.preventDefault();
    const cmd = commands[paletteActiveIndex];
    if (cmd) runPaletteCommand(cmd);
  }
});

document.getElementById("btn-expand-log")?.addEventListener("click", () => {
  toggleActivityLogExpanded();
});

document.querySelectorAll(".layout-preset-btn").forEach((btn) => {
  btn.addEventListener("click", async () => {
    if (!cachedSettings) return;
    const patch = { ...cachedSettings };
    applyLayoutPreset(patch, btn.dataset.preset);
    const res = await api("/api/settings", {
      method: "POST",
      body: JSON.stringify({ settings: patch }),
    });
    if (res.ok) {
      const data = await res.json();
      cachedSettings = data.settings;
      await refreshAll();
    }
  });
});

document.addEventListener("keydown", (e) => {
  maybeRequestNotificationPermissionOnGesture();
  if (document.getElementById("app-main")?.classList.contains("hidden")) return;
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key === "k") {
    e.preventDefault();
    openCommandPalette();
  } else if (mod && e.key === ",") {
    e.preventDefault();
    openSettingsDialog().catch(console.error);
  } else if (mod && e.key === "Enter") {
    e.preventDefault();
    clearTimeout(autoAddTimer);
    flushAutoAddFromInput().catch(console.error);
  } else if (mod && e.key === "d") {
    e.preventDefault();
    postAction("/api/downloads/start", "Downloads could not start.")
      .then(refreshAll)
      .catch(() => {});
  } else if (mod && e.key === "f") {
    e.preventDefault();
    focusActiveSearch();
  } else if (mod && e.key === "l") {
    e.preventDefault();
    toggleActivityLogExpanded();
  } else if (e.key === "Escape") {
    if (!document.getElementById("command-palette")?.classList.contains("hidden")) {
      closeCommandPalette();
      return;
    }
    document.getElementById("settings-dialog")?.close();
    document.getElementById("about-dialog")?.close();
  }
});

document.addEventListener("click", () => {
  maybeRequestNotificationPermissionOnGesture();
}, { once: false });

document.getElementById("btn-playlist-preview-cancel")?.addEventListener("click", () => {
  document.getElementById("playlist-preview-dialog")?.close();
});

document.getElementById("import-urls-form")?.addEventListener("submit", async (e) => {
  e.preventDefault();
  document.getElementById("import-urls-dialog")?.close();
  try {
    await submitImportUrlsFromDialog();
  } catch (err) {
    notifyError(err.message || String(err));
  }
});

document.getElementById("btn-import-urls-cancel")?.addEventListener("click", () => {
  document.getElementById("import-urls-dialog")?.close();
});

document.body.classList.add("view-downloader");

async function tryConnectWithoutToken() {
  try {
    const res = await fetch("/api/status");
    if (!res.ok) return false;
    ipAuthBypass = true;
    showApp();
    refreshAll().catch(() => {});
    connectSse();
    startStatusPoll();
    startFallbackPolling();
    return true;
  } catch {
    return false;
  }
}

async function bootstrapAuth() {
  const urlToken = tokenFromPageUrl();
  if (urlToken) {
    stripTokenFromPageUrl();
    const input = document.getElementById("token-input");
    if (input) input.value = urlToken;
    if (await saveTokenFromForm()) {
      const savedView = localStorage.getItem(VIEW_STORAGE_KEY);
      if (savedView === "convert" || savedView === "library") setView(savedView);
      return;
    }
  }

  const saved = token();
  if (saved) {
    const input = document.getElementById("token-input");
    if (input) input.value = saved;
    if (await saveTokenFromForm()) {
      const savedView = localStorage.getItem(VIEW_STORAGE_KEY);
      if (savedView === "convert" || savedView === "library") setView(savedView);
      return;
    }
  }

  if (!(await tryConnectWithoutToken())) {
    showAuthPanel();
  }
}

bootstrapAuth().catch(() => showAuthPanel());

document
  .getElementById("set-convert-size-limit-kind")
  ?.addEventListener("change", updateConvertSizeLimitFieldsVisibility);
