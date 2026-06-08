const TOKEN_KEY = "rustdl_web_token";
const WEB_THEME_KEY = "rustdl_web_theme";

let cachedSettings = null;
let logLinesCache = [];
/** @type {string | null} */
let queueStatusFilter = null;
let queueSearchSaveTimer = null;
let logExpanded = false;

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

function renderLogView() {
  const log = document.getElementById("log-view");
  if (!log) return;
  const relative = !!(cachedSettings || {}).log_relative_time;
  const atBottom = log.scrollHeight - log.scrollTop - log.clientHeight < 24;
  log.textContent = logLinesCache.map((l) => formatLogLineDisplay(l, relative)).join("\n");
  if (shouldAutoscrollLog() || atBottom) {
    log.scrollTop = log.scrollHeight;
  }
}

function applyWebTheme(theme) {
  const t = theme === "light" ? "light" : "dark";
  document.body.classList.toggle("theme-light", t === "light");
  localStorage.setItem(WEB_THEME_KEY, t);
  const btn = document.getElementById("btn-theme-toggle");
  if (btn) btn.textContent = t === "light" ? "Dark theme" : "Light theme";
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

async function saveQueueSearchSetting(value) {
  if (!cachedSettings) return;
  const patch = { ...cachedSettings, queue_search: value };
  try {
    const res = await api("/api/settings", {
      method: "POST",
      body: JSON.stringify({ settings: patch }),
    });
    if (res.ok) {
      const data = await res.json();
      cachedSettings = data.settings;
    }
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
  }, delayMs);
}
/** @type {object | null} */
let lastStatusPayload = null;
/** @type {object | null} */
let lastConvertPayload = null;

let cachedHasYtDlp = false;

