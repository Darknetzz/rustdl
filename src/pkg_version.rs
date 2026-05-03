//! Single source for `CARGO_PKG_VERSION` in user-visible strings and HTTP clients.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Set in `build.rs` at compile time (UTC). Falls back if the env is ever missing.
pub const BUILD_DATE: &str = option_env!("RUSTDL_BUILD_DATE").unwrap_or("unknown");
