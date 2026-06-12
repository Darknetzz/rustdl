use std::path::PathBuf;
use std::process::Command;

/// Process priority for yt-dlp / ffmpeg child processes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubprocessPriority {
    Normal,
    BelowNormal,
    Idle,
}

pub fn logical_cpu_count() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(8)
        .max(1)
}

pub fn normalize_subprocess_priority(raw: &str) -> SubprocessPriority {
    match raw.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "below_normal" | "belownormal" => SubprocessPriority::BelowNormal,
        "idle" | "low" => SubprocessPriority::Idle,
        _ => SubprocessPriority::Normal,
    }
}

pub fn subprocess_priority_storage_value(priority: SubprocessPriority) -> &'static str {
    match priority {
        SubprocessPriority::Normal => "normal",
        SubprocessPriority::BelowNormal => "below_normal",
        SubprocessPriority::Idle => "idle",
    }
}

pub fn subprocess_priority_label(priority: SubprocessPriority) -> &'static str {
    match priority {
        SubprocessPriority::Normal => "Normal",
        SubprocessPriority::BelowNormal => "Below normal",
        SubprocessPriority::Idle => "Idle",
    }
}

#[cfg(windows)]
fn subprocess_creation_flags(priority: SubprocessPriority) -> u32 {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const NORMAL_PRIORITY_CLASS: u32 = 0x0000_0020;
    const IDLE_PRIORITY_CLASS: u32 = 0x0000_0040;
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
    let class = match priority {
        SubprocessPriority::Normal => NORMAL_PRIORITY_CLASS,
        SubprocessPriority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        SubprocessPriority::Idle => IDLE_PRIORITY_CLASS,
    };
    CREATE_NO_WINDOW | class
}

#[cfg(unix)]
fn subprocess_unix_nice(priority: SubprocessPriority) -> i32 {
    match priority {
        SubprocessPriority::Normal => 0,
        SubprocessPriority::BelowNormal => 10,
        SubprocessPriority::Idle => 19,
    }
}

/// Apply hidden-console (Windows) and optional lowered priority for background work.
pub fn apply_subprocess_launch(cmd: &mut Command, priority: SubprocessPriority) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(subprocess_creation_flags(priority));
    }
    #[cfg(unix)]
    {
        if priority != SubprocessPriority::Normal {
            use std::os::unix::process::CommandExt;
            let nice = subprocess_unix_nice(priority);
            unsafe {
                cmd.pre_exec(move || {
                    libc::setpriority(libc::PRIO_PROCESS, 0, nice);
                    Ok(())
                });
            }
        }
    }
    let _ = priority;
}

pub fn apply_subprocess_launch_tokio(cmd: &mut tokio::process::Command, priority: SubprocessPriority) {
    #[cfg(windows)]
    {
        cmd.creation_flags(subprocess_creation_flags(priority));
    }
    #[cfg(unix)]
    {
        if priority != SubprocessPriority::Normal {
            let nice = subprocess_unix_nice(priority);
            unsafe {
                cmd.pre_exec(move || {
                    libc::setpriority(libc::PRIO_PROCESS, 0, nice);
                    Ok(())
                });
            }
        }
    }
    let _ = priority;
}

/// On Windows, prevent child processes (yt-dlp, ffmpeg, PowerShell, etc.) from flashing a console.
#[cfg(windows)]
pub(crate) fn no_console_window(cmd: &mut Command) {
    apply_subprocess_launch(cmd, SubprocessPriority::Normal);
}

#[cfg(not(windows))]
pub(crate) fn no_console_window(_cmd: &mut Command) {}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_subprocess_priority_values() {
        assert_eq!(
            normalize_subprocess_priority("below-normal"),
            SubprocessPriority::BelowNormal
        );
        assert_eq!(normalize_subprocess_priority("idle"), SubprocessPriority::Idle);
        assert_eq!(
            normalize_subprocess_priority("unknown"),
            SubprocessPriority::Normal
        );
    }
}

pub fn resolve_executable(custom_path: &str, default_exe: &str) -> String {
    let trimmed = custom_path.trim();
    if trimmed.is_empty() {
        default_exe.to_owned()
    } else {
        trimmed.to_owned()
    }
}
