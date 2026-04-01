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
