//! Shared HTTP client for desktop thumbnail fetches and LAN web proxy requests.

use std::time::Duration;

use crate::config::AppSettings;

/// Builds the process-wide `reqwest` client (proxy, timeouts, user agent).
pub fn build_http_client(settings: &AppSettings) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(15))
        .user_agent(format!("rustdl/{}", crate::pkg_version::VERSION));
    let proxy = settings.yt_proxy.trim();
    if !proxy.is_empty() {
        if let Ok(p) = reqwest::Proxy::all(proxy) {
            builder = builder.proxy(p);
        }
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}
