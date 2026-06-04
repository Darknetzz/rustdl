//! Single source for `CARGO_PKG_VERSION` in user-visible strings and HTTP clients.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// UTC epoch seconds at compile time (`build.rs`). Older builds may lack this.
pub const BUILD_UNIX: Option<&str> = option_env!("RUSTDL_BUILD_UNIX");

/// Legacy UTC label from older builds (before `RUSTDL_BUILD_UNIX`).
pub const BUILD_DATE: &str = match option_env!("RUSTDL_BUILD_DATE") {
    Some(s) => s,
    None => "unknown",
};

/// Build time in the local timezone of the machine running rustdl.
pub fn build_date_local() -> String {
    use chrono::{Local, TimeZone, Utc};
    if let Some(s) = BUILD_UNIX {
        if let Ok(ts) = s.parse::<i64>() {
            if let Some(dt) = Utc.timestamp_opt(ts, 0).single() {
                return dt
                    .with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M %Z")
                    .to_string();
            }
        }
    }
    BUILD_DATE.to_string()
}
