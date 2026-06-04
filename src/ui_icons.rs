//! Material Icons strings for buttons. Emojis often render as empty squares because the default
//! egui font does not include color emoji glyphs on Windows.
use egui_material_icons::icons as m;

pub const CLEAR_LOG: &str = m::ICON_CLEAR_ALL;
/// Clear a list/queue (same glyph as clear log).
pub const CLEAR_QUEUE: &str = m::ICON_CLEAR_ALL;
/// Open activity / download log window.
pub const LOGS: &str = m::ICON_ARTICLE;
pub const IMPORT_FILE: &str = m::ICON_DESCRIPTION;
pub const EXPORT: &str = m::ICON_UPLOAD;
pub const USE_DOWNLOADS: &str = m::ICON_DOWNLOAD;
pub const OPEN_FOLDER: &str = m::ICON_FOLDER_OPEN;
pub const BROWSE: &str = m::ICON_FOLDER_OPEN;
pub const RETRY: &str = m::ICON_REFRESH;
/// Stop active work and return items to ready (idle).
pub const CANCEL_TO_READY: &str = m::ICON_STOP;
pub const CANCEL_TO_REMOVE: &str = m::ICON_DELETE_FOREVER;
pub const RECHECK: &str = m::ICON_SEARCH;
pub const SCAN: &str = m::ICON_SCAN;
pub const SETTINGS: &str = m::ICON_SETTINGS;
pub const SAVE: &str = m::ICON_SAVE;
pub const DISMISS: &str = m::ICON_CLOSE;
pub const CLOSE: &str = m::ICON_CLOSE;
pub const EXIT: &str = m::ICON_EXIT_TO_APP;
pub const ADD: &str = m::ICON_ADD;
pub const REMOVE: &str = m::ICON_DELETE;
pub const TOKEN: &str = m::ICON_VPN_KEY;
pub const RESET: &str = m::ICON_RESTART_ALT;

pub const NAV_DOWNLOADER: &str = m::ICON_CLOUD_DOWNLOAD;
pub const NAV_AV1: &str = m::ICON_VIDEO_SETTINGS;
pub const VIDEOS: &str = m::ICON_VIDEO_LIBRARY;
pub const DOCK_VIDEOS: &str = m::ICON_DOCK_TO_BOTTOM;
pub const UNDOCK_VIDEOS: &str = m::ICON_OPEN_IN_NEW;
pub const DOCK_LOG: &str = m::ICON_DOCK_TO_BOTTOM;
pub const UNDOCK_LOG: &str = m::ICON_OPEN_IN_NEW;
pub const SHOW_ALL: &str = m::ICON_LIST;
pub const CLEAR_SEARCH: &str = m::ICON_CLOSE;

pub const TAB_SHARED: &str = m::ICON_TUNE;
pub const TAB_DOWNLOADER: &str = m::ICON_CLOUD_DOWNLOAD;
pub const TAB_AV1: &str = m::ICON_VIDEO_SETTINGS;

pub const PRESET_BEST: &str = m::ICON_STAR;
pub const PRESET_AUDIO: &str = m::ICON_MUSIC_NOTE;
pub const PRESET_FAST: &str = m::ICON_BOLT;
pub const PRESET_ARCHIVE: &str = m::ICON_ARCHIVE;

pub const COPY_CLIPBOARD: &str = m::ICON_CONTENT_PASTE;
pub const UPDATE_CHECK: &str = m::ICON_SYNC;
/// Open release / get update (browser).
pub const UPDATE_OPEN: &str = m::ICON_OPEN_IN_NEW;

pub const CHECK_STREAMS: &str = m::ICON_SEARCH;
pub const REDOWNLOAD: &str = m::ICON_REFRESH;
pub const OPEN_FILE: &str = m::ICON_PLAY_CIRCLE;
pub const PLAY: &str = m::ICON_PLAY_ARROW;
pub const ARROW_FORWARD: &str = m::ICON_ARROW_FORWARD;
pub const REVEAL_FOLDER: &str = m::ICON_LAUNCH;
pub const CARD_DELETE: &str = m::ICON_DELETE;

/// Per-video status chip glyphs (aligned with web `statusChipIcon`).
pub const STATUS_RESOLVING: &str = m::ICON_SEARCH;
pub const STATUS_IDLE: &str = m::ICON_HOURGLASS_EMPTY;
pub const STATUS_QUEUED: &str = m::ICON_SCHEDULE;
pub const STATUS_DOWNLOADING: &str = m::ICON_DOWNLOAD;
pub const STATUS_DONE: &str = m::ICON_CHECK;
pub const STATUS_FAILED: &str = m::ICON_CLOSE;
pub const STATUS_SKIPPED: &str = m::ICON_BLOCK;
