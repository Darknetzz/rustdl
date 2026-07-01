/** Quality watchlist panel for the LAN web UI. */

let watchlistGeneration = 0;

function formatWatchlistResolution(height) {
  if (!height) return "—";
  return `${height}p`;
}

function watchlistStatusLabel(entry) {
  if (entry.improved_pending) return "↑ improved";
  if (entry.paused) return "paused";
  if (entry.last_probe_error) return "error";
  return "watching";
}

async function refreshWatchlistPanel() {
  const panel = document.getElementById("watchlist-panel");
  const list = document.getElementById("watchlist-list");
  if (!panel || !list) return;
  try {
    const res = await api("/api/watchlist");
    const data = await res.json();
    watchlistGeneration = data.generation || 0;
    const enabled = cachedSettings?.watchlist_enabled;
    const hasEntries = (data.entries || []).length > 0;
    panel.classList.toggle("hidden", !enabled && !hasEntries);
    list.replaceChildren();
    if (!hasEntries) {
      const hint = document.createElement("p");
      hint.className = "hint";
      hint.textContent =
        "Add a Done download via More info → Watch for better quality, paste a URL below, or enable the watchlist in Settings → Downloader.";
      list.appendChild(hint);
      return;
    }
    for (const entry of data.entries) {
      const row = document.createElement("div");
      row.className = "watchlist-row";
      const status = document.createElement("span");
      status.className = `watchlist-status${entry.improved_pending ? " improved" : ""}`;
      status.textContent = watchlistStatusLabel(entry);
      const title = document.createElement("span");
      title.className = "watchlist-title";
      title.textContent = entry.title || entry.url;
      const reso = document.createElement("span");
      reso.className = "watchlist-reso hint";
      reso.textContent = `${formatWatchlistResolution(entry.baseline_height)} → ${formatWatchlistResolution(entry.last_probe_height)}`;
      const actions = document.createElement("div");
      actions.className = "watchlist-actions btn-group";
      if (entry.improved_pending) {
        const queueBtn = document.createElement("button");
        queueBtn.type = "button";
        queueBtn.className = "secondary";
        queueBtn.textContent = "Queue";
        queueBtn.onclick = () =>
          api(`/api/watchlist/${entry.entry_id}/enqueue`, { method: "POST" })
            .then(() => refreshWatchlistPanel())
            .catch((e) => notifyError(e.message));
        actions.appendChild(queueBtn);
      }
      const pauseBtn = document.createElement("button");
      pauseBtn.type = "button";
      pauseBtn.className = "secondary";
      pauseBtn.textContent = entry.paused ? "Resume" : "Pause";
      pauseBtn.onclick = () =>
        api(`/api/watchlist/${entry.entry_id}/pause`, {
          method: "POST",
          body: JSON.stringify({ paused: !entry.paused }),
        })
          .then(() => refreshWatchlistPanel())
          .catch((e) => notifyError(e.message));
      const removeBtn = document.createElement("button");
      removeBtn.type = "button";
      removeBtn.className = "secondary danger";
      removeBtn.textContent = "Remove";
      removeBtn.onclick = () =>
        api(`/api/watchlist/${entry.entry_id}`, { method: "DELETE" })
          .then(() => refreshWatchlistPanel())
          .catch((e) => notifyError(e.message));
      actions.append(pauseBtn, removeBtn);
      row.append(status, title, reso, actions);
      list.appendChild(row);
    }
  } catch (e) {
    console.error(e);
  }
}

async function addWatchlistFromQueueItem(itemId) {
  await api(`/api/watchlist/from-queue/${itemId}`, { method: "POST" });
  await refreshWatchlistPanel();
  showToast("Added to quality watchlist.");
}

function initWatchlistUi() {
  document.getElementById("watchlist-add-form")?.addEventListener("submit", (e) => {
    e.preventDefault();
    const input = document.getElementById("watchlist-add-url");
    const url = input?.value?.trim();
    if (!url) return;
    api("/api/watchlist", { method: "POST", body: JSON.stringify({ url }) })
      .then(() => {
        if (input) input.value = "";
        return refreshWatchlistPanel();
      })
      .then(() => showToast("Watchlist URL added."))
      .catch((err) => notifyError(err.message || String(err)));
  });
  document.getElementById("btn-watchlist-probe")?.addEventListener("click", () => {
    api("/api/watchlist/probe", { method: "POST" })
      .then(() => showToast("Watchlist check scheduled."))
      .catch((e) => notifyError(e.message));
  });
}

document.addEventListener("DOMContentLoaded", () => initWatchlistUi());
