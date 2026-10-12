use std::path::{Path, PathBuf};
use std::process::Command;

pub fn open_path(path: &Path) -> Result<(), String> {
    opener::open(path).map_err(|e| e.to_string())
}

pub fn open_str_path(path: &str) -> Result<(), String> {
    opener::open(path).map_err(|e| e.to_string())
}

pub fn open_browser(url: &str) -> Result<(), String> {
    opener::open_browser(url).map_err(|e| e.to_string())
}

/// Open a text/log file: prefer Notepad++ on Windows when installed, else the OS default handler.
pub fn open_text_log(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        if let Some(npp) = find_notepad_plus_plus() {
            return open_with_notepad_plus_plus(&npp, path);
        }
    }
    open_path(path)
}

#[cfg(windows)]
fn open_with_notepad_plus_plus(npp: &Path, path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new(npp)
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch Notepad++: {e}"))
}

#[cfg(windows)]
fn find_notepad_plus_plus() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        r"C:\Program Files\Notepad++\notepad++.exe",
        r"C:\Program Files (x86)\Notepad++\notepad++.exe",
    ];
    for candidate in CANDIDATES {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(local).join(r"Programs\Notepad++\notepad++.exe");
        if p.is_file() {
            return Some(p);
        }
    }
    which_on_path("notepad++.exe")
}

#[cfg(windows)]
fn which_on_path(exe: &str) -> Option<PathBuf> {
    let Ok(path_var) = std::env::var("PATH") else {
        return None;
    };
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn pick_url_input_file() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Select URL list file")
        .add_filter("URL files", &["txt", "csv"])
        .add_filter("Text", &["txt"])
        .add_filter("CSV", &["csv"])
        .pick_file()
}

pub fn pick_convert_input_files() -> Vec<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Select video files for conversion")
        .add_filter("Video files", crate::transcode::CONVERT_VIDEO_EXTENSIONS)
        .pick_files()
        .unwrap_or_default()
}

pub fn pick_convert_input_folder() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Select folder for conversion")
        .pick_folder()
}