/** Last queue generation from `/api/queue` (skip rebuild when unchanged). */
let lastQueueGeneration = 0;
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
  if (reason === "no_token" || !token()) {
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
  if (reason === "no_token" || !token()) {
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
  renderNavbarDiskSpace(data.output_disk_space);
  updateTopbarVersion(data);
  updateSettingsOutputDiskHint(data.output_disk_space);
  updateQuitButtonState();
  renderTools(data.tools);
  renderConfigWarnings(data.config_warnings);
  cachedHasYtDlp = data.tools?.yt_dlp?.ok === true;
  updateDownloadControlButtons(data);
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
      pulse: false,
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
  root.className =
    "navbar-status navbar-status-" +
    info.slug +
    (info.pulse ? " navbar-status-pulse" : "");
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
  if (!confirm(msg)) return;
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

function updateSettingsOutputDiskHint(disk) {
  const el = document.getElementById("settings-output-disk");
  if (!el) return;
  if (!disk || disk.total_bytes == null) {
    el.classList.add("hidden");
    el.innerHTML = "";
    return;
  }
  const vol = disk.volume_label ? ` (${disk.volume_label})` : "";
  el.innerHTML = `Destination disk${vol}: ${diskSpaceFreeHtml(disk)} / ${formatBytes(
    disk.total_bytes
  )} total${diskSpaceBarHtml(disk)}`;
  el.classList.remove("hidden");
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
  el.innerHTML = `<span class="status-dot" aria-hidden="true"></span>Disk${vol}: ${diskSpaceFreeHtml(
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
  return `/api/media/${itemId}?token=${encodeURIComponent(token())}`;
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
  const t = token();
  if (!t) return null;
  return `/api/thumbnail/${itemId}?token=${encodeURIComponent(t)}`;
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

  if (!showThumbnails || !itemHasThumbnailSource(item)) {
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
  trigger.className = "btn-menu-trigger secondary";
  trigger.title = "Remove from queue or delete the saved file";
  setButtonLabel(trigger, ICON.remove, "Remove...");
  menu.appendChild(trigger);

  const panel = document.createElement("div");
  panel.className = "btn-menu-panel";
  panel.setAttribute("role", "menu");

  const removeBtn = document.createElement("button");
  removeBtn.type = "button";
  removeBtn.className = "btn-menu-item";
  removeBtn.textContent = "Remove from queue";
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
    deleteBtn.textContent = "Delete file";
    deleteBtn.title =
      "Delete the downloaded file on disk. The queue row stays until you remove it.";
    deleteBtn.onclick = (e) => {
      e.preventDefault();
      menu.open = false;
      const name = item.media_filename || "this file";
      if (!confirm(`Delete ${name} from the output folder?`)) return;
      deleteQueueItemFile(item.item_id).catch((err) =>
        alert(err.message || String(err))
      );
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

function renderQueueCard(item, settings) {
  const s = settings || {};
  const showThumbnails = s.show_thumbnails !== false;
  const compact = !!s.compact_cards;
  const hideSubtitle = !!s.hide_card_subtitle;
  const slug = statusSlug(item.status);
  const highlightDone = slug === "done" && !item.error;

  const card = document.createElement("article");
  card.className = "card" + (compact ? " compact" : "") + (highlightDone ? " card-done-highlight" : "");
  card.dataset.itemId = String(item.item_id);

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
  if (canCancel(item)) {
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "warning";
    setButtonLabel(cancel, ICON.stop, "Cancel");
    cancel.onclick = () => cancelItem(item.item_id);
    group.appendChild(cancel);
  }
  appendRedownloadButton(group, item);
  appendRemoveMenuButton(group, item);
  if (group.childElementCount > 0) {
    card.appendChild(actions);
  }

  return card;
}

function renderQueueCardListRow(item, settings) {
  const s = settings || {};
  const showThumbnails = s.show_thumbnails !== false;
  const slug = statusSlug(item.status);
  const card = document.createElement("article");
  card.className = "card";
  card.dataset.itemId = String(item.item_id);

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
  if (slug === "downloading" || slug === "queued") {
    const pct = document.createElement("span");
    pct.className = "card-footer";
    pct.textContent = ` ${item.percent.toFixed(0)}%`;
    body.appendChild(pct);
  }
  card.appendChild(body);

  const { bar: actions, group } = createCardActionBar();
  appendPlayButton(group, item, thumb);
  if (canCancel(item)) {
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "warning";
    setButtonLabel(cancel, ICON.stop, "Cancel");
    cancel.onclick = () => cancelItem(item.item_id);
    group.appendChild(cancel);
  }
  appendRedownloadButton(group, item);
  appendRemoveMenuButton(group, item);
  if (group.childElementCount > 0) {
    card.appendChild(actions);
  }

  return card;
}

function findQueueCard(itemId) {
  return document.querySelector(`#queue [data-item-id="${itemId}"]`);
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
  root.className = "queue" + (settings.card_list_layout ? " list-layout" : "");
  root.innerHTML = "";
  const searchInput = document.getElementById("queue-search");
  const searchQuery = searchInput ? searchInput.value : settings.queue_search || "";
  const items = (data.items || []).filter(
    (item) => itemMatchesSearch(item, searchQuery) && itemMatchesStatusFilter(item, queueStatusFilter)
  );
  if (!items.length) {
    const empty = document.createElement("p");
    empty.className = "hint";
    empty.textContent = searchQuery || queueStatusFilter
      ? "No queue items match the current search or filter."
      : "Queue is empty. Add URLs above.";
    root.appendChild(empty);
    return;
  }
  for (const item of items) {
    const card = settings.card_list_layout
      ? renderQueueCardListRow(item, settings)
      : renderQueueCard(item, settings);
    root.appendChild(card);
  }
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
  if (confirmMessage && !confirm(confirmMessage)) return;
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
  if (!t) return;
  const es = new EventSource(`/api/events?token=${encodeURIComponent(t)}`);
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

function populateSettingsForm(s, commandPreview) {
  setCheck("set-show-thumbnails", s.show_thumbnails);
  setCheck("set-compact-cards", s.compact_cards);
  setCheck("set-hide-subtitle", s.hide_card_subtitle);
  setCheck("set-card-list", s.card_list_layout);
  setCheck("set-autoscroll-log", s.autoscroll_log);
  setCheck("set-log-relative", s.log_relative_time);
  setVal("set-log-max", s.log_max_chars);
  setVal("set-ffmpeg-path", s.ffmpeg_path);
  setVal("set-ffprobe-path", s.ffprobe_path);
  const qs = document.getElementById("queue-search");
  if (qs && document.activeElement !== qs) {
    qs.value = s.queue_search || "";
  }

  setCheck("set-auto-add", s.auto_add_pasted_urls);
  setCheck("set-auto-start", s.auto_start_downloads);
  setCheck("set-enqueue-convert", s.enqueue_downloads_to_convert);
  setVal("set-workers", s.worker_count);
  setVal("set-output-dir", s.output_dir);
  setVal("set-yt-dlp-path", s.yt_dlp_path);

  setVal("set-output-template", s.output_filename_template);
  setVal("set-quality", s.quality_preset);
  setVal("set-quality-custom", s.quality_format_custom);
  setVal("set-merge-container", s.merge_container);
  setVal("set-playlist-cap", s.playlist_preview_cap);
  setVal("set-download-archive", s.yt_download_archive);
  setVal("set-proxy", s.yt_proxy);
  setVal("set-limit-rate", s.yt_limit_rate);
  setCheck("set-sponsorblock-remove", s.yt_sponsorblock_remove);
  setVal("set-sponsorblock-mark", s.yt_sponsorblock_mark);

  setCheck("set-unlimited-retries", s.yt_dlp_unlimited_retries);
  setVal("set-retry-count", s.yt_dlp_retry_count);
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
  setVal("set-convert-min-shrink", s.convert_min_shrink_percent);
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

  document.getElementById("command-preview").textContent = commandPreview || "";
  updateQualityCustomVisibility();
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
  s.ffmpeg_path = document.getElementById("set-ffmpeg-path").value;
  s.ffprobe_path = document.getElementById("set-ffprobe-path").value;

  s.auto_add_pasted_urls = document.getElementById("set-auto-add").checked;
  s.auto_start_downloads = document.getElementById("set-auto-start").checked;
  s.enqueue_downloads_to_convert = document.getElementById("set-enqueue-convert").checked;
  s.worker_count = parseInt(document.getElementById("set-workers").value, 10) || 3;
  s.output_dir = document.getElementById("set-output-dir").value;
  s.yt_dlp_path = document.getElementById("set-yt-dlp-path").value;
  s.active_profile = document.getElementById("set-active-profile").value;

  s.output_filename_template = document.getElementById("set-output-template").value;
  s.quality_preset = document.getElementById("set-quality").value;
  s.quality_format_custom = document.getElementById("set-quality-custom").value;
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
  s.convert_min_shrink_percent =
    parseFloat(document.getElementById("set-convert-min-shrink").value) || 0;
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

  if (s.ffmpeg_extract_audio_mp3) s.ffmpeg_remux_mp4 = false;
  s.convert_max_width = Math.min(7680, Math.max(320, s.convert_max_width));
  s.convert_min_shrink_percent = Math.min(95, Math.max(0, s.convert_min_shrink_percent));
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
}

async function openSettingsDialog() {
  const [settingsRes, profilesRes] = await Promise.all([
    api("/api/settings"),
    api("/api/profiles"),
  ]);
  const settingsData = await settingsRes.json();
  const profilesData = await profilesRes.json();
  cachedSettings = settingsData.settings;
  populateSettingsForm(cachedSettings, settingsData.command_preview);
  populateProfiles(profilesData);
  document.getElementById("settings-dialog").showModal();
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

function saveTokenFromForm() {
  const v = document.getElementById("token-input").value.trim();
  if (!v) return;
  localStorage.setItem(TOKEN_KEY, v);
  clearThumbnailCaches();
  document.getElementById("auth-status").textContent = "Token saved.";
  showApp();
  refreshAll().catch((e) => {
    document.getElementById("auth-status").textContent =
      e instanceof Error ? e.message : String(e);
  });
  connectSse();
}

document.getElementById("auth-form").addEventListener("submit", (e) => {
  e.preventDefault();
  saveTokenFromForm();
});

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

document.getElementById("btn-clear-url-input").onclick = () => clearUrlInput();

const queueClearMount = document.getElementById("queue-clear-menu");
if (queueClearMount) mountClearQueueMenu(queueClearMount);

document.getElementById("btn-clear-log").onclick = () =>
  clearActivityLog().catch((e) => alert(e.message || String(e)));

document.getElementById("url-input").addEventListener("input", scheduleAutoAddFromInput);

document.getElementById("btn-start").onclick = async () => {
  await api("/api/downloads/start", { method: "POST" });
  await refreshAll();
};
document.getElementById("btn-pause").onclick = async () => {
  await api("/api/downloads/pause", { method: "POST" });
  await refreshAll();
};
document.getElementById("btn-resume").onclick = async () => {
  await api("/api/downloads/resume", { method: "POST" });
  await refreshAll();
};

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
  if (!cachedSettings) return;
  const wasThumbnails = cachedSettings.show_thumbnails !== false;
  const patch = collectSettingsForm(cachedSettings);
  const res = await api("/api/settings", { method: "POST", body: JSON.stringify({ settings: patch }) });
  if (res.ok) {
    const data = await res.json();
    cachedSettings = data.settings;
  } else {
    cachedSettings = patch;
  }
  statusFlags.auto_add_pasted_urls = !!cachedSettings.auto_add_pasted_urls;
  if (cachedSettings.show_thumbnails && !wasThumbnails) {
    clearThumbnailCaches();
  }
  renderLogView();
  document.getElementById("settings-dialog").close();
  await refreshAll();
};

document.querySelectorAll(".settings-tab").forEach((btn) => {
  btn.onclick = () => switchSettingsTab(btn.dataset.tab);
});

document.getElementById("set-quality").onchange = updateQualityCustomVisibility;

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
  currentView = view === "convert" ? "convert" : "downloader";
  document.body.classList.remove("view-downloader", "view-convert");
  document.body.classList.add(
    currentview === "convert" ? "view-convert" : "view-downloader"
  );
  document.querySelectorAll(".nav-btn").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.view === currentView);
  });
  document.getElementById("downloader-main").classList.toggle("hidden", currentView !== "downloader");
  document.getElementById("convert-main").classList.toggle("hidden", currentView !== "convert");
  const dlActions = document.getElementById("downloader-only-actions");
  if (dlActions) dlActions.classList.toggle("hidden", currentView !== "downloader");
  if (currentview === "convert") refreshConvert().catch(() => {});
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
  const t = token();
  if (!t) return null;
  return `/api/convert/thumbnail/${itemId}?token=${encodeURIComponent(t)}`;
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

