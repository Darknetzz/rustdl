//! Single source for `CARGO_PKG_VERSION` in user-visible strings and HTTP clients.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
