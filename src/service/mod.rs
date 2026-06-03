//! Shared download service and LAN web control plane.

pub mod core;
pub mod core_av1;
pub mod core_events;
pub mod web;

pub use core::{CancelPostAction, DownloadCore, SharedCore};

use std::sync::Arc;

/// Handle shared by the egui app and the optional web server.
pub struct RustdlService {
    pub core: SharedCore,
}

impl RustdlService {
    pub fn new(
        runtime: Arc<tokio::runtime::Runtime>,
    ) -> (Self, crossbeam_channel::Receiver<crate::app::UiEvent>) {
        let (core, rx) = DownloadCore::new_shared(runtime.clone());
        core_events::spawn_core_event_loop(runtime, core.clone());
        (Self { core: core.clone() }, rx)
    }

    pub fn shared_core(&self) -> SharedCore {
        self.core.clone()
    }
}