function renderConvertCard(item, showThumbnails) {
  const slug = convertSlug(item);
  const active = slug === "downloading" || slug === "queued";
  const card = document.createElement("article");
  card.className = "card" + (item.will_skip_target ? " convert-will-skip" : "");

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
  body.appendChild(ConvertMediaBadges(item));

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

  const chipWrap = document.createElement("div");
  chipWrap.className = "card-actions";
  const chip = document.createElement("span");
  setStatusChip(chip, slug, item.status_label || item.status || "");
  chipWrap.appendChild(chip);
  card.appendChild(chipWrap);

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
  const cancelBtn = document.getElementById("btn-convert-cancel");
  const retrySkippedBtn = document.getElementById("btn-convert-retry-skipped");
  const readyCount = data.items.filter((it) => it.status === "Idle").length;
  const skippedCount = data.items.filter((it) => it.skipped).length;
  if (startBtn) startBtn.disabled = data.running || !data.has_ffmpeg || !data.has_ffprobe || readyCount === 0;
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
        ? `Reset ${skippedCount} skipped item(s) to ready (adjust Min shrink % first if needed)`
        : "No skipped items";
  }

  const root = document.getElementById("convert-queue");
  if (!root) return;
  const showThumbnails = (cachedSettings || {}).show_thumbnails !== false;
  pruneconvertThumbKeys(data.items);
  root.innerHTML = "";
  if (!data.items.length) {
    const empty = document.createElement("p");
    empty.className = "hint convert-empty";
    empty.textContent = "Nothing here yet. Add file or folder paths above, then Scan inputs.";
    root.appendChild(empty);
    return;
  }
  for (const label of ["Active", "Ready", "Failed", "Skipped", "Done"]) {
    const group = data.items.filter((it) => convertGroup(it) === label);
    if (!group.length) continue;
    const header = document.createElement("h3");
    header.className = "convert-group-header";
    header.textContent = `${label} (${group.length})`;
    root.appendChild(header);
    for (const item of group) {
      root.appendChild(renderConvertCard(item, showThumbnails));
    }
  }
}

