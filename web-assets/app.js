const TOKEN_KEY = "rustdl_web_token";

let cachedSettings = null;

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
/** @type {object | null} */
let lastStatusPayload = null;
/** @type {object | null} */
let lastAv1Payload = null;

let cachedHasYtDlp = false;

/** Skip re-fetching thumbnails that already returned 404 until item metadata changes. */
const thumbFailedKeys = new Set();
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

async function api(path, options = {}) {
  const res = await fetch(path, {
    ...options,
    headers: { ...headers(), ...(options.headers || {}) },
  });
  if (res.status === 401) {
    document.getElementById("auth-panel").classList.remove("hidden");
    document.getElementById("app-main").classList.add("hidden");
    throw new Error(
      "Token rejected. Copy the current API token from rustdl Settings → Shared (LAN web UI), paste it below, then click Save token."
    );
  }
  return res;
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
  updateSettingsOutputDiskHint(data.output_disk_space);
  updateQuitButtonState();
  renderTools(data.tools);
  cachedHasYtDlp = data.tools?.yt_dlp?.ok === true;
  updateDownloadControlButtons(data);
}

/**
 * @returns {{ slug: string, label: string, pulse: boolean, title: string }}
 */
function deriveNavbarStatus(statusData, av1Data) {
  const s = statusData?.status || {};
  const resolving = s.resolving || 0;
  const queued = s.queued || 0;
  const active = s.active || 0;
  const ready = s.ready || 0;
  const paused = !!statusData?.downloads_paused;
  const queueRunning = statusData?.queue_running ?? 0;
  const av1Running = !!statusData?.av1_running || !!av1Data?.running;
  const av1Resolving =
    av1Data?.items?.some(
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
  if (av1Running) {
    return {
      slug: "converting",
      label: "Converting",
      pulse: true,
      title: "AV1 batch encode in progress",
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
  if (resolving > 0 || av1Resolving) {
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
  const info = deriveNavbarStatus(lastStatusPayload, lastAv1Payload);
  root.className =
    "navbar-status navbar-status-" +
    info.slug +
    (info.pulse ? " navbar-status-pulse" : "");
  root.title = info.title;
  const label = root.querySelector(".navbar-status-label");
  if (label) label.textContent = info.label;
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
  const shuttingDown = statusFlags.shutdown_pending || shuttingDown;

  const canPause = !paused && (queued > 0 || active > 0);
  const canResume = paused;
  const canStart = !paused && ready > 0 && cachedHasYtDlp;

  pauseBtn.disabled = shuttingDown || !canPause;
  resumeBtn.disabled = shuttingDown || !canResume;
  startBtn.disabled = shuttingDown || !canStart;

  pauseBtn.title = shuttingDown
    ? "Unavailable while shutting down"
    : canPause
      ? "Pause active and queued downloads"
      : paused
        ? "Downloads are already paused"
        : "No queued or active downloads to pause";

  resumeBtn.title = shuttingDown
    ? "Unavailable while shutting down"
    : canResume
      ? "Resume downloads and start ready items"
      : "Downloads are not paused";

  startBtn.title = shuttingDown
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
    el.className = `status-badge status-${slug}`;
    el.innerHTML = `<span class="status-dot" aria-hidden="true"></span>${count} ${label}`;
    root.appendChild(el);
  }

  appendOutputDiskSpaceBadge(root, data.output_disk_space);
}

function diskSpaceLevel(disk) {
  return disk?.level || "ok";
}

function diskSpaceFreeHtml(disk) {
  const level = diskSpaceLevel(disk);
  const free = formatBytes(disk.available_bytes);
  return `<span class="disk-space-free disk-space-free-${level}">${free} free</span>`;
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
  const pct =
    disk.percent_free != null && isFinite(disk.percent_free)
      ? ` · <span class="disk-space-pct disk-space-pct-${diskSpaceLevel(disk)}">${Math.round(
          disk.percent_free
        )}% free</span>`
      : "";
  el.innerHTML = `Destination disk${vol}: ${diskSpaceFreeHtml(disk)} / ${formatBytes(
    disk.total_bytes
  )} total${pct}`;
  el.classList.remove("hidden");
}

function appendOutputDiskSpaceBadge(root, disk) {
  if (!disk || disk.total_bytes == null) return;
  const level = diskSpaceLevel(disk);
  const el = document.createElement("span");
  el.className = `status-badge disk-space disk-space-${level}`;
  const vol = disk.volume_label ? ` (${disk.volume_label})` : "";
  const pct =
    disk.percent_free != null && isFinite(disk.percent_free)
      ? ` · <span class="disk-space-pct disk-space-pct-${level}">${Math.round(
          disk.percent_free
        )}% free</span>`
      : "";
  el.title = "Free and total space on the output folder volume";
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
  const line = item.source_line || item.webpage_url || "";
  return /youtu\.be\/|youtube\.com\/watch|youtube\.com\/shorts/i.test(line);
}

function thumbCacheKey(item) {
  return [
    item.item_id,
    item.video_id || "",
    item.thumbnail_url || "",
    item.source_line || "",
    item.webpage_url || "",
  ].join("|");
}

function revokeThumbBlob(cacheKey) {
  const url = thumbBlobCache.get(cacheKey);
  if (url) {
    URL.revokeObjectURL(url);
    thumbBlobCache.delete(cacheKey);
  }
}

function pruneThumbFailedKeys(activeItems) {
  const active = new Set(activeItems.map((item) => thumbCacheKey(item)));
  for (const key of thumbFailedKeys) {
    if (!active.has(key)) thumbFailedKeys.delete(key);
  }
  for (const key of thumbBlobCache.keys()) {
    if (!active.has(key)) revokeThumbBlob(key);
  }
  for (const key of thumbInflight.keys()) {
    if (!active.has(key)) thumbInflight.delete(key);
  }
}

async function fetchQueueThumbnailBlob(item) {
  const cacheKey = thumbCacheKey(item);
  if (thumbBlobCache.has(cacheKey)) {
    return thumbBlobCache.get(cacheKey);
  }
  if (thumbFailedKeys.has(cacheKey)) {
    return null;
  }
  if (thumbInflight.has(cacheKey)) {
    return thumbInflight.get(cacheKey);
  }
  const apiUrl = thumbnailApiUrl(item.item_id);
  if (!apiUrl) {
    return null;
  }
  const work = (async () => {
    try {
      const res = await fetch(apiUrl, { headers: headers() });
      if (!res.ok) {
        if (res.status !== 401) {
          thumbFailedKeys.add(cacheKey);
        }
        return null;
      }
      const blob = await res.blob();
      if (blob.size < 32) {
        thumbFailedKeys.add(cacheKey);
        return null;
      }
      const objUrl = URL.createObjectURL(blob);
      thumbBlobCache.set(cacheKey, objUrl);
      thumbFailedKeys.delete(cacheKey);
      return objUrl;
    } catch {
      thumbFailedKeys.add(cacheKey);
      return null;
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

function applyThumbBlobToImg(img, placeholder, cacheKey, objUrl) {
  img.onload = () => {
    img.classList.remove("hidden");
    placeholder.classList.add("hidden");
    thumbFailedKeys.delete(cacheKey);
  };
  img.onerror = () => {
    thumbFailedKeys.add(cacheKey);
    revokeThumbBlob(cacheKey);
    img.classList.add("hidden");
    img.removeAttribute("src");
    placeholder.textContent = "Thumbnail unavailable";
    placeholder.classList.remove("hidden");
  };
  img.src = objUrl;
}

function attachCardThumbnail(img, placeholder, item, showThumbnails) {
  img.classList.add("hidden");
  placeholder.classList.remove("hidden");
  placeholder.textContent = thumbPlaceholderText(item, showThumbnails);

  if (!showThumbnails || !itemHasThumbnailSource(item)) {
    return;
  }
  if (!thumbnailApiUrl(item.item_id)) {
    placeholder.textContent = "Save API token to load thumbnails";
    return;
  }
  const cacheKey = thumbCacheKey(item);
  if (thumbFailedKeys.has(cacheKey)) {
    placeholder.textContent = "Thumbnail unavailable";
    return;
  }

  const cached = thumbBlobCache.get(cacheKey);
  if (cached) {
    applyThumbBlobToImg(img, placeholder, cacheKey, cached);
    return;
  }

  placeholder.textContent = "Fetching thumbnail…";
  fetchQueueThumbnailBlob(item).then((objUrl) => {
    if (!img.isConnected) return;
    if (objUrl) {
      applyThumbBlobToImg(img, placeholder, cacheKey, objUrl);
    } else {
      placeholder.textContent = thumbFailedKeys.has(cacheKey)
        ? "Thumbnail unavailable"
        : "Save API token to load thumbnails";
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

function renderQueueCardListRow(item) {
  const slug = statusSlug(item.status);
  const card = document.createElement("article");
  card.className = "card";

  const thumb = document.createElement("div");
  thumb.className = "card-thumb";
  const placeholder = document.createElement("span");
  placeholder.className = "card-thumb-placeholder";
  placeholder.textContent = "…";
  const img = document.createElement("img");
  img.alt = "";
  img.className = "hidden";
  thumb.appendChild(img);
  attachCardThumbnail(img, placeholder, item, true);
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

async function refreshQueue() {
  const res = await api("/api/queue");
  const data = await res.json();
  const root = document.getElementById("queue");
  const settings = cachedSettings || {};
  pruneThumbFailedKeys(data.items);
  stopActiveMedia();
  root.className = "queue" + (settings.card_list_layout ? " list-layout" : "");
  root.innerHTML = "";
  for (const item of data.items) {
    const card = settings.card_list_layout
      ? renderQueueCardListRow(item)
      : renderQueueCard(item, settings);
    root.appendChild(card);
  }
}

async function refreshLogs() {
  const res = await api("/api/logs");
  const data = await res.json();
  const log = document.getElementById("log-view");
  log.textContent = data.lines.join("\n");
  log.scrollTop = log.scrollHeight;
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
    refreshAv1(),
  ]);
}

function connectSse() {
  const t = token();
  if (!t) return;
  const es = new EventSource(`/api/events?token=${encodeURIComponent(t)}`);
  es.onmessage = (ev) => {
    try {
      const data = JSON.parse(ev.data);
      if (data?.type === "shutdown") {
        showShutdownNotice(
          "rustdl has shut down. You can close this tab."
        );
        es.close();
        return;
      }
    } catch {
      /* ignore malformed payloads */
    }
    refreshAll().catch(() => {});
  };
  es.onerror = () => {
    es.close();
    if (shuttingDown) {
      showShutdownNotice("rustdl has shut down. You can close this tab.");
      return;
    }
    setTimeout(connectSse, 3000);
  };
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

  setCheck("set-auto-add", s.auto_add_pasted_urls);
  setCheck("set-auto-start", s.auto_start_downloads);
  setCheck("set-enqueue-av1", s.enqueue_downloads_to_av1);
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

  setVal("set-av1-bitrate", s.av1_target_bitrate);
  setVal("set-av1-max-width", s.av1_max_width);
  setVal("set-av1-preset", s.av1_size_preset);
  setVal("set-av1-min-shrink", s.av1_min_shrink_percent);
  setVal("set-av1-encoder-override", s.av1_encoder_override);
  setCheck("set-av1-recursive", s.av1_recursive);
  setCheck("set-av1-dry-run", s.av1_dry_run);
  setCheck("set-av1-auto-start", s.av1_auto_start_on_add);
  setCheck("set-av1-overwrite", s.av1_overwrite);
  setCheck("set-av1-reencode", s.av1_reencode_av1);
  setCheck("set-av1-recommended-container", s.av1_use_recommended_container);
  setCheck("set-av1-delete-original", s.av1_delete_original);
  setCheck("set-av1-rename-original", s.av1_rename_original);
  setCheck("set-av1-remember-queue", s.av1_remember_queue);

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
  s.enqueue_downloads_to_av1 = document.getElementById("set-enqueue-av1").checked;
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

  s.av1_target_bitrate = document.getElementById("set-av1-bitrate").value;
  s.av1_max_width = parseInt(document.getElementById("set-av1-max-width").value, 10) || 1920;
  s.av1_size_preset = document.getElementById("set-av1-preset").value;
  s.av1_min_shrink_percent =
    parseFloat(document.getElementById("set-av1-min-shrink").value) || 0;
  s.av1_encoder_override = document.getElementById("set-av1-encoder-override").value;
  s.av1_recursive = document.getElementById("set-av1-recursive").checked;
  s.av1_dry_run = document.getElementById("set-av1-dry-run").checked;
  s.av1_auto_start_on_add = document.getElementById("set-av1-auto-start").checked;
  s.av1_overwrite = document.getElementById("set-av1-overwrite").checked;
  s.av1_reencode_av1 = document.getElementById("set-av1-reencode").checked;
  s.av1_use_recommended_container = document.getElementById(
    "set-av1-recommended-container",
  ).checked;
  s.av1_delete_original = document.getElementById("set-av1-delete-original").checked;
  s.av1_rename_original = document.getElementById("set-av1-rename-original").checked;
  s.av1_remember_queue = document.getElementById("set-av1-remember-queue").checked;

  if (s.ffmpeg_extract_audio_mp3) s.ffmpeg_remux_mp4 = false;
  s.av1_max_width = Math.min(7680, Math.max(320, s.av1_max_width));
  s.av1_min_shrink_percent = Math.min(95, Math.max(0, s.av1_min_shrink_percent));
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
  document.getElementById("settings-tab-av1").hidden = name !== "av1";
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

function saveTokenFromForm() {
  const v = document.getElementById("token-input").value.trim();
  if (!v) return;
  localStorage.setItem(TOKEN_KEY, v);
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

document.getElementById("btn-settings").onclick = () => openSettingsDialog().catch(console.error);

document.getElementById("btn-settings-cancel").onclick = () => {
  document.getElementById("settings-dialog").close();
};

document.getElementById("settings-form").onsubmit = async (e) => {
  e.preventDefault();
  if (!cachedSettings) return;
  const patch = collectSettingsForm(cachedSettings);
  await api("/api/settings", { method: "POST", body: JSON.stringify({ settings: patch }) });
  cachedSettings = patch;
  statusFlags.auto_add_pasted_urls = !!patch.auto_add_pasted_urls;
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

/* ----------------------------- AV1 converter ----------------------------- */

let currentView = "downloader";
/** Skip re-fetching AV1 thumbnails that already failed until the source changes. */
const av1ThumbFailedKeys = new Set();
const av1ThumbBlobCache = new Map();
/** @type {Map<string, Promise<string|null>>} */
const av1ThumbInflight = new Map();

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
  currentView = view === "av1" ? "av1" : "downloader";
  document.querySelectorAll(".nav-btn").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.view === currentView);
  });
  document.getElementById("downloader-main").classList.toggle("hidden", currentView !== "downloader");
  document.getElementById("av1-main").classList.toggle("hidden", currentView !== "av1");
  const dlActions = document.getElementById("downloader-only-actions");
  if (dlActions) dlActions.classList.toggle("hidden", currentView !== "downloader");
  if (currentView === "av1") refreshAv1().catch(() => {});
}

function av1Slug(item) {
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

function av1Group(item) {
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

function av1ThumbKey(item) {
  return `${item.item_id}|${item.source_path || ""}`;
}

function av1ThumbnailUrl(itemId) {
  const t = token();
  if (!t) return null;
  return `/api/av1/thumbnail/${itemId}?token=${encodeURIComponent(t)}`;
}

function applyAv1ThumbBlobToImg(img, placeholder, key, objUrl) {
  img.onload = () => {
    img.classList.remove("hidden");
    placeholder.classList.add("hidden");
    av1ThumbFailedKeys.delete(key);
  };
  img.onerror = () => {
    av1ThumbFailedKeys.add(key);
    revokeAv1ThumbBlob(key);
    img.classList.add("hidden");
    img.removeAttribute("src");
    placeholder.textContent = "No preview available";
    placeholder.classList.remove("hidden");
  };
  img.src = objUrl;
}

function attachAv1Thumbnail(img, placeholder, item, showThumbnails) {
  img.classList.add("hidden");
  placeholder.classList.remove("hidden");
  if (!showThumbnails) {
    placeholder.textContent = "Thumbnails off";
    return;
  }
  if (!av1ThumbnailUrl(item.item_id)) {
    placeholder.textContent = "Save API token to load thumbnails";
    return;
  }
  const key = av1ThumbKey(item);
  if (av1ThumbFailedKeys.has(key)) {
    placeholder.textContent = "No preview available";
    return;
  }
  const cached = av1ThumbBlobCache.get(key);
  if (cached) {
    applyAv1ThumbBlobToImg(img, placeholder, key, cached);
    return;
  }
  placeholder.textContent = "Loading preview…";
  fetchAv1ThumbnailBlob(item).then((objUrl) => {
    if (!img.isConnected) return;
    if (objUrl) {
      applyAv1ThumbBlobToImg(img, placeholder, key, objUrl);
    } else {
      placeholder.textContent = av1ThumbFailedKeys.has(key)
        ? "No preview available"
        : "Save API token to load thumbnails";
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

function av1WillSkipNotice() {
  const el = document.createElement("p");
  el.className = "av1-will-skip-notice";
  el.textContent = "Will skip · already AV1 (re-encode disabled)";
  return el;
}

function av1MediaBadges(item) {
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

function renderAv1Card(item, showThumbnails) {
  const slug = av1Slug(item);
  const active = slug === "downloading" || slug === "queued";
  const card = document.createElement("article");
  card.className = "card" + (item.will_skip_av1 ? " av1-will-skip" : "");

  const thumb = document.createElement("div");
  thumb.className = "card-thumb";
  const img = document.createElement("img");
  img.alt = "";
  img.className = "hidden";
  const placeholder = document.createElement("span");
  placeholder.className = "card-thumb-placeholder";
  thumb.appendChild(img);
  attachAv1Thumbnail(img, placeholder, item, showThumbnails);
  thumb.appendChild(placeholder);
  card.appendChild(thumb);

  const body = document.createElement("div");
  body.className = "card-body";

  const title = document.createElement("h3");
  title.className = "card-title";
  title.textContent = baseName(item.source_path) || "(unknown)";
  title.title = item.source_path || "";
  body.appendChild(title);

  if (item.will_skip_av1) {
    body.appendChild(av1WillSkipNotice());
  }
  body.appendChild(av1MediaBadges(item));

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
  pathsEl.className = "av1-paths";
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

function renderAv1Summary(data) {
  const root = document.getElementById("av1-summary");
  if (!root) return;
  root.innerHTML = "";

  const running = document.createElement("span");
  running.className = "status-badge " + (data.running ? "status-live" : "status-paused");
  running.innerHTML = `<span class="status-dot" aria-hidden="true"></span>${data.running ? "Converting" : "Idle"}`;
  root.appendChild(running);

  const counts = {};
  for (const it of data.items) {
    const g = av1Group(it);
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
    const saved = Math.max(0, inB - outB);
    const pct = inB > 0 ? ((saved / inB) * 100).toFixed(1) : "0.0";
    const el = document.createElement("span");
    el.className = "status-badge status-done";
    el.textContent = `Saved ${formatBytes(saved)} (${pct}%) across ${sum.completed} file(s)`;
    root.appendChild(el);
  }
  if (sum.pending_count > 0) {
    const el = document.createElement("span");
    el.className = "status-badge status-queued";
    el.textContent = `${sum.pending_count} pending · ${formatBytes(sum.pending_input_bytes || 0)}`;
    root.appendChild(el);
  }
}

function renderAv1Encoder(data) {
  const el = document.getElementById("av1-encoder");
  if (!el) return;
  const parts = [];
  if (data.encoder) parts.push(`Encoder: ${data.encoder.label}`);
  else if (!data.has_ffmpeg) parts.push("Encoder: ffmpeg not found (set the path in Settings → Shared)");
  if (!data.has_ffprobe) parts.push("ffprobe not found — metadata and start are disabled");
  el.textContent = parts.join(" · ");
}

function revokeAv1ThumbBlob(key) {
  const url = av1ThumbBlobCache.get(key);
  if (url) {
    URL.revokeObjectURL(url);
    av1ThumbBlobCache.delete(key);
  }
}

function pruneAv1ThumbKeys(items) {
  const active = new Set(items.map((it) => av1ThumbKey(it)));
  for (const key of av1ThumbFailedKeys) {
    if (!active.has(key)) av1ThumbFailedKeys.delete(key);
  }
  for (const key of av1ThumbBlobCache.keys()) {
    if (!active.has(key)) revokeAv1ThumbBlob(key);
  }
  for (const key of av1ThumbInflight.keys()) {
    if (!active.has(key)) av1ThumbInflight.delete(key);
  }
}

async function fetchAv1ThumbnailBlob(item) {
  const cacheKey = av1ThumbKey(item);
  if (av1ThumbBlobCache.has(cacheKey)) {
    return av1ThumbBlobCache.get(cacheKey);
  }
  if (av1ThumbFailedKeys.has(cacheKey)) {
    return null;
  }
  if (av1ThumbInflight.has(cacheKey)) {
    return av1ThumbInflight.get(cacheKey);
  }
  const apiUrl = av1ThumbnailUrl(item.item_id);
  if (!apiUrl) {
    return null;
  }
  const work = (async () => {
    try {
      const res = await fetch(apiUrl, { headers: headers() });
      if (!res.ok) {
        if (res.status !== 401) {
          av1ThumbFailedKeys.add(cacheKey);
        }
        return null;
      }
      const blob = await res.blob();
      if (blob.size < 32) {
        av1ThumbFailedKeys.add(cacheKey);
        return null;
      }
      const objUrl = URL.createObjectURL(blob);
      av1ThumbBlobCache.set(cacheKey, objUrl);
      av1ThumbFailedKeys.delete(cacheKey);
      return objUrl;
    } catch {
      av1ThumbFailedKeys.add(cacheKey);
      return null;
    }
  })();
  av1ThumbInflight.set(cacheKey, work);
  try {
    return await work;
  } finally {
    av1ThumbInflight.delete(cacheKey);
  }
}

async function refreshAv1() {
  let data;
  try {
    const res = await api("/api/av1/queue");
    if (!res.ok) return;
    data = await res.json();
  } catch {
    return;
  }
  lastAv1Payload = data;
  // Keep the textarea in sync with the server unless the user is editing it.
  const input = document.getElementById("av1-input");
  if (input && document.activeElement !== input) {
    input.value = data.input_paths || "";
  }
  renderAv1Encoder(data);
  renderAv1Summary(data);
  renderNavbarStatus();

  const startBtn = document.getElementById("btn-av1-start");
  const cancelBtn = document.getElementById("btn-av1-cancel");
  const readyCount = data.items.filter((it) => it.status === "Idle").length;
  if (startBtn) startBtn.disabled = data.running || !data.has_ffmpeg || !data.has_ffprobe || readyCount === 0;
  if (cancelBtn) cancelBtn.disabled = !data.running;

  const root = document.getElementById("av1-queue");
  if (!root) return;
  const showThumbnails = (cachedSettings || {}).show_thumbnails !== false;
  pruneAv1ThumbKeys(data.items);
  root.innerHTML = "";
  if (!data.items.length) {
    const empty = document.createElement("p");
    empty.className = "hint av1-empty";
    empty.textContent = "Nothing here yet. Add file or folder paths above, then Scan inputs.";
    root.appendChild(empty);
    return;
  }
  for (const label of ["Active", "Ready", "Failed", "Skipped", "Done"]) {
    const group = data.items.filter((it) => av1Group(it) === label);
    if (!group.length) continue;
    const header = document.createElement("h3");
    header.className = "av1-group-header";
    header.textContent = `${label} (${group.length})`;
    root.appendChild(header);
    for (const item of group) {
      root.appendChild(renderAv1Card(item, showThumbnails));
    }
  }
}

async function av1Scan() {
  const input = document.getElementById("av1-input");
  const paths = (input ? input.value : "")
    .split(/\n+/)
    .map((s) => s.trim())
    .filter(Boolean);
  if (!paths.length) return;
  await api("/api/av1/scan", { method: "POST", body: JSON.stringify({ paths }) });
  await refreshAv1();
}

async function av1Start() {
  await api("/api/av1/start", { method: "POST" });
  await refreshAv1();
}

async function av1Cancel() {
  await api("/api/av1/cancel", { method: "POST" });
  await refreshAv1();
}

async function av1Clear() {
  if (!confirm("Clear the entire AV1 queue?")) return;
  await api("/api/av1/clear", { method: "POST" });
  await refreshAv1();
}

document.querySelectorAll(".nav-btn").forEach((btn) => {
  btn.onclick = () => setView(btn.dataset.view);
});
document.getElementById("btn-av1-scan").onclick = () => av1Scan().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-av1-start").onclick = () => av1Start().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-av1-cancel").onclick = () => av1Cancel().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-av1-clear").onclick = () => av1Clear().catch((e) => alert(e.message || String(e)));
document.getElementById("btn-av1-settings").onclick = () =>
  openSettingsDialog().then(() => switchSettingsTab("av1")).catch(console.error);

applyStaticButtonIcons();

if (token()) {
  document.getElementById("token-input").value = token();
  showApp();
  refreshAll().catch(() => {});
  connectSse();
  refreshIntervalId = setInterval(() => {
    if (shuttingDown) return;
    refreshAll().catch(() => {});
  }, 5000);
}
