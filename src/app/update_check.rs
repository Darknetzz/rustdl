use std::process::Command;

use reqwest::Client;

use crate::app_parsing::is_version_newer;
use crate::pkg_version;

/// Parses GitHub `releases/latest` JSON body into tag (without leading `v`) and release page URL.
pub(crate) fn parse_latest_release_json(raw: &str) -> Result<(String, String), String> {
    let json: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .trim_start_matches('v')
        .to_owned();
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    if tag.is_empty() || html_url.is_empty() {
        return Err("Missing release tag/url in API response".to_owned());
    }
    Ok((tag, html_url))
}

pub(crate) async fn check_latest_release_async(
    client: &Client,
) -> Result<(String, String, bool), String> {
    let (owner, repo) = detect_github_repo().ok_or_else(|| {
        "Could not detect GitHub repository (set package.repository or run from git checkout)"
            .to_owned()
    })?;
    let api = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let resp = client
        .get(&api)
        .header("User-Agent", "rustdl-update-check")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let raw = resp.text().await.map_err(|e| e.to_string())?;
    let (tag, html_url) = parse_latest_release_json(&raw)?;
    let newer = is_version_newer(&tag, pkg_version::VERSION);
    Ok((tag, html_url, newer))
}

pub(crate) fn detect_github_repo() -> Option<(String, String)> {
    if let Some(repo_url) = option_env!("CARGO_PKG_REPOSITORY") {
        if let Some(parsed) = parse_github_owner_repo(repo_url) {
            return Some(parsed);
        }
    }
    let out = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let remote = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    parse_github_owner_repo(&remote)
}

fn parse_github_owner_repo(url: &str) -> Option<(String, String)> {
    let u = url.trim();
    if u.is_empty() {
        return None;
    }
    let u = u
        .strip_prefix("git@github.com:")
        .or_else(|| u.strip_prefix("ssh://git@github.com/"))
        .unwrap_or(u);
    let u = u.strip_prefix("https://github.com/").unwrap_or(u);
    let u = u.strip_prefix("http://github.com/").unwrap_or(u);
    let u = u.strip_suffix(".git").unwrap_or(u);
    let mut parts = u.split('/').filter(|s| !s.is_empty());
    let owner = parts.next()?.to_owned();
    let repo = parts.next()?.to_owned();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo))
}

#[cfg(test)]
mod tests {
    use super::{parse_github_owner_repo, parse_latest_release_json};

    #[test]
    fn parse_release_json_tag_and_url() {
        let raw =
            r#"{"tag_name":"v1.2.3","html_url":"https://github.com/o/r/releases/tag/v1.2.3"}"#;
        let (tag, url) = parse_latest_release_json(raw).unwrap();
        assert_eq!(tag, "1.2.3");
        assert_eq!(url, "https://github.com/o/r/releases/tag/v1.2.3");
    }

    #[test]
    fn parse_release_json_rejects_empty_tag() {
        let raw = r#"{"tag_name":"","html_url":"https://x"}"#;
        assert!(parse_latest_release_json(raw).is_err());
    }

    #[test]
    fn parse_github_https() {
        assert_eq!(
            parse_github_owner_repo("https://github.com/foo/bar"),
            Some(("foo".to_owned(), "bar".to_owned()))
        );
    }

    #[test]
    fn parse_github_ssh() {
        assert_eq!(
            parse_github_owner_repo("git@github.com:org/repo.git"),
            Some(("org".to_owned(), "repo".to_owned()))
        );
    }
}
