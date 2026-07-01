/** HTTP client for the rustdl LAN web API (loaded before app.js). */
const TOKEN_KEY = "rustdl_web_token";

function token() {
  return localStorage.getItem(TOKEN_KEY) || "";
}

function apiAuthOptional() {
  return !!token() || (typeof ipAuthBypass !== "undefined" && ipAuthBypass);
}

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

function imageFetchHeaders() {
  const h = {};
  const t = token();
  if (t) h["X-Rustdl-Token"] = t;
  return h;
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

async function api(path, options = {}) {
  const res = await fetch(path, {
    ...options,
    headers: { ...headers(), ...(options.headers || {}) },
  });
  if (res.status === 401) {
    const msg =
      "Token rejected. Copy the current API token from rustdl Settings → Web UI, paste it below, then click Save token.";
    if (typeof showAuthPanel === "function") showAuthPanel(msg);
    throw new Error(msg);
  }
  if (!res.ok) {
    throw new Error(await readApiError(res, `Request failed (${res.status})`));
  }
  return res;
}

async function postAction(path, fallback, options = {}) {
  const res = await api(path, { method: "POST", ...options });
  return res;
}

async function browseHostPath(kind, title) {
  const res = await api("/api/browse", {
    method: "POST",
    body: JSON.stringify({ kind, title }),
  });
  return res.json();
}
