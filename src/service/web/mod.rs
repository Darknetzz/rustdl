//! LAN web UI: static assets, REST API, and SSE event stream.

mod api;
mod assets;
mod auth;
mod media;
mod server;

pub use server::{
    resolve_web_bind_address, spawn_web_server, spawn_web_server_at, web_ui_browser_url,
    WebServerHandle,
};
