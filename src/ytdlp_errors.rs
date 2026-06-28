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
}
