use std::path::Path;

pub fn open_path(path: &Path) -> Result<(), String> {
    opener::open(path).map_err(|e| e.to_string())
}

pub fn open_str_path(path: &str) -> Result<(), String> {
    opener::open(path).map_err(|e| e.to_string())
}

pub fn open_browser(url: &str) -> Result<(), String> {
    opener::open_browser(url).map_err(|e| e.to_string())
}

pub fn pick_url_input_file() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Select URL list file")
        .add_filter("URL files", &["txt", "csv"])
        .add_filter("Text", &["txt"])
        .add_filter("CSV", &["csv"])
        .pick_file()
}

pub fn pick_av1_input_files() -> Vec<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Select video files for AV1 conversion")
        .add_filter(
            "Video files",
            &["mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv"],
        )
        .pick_files()
        .unwrap_or_default()
}

pub fn pick_av1_input_folder() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Select folder for AV1 conversion")
        .pick_folder()
}
