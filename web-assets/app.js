const TOKEN_KEY = "rustdl_web_token";

let cachedSettings = null;

const AUTO_ADD_MS = 700;
let autoAddTimer = null;
const statusFlags = {
  auto_add_pasted_urls: false,
  add_in_progress: false,
};

let cachedHasYtDlp = false;

/** @type {Map<number, string>} blob URLs to revoke when an item leaves the queue */
const thumbObjectUrls = new Map();

/** @type {Map<number, string>} skip re-fetch after 404 until item metadata changes */
const thumbFailedKeys = new Map();

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
  const s = data.status;
  statusFlags.auto_add_pasted_urls = !!data.auto_add_pasted_urls;
  statusFlags.add_in_progress = !!data.add_in_progress;
  document.getElementById("status-summary").textContent =
    `Paused: ${data.downloads_paused} · Resolving ${s.resolving} · Ready ${s.ready} · ` +
    `Queued ${s.queued} · Active ${s.active} · Done ${s.done} · Failed ${s.failed}`;
  renderTools(data.tools);
  cachedHasYtDlp = data.tools?.yt_dlp?.ok === true;
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

function thumbSourceKey(item) {
  return [
    item.item_id,
    item.status,
    item.video_id || "",
    item.thumbnail_url || "",
    item.title || "",
    item.source_line || "",
  ].join("|");
}

function pruneThumbCaches(activeIds) {
  for (const id of thumbObjectUrls.keys()) {
    if (!activeIds.has(id)) {
      URL.revokeObjectURL(thumbObjectUrls.get(id));
      thumbObjectUrls.delete(id);
    }
  }
  for (const id of thumbFailedKeys.keys()) {
    if (!activeIds.has(id)) thumbFailedKeys.delete(id);
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

function appendPlayButton(actions, item, thumb) {
  if (!item.playable) return;
  const play = document.createElement("button");
  play.type = "button";
  play.className = "primary";
  play.textContent = "Play";
  play.onclick = () => toggleCardMedia(item, thumb);
  actions.appendChild(play);
}

async function loadCardThumbnail(img, placeholder, itemId) {
  const prev = thumbObjectUrls.get(itemId);
  if (prev) {
    URL.revokeObjectURL(prev);
    thumbObjectUrls.delete(itemId);
  }
  const res = await fetch(`/api/thumbnail/${itemId}`, { headers: headers() });
  if (!res.ok) throw new Error(`thumbnail ${res.status}`);
  const blob = await res.blob();
  if (blob.size < 32) {
    throw new Error("thumbnail empty");
  }
  const objectUrl = URL.createObjectURL(blob);
  thumbObjectUrls.set(itemId, objectUrl);
  img.src = objectUrl;
  img.classList.remove("hidden");
  placeholder.classList.add("hidden");
}

function queueCardThumbnailLoad(img, placeholder, item) {
  const key = thumbSourceKey(item);
  if (thumbFailedKeys.get(item.item_id) === key) {
    img.classList.add("hidden");
    placeholder.textContent = "Thumbnail unavailable";
    placeholder.classList.remove("hidden");
    return;
  }
  const cachedUrl = thumbObjectUrls.get(item.item_id);
  if (cachedUrl) {
    img.src = cachedUrl;
    img.classList.remove("hidden");
    placeholder.classList.add("hidden");
    return;
  }
  loadCardThumbnail(img, placeholder, item.item_id)
    .then(() => {
      thumbFailedKeys.delete(item.item_id);
    })
    .catch((err) => {
      thumbFailedKeys.set(item.item_id, key);
      img.classList.add("hidden");
      const msg =
        err instanceof Error && err.message.startsWith("thumbnail 401")
          ? "Thumbnail denied (check API token)"
          : "Thumbnail unavailable";
      placeholder.textContent = msg;
      placeholder.classList.remove("hidden");
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

function appendRedownloadButton(actions, item) {
  if (!canRedownload(item)) return;
  const slug = statusSlug(item.status);
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "secondary";
  btn.textContent = slug === "failed" ? "Retry download" : "Re-download";
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
  if (showThumbnails && token() && itemHasThumbnailSource(item)) {
    queueCardThumbnailLoad(img, placeholder, item);
  }
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
  chip.className = `status-chip status-${slug}`;
  chip.textContent = item.status || "Idle";
  badges.appendChild(chip);
  body.appendChild(badges);

  const footer = document.createElement("p");
  footer.className = `card-footer status-${slug}`;
  footer.textContent = footerStatusText(item);
  body.appendChild(footer);

  card.appendChild(body);

  const actions = document.createElement("div");
  actions.className = "card-actions";
  appendPlayButton(actions, item, thumb);
  if (canCancel(item)) {
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "secondary";
    cancel.textContent = "Cancel";
    cancel.onclick = () => cancelItem(item.item_id);
    actions.appendChild(cancel);
  }
  appendRedownloadButton(actions, item);
  card.appendChild(actions);

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
  if (token() && itemHasThumbnailSource(item)) {
    queueCardThumbnailLoad(img, placeholder, item);
  }
  thumb.appendChild(placeholder);
  card.appendChild(thumb);

  const body = document.createElement("div");
  body.className = "card-body";
  const chip = document.createElement("span");
  chip.className = `status-chip status-${slug}`;
  chip.textContent = item.status;
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

  const actions = document.createElement("div");
  actions.className = "card-actions";
  appendPlayButton(actions, item, thumb);
  if (canCancel(item)) {
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "secondary";
    cancel.textContent = "Cancel";
    cancel.onclick = () => cancelItem(item.item_id);
    actions.appendChild(cancel);
  }
  appendRedownloadButton(actions, item);
  if (actions.childElementCount > 0) {
    card.appendChild(actions);
  }

  return card;
}

async function refreshQueue() {
  const res = await api("/api/queue");
  const data = await res.json();
  const root = document.getElementById("queue");
  const settings = cachedSettings || {};
  const activeIds = new Set(data.items.map((item) => item.item_id));
  pruneThumbCaches(activeIds);
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

async function redownloadItem(id) {
  const res = await api(`/api/downloads/redownload/${id}`, { method: "POST" });
  if (!res.ok) {
    throw new Error(
      "Re-download could not start (missing URL, invalid output folder, or yt-dlp unavailable)."
    );
  }
  await refreshAll();
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
  ]);
}

function connectSse() {
  const t = token();
  if (!t) return;
  const es = new EventSource(`/api/events?token=${encodeURIComponent(t)}`);
  es.onmessage = () => refreshAll().catch(() => {});
  es.onerror = () => {
    es.close();
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

  if (s.ffmpeg_extract_audio_mp3) s.ffmpeg_remux_mp4 = false;
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

document.getElementById("btn-add").onclick = async () => {
  clearTimeout(autoAddTimer);
  await flushAutoAddFromInput();
};

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

if (token()) {
  document.getElementById("token-input").value = token();
  showApp();
  refreshAll().catch(() => {});
  connectSse();
  setInterval(() => refreshAll().catch(() => {}), 5000);
}
