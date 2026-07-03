use std::path::{Path, PathBuf};
use std::process::Command;

use reqwest::Client;
use serde_json::Value;

use crate::app_parsing::is_version_newer;
use crate::external_tools::no_console_window;
use crate::pkg_version;

/// Release asset URLs from the GitHub API (`browser_download_url` and `url`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlatformReleaseAsset {
    pub browser_download_url: String,
    pub api_url: String,
}

/// Semver string from a GitHub release tag (`rustdl-v0.5.0`, `v0.5.0`, `0.5.0`, …).
pub(crate) fn normalize_release_tag(tag: &str) -> String {
    let t = tag.trim();
    if let Some(rest) = t.strip_prefix("rustdl-v") {
        return rest.to_owned();
    }
    if let Some(rest) = t.strip_prefix("rustdl-") {
        return rest.trim_start_matches('v').to_owned();
    }
    t.trim_start_matches('v').to_owned()
}

/// Parses a GitHub release JSON object into version, release page URL, and optional platform asset URL.
pub(crate) fn parse_release_json(
    release: &Value,
) -> Result<(String, String, Option<PlatformReleaseAsset>), String> {
    let tag_name = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    let version = normalize_release_tag(tag_name);
    let html_url = release
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    if version.is_empty() || html_url.is_empty() {
        return Err("Missing release tag/url in API response".to_owned());
    }
    let download_asset = pick_platform_release_asset(release);
    Ok((version, html_url, download_asset))
}

pub(crate) fn pick_platform_release_asset(release: &Value) -> Option<PlatformReleaseAsset> {
    let assets = release.get("assets")?.as_array()?;
    let preferred = platform_asset_names();
    for name in preferred {
        for asset in assets {
            if asset.get("name").and_then(|v| v.as_str()) == Some(name) {
                let browser_download_url = asset
                    .get("browser_download_url")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)?;
                let api_url = asset
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&browser_download_url)
                    .to_owned();
                return Some(PlatformReleaseAsset {
                    browser_download_url,
                    api_url,
                });
            }
        }
    }
    None
}

/// Legacy helper for tests — returns the browser download URL only.
#[cfg(test)]
pub(crate) fn pick_platform_asset_url(release: &Value) -> Option<String> {
    pick_platform_release_asset(release).map(|asset| asset.browser_download_url)
}

fn platform_asset_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["rustdl.exe"]
    }
    #[cfg(target_os = "macos")]
    {
        &[
            "rustdl-aarch64-apple-darwin",
            "rustdl-x86_64-apple-darwin",
            "rustdl",
        ]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &["rustdl-x86_64-unknown-linux-gnu", "rustdl"]
    }
    #[cfg(not(any(windows, unix)))]
    {
        &["rustdl"]
    }
}

fn pick_latest_published_release(releases: &[Value]) -> Option<&Value> {
    releases.iter().find(|release| {
        !release
            .get("draft")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
            && !release
                .get("prerelease")
                .and_then(|v| v.as_bool())
                .unwrap_or(true)
    })
}

