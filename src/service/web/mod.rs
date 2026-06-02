//! LAN web UI: static assets, REST API, and SSE event stream.

mod api;
mod assets;
mod auth;
mod media;
mod server;

pub use server::{spawn_web_server, WebServerHandle};
