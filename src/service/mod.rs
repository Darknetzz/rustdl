//! Shared download service and LAN web control plane.

pub mod core;
pub mod core_convert;
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
        Self::new_with_restore_policy(runtime, true)
    }

    pub fn new_gui(
        runtime: Arc<tokio::runtime::Runtime>,
    ) -> (Self, crossbeam_channel::Receiver<crate::app::UiEvent>) {
        let settings = crate::config::load_settings();
        let auto = crate::config::session_restore_auto_load(&settings.session_restore_preference);
        Self::new_with_restore_policy(runtime, auto)
    }

    fn new_with_restore_policy(
        runtime: Arc<tokio::runtime::Runtime>,
        auto_restore: bool,
    ) -> (Self, crossbeam_channel::Receiver<crate::app::UiEvent>) {
        let (core, rx) = DownloadCore::new_shared(runtime.clone(), auto_restore);
        core_events::spawn_core_event_loop(runtime, core.clone());
        (Self { core: core.clone() }, rx)
    }

    pub fn shared_core(&self) -> SharedCore {
        self.core.clone()
    }
}
