use std::net::SocketAddr;
use std::sync::Arc;

use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::config::AppSettings;
use crate::service::core::SharedCore;
use crate::service::web::api::{ApiState, api_router};

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

pub fn spawn_web_server(
    runtime: Arc<Runtime>,
    core: SharedCore,
    settings: &AppSettings,
) -> Option<WebServerHandle> {
    if !settings.web_ui_enabled {
        return None;
    }
    let bind = settings.web_bind_address.trim();
    if bind.is_empty() {
        eprintln!("rustdl: web UI enabled but bind address is empty");
        return None;
    }
    let addr: SocketAddr = match bind.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("rustdl: invalid web bind address {bind:?}: {e}");
            return None;
        }
    };
    if settings.web_auth_token.trim().is_empty() {
        eprintln!("rustdl: web UI requires a non-empty auth token (see Settings → Shared)");
        return None;
    }

    let state = ApiState {
        core: core.clone(),
    };
    let app = api_router(state);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let join = runtime.spawn(async move {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("rustdl: web UI failed to bind {addr}: {e}");
                return;
            }
        };
        eprintln!("rustdl: web UI listening on http://{addr}");
        let serve = axum::serve(listener, app);
        tokio::select! {
            _ = serve => {},
            _ = shutdown_rx => {},
        }
    });

    Some(WebServerHandle {
        shutdown_tx: Some(shutdown_tx),
        join: Some(join),
    })
}
