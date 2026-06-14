//! LAN web UI: static assets, REST API, and SSE event stream.

mod api;
mod assets;
mod auth;
mod convert_api;
mod media;
mod server;

pub use server::{
    resolve_web_bind_address, spawn_web_server_at, try_spawn_web_server, web_ui_browser_url,
    WebServerHandle,
};
