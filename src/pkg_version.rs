//! Single source for `CARGO_PKG_VERSION` in user-visible strings and HTTP clients.

include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build time in the local timezone of the machine running rustdl.
pub fn build_date_local() -> String {
    use chrono::{Local, TimeZone, Utc};
    if let Some(dt) = Utc.timestamp_opt(BUILD_UNIX, 0).single() {
        return dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M %Z")
            .to_string();
    }
    BUILD_DATE_UTC.to_string()
}
