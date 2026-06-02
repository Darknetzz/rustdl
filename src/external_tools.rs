use std::path::PathBuf;
use std::process::Command;

/// On Windows, prevent child processes (yt-dlp, ffmpeg, PowerShell, etc.) from flashing a console.
#[cfg(windows)]
pub(crate) fn no_console_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn no_console_window(_cmd: &mut Command) {}

#[cfg(windows)]
pub(crate) fn no_console_window_tokio(cmd: &mut tokio::process::Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub(crate) fn no_console_window_tokio(_cmd: &mut tokio::process::Command) {}

pub fn which(exe: &str) -> Option<PathBuf> {
    which::which(exe).ok()
}

pub fn executable_exists(custom_path: &str, default_exe: &str) -> bool {
    let trimmed = custom_path.trim();
    if !trimmed.is_empty() {
        return PathBuf::from(trimmed).is_file() || which(trimmed).is_some();
    }
    which(default_exe).is_some()
}

pub fn resolve_executable(custom_path: &str, default_exe: &str) -> String {
    let trimmed = custom_path.trim();
    if trimmed.is_empty() {
        default_exe.to_owned()
    } else {
        trimmed.to_owned()
    }
}
