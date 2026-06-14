# AGENTS.md

Guidance for AI agents and automation working in this repository.

## What this project is

**rustdl** is a desktop application (Rust + [eframe](https://github.com/emilk/egui)/egui) for managing [yt-dlp](https://github.com/yt-dlp/yt-dlp) download queues. It also includes:

- **Downloader mode** — paste URLs, preview cards, queue downloads, activity log, profiles, settings.
- **Video Converter mode** — local file/folder transcoding via ffmpeg (AV1 default; H.265/H.264 optional; desktop GUI and LAN web UI).
- **Optional LAN web UI** — Axum HTTP server + embedded `web-assets/` for remote downloader and converter queue control.
- **Headless CLI** — `--download`, `--web-only`, `--list-profiles` (see `README.md`).

The crate library root is `src/lib.rs`; the binary calls `rustdl::main_entry()` from `src/main.rs`.

**Canonical repository:** https://github.com/Darknetzz/rustdl (GitLab mirror: https://gitlab.roste.org/kriss/rustdl). User-facing links, `Cargo.toml` `repository` / `homepage`, and `pkg_version::GITHUB_*` constants should all use that GitHub URL—not the old monorepo [Darknetzz/code](https://github.com/Darknetzz/code) path `Rust/rustdl/` (historical context only; see `MIGRATION.md`). Default branch on GitHub/GitLab is **`dev`**.

## Building the binary

**Prefer the repo build scripts** (they `cd` to the repo root and run `cargo build --release`):

| Platform | Command |
|----------|---------|
| Windows (PowerShell) | `.\scripts\build_binary.ps1` |
| Linux / macOS | `./scripts/build_binary.sh` |

Output:

- Windows: `target\release\rustdl.exe`
- Unix: `target/release/rustdl`

On Windows, release builds fail with “Access is denied” if `rustdl.exe` is still running (e.g. symlink on `PATH`). Close the app first, then rebuild.

Do **not** assume `cargo build --release` from an arbitrary cwd unless you have already changed to the repo root.

## Running and testing locally

```bash
cargo run                    # GUI (default)
cargo run -- --download URL  # headless download
cargo check                  # fast compile check when exe is locked
```

Before a PR, run the local CI script (GitHub Actions workflows are manual-only to avoid runner cost):

| Platform | Command |
|----------|---------|
| Windows (PowerShell) | `.\scripts\ci_local.ps1` |
| Linux / macOS | `./scripts/ci_local.sh` |

Includes `fmt`, `clippy`, `test`, `cargo deny`, and `cargo audit`. Pass `-SkipDeny` / `-SkipAudit` (or `--skip-deny` / `--skip-audit`) to skip the optional tools.

MSRV: **Rust 1.76+** (`rust-version` in `Cargo.toml`).

## Repository layout

| Path | Role |
|------|------|
| `src/app/` | egui UI: `eframe_app.rs` (main window), `videos_panel.rs`, `cards.rs`, `settings_panel.rs`, `log_panel.rs`, `convert_panel.rs` |
| `src/app_ui.rs` | Shared UI helpers (buttons, badges, layout, `compute_main_column_split`) |
| `src/service/` | Background core + Tokio; `web/` for LAN API and static assets |
| `src/ytdlp.rs`, `src/ytdlp_download_args.rs` | yt-dlp invocation and argument building |
| `src/config.rs`, `src/models.rs`, `src/profiles.rs` | Settings, queue models, download profiles |
| `src/cli.rs` | CLI / `--web-only` entry |
| `web-assets/` | LAN web UI (`index.html`, `app.js`, `style.css`) |
| `tests/` | Integration tests (ytdlp fixtures, queue perf, subprocess smoke) |
| `scripts/build_binary.ps1`, `scripts/build_binary.sh` | Release binary build |
| `scripts/bump_version.ps1`, `scripts/bump_version.sh` | Semver bump in `Cargo.toml` + annotated `rustdl-vX.Y.Z` tag on the bump commit |
| `scripts/release.ps1`, `scripts/release.sh` | Cut a release (finalize changelog, commit, tag, optional push) |
| `scripts/publish_dev_release.ps1`, `scripts/publish_dev_release.sh` | Build and refresh the rolling **`rustdl-dev`** GitHub pre-release |
| `scripts/push_dev.ps1`, `scripts/push_dev.sh` | Push `dev` to GitHub, publish rolling dev release, mirror GitLab |
| `scripts/install_dev_release_hook.ps1`, `scripts/install_dev_release_hook.sh` | One-time: enable `.githooks/pre-push` auto-publish on `git push github dev` |
| `.githooks/pre-push` | Git hook (via `core.hooksPath`) — schedules rolling dev release after push |
| `scripts/dev_release_webhook.py` | Optional GitHub **push** webhook listener (no GitHub Actions) |
| `packaging/winget/Darknetzz.rustdl.yaml` | Example [winget](https://github.com/microsoft/winget-cli) manifest (portable `rustdl.exe` from GitHub Releases) |
| `deny.toml` | `cargo deny` policy (CI on `dev` pushes) |

User data (not in repo): `<config_dir>/rustdl/` — `rustdl_config.json`, `rustdl_queue.json`, `rustdl_activity_log.json`. See `README.md` for paths.

## Architecture notes for code changes

- **UI thread vs background work**: `PydlApp` in `src/app/mod.rs` owns egui state; download/encode work runs on a shared Tokio `Runtime` via `src/service/core.rs`. UI updates arrive through channels (`src/app/events.rs`, `core_sync.rs`).
- **Queue persistence**: `src/app/queue_persist.rs` saves/restores the downloader queue; converter queue in `src/convert_state.rs` / `rustdl_convert_queue.json`.
- **External tools**: Resolved via `PATH` or custom paths in settings (`src/external_tools.rs`). Requires `yt-dlp`; `ffmpeg` / `ffprobe` optional but needed for many features.
- **Windows-only**: Browser URL drag-and-drop (`src/win_drop_target.rs`), console detach for GUI (`src/cli.rs`).

When editing UI spacing or panels, check both **docked** (main window) and **floating** (`draw_videos_window`, `draw_logs_window`) code paths in `src/app/videos_panel.rs` and related modules. Both paths should go through `VideosQueueLayout` in `videos_panel.rs` (shared scroll/footer/log reserve math).

### Layout QA checklist (manual)

After changing queue or log panel layout (`videos_panel.rs`, `log_panel.rs`, `app_ui.rs`):

1. **Docked Videos** — resize bottom panel; card list fills space below toolbar; footer controls stay visible.
2. **Floating Videos** — resize window; list scrolls; dock/undock toggles work.
3. **Docked log under Videos** — log lines fill remaining panel height after resize.
4. **Floating Activity log** — resize window; log lines fill viewport.
5. **Mode switch** — Downloader ↔ Video Converter preserves panel sizes; mode tint/stripe visible.
6. **Large queue** — import or restore ~200 items; list layout stays responsive (`RUSTDL_PROFILE=1` optional).

## Versioning and releases

- App version: `Cargo.toml` `version` field (also `rustdl --version` / About).
- User-facing history: `CHANGELOG.md` ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/)), [Semantic Versioning](https://semver.org/).
- Git remotes: **`github`** (canonical; triggers release CI) and **`gitlab`** (mirror). There is no `origin` remote.

### Day-to-day development

1. **User-visible change** → add a bullet under `## [Unreleased]` in `CHANGELOG.md` (same commit as the change).
2. **Medium or larger change** → bump `Cargo.toml` in that same commit (still under `[Unreleased]` until release day):

   | Platform | Command |
   |----------|---------|
   | Windows | `.\scripts\bump_version.ps1` / `minor` / `major` |
   | Unix | `./scripts/bump_version.sh` / `minor` / `major` |

   Default is **patch** (`0.4.6` → `0.4.7`). Skip bumps for trivial fixes and non-user-facing work.

   **Tag on every bump:** the bump scripts create an annotated tag `rustdl-vX.Y.Z` on `HEAD` when that commit already contains the new `Cargo.toml` version. After a manual version edit, commit first, then run `.\scripts\bump_version.ps1 -TagOnly` or `./scripts/bump_version.sh --tag-only`. **Do not push** `rustdl-v*` tags until release day. `release.ps1` / `release.sh` move an existing bump tag to the release commit with `-f`.

3. **Before opening a PR** → run CI checks locally (see **Running and testing locally**).

### CHANGELOG on every commit

When committing **any** product or user-visible change, update `CHANGELOG.md` in the **same commit**:

1. Add a bullet under `## [Unreleased]` in the right subsection (`Added`, `Changed`, `Fixed`, `Removed`, `Documentation`, etc.).
2. Write for end users, not implementers (what changed in the app, not file names or refactors).
3. One line per notable item; group related tweaks under a single bullet when sensible.
4. Skip `CHANGELOG.md` only for changes with **no** user-facing effect (e.g. CI-only, internal refactors, agent/docs-only edits to `AGENTS.md`).

Do not wait until release day to record changes—the `[Unreleased]` section is the running draft.

### Version bump on medium/bigger commits

When committing **medium or larger** user-visible work, bump `version` in `Cargo.toml` in the **same commit** as the `CHANGELOG.md` update (still under `[Unreleased]` until a release is cut):

| Size | Semver | Examples |
|------|--------|----------|
| **Patch** (+0.0.1) | `Z` | Bug fixes, small UX polish, single-setting tweaks |
| **Minor** (+0.1.0) | `Y` | New features, notable behavior changes, multi-area improvements |
| **Major** (+1.0.0) | `X` | Breaking changes (rare) |

**Bump the version** for anything you would call a medium or bigger change—do not wait for release day.

**Skip the version bump** for trivial one-off fixes (typo, tiny tweak) and changes with no user-facing effect (CI, internal refactors, docs-only edits such as this file).

When you bump `version` in `Cargo.toml` (via the bump scripts or by hand), **always create the matching `rustdl-vX.Y.Z` tag** on that commit before finishing the task (`-TagOnly` after a manual edit).

### Cutting a release

Use the release scripts on a **clean** `dev` checkout (all `[Unreleased]` work already committed; `Cargo.toml` version is the number you are shipping):

| Step | Windows | Unix |
|------|---------|------|
| Preview | `.\scripts\release.ps1 -DryRun` | `./scripts/release.sh --dry-run` |
| Cut locally | `.\scripts\release.ps1` | `./scripts/release.sh` |
| Publish | `.\scripts\release.ps1 -Push -Yes` | `./scripts/release.sh --push --yes` |

The script runs `cargo fmt --check`, `clippy`, and `test` unless you pass `-SkipChecks` / `--skip-checks`. It then:

1. Moves `[Unreleased]` bullets into `## [X.Y.Z] - YYYY-MM-DD` (leaves `[Unreleased]` empty).
2. Updates compare links at the bottom of `CHANGELOG.md`.
3. Commits `release: vX.Y.Z` on `dev`.
4. Creates annotated tag `rustdl-vX.Y.Z` (prefer this prefix; bare `v*` tags also trigger CI).
5. With `-Push` / `--push`, pushes `dev` and the tag to **`github`** (override with `-Remote` / `--remote`).

**Manual equivalent** (if you cannot run the scripts):

1. Finish the changelog — move `[Unreleased]` into a dated `## [X.Y.Z]` section; leave `[Unreleased]` empty.
2. Confirm `Cargo.toml` `version` matches `X.Y.Z`.
3. Update compare links — `[X.Y.Z]: …/compare/rustdl-vPREV…rustdl-vX.Y.Z` and `[Unreleased]: …/compare/rustdl-vX.Y.Z…dev`.
4. Commit on `dev` (`release: vX.Y.Z`).
5. `git tag rustdl-vX.Y.Z` then `git push github dev` and `git push github rustdl-vX.Y.Z`.

Build release binaries locally (`.\scripts\build_binary.ps1` / `./scripts/build_binary.sh`); publish with `gh release create` / `gh release upload` if desired. Mirror to GitLab separately if needed (`git push gitlab dev --tags`).

### Windows winget

End-user install (after the manifest is accepted in [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs)):

```powershell
winget install Darknetzz.rustdl
```

This repo keeps an **example manifest** at `packaging/winget/Darknetzz.rustdl.yaml` (`PackageIdentifier: Darknetzz.rustdl`, portable x64 `rustdl.exe`). User-facing docs live in `README.md` → **Install (Windows)**.

**On each release** that should ship via winget:

1. Build/upload `rustdl.exe` to the GitHub release (`rustdl-vX.Y.Z` tag asset URL).
2. Update `packaging/winget/Darknetzz.rustdl.yaml`: `PackageVersion`, `InstallerUrl`, and `InstallerSha256` (hash of the uploaded exe).
3. Open a PR to **microsoft/winget-pkgs** (maintainer fork, e.g. `Darknetzz/winget-pkgs`, branch `darknetzz-rustdl-X.Y.Z`) with the versioned manifest under `manifests/d/Darknetzz/rustdl/<version>/`. Use `winget validate` / the PR checklist before submit.

Do not commit a local `winget-pkgs/` clone; it is a separate checkout for PR prep only.

### Rolling dev release (no GitHub Actions)

GitHub does not run custom hooks on push, and this repo keeps Actions **manual-only**. To refresh a **rolling pre-release** (`rustdl-dev`) on every `dev` push without Actions:

| Approach | When to use |
|----------|-------------|
| **`.githooks/pre-push` (recommended)** | One-time install per clone; runs after any successful `git push github dev` (including via `pushall`). |
| **`push_dev` scripts** | Manual all-in-one push + publish (no hook). |
| **`dev_release_webhook.py`** | Optional server; GitHub **push** webhook when pushes come from machines without the hook. |

**Automatic publish (recommended, one-time per clone):**

| Platform | Command |
|----------|---------|
| Windows | `.\scripts\install_dev_release_hook.ps1` |
| Unix | `./scripts/install_dev_release_hook.sh` |

Sets `core.hooksPath = .githooks`. After that, **any** successful `git push github dev` (or `pushall`, which pushes `github` first) schedules a background build + `gh release` upload to **`rustdl-dev`**. Log: `%TEMP%\rustdl-dev-release.log` (Windows) or `$TMPDIR/rustdl-dev-release.log` (Unix). Disable: same script with `-Uninstall` / `--uninstall`.

Requires **`gh auth login`** with `repo` scope. Only pushes to remote **`github`** ref **`dev`** trigger publish (GitLab mirror pushes do not).

**Manual all-in-one push + publish:**

| Platform | Command |
|----------|---------|
| Windows | `.\scripts\push_dev.ps1` |
| Unix | `./scripts/push_dev.sh` |

Use when the hook is not installed. Pushes `dev` to **`github`**, runs `publish_dev_release`, then mirrors **`gitlab`**. `-SkipGitlab` / `--skip-gitlab` if the mirror is unreachable.

**Publish only** (already pushed, or webhook checkout):

| Platform | Command |
|----------|---------|
| Windows | `.\scripts\publish_dev_release.ps1` |
| Unix | `./scripts/publish_dev_release.sh` |

The release tag is always **`rustdl-dev`** (pre-release); each run moves `--target` to the built commit and `--clobber`-uploads the platform binary (`rustdl.exe` on Windows, `rustdl` on Unix). Notes include `[Unreleased]` from `CHANGELOG.md`.

**Webhook (optional):** on a build machine with this repo, `gh`, and Rust:

```bash
export RUSTDL_WEBHOOK_SECRET='…'   # same secret as GitHub → Settings → Webhooks
python scripts/dev_release_webhook.py
```

Configure the webhook for **push** events on `Darknetzz/rustdl`. Payload URL path defaults to `/rustdl-dev-release` (port `8766`). Use HTTPS reverse proxy in production.

**First release / missing older tags:** compare links use `rustdl-vPREV...rustdl-vX.Y.Z`. If `rustdl-vPREV` was never pushed (this repo had changelog-only versions before tagging), either backfill that tag on the old release commit or accept that the compare URL works only after both tags exist.

### GitHub Actions (optional, manual only)

`.github/workflows/ci.yml` and `.github/workflows/release.yml` are **`workflow_dispatch` only** (no runs on push, PR, or tag) to avoid GitHub runner cost. Use `scripts/ci_local.ps1` / `ci_local.sh` and local build/release scripts instead. Workflows remain in the repo for emergency manual runs from the GitHub Actions UI if needed.

## Agent conventions

- **Scope**: Smallest correct diff; match existing naming and patterns in the file you touch.
- **Comments**: Only for non-obvious behavior; prefer clear code.
- **Tests**: Add or extend tests when fixing real behavior bugs; avoid trivial tests unless requested.
- **Docs**: Do not add new markdown files unless asked (this file is the exception the user requested).
- **Git**: Do not commit, push, or open PRs unless the user explicitly asks. Do not change git config. When committing user-facing work, include `CHANGELOG.md` updates under `[Unreleased]`; bump `Cargo.toml` `version` on medium/bigger commits (see **Versioning and releases**) and tag the bump commit with `rustdl-vX.Y.Z` (bump scripts or `-TagOnly`; do not push the tag until release unless asked).
- **Secrets**: Never commit API tokens, config exports, or user `rustdl_config.json` contents.

## Further reading

- `README.md` — features, settings tables, LAN security notes, platform behavior.
- `MIGRATION.md` — remotes (`github`, `gitlab`), branch policy, monorepo history.
- `CHANGELOG.md` — recent product changes.