pub(crate) async fn check_latest_release_async(
    client: &Client,
    github_token: Option<&str>,
) -> Result<(String, String, Option<PlatformReleaseAsset>, bool), String> {
    let has_token = github_token.is_some_and(|token| !token.trim().is_empty());
    let (owner, repo) = detect_github_repo();
    let api_latest = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let resp = github_request(client, &api_latest, github_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let (version, html_url, download_asset) = if resp.status().as_u16() == 404 {
        match fetch_latest_from_release_list(client, &owner, &repo, github_token).await {
            Ok(release) => release,
            Err(err) if !has_token => {
                let _ = err;
                return Err(github_api_error(resp.status(), has_token));
            }
            Err(err) => return Err(err),
        }
    } else if resp.status().is_success() {
        let raw = resp.text().await.map_err(|e| e.to_string())?;
        let json: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        parse_release_json(&json)?
    } else {
        return Err(github_api_error(resp.status(), has_token));
    };

    let newer = is_version_newer(&version, pkg_version::VERSION);
    Ok((version, html_url, download_asset, newer))
}

fn github_request(
    client: &Client,
    url: &str,
    github_token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = client.get(url).header("User-Agent", "rustdl-update-check");
    if let Some(token) = github_token.filter(|value| !value.trim().is_empty()) {
        req = req.header("Authorization", format!("Bearer {}", token.trim()));
    }
    req
}

fn github_api_error(status: reqwest::StatusCode, has_token: bool) -> String {
    if status.as_u16() == 404 && !has_token {
        "GitHub returned 404. If the repository is private, add a personal access token in \
         Settings → General → Backup (GitHub releases token), or set RUSTDL_GITHUB_TOKEN."
            .to_owned()
    } else if status.as_u16() == 404 {
        "GitHub returned 404 (no releases found, or the token lacks access to this repository)."
            .to_owned()
    } else if status.as_u16() == 401 {
        "GitHub rejected the token (HTTP 401). Check Settings → General → Backup → GitHub releases token."
            .to_owned()
    } else {
        format!("HTTP {status}")
    }
}

async fn fetch_latest_from_release_list(
    client: &Client,
    owner: &str,
    repo: &str,
    github_token: Option<&str>,
) -> Result<(String, String, Option<PlatformReleaseAsset>), String> {
    let has_token = github_token.is_some_and(|token| !token.trim().is_empty());
    let api = format!("https://api.github.com/repos/{owner}/{repo}/releases?per_page=30");
    let resp = github_request(client, &api, github_token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(github_api_error(resp.status(), has_token));
    }
    let raw = resp.text().await.map_err(|e| e.to_string())?;
    let releases: Vec<Value> = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let release = pick_latest_published_release(&releases)
        .ok_or_else(|| "No published GitHub releases found".to_owned())?;
    parse_release_json(release)
}

pub(crate) async fn download_release_asset_async(
    client: &Client,
    asset: &PlatformReleaseAsset,
    version: &str,
    github_token: Option<&str>,
) -> Result<PathBuf, String> {
    let has_token = github_token.is_some_and(|token| !token.trim().is_empty());
    let req = if has_token {
        client
            .get(&asset.api_url)
            .header("Accept", "application/octet-stream")
            .header(
                "Authorization",
                format!("Bearer {}", github_token.unwrap_or("").trim()),
            )
    } else {
        client.get(&asset.browser_download_url)
    };
    let resp = req
        .header("User-Agent", "rustdl-update-download")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "Download failed: {}",
            github_api_error(resp.status(), has_token)
        ));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let mut path = std::env::temp_dir();
    #[cfg(windows)]
    {
        path.push(format!("rustdl-{version}.exe"));
    }
    #[cfg(not(windows))]
    {
        path.push(format!("rustdl-{version}"));
    }
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|e| format!("Failed to save update: {e}"))?;
    Ok(path)
}

