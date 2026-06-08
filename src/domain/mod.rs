//! Shared domain types used by the GUI, download core, and LAN web API.

pub mod done_file_index;
pub mod events;

pub use done_file_index::{DoneFileIndex, DONE_LOOKUP_MAX_ENTRIES};
pub use events::{is_throttled_download_log_line, try_send_ui, UiEvent, UiEventBus};