async function convertScan() {
  const input = document.getElementById("convert-input");
  const paths = (input ? input.value : "")
    .split(/\n+/)
    .map((s) => s.trim())
    .filter(Boolean);
  if (!paths.length) return;
  await api("/api/convert/scan", { method: "POST", body: JSON.stringify({ paths }) });
  await refreshConvert();
}

async function convertStart() {
  await api("/api/convert/start", { method: "POST" });
  await refreshConvert();
}

async function convertCancel() {
  await api("/api/convert/cancel", { method: "POST" });
  await refreshConvert();
}

async function convertClear() {
  if (!confirm("Clear the entire Convert queue?")) return;
  await api("/api/convert/clear", { method: "POST" });
  await refreshConvert();
}

async function convertRetrySkipped() {
  await api("/api/convert/retry-skipped", { method: "POST" });
  await refreshConvert();
}

document.querySelectorAll(".nav-btn").forEach((btn) => {
  btn.onclick = () => setView(btn.dataset.view);
});
document.getElementById("btn-convert-scan").onclick = () => convertScan().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-start").onclick = () => convertStart().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-cancel").onclick = () => convertCancel().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-clear").onclick = () => convertClear().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-retry-skipped").onclick = () =>
  convertRetrySkipped().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-convert-settings").onclick = () =>
  openSettingsDialog().then(() => switchSettingsTab("convert")).catch(console.error);

