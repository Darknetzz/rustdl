use rustdl::ytdlp::{self, PROGRESS_PREFIX};
use serde_json::json;

#[test]
fn progress_fixture_template_prefix() {
    let line = format!("{PROGRESS_PREFIX} 42.5% of  100.00MiB at  2.00MiB/s ETA 00:30");
    let (pct, size) = ytdlp::parse_progress_line(&line);
    assert_eq!(pct, Some(42.5));
    assert_eq!(size.as_deref(), Some("100.00MiB"));
}

#[test]
fn progress_fixture_regular_download_line() {
    let line = "[download]  10.0% of 50.00MiB at 1.00MiB/s ETA 00:40";
    let (pct, size) = ytdlp::parse_progress_line(line);
    assert_eq!(pct, Some(10.0));
    assert_eq!(size.as_deref(), Some("50.00MiB"));
}

#[test]
fn progress_fixture_no_match_returns_none() {
    let (pct, size) = ytdlp::parse_progress_line("Starting download...");
    assert!(pct.is_none());
    assert!(size.is_none());
}

#[test]
fn download_args_fixture_best_quality() {
    use rustdl::config::AppSettings;
    use rustdl::ytdlp_download_args::{build_download_extra_args, quality_format_args};

    let settings = AppSettings::default();
    let quality = quality_format_args(&settings);
    assert_eq!(quality[0], "-f");
    assert!(quality[1].contains("bestvideo"));

    let args = build_download_extra_args(&settings);
    assert!(args.windows(2).any(|w| w[0] == "-f"));
}

#[test]
fn max_video_resolution_from_entry_picks_highest_format() {
    let entry = json!({
        "formats": [
            { "height": 720, "width": 1280, "vcodec": "avc1" },
            { "height": 1080, "width": 1920, "vcodec": "avc1" }
        ]
    });
    let (w, h) = ytdlp::max_video_resolution_from_entry(&entry);
    assert_eq!(h, Some(1080));
    assert_eq!(w, Some(1920));
}

#[test]
fn parse_flat_playlist_json_caps_entries() {
    let root = json!({
        "entries": [
            { "url": "https://example.com/1", "title": "One" },
            { "url": "https://example.com/2", "title": "Two" },
            { "url": "https://example.com/3", "title": "Three" }
        ]
    });
    let preview = ytdlp::parse_flat_playlist_json(&root, 2);
    assert_eq!(preview.urls.len(), 2);
}
