use std::path::PathBuf;

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

