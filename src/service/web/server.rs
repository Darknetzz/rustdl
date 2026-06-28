use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::config::{validate_web_tls_settings, web_tls_enabled, AppSettings};
use crate::service::core::SharedCore;
use crate::service::web::api::{api_router, ApiState};

/// Browser-openable URL for the LAN web UI (maps `0.0.0.0` to this machine).
pub fn web_ui_browser_url(settings: &AppSettings) -> String {
    let bind = settings.web_bind_address.trim();
    let scheme = if web_tls_enabled(settings) {
        "https"
    } else {
        "http"
    };
    let with_scheme = if bind.starts_with("http://") || bind.starts_with("https://") {
        let stripped = bind
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        format!("{scheme}://{stripped}/")
    } else if bind.ends_with('/') {
        format!("{scheme}://{bind}")
    } else {
        format!("{scheme}://{bind}/")
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
    TlsConfig(String),
}

impl WebServerStartError {
    pub fn message(&self) -> String {
        match self {
            Self::EmptyBind => "web bind address is empty".to_owned(),
            Self::InvalidBind(detail) => format!("invalid web bind address: {detail}"),
            Self::EmptyToken => {
                "web UI requires a non-empty auth token (see Settings, Web UI tab)".to_owned()
            }
            Self::BindFailed(detail) => {
                format!(
                    "web UI failed to bind: {detail} (another rustdl instance may already be using this port)"
                )
            }
            Self::TlsConfig(detail) => detail.clone(),
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
    let fallback_addr: SocketAddr = fallback
        .parse()
        .map_err(|e| WebServerStartError::InvalidBind(format!("{fallback:?}: {e}")))?;
    let host = host
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| fallback_addr.ip().to_string());
    let port = port.unwrap_or(fallback_addr.port());
    Ok(format!("{host}:{port}"))
}

pub fn try_spawn_web_server(
    runtime: Arc<Runtime>,
    core: SharedCore,
    settings: &AppSettings,
) -> Result<Option<WebServerHandle>, WebServerStartError> {
    if !settings.web_ui_enabled {
        return Ok(None);
    }
    validate_web_tls_settings(settings).map_err(WebServerStartError::TlsConfig)?;
    let tls = web_tls_enabled(settings);
    spawn_web_server_at(
        runtime,
        core,
        settings.web_bind_address.trim(),
        settings.web_auth_token.trim(),
        if tls {
            Some(settings.web_tls_cert_path.as_str())
        } else {
            None
        },
        if tls {
            Some(settings.web_tls_key_path.as_str())
        } else {
            None
        },
        None,
    )
    .map(Some)
}

pub fn spawn_web_server_at(
    runtime: Arc<Runtime>,
    core: SharedCore,
    bind: &str,
    auth_token: &str,
    tls_cert: Option<&str>,
    tls_key: Option<&str>,
    process_exit: Option<tokio::sync::oneshot::Sender<()>>,
) -> Result<WebServerHandle, WebServerStartError> {
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
    if let Some(tx) = process_exit {
        state.set_process_exit_notifier(tx);
    }
    let app = api_router(state);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let join = if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
        let cert_path = cert.trim().to_owned();
        let key_path = key.trim().to_owned();
        runtime.spawn(async move {
            let config = match axum_server::tls_rustls::RustlsConfig::from_pem_file(
                Path::new(&cert_path),
                Path::new(&key_path),
            )
            .await
            {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("rustdl: web UI TLS load failed ({cert_path}, {key_path}): {e}");
                    return;
                }
            };
            let handle = axum_server::Handle::new();
            let graceful = handle.clone();
            let shutdown = async move {
                let _ = shutdown_rx.await;
                graceful.graceful_shutdown(None);
            };
            let serve = axum_server::bind_rustls(addr, config)
                .handle(handle)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>());
            tokio::select! {
                result = serve => {
                    if let Err(e) = result {
                        eprintln!("rustdl: web UI HTTPS server error on {addr}: {e}");
                    }
                }
                _ = shutdown => {},
            }
        })
    } else {
        let std_listener = std::net::TcpListener::bind(addr)
            .map_err(|e| WebServerStartError::BindFailed(format!("{addr}: {e}")))?;
        std_listener
            .set_nonblocking(true)
            .map_err(|e| WebServerStartError::BindFailed(format!("{addr}: {e}")))?;
        runtime.spawn(async move {
            let listener = match tokio::net::TcpListener::from_std(std_listener) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("rustdl: web UI failed to start listener on {addr}: {e}");
                    return;
                }
            };
            let serve = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            );
            tokio::select! {
                _ = serve => {},
                _ = shutdown_rx => {},
            }
        })
    };

    Ok(WebServerHandle {
        shutdown_tx: Some(shutdown_tx),
        join: Some(join),
    })
}

#[cfg(test)]
mod tests {
    use super::{resolve_web_bind_address, web_ui_browser_url};
    use crate::config::AppSettings;

    #[test]
    fn web_ui_browser_url_maps_wildcard_bind() {
        assert_eq!(
            web_ui_browser_url(&AppSettings::default()),
            "http://127.0.0.1:8765/"
        );
    }

    #[test]
    fn web_ui_browser_url_adds_scheme() {
        let s = AppSettings {
            web_bind_address: "127.0.0.1:8765".to_owned(),
            ..Default::default()
        };
        assert_eq!(web_ui_browser_url(&s), "http://127.0.0.1:8765/");
    }

    #[test]
    fn web_ui_browser_url_uses_https_when_tls_enabled() {
        let s = AppSettings {
            web_bind_address: "127.0.0.1:8765".to_owned(),
            ..Default::default()
        };
        assert_eq!(web_ui_browser_url(&s), "http://127.0.0.1:8765/");
        let dir = std::env::temp_dir().join(format!("rustdl_tls_url_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");
        std::fs::write(&cert, "dummy").expect("cert");
        std::fs::write(&key, "dummy").expect("key");
        let s = AppSettings {
            web_bind_address: "127.0.0.1:8765".to_owned(),
            web_tls_cert_path: cert.to_string_lossy().into_owned(),
            web_tls_key_path: key.to_string_lossy().into_owned(),
            ..Default::default()
        };
        assert_eq!(web_ui_browser_url(&s), "https://127.0.0.1:8765/");
        let _ = std::fs::remove_dir_all(&dir);
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
