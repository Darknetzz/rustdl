/** Host-native folder/file picker helpers (LAN web UI). */

function wireBrowseButton(buttonId, inputId, kind, title) {
  const btn = document.getElementById(buttonId);
  const input = document.getElementById(inputId);
  if (!btn || !input) return;
  btn.addEventListener("click", () => {
    browseHostPath(kind, title)
      .then((data) => {
        const path = data.path || data.paths?.[0];
        if (!path) return;
        if (kind === "files" && data.paths?.length) {
          const existing = input.value.trim();
          const merged = existing
            ? `${existing}\n${data.paths.join("\n")}`
            : data.paths.join("\n");
          input.value = merged;
        } else {
          input.value = path;
        }
        input.dispatchEvent(new Event("input", { bubbles: true }));
        input.dispatchEvent(new Event("change", { bubbles: true }));
      })
      .catch((e) => {
        if (typeof notifyError === "function") notifyError(e.message || String(e));
      });
  });
}

function initBrowseButtons() {
  wireBrowseButton("btn-browse-output-dir", "set-output-dir", "folder", "Select output folder");
  wireBrowseButton("btn-browse-cookies", "set-cookies", "file", "Select cookies file");
  wireBrowseButton("btn-browse-convert-input", "convert-input", "files", "Select video file(s)");
  wireBrowseButton("btn-browse-watch-folder", "set-watch-folder-path", "folder", "Select watch folder");
  wireBrowseButton(
    "btn-browse-convert-output-dir",
    "set-convert-output-dir",
    "folder",
    "Select convert output folder",
  );
  wireBrowseButton(
    "btn-browse-convert-watch-path",
    "set-convert-watch-path",
    "folder",
    "Select convert watch folder",
  );
  wireBrowseButton("btn-browse-web-tls-cert", "set-web-tls-cert", "file", "Select TLS certificate");
  wireBrowseButton("btn-browse-web-tls-key", "set-web-tls-key", "file", "Select TLS private key");
}

document.addEventListener("DOMContentLoaded", () => initBrowseButtons());