applyStaticButtonIcons();

initWebTheme();

document.getElementById("btn-theme-toggle")?.addEventListener("click", () => {
  const next = document.body.classList.contains("theme-light") ? "dark" : "light";
  applyWebTheme(next);
});

document.getElementById("queue-search")?.addEventListener("input", (e) => {
  refreshQueue(true).catch(console.error);
  clearTimeout(queueSearchSaveTimer);
  queueSearchSaveTimer = setTimeout(() => saveQueueSearchSetting(e.target.value), 400);
});

document.getElementById("btn-expand-log")?.addEventListener("click", () => {
  logExpanded = !logExpanded;
  document.getElementById("log-view")?.classList.toggle("log-expanded", logExpanded);
  const btn = document.getElementById("btn-expand-log");
  if (btn) btn.textContent = logExpanded ? "Collapse log" : "Expand log";
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
  if (document.getElementById("app-main")?.classList.contains("hidden")) return;
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key === ",") {
    e.preventDefault();
    openSettingsDialog().catch(console.error);
  } else if (mod && e.key === "Enter") {
    e.preventDefault();
    clearTimeout(autoAddTimer);
    flushAutoAddFromInput().catch(console.error);
  } else if (mod && e.key === "d") {
    e.preventDefault();
    api("/api/downloads/start", { method: "POST" }).then(refreshAll).catch(console.error);
  } else if (mod && e.key === "f") {
    e.preventDefault();
    document.getElementById("queue-search")?.focus();
  } else if (mod && e.key === "l") {
    e.preventDefault();
    logExpanded = !logExpanded;
    document.getElementById("log-view")?.classList.toggle("log-expanded", logExpanded);
  } else if (e.key === "Escape") {
    document.getElementById("settings-dialog")?.close();
    document.getElementById("about-dialog")?.close();
  }
});

document.body.classList.add("view-downloader");

if (token()) {
  document.getElementById("token-input").value = token();
  showApp();
  refreshAll().catch(() => {});
  connectSse();
  startFallbackPolling();
}
