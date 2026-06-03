use std::net::SocketAddr;
use std::sync::Arc;

use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::config::AppSettings;
use crate::service::core::SharedCore;
use crate::service::web::api::{ApiState, api_router};

/// Browser-openable URL for the LAN web UI (maps `0.0.0.0` to this machine).
pub fn web_ui_browser_url(bind_address: &str) -> String {
    let bind = bind_address.trim();
    let with_scheme = if bind.starts_with("http://") || bind.starts_with("https://") {
        if bind.ends_with('/') {
            bind.to_owned()
        } else {
            format!("{bind}/")
        }
    } else {
        format!("http://{bind}/")
    };
    with_scheme.replace("://0.0.0.0", "://127.0.0.1")
}

pub struct WebServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl WebServerHandle {
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(j) = self.join.take() {
            j.abort();
        }
    }
}

impl Drop for WebServerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebServerStartError {
    EmptyBind,
    InvalidBind(String),
    EmptyToken,
    BindFailed(String),
}

impl WebServerStartError {
    pub fn message(&self) -> String {
        match self {
            Self::EmptyBind => "web bind address is empty".to_owned(),
            Self::InvalidBind(detail) => format!("invalid web bind address: {detail}"),
            Self::EmptyToken => {
                "web UI requires a non-empty auth token (see Settings → Shared)".to_owned()
            }
            Self::BindFailed(detail) => {
                format!(
                    "web UI failed to bind: {detail} (another rustdl instance may already be using this port)"
                )
            }
        }
    }
}

/// Merges optional `--host` / `--port` overrides with `fallback` (e.g. saved `web_bind_address`).
pub fn resolve_web_bind_address(
    host: Option<&str>,
    port: Option<u16>,
    fallback: &str,
) -> Result<String, WebServerStartError> {
    let fallback = fallback.trim();
    if host.is_none() && port.is_none() {
        if fallback.is_empty() {
            return Err(WebServerStartError::EmptyBind);
        }
        return Ok(fallback.to_owned());
    }
    let fallback_addr: SocketAddr = fallback.parse().map_err(|e| {
        WebServerStartError::InvalidBind(format!("{fallback:?}: {e}"))
    })?;
    let host = host
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| fallback_addr.ip().to_string());
    let port = port.unwrap_or(fallback_addr.port());
    Ok(format!("{host}:{port}"))
}

pub fn spawn_web_server(
    runtime: Arc<Runtime>,
    core: SharedCore,
    settings: &AppSettings,
) -> Option<WebServerHandle> {
    if !settings.web_ui_enabled {
        return None;
    }
    match spawn_web_server_at(
        runtime,
        core,
        settings.web_bind_address.trim(),
        settings.web_auth_token.trim(),
    ) {
        Ok((handle, _)) => {
            eprintln!(
                "rustdl: web UI listening on http://{}",
                settings.web_bind_address.trim()
            );
            Some(handle)
        }
        Err(e) => {
            eprintln!("rustdl: {}", e.message());
            None
        }
    }
}

pub fn spawn_web_server_at(
    runtime: Arc<Runtime>,
    core: SharedCore,
    bind: &str,
    auth_token: &str,
) -> Result<(WebServerHandle, super::api::ApiState), WebServerStartError> {
    let bind = bind.trim();
    if bind.is_empty() {
        return Err(WebServerStartError::EmptyBind);
    }
    let addr: SocketAddr = bind
        .parse()
        .map_err(|e| WebServerStartError::InvalidBind(format!("{bind:?}: {e}")))?;
    if auth_token.trim().is_empty() {
        return Err(WebServerStartError::EmptyToken);
    }

    let state = ApiState::new(core.clone());
    let app = api_router(state);

    let std_listener = std::net::TcpListener::bind(addr).map_err(|e| {
        WebServerStartError::BindFailed(format!("{addr}: {e}"))
    })?;
    std_listener
        .set_nonblocking(true)
        .map_err(|e| WebServerStartError::BindFailed(format!("{addr}: {e}")))?;
    let listener = tokio::net::TcpListener::from_std(std_listener).map_err(|e| {
        WebServerStartError::BindFailed(format!("{addr}: {e}"))
    })?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let join = runtime.spawn(async move {
        let serve = axum::serve(listener, app);
        tokio::select! {
            _ = serve => {},
            _ = shutdown_rx => {},
        }
    });

    Ok((
        WebServerHandle {
            shutdown_tx: Some(shutdown_tx),
            join: Some(join),
        },
        state,
    ))
}

#[cfg(test)]
mod tests {
    use super::{resolve_web_bind_address, web_ui_browser_url};

    #[test]
    fn web_ui_browser_url_maps_wildcard_bind() {
        assert_eq!(
            web_ui_browser_url("0.0.0.0:8765"),
            "http://127.0.0.1:8765/"
        );
    }

    #[test]
    fn web_ui_browser_url_adds_scheme() {
        assert_eq!(
            web_ui_browser_url("127.0.0.1:8765"),
            "http://127.0.0.1:8765/"
        );
    }

    #[test]
    fn resolve_web_bind_address_uses_fallback_when_no_overrides() {
        assert_eq!(
            resolve_web_bind_address(None, None, "0.0.0.0:8765").unwrap(),
            "0.0.0.0:8765"
        );
    }

    #[test]
    fn resolve_web_bind_address_overrides_host_and_port() {
        assert_eq!(
            resolve_web_bind_address(Some("127.0.0.1"), Some(9000), "0.0.0.0:8765").unwrap(),
            "127.0.0.1:9000"
        );
    }

    #[test]
    fn resolve_web_bind_address_overrides_port_only() {
        assert_eq!(
            resolve_web_bind_address(None, Some(9000), "0.0.0.0:8765").unwrap(),
            "0.0.0.0:9000"
        );
    }
}