/// Replace the running binary on Windows after exit (detached PowerShell helper).
#[cfg(windows)]
pub(crate) fn schedule_apply_downloaded_update(downloaded: &Path) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let script_path =
        std::env::temp_dir().join(format!("rustdl-apply-update-{}.ps1", std::process::id()));
    let downloaded = downloaded.display().to_string().replace('\'', "''");
    let current = current.display().to_string().replace('\'', "''");
    let script_path_ps = script_path.display().to_string().replace('\'', "''");
    let script = format!(
        r#"Start-Sleep -Seconds 2
$dst = '{current}'
$src = '{downloaded}'
$bak = "$dst.bak"
if (Test-Path -LiteralPath $bak) {{ Remove-Item -LiteralPath $bak -Force -ErrorAction SilentlyContinue }}
if (Test-Path -LiteralPath $dst) {{ Move-Item -LiteralPath $dst -Destination $bak -Force }}
Move-Item -LiteralPath $src -Destination $dst -Force
Start-Process -FilePath $dst
Start-Sleep -Seconds 5
Remove-Item -LiteralPath $bak -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath '{script_path_ps}' -Force -ErrorAction SilentlyContinue
"#
    );
    std::fs::write(&script_path, script).map_err(|e| e.to_string())?;
    let mut cmd = Command::new("powershell");
    no_console_window(&mut cmd);
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
        "-File",
        &script_path.to_string_lossy(),
    ]);
    cmd.spawn()
        .map_err(|e| format!("Failed to start updater: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn schedule_apply_downloaded_update(downloaded: &Path) -> Result<(), String> {
    let _ = downloaded;
    Err(
        "In-app install is only supported on Windows; open the release page to update manually."
            .to_owned(),
    )
}

pub(crate) fn detect_github_repo() -> (String, String) {
    if let Some(repo_url) = option_env!("CARGO_PKG_REPOSITORY") {
        if let Some(parsed) = parse_github_owner_repo(repo_url) {
            return parsed;
        }
    }
    for remote in ["github", "origin"] {
        let mut cmd = Command::new("git");
        no_console_window(&mut cmd);
        if let Ok(out) = cmd
            .args(["config", "--get", &format!("remote.{remote}.url")])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
        {
            if out.status.success() {
                let url = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                if is_github_remote(&url) {
                    if let Some(parsed) = parse_github_owner_repo(&url) {
                        return parsed;
                    }
                }
            }
        }
    }
    (
        pkg_version::GITHUB_OWNER.to_owned(),
        pkg_version::GITHUB_REPO.to_owned(),
    )
}

fn is_github_remote(url: &str) -> bool {
    url.trim().to_ascii_lowercase().contains("github.com")
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
    use super::{
        detect_github_repo, normalize_release_tag, parse_github_owner_repo, parse_release_json,
        pick_platform_asset_url,
    };
    use crate::pkg_version;

    #[test]
    fn normalize_release_tag_handles_rustdl_prefix() {
        assert_eq!(normalize_release_tag("rustdl-v0.5.0"), "0.5.0");
        assert_eq!(normalize_release_tag("v1.2.3"), "1.2.3");
    }

    #[test]
    fn parse_release_json_tag_and_url() {
        let raw =
            r#"{"tag_name":"v1.2.3","html_url":"https://github.com/o/r/releases/tag/v1.2.3"}"#;
        let json: serde_json::Value = serde_json::from_str(raw).unwrap();
        let (tag, url, asset) = parse_release_json(&json).unwrap();
        assert_eq!(tag, "1.2.3");
        assert_eq!(url, "https://github.com/o/r/releases/tag/v1.2.3");
        assert!(asset.is_none());
    }

    #[test]
    fn github_api_error_private_repo_hint() {
        assert!(super::github_api_error(reqwest::StatusCode::NOT_FOUND, false).contains("private"));
    }

    #[test]
    fn parse_release_json_rustdl_tag_and_windows_asset() {
        let raw = r#"{
            "tag_name":"rustdl-v0.5.0",
            "html_url":"https://github.com/o/r/releases/tag/rustdl-v0.5.0",
            "assets":[{"name":"rustdl.exe","url":"https://api.github.com/repos/o/r/releases/assets/1","browser_download_url":"https://example.com/rustdl.exe"}]
        }"#;
        let json: serde_json::Value = serde_json::from_str(raw).unwrap();
        let (tag, url, asset) = parse_release_json(&json).unwrap();
        assert_eq!(tag, "0.5.0");
        assert_eq!(url, "https://github.com/o/r/releases/tag/rustdl-v0.5.0");
        #[cfg(windows)]
        {
            let asset = asset.unwrap();
            assert_eq!(asset.browser_download_url, "https://example.com/rustdl.exe");
            assert_eq!(
                asset.api_url,
                "https://api.github.com/repos/o/r/releases/assets/1"
            );
        }
        #[cfg(not(windows))]
        assert!(asset.is_none());
    }

    #[test]
    fn parse_release_json_rejects_empty_tag() {
        let raw = r#"{"tag_name":"","html_url":"https://x"}"#;
        let json: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert!(parse_release_json(&json).is_err());
    }

    #[test]
    fn pick_platform_asset_url_prefers_rustdl_exe_on_windows() {
        let raw = r#"{"assets":[{"name":"notes.md","browser_download_url":"https://x/n"},{"name":"rustdl.exe","browser_download_url":"https://x/e"}]}"#;
        let json: serde_json::Value = serde_json::from_str(raw).unwrap();
        #[cfg(windows)]
        assert_eq!(
            pick_platform_asset_url(&json).as_deref(),
            Some("https://x/e")
        );
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

    #[test]
    fn detect_github_repo_defaults_to_rustdl() {
        let (owner, repo) = detect_github_repo();
        assert_eq!(owner, pkg_version::GITHUB_OWNER);
        assert_eq!(repo, pkg_version::GITHUB_REPO);
    }
}
