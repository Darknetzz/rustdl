const TOKEN_KEY = "rustdl_web_token";

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
    throw new Error("unauthorized");
  }
  return res;
}

function showApp() {
  document.getElementById("auth-panel").classList.add("hidden");
  document.getElementById("app-main").classList.remove("hidden");
}

async function refreshStatus() {
  const res = await api("/api/status");
  const data = await res.json();
  const s = data.status;
  document.getElementById("status-summary").textContent =
    `Paused: ${data.downloads_paused} · Resolving ${s.resolving} · Ready ${s.ready} · ` +
    `Queued ${s.queued} · Active ${s.active} · Done ${s.done} · Failed ${s.failed}`;
}

async function refreshQueue() {
  const res = await api("/api/queue");
  const data = await res.json();
  const root = document.getElementById("queue");
  root.innerHTML = "";
  for (const item of data.items) {
    const card = document.createElement("article");
    card.className = "card";
    const img = document.createElement("img");
    img.alt = "";
    if (item.thumbnail_url && token()) {
      img.src = `/api/thumbnail/${item.item_id}`;
    }
    const body = document.createElement("div");
    const title = document.createElement("p");
    title.className = "card-title";
    title.textContent = item.title || item.source_line || "(no title)";
    const meta = document.createElement("p");
    meta.className = "card-meta";
    meta.textContent =
      `${item.status} · ${item.percent.toFixed(0)}% · ${item.speed_text} · ${item.eta_text}`;
    if (item.detail) meta.textContent += ` · ${item.detail}`;
    body.appendChild(title);
    body.appendChild(meta);
    const actions = document.createElement("div");
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "secondary";
    cancel.textContent = "Cancel";
    cancel.onclick = () => cancelItem(item.item_id);
    actions.appendChild(cancel);
    card.appendChild(img);
    card.appendChild(body);
    card.appendChild(actions);
    root.appendChild(card);
  }
}

async function refreshLogs() {
  const res = await api("/api/logs");
  const data = await res.json();
  document.getElementById("log-view").textContent = data.lines.join("\n");
}

async function cancelItem(id) {
  await api(`/api/downloads/cancel/${id}`, { method: "POST" });
  await refreshAll();
}

async function refreshAll() {
  await Promise.all([refreshStatus(), refreshQueue(), refreshLogs()]);
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

document.getElementById("btn-save-token").onclick = () => {
  const v = document.getElementById("token-input").value.trim();
  if (!v) return;
  localStorage.setItem(TOKEN_KEY, v);
  document.getElementById("auth-status").textContent = "Token saved.";
  showApp();
  refreshAll().catch((e) => {
    document.getElementById("auth-status").textContent = String(e);
  });
  connectSse();
};

document.getElementById("btn-add").onclick = async () => {
  const text = document.getElementById("url-input").value;
  const urls = text.split(/\n+/).map((s) => s.trim()).filter(Boolean);
  if (!urls.length) return;
  await api("/api/queue", {
    method: "POST",
    body: JSON.stringify({ urls }),
  });
  document.getElementById("url-input").value = "";
  await refreshAll();
};

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

document.getElementById("btn-settings").onclick = async () => {
  const res = await api("/api/settings");
  const data = await res.json();
  const s = data.settings;
  document.getElementById("set-output-dir").value = s.output_dir || "";
  document.getElementById("set-workers").value = s.worker_count || 3;
  document.getElementById("set-quality").value = s.quality_preset || "best";
  document.getElementById("set-auto-start").checked = !!s.auto_start_downloads;
  document.getElementById("set-extra-args").value = s.yt_dlp_extra_args || "";
  document.getElementById("command-preview").textContent = data.command_preview || "";
  document.getElementById("settings-dialog").showModal();
};

document.getElementById("btn-settings-cancel").onclick = () => {
  document.getElementById("settings-dialog").close();
};

document.getElementById("settings-form").onsubmit = async (e) => {
  e.preventDefault();
  const res = await api("/api/settings");
  const data = await res.json();
  const s = data.settings;
  s.output_dir = document.getElementById("set-output-dir").value;
  s.worker_count = parseInt(document.getElementById("set-workers").value, 10) || 3;
  s.quality_preset = document.getElementById("set-quality").value;
  s.auto_start_downloads = document.getElementById("set-auto-start").checked;
  s.yt_dlp_extra_args = document.getElementById("set-extra-args").value;
  await api("/api/settings", {
    method: "POST",
    body: JSON.stringify({ settings: s }),
  });
  document.getElementById("settings-dialog").close();
  await refreshAll();
};

if (token()) {
  document.getElementById("token-input").value = token();
  showApp();
  refreshAll().catch(() => {});
  connectSse();
  setInterval(() => refreshAll().catch(() => {}), 5000);
}
