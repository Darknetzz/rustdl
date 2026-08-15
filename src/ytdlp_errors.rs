//! Classifies yt-dlp download failures for app-level retry logic.

/// Returns true when the error text suggests a transient network/connection issue.
pub fn is_transient_download_error(err: &str) -> bool {
    let msg = err.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "timed out",
        "timeout",
        "connection reset",
        "connection refused",
        "connection aborted",
        "temporary failure",
        "read timed out",
        "http error 408",
        "http error 502",
        "http error 503",
        "http error 504",
        "408",
        "502",
        "503",
        "504",
        "giving up after",
        "network is unreachable",
        "ssl:",
        "tls handshake",
        "connection broken",
        "remote end closed",
        "unexpected eof",
        "failed to establish a new connection",
        "name or service not known",
        "nodename nor servname provided",
    ];
    NEEDLES.iter().any(|needle| msg.contains(needle))
}

/// Returns true when yt-dlp rejected the selected `-f` / quality format.
pub fn is_format_unavailable_error(err: &str) -> bool {
    let msg = err.to_ascii_lowercase();
    msg.contains("requested format is not available")
        || msg.contains("format is not available")
        || msg.contains("no video formats found")
        || msg.contains("no formats found")
}

/// Short, actionable hint for common yt-dlp failures (shown on the queue row).
pub fn download_failure_user_hint(err: &str) -> Option<&'static str> {
    let msg = err.to_ascii_lowercase();
    if msg.contains("unable to extract flashvars")
        || (msg.contains("[generic]") && msg.contains("unable to extract"))
    {
        return Some(
            "yt-dlp used the generic extractor and could not read this page. \
             Use the site’s watch-page URL (not a redirect/embed), update yt-dlp, \
             and add cookies in Settings if login is required.",
        );
    }
    if msg.contains("skipping format")
        || msg.contains("unsupported url format")
        || msg.contains("no video formats found")
        || msg.contains("no formats found")
    {
        return Some(
            "yt-dlp found no usable formats for this page. Update yt-dlp, confirm ffmpeg is set in Settings, and add cookies if the site requires login.",
        );
    }
    if msg.contains("no suitable extractors") || msg.contains("unsupported url") {
        return Some("This URL is not supported by yt-dlp. Verify the link and update yt-dlp.");
    }
    if msg.contains("video unavailable") {
        return Some("Video unavailable. It may be deleted, region-locked, or require cookies.");
    }
    if msg.contains("private video") || msg.contains("members only") {
        return Some("Video is private or members-only. Add cookies in Settings and retry.");
    }
    if is_format_unavailable_error(err) {
        return Some(
            "Selected quality/format is not available. Update yt-dlp, confirm ffmpeg is set, and avoid a tight minimum height/FPS or custom -f.",
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_timeout_and_connection_errors() {
        assert!(is_transient_download_error(
            "ERROR: Unable to download video: HTTP Error 503: Service Unavailable"
        ));
        assert!(is_transient_download_error(
            "yt-dlp exited with 1\n--- recent stderr ---\nRead timed out"
        ));
        assert!(is_transient_download_error("Connection reset by peer"));
    }

    #[test]
    fn non_transient_errors() {
        assert!(!is_transient_download_error("Video unavailable"));
        assert!(!is_transient_download_error("Private video"));
        assert!(!is_transient_download_error("Cancelled by user."));
        assert!(!is_transient_download_error(
            "ERROR: [youtube] abc: Requested format is not available."
        ));
    }

    #[test]
    fn format_unavailable_errors() {
        assert!(is_format_unavailable_error(
            "ERROR: [youtube] xh5ASlG: Requested format is not available. Use --list-formats for a list of available formats"
        ));
        assert!(!is_format_unavailable_error("Video unavailable"));
    }

    #[test]
    fn generic_flashvars_hint() {
        let err =
            "ERROR: [generic] Unable to extract flashvars; please report this issue on GitHub";
        assert!(download_failure_user_hint(err).is_some());
        assert!(download_failure_user_hint(err)
            .unwrap()
            .contains("generic extractor"));
    }

    #[test]
    fn extractor_skipped_formats_hint() {
        let err = "WARNING: Skipping format \"h264-720p\": unsupported URL format\n\
             ERROR: No video formats found!";
        let hint = download_failure_user_hint(err).unwrap();
        assert!(hint.contains("no usable formats"));
        assert!(hint.contains("Update yt-dlp"));
    }

    #[test]
    fn requested_format_unavailable_hint() {
        let hint = download_failure_user_hint(
            "ERROR: Requested format is not available. Use --list-formats for a list of available formats",
        )
        .unwrap();
        assert!(hint.contains("Selected quality/format is not available"));
        assert!(hint.contains("Update yt-dlp"));
        assert!(!hint.contains("Best quality"));
    }
}
