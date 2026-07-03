# AGENTS.md

Guidance for AI agents and automation working in this repository.

## What this project is

**rustdl** is a desktop application (Rust + [eframe](https://github.com/emilk/egui)/egui) for managing [yt-dlp](https://github.com/yt-dlp/yt-dlp) download queues. It is a **multi-surface product**: one shared download/convert engine backs the egui desktop app, an optional LAN web UI, and headless CLI modes. There are **no Cargo `[features]`** — everything ships in a single binary (~35k lines of Rust + ~5k lines of LAN web JS).

The crate library root is `src/lib.rs`; the binary calls `rustdl::main_entry()` from `src/main.rs`.

### Product surfaces

| Surface | Entry | Implementation |
|---------|-------|----------------|
| **Desktop GUI** | `cargo run` / `rustdl` | `PydlApp` in `src/app/mod.rs`, `eframe_app.rs` |
| **LAN web UI** | Settings → Web UI or `--web-only` | `src/service/web/` + `web-assets/` (Axum REST + SSE) |
| **Headless CLI** | Flags below | `src/cli.rs` — `--download` bypasses shared queue; other flags use persisted state |

### Feature areas (all active in the shipped product)

- **Downloader mode** — paste URLs, preview cards, queue downloads, activity log, profiles, scheduled start, watch folders, queue templates, download library.
- **Video Converter mode** — local file/folder transcoding via ffmpeg (AV1 default; H.265/H.264 optional); parallel workers, GPU encode fairness, convert presets; desktop + LAN web UI.
- **Quality watchlist** — periodically re-probe saved URLs; log when max available resolution improves; optional auto-enqueue (`watchlist.rs`, `watchlist_panel.rs`, `core_watchlist.rs`; Settings → Downloader). **Desktop only** — not exposed on LAN web UI yet.
- **Command palette** — `Ctrl+K` / `Cmd+K`; layout presets (Compact / Review / Minimal), dock/float panels, quick queue actions (`command_palette.rs`; mirrored in `web-assets/app.js`).
- **Download library** — index of completed files under the output folder; desktop window + LAN **Library** tab (`domain/done_file_index.rs`, `core.rs` refresh loop).
- **More info** — per-row popup with yt-dlp source fields and ffprobe file details after download (`cards.rs`).
- **System tray** — Windows + Linux minimize-to-tray while downloads/encodes continue (`tray.rs`).
- **Windows URL drag-and-drop** — browser URI drops from Chrome/Firefox (`win_drop_target.rs`); other platforms: paste or file drops only.

User-facing feature docs live in `README.md`; recent additions are also in `CHANGELOG.md` under `[Unreleased]` / latest version.

### CLI flags

| Flag | Purpose |
|------|---------|
| *(no args)* | Start GUI |
| `--download URL` | Headless yt-dlp run (URL, `@file.txt`, or `-` for stdin); no shared queue or activity log |
| `--profile NAME` | With `--download`: apply named profile first |
| `--output-dir PATH` | With `--download`: override output folder |
| `--dry-run` | With `--download`: print planned args only |
| `--enqueue URL\|@file\|-` | Append URLs to saved download queue |
| `--start-queue` | Start persisted download queue and wait |
| `--convert-batch` | Start persisted convert batch and wait |
| `--web-only` | Headless LAN web UI (no GUI window) |
| `--host ADDR`, `--port PORT` | With `--web-only`: bind override (defaults from settings) |
| `--list-profiles` | Print download profile names |
| `-h`, `--help` / `-V`, `--version` | Help and version |

See `README.md` → **Run** for examples.

### Codebase at a glance

| Layer | ~Lines | Notes |
|-------|--------|-------|
| `src/` Rust | ~35k | Single crate, 75 `.rs` files |
| `web-assets/app.js` | ~5.3k | Second UI stack for LAN web (duplicates much queue/settings UX) |
| `tests/` | ~5.7k | Integration + perf (`queue_perf.rs` is large) |
| `scripts/` | ~3k | Build, release, CI — not shipped |

**Hotspot files** (start here when debugging layout or queue behavior): `src/app/mod.rs`, `src/app_ui.rs`, `src/app/settings_panel.rs`, `src/service/core.rs`, `src/service/web/api.rs`, `web-assets/app.js`.

**Dual UI rule:** desktop changes often need matching updates in `web-assets/app.js` and/or `src/service/web/api.rs` when the LAN web UI exposes the same control.

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

### Core application (`src/`)

| Path | Role |
|------|------|
| **`src/app/mod.rs`** | `PydlApp` — main egui state hub (~2.7k lines); mode toggle, settings, web server handle |
| **`src/app/eframe_app.rs`** | `eframe::App` impl; frame loop, panel routing |
| **`src/app/videos_panel.rs`** | Download queue panel (docked + floating); `VideosQueueLayout` scroll/footer math |
| **`src/app/cards.rs`** | Preview cards / list rows, **More info** popup, per-item actions |
| **`src/app/convert_panel.rs`** | Video Converter queue UI |
| **`src/app/settings_panel.rs`** | Settings window (General / Downloader / Converter / Web UI tabs) |
| **`src/app/log_panel.rs`** | Activity log (docked under queue + floating window) |
| **`src/app/watchlist_panel.rs`** | Quality watchlist collapsible panel |
| **`src/app/command_palette.rs`** | `Ctrl+K` command palette |
| **`src/app/core_sync.rs`** | Mirrors `DownloadCore` ↔ `PydlApp` each frame |
| **`src/app/events.rs`** | UI event types from background |
| **`src/app/queue_persist.rs`** | Save/restore download queue |
| **`src/app/settings_persist.rs`** | Settings save on change |
| **`src/app/download_control.rs`** | Start/pause/cancel download session |
| **`src/app/thumbnails.rs`** | Card thumbnail loading |
| **`src/app/input_lines.rs`** | URL paste / validation / dedupe |
| **`src/app/about.rs`**, **`update_check.rs`**, **`web_qr.rs`** | About dialog, GitHub release check, LAN QR code |
| **`src/app/queue_cache.rs`**, **`background_spawn.rs`**, **`done_file_index.rs`** | Thin re-exports to canonical modules |
| **`src/app_ui.rs`** | Shared UI helpers (buttons, badges, layout presets, `queue_body_layout_heights`, viewport clamp) |
| **`src/service/core.rs`** | `DownloadCore` — queue engine, yt-dlp workers, persistence hooks, library index |
| **`src/service/core_convert.rs`** | Converter batch logic on shared core |
| **`src/service/core_watchlist.rs`** | Watchlist probe scheduling |
| **`src/service/core_events.rs`** | Tokio event loop + watch-folder polling |
| **`src/service/background_spawn.rs`** | Shared background task helpers |
| **`src/service/mod.rs`** | `RustdlService` — shared handle for GUI + web |
| **`src/service/web/`** | LAN server: `server.rs`, `api.rs`, `convert_api.rs`, `auth.rs`, `media.rs`, `assets.rs` |
| **`src/domain/`** | Shared types: `events.rs` (`UiEvent`), `done_file_index.rs` |
| **`src/ytdlp.rs`**, **`ytdlp_download_args.rs`**, **`ytdlp_errors.rs`** | yt-dlp subprocess, args, errors |
| **`src/transcode.rs`** | ffmpeg encode pipeline, encoder auto-detect |
| **`src/config.rs`** | `AppSettings`, paths, load/save |
| **`src/models.rs`** | Queue item types and status |
| **`src/profiles.rs`** | Download profiles (built-in + user) |
| **`src/convert_state.rs`**, **`convert_presets.rs`**, **`convert_size_limit.rs`** | Converter queue persistence and presets |
| **`src/watchlist.rs`** | Watchlist store + `rustdl_watchlist.json` |
| **`src/watch_folder.rs`** | Auto-enqueue from `.url` / `.txt` drops |
| **`src/queue_templates.rs`** | Saved downloader queue templates |
| **`src/cli.rs`** | CLI entry and headless modes |
| **`src/external_tools.rs`** | Resolve yt-dlp / ffmpeg / ffprobe on `PATH` |
| **`src/thumbnail_store.rs`**, **`media_metadata.rs`**, **`download_organize.rs`** | Thumbnails, ffprobe, output templates |
| **`src/tray.rs`** | System tray (Windows + Linux) |
| **`src/win_drop_target.rs`**, **`win_icon.rs`**, **`win_window.rs`** | Windows-only shell integration |
| **`src/theme.rs`**, **`ui_icons.rs`**, **`app_icon.rs`** | Theming and icons |

### Web, tests, tooling

| Path | Role |
|------|------|
| `web-assets/` | LAN web UI (`index.html`, `app.js`, `style.css`, fonts) |
| `tests/ytdlp_fixtures.rs` | yt-dlp argument / fixture tests |
| `tests/queue_perf.rs` | Queue layout and perf regression tests |
| `tests/subprocess_smoke.rs` | Subprocess smoke tests |
| `scripts/build_binary.ps1`, `scripts/build_binary.sh` | Release binary build |
| `scripts/ci_local.ps1`, `scripts/ci_local.sh` | Local fmt / clippy / test / deny / audit |
| `scripts/bump_version.ps1`, `scripts/bump_version.sh` | Semver bump in `Cargo.toml` + annotated `rustdl-vX.Y.Z` tag on the bump commit |
| `scripts/release.ps1`, `scripts/release.sh` | Optional: finalize `[Unreleased]` changelog + compare links (`release: vX.Y.Z` commit) |
| `scripts/publish_dev_release.ps1`, `scripts/publish_dev_release.sh` | Build and refresh the rolling **`rustdl-dev`** GitHub pre-release |
| `scripts/publish_stable_release.ps1`, `scripts/publish_stable_release.sh` | Tag and publish **`rustdl-vX.Y.Z`** when the Cargo version is new on GitHub |
| `scripts/push_dev.ps1`, `scripts/push_dev.sh` | Push `dev` to GitHub, publish rolling dev + stable releases, mirror GitLab |
| `scripts/install_dev_release_hook.ps1`, `scripts/install_dev_release_hook.sh` | One-time: enable `.githooks/pre-push` auto-publish on `git push github dev` |
| `.githooks/pre-push` | Git hook (via `core.hooksPath`) — schedules rolling dev + stable release publish after push |
| `scripts/dev_release_webhook.py` | Optional GitHub **push** webhook listener (no GitHub Actions) |
| `packaging/winget/Darknetzz.rustdl.yaml` | Example [winget](https://github.com/microsoft/winget-cli) manifest (portable `rustdl.exe` from GitHub Releases) |
| `deny.toml` | `cargo deny` policy (CI on `dev` pushes) |

### User data (not in repo)

Under `<config_dir>/rustdl/` (see `README.md` for OS paths):

| File / folder | Purpose |
|---------------|---------|
| `rustdl_config.json` | All settings |
| `rustdl_queue.json` | Saved download queue |
| `rustdl_convert_queue.json` | Saved Video Converter queue (when remember enabled) |
| `rustdl_activity_log.json` | Persisted activity log |
| `rustdl_profiles.json` | User-defined download profiles |
| `rustdl_watchlist.json` | Quality watchlist entries |
| `rustdl_convert_presets.json` | User convert encoding presets |
| `queue_templates/` | Saved downloader queue templates |

Never commit these files or paste their contents into the repo.

## Architecture notes for code changes

```text
surfaces          GUI (egui)          LAN web (app.js)
                      |                      |
                 core_sync.rs            api.rs / convert_api.rs
                      \                    /
                    DownloadCore (service/core.rs)
                      /         |         \
              ytdlp.rs    transcode.rs   watchlist.rs
                      \         |         /
                   Tokio Runtime + subprocess workers
```

- **UI thread vs background work**: `PydlApp` in `src/app/mod.rs` owns egui state; download/encode work runs on a shared Tokio `Runtime` via `src/service/core.rs`. UI updates arrive through channels (`src/domain/events.rs`, `src/app/events.rs`, `core_sync.rs`). `RustdlService` in `service/mod.rs` constructs one `SharedCore` for GUI and optional web server.
- **Queue persistence**: `src/app/queue_persist.rs` saves/restores the downloader queue; converter queue in `src/convert_state.rs` / `rustdl_convert_queue.json`. Core owns authoritative queue state; GUI mirrors it via `core_sync.rs`.
- **LAN web API**: `service/web/server.rs` spawns Axum; `api.rs` (downloader + library + settings) and `convert_api.rs` (converter) call the same `SharedCore` as the desktop app. Static UI from `web-assets/` via `assets.rs` (`rust-embed`). Auth: API token + optional IP whitelist (`auth.rs`); optional TLS from settings.
- **External tools**: Resolved via `PATH` or custom paths in settings (`src/external_tools.rs`). Requires `yt-dlp`; `ffmpeg` / `ffprobe` optional but needed for converter, thumbnails, **More info**, and watchlist probes.
- **Platform `cfg`**: `tray.rs` (Windows + Linux); `win_*` modules (Windows only). No macOS tray yet. Linux GUI: `persist_window: false` in `lib.rs` to avoid off-screen restore.

When editing UI spacing or panels, check both **docked** (main window) and **floating** (`draw_videos_window`, `draw_logs_window`) code paths in `src/app/videos_panel.rs` and related modules. Both paths should go through `VideosQueueLayout` in `videos_panel.rs` (shared scroll/footer/log reserve math).

### Where to change common things

| Task | Primary files |
|------|-----------------|
| Download queue UI / cards | `videos_panel.rs`, `cards.rs`, `app_ui.rs` |
| Converter UI | `convert_panel.rs`, `core_convert.rs`, `transcode.rs` |
| Settings field or tab | `settings_panel.rs`, `config.rs` |
| yt-dlp flags / args | `ytdlp_download_args.rs`, `ytdlp.rs`, `profiles.rs` |
| Queue worker behavior | `service/core.rs`, `core_events.rs` |
| Quality watchlist | `watchlist.rs`, `watchlist_panel.rs`, `core_watchlist.rs` |
| LAN web endpoint or web UX | `service/web/api.rs`, `convert_api.rs`, `web-assets/app.js` |
| Command palette action | `command_palette.rs`, then web palette in `app.js` if exposed |
| Download library / completed files | `domain/done_file_index.rs`, `core.rs` refresh helpers |
| CLI headless mode | `cli.rs` |
| New persisted user file | `config.rs` (path helper), relevant store module, `README.md` + this file |
| Layout math / presets | `app_ui.rs`, `videos_panel.rs`, tests in `tests/queue_perf.rs` |

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
- Git remotes: **`github`** (canonical) and **`gitlab`** (mirror). There is no `origin` remote.
- **Stable releases:** every new `Cargo.toml` version is published to GitHub as `rustdl-vX.Y.Z` when `dev` is pushed (pre-push hook or `push_dev`). The rolling **`rustdl-dev`** pre-release is separate and always tracks the tip of `dev`.

### Day-to-day development

1. **User-visible change** → add a bullet under `## [Unreleased]` in `CHANGELOG.md` (same commit as the change).
2. **Medium or larger change** → bump `Cargo.toml` in that same commit and add a dated `## [X.Y.Z]` section (or bullets under `[Unreleased]`) in `CHANGELOG.md`:

   | Platform | Command |
   |----------|---------|
   | Windows | `.\scripts\bump_version.ps1` / `minor` / `major` |
   | Unix | `./scripts/bump_version.sh` / `minor` / `major` |

   Default is **patch** (`0.4.6` → `0.4.7`). Skip bumps for trivial fixes and non-user-facing work.

   **Tag on every bump:** the bump scripts create an annotated tag `rustdl-vX.Y.Z` on `HEAD` when that commit already contains the new `Cargo.toml` version. After a manual version edit, commit first, then run `.\scripts\bump_version.ps1 -TagOnly` or `./scripts/bump_version.sh --tag-only`. **Push `dev` to `github`** (or run `push_dev` / `publish_stable_release`) to publish the stable GitHub release.

3. **Before opening a PR** → run CI checks locally (see **Running and testing locally**).

### CHANGELOG on every commit

When committing **any** product or user-visible change, update `CHANGELOG.md` in the **same commit**:

1. Add a bullet under `## [Unreleased]` in the right subsection (`Added`, `Changed`, `Fixed`, `Removed`, `Documentation`, etc.).
2. Write for end users, not implementers (what changed in the app, not file names or refactors).
3. One line per notable item; group related tweaks under a single bullet when sensible.
4. Skip `CHANGELOG.md` only for changes with **no** user-facing effect (e.g. CI-only, internal refactors, agent/docs-only edits to `AGENTS.md`).

Do not wait until release day to record changes—the `[Unreleased]` section is the running draft.

### Version bump on medium/bigger commits

When committing **medium or larger** user-visible work, bump `version` in `Cargo.toml` in the **same commit** as the `CHANGELOG.md` update:

| Size | Semver | Examples |
|------|--------|----------|
| **Patch** (+0.0.1) | `Z` | Bug fixes, small UX polish, single-setting tweaks |
| **Minor** (+0.1.0) | `Y` | New features, notable behavior changes, multi-area improvements |
| **Major** (+1.0.0) | `X` | Breaking changes (rare) |

**Bump the version** for anything you would call a medium or bigger change.

**Skip the version bump** for trivial one-off fixes (typo, tiny tweak) and changes with no user-facing effect (CI, internal refactors, docs-only edits such as this file).

When you bump `version` in `Cargo.toml` (via the bump scripts or by hand), **always create the matching `rustdl-vX.Y.Z` tag** on that commit (`-TagOnly` after a manual edit), then push `dev` so the stable release is published.

### Changelog finalization (optional)

`release.ps1` / `release.sh` are **optional** helpers when you want to move `[Unreleased]` bullets into a dated `## [X.Y.Z]` section and refresh compare links at the bottom of `CHANGELOG.md`. They are **not** required to publish — `publish_stable_release` reads the matching `## [X.Y.Z]` section from `CHANGELOG.md` (via `extract_release_notes`); it no longer falls back to `[Unreleased]`. Add the dated version section before publishing. To fix older GitHub releases: `.\scripts\refresh_release_notes.ps1`.

### Manual release script (changelog housekeeping)

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

### Dev push publish (no GitHub Actions)

GitHub does not run custom hooks on push, and this repo keeps Actions **manual-only**. On every `dev` push to **`github`**, the hook (or `push_dev`) builds once, refreshes the rolling **`rustdl-dev`** pre-release, and publishes a **stable** `rustdl-vX.Y.Z` release when that Cargo version is not on GitHub yet.

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

Sets `core.hooksPath = .githooks`. After that, **any** successful `git push github dev` (or `pushall`, which pushes `github` first) schedules a background build + `gh release` upload to **`rustdl-dev`** and any missing stable **`rustdl-vX.Y.Z`**. Log: `%TEMP%\rustdl-dev-release.log` (Windows) or `$TMPDIR/rustdl-dev-release.log` (Unix). Disable: same script with `-Uninstall` / `--uninstall`.

Requires **`gh auth login`** with `repo` scope. Only pushes to remote **`github`** ref **`dev`** trigger publish (GitLab mirror pushes do not).

**Manual all-in-one push + publish:**

| Platform | Command |
|----------|---------|
| Windows | `.\scripts\push_dev.ps1` |
| Unix | `./scripts/push_dev.sh` |

Use when the hook is not installed. Pushes `dev` to **`github`**, runs `publish_dev_release` + `publish_stable_release`, then mirrors **`gitlab`**. `-SkipGitlab` / `--skip-gitlab` if the mirror is unreachable.

**Publish only** (already pushed, or webhook checkout):

| Platform | Command |
|----------|---------|
| Windows | `.\scripts\publish_dev_release.ps1` ; `.\scripts\publish_stable_release.ps1` |
| Unix | `./scripts/publish_dev_release.sh` ; `./scripts/publish_stable_release.sh` |

**`rustdl-dev`** is always a pre-release at the tip of `dev`. **`rustdl-vX.Y.Z`** is a stable release per `Cargo.toml` version; each version is published once (re-run with `-Force` / `--force` to refresh).

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

- **Read this file first** for architecture, file map, and release rules; use `README.md` for end-user feature wording and settings tables.
- **CHANGELOG (required)**: Any user-visible change must include a `CHANGELOG.md` update under `## [Unreleased]` in the **same commit** as the code. Write for end users (what changed in the app, not file names). Do not mark a user-facing task complete without a changelog bullet. Skip only for changes with no user-facing effect (CI, internal refactors, agent/docs-only edits to this file). See **CHANGELOG on every commit** below.
- **Dual UI**: If a feature is exposed on the LAN web UI, update `web-assets/app.js` and web API handlers — not just egui panels.
- **Scope**: Smallest correct diff; match existing naming and patterns in the file you touch. Prefer extending existing helpers in `app_ui.rs` / `core.rs` over new abstractions.
- **Comments**: Only for non-obvious behavior; prefer clear code.
- **Tests**: Add or extend tests when fixing real behavior bugs; layout regressions often go in `tests/queue_perf.rs`. Avoid trivial tests unless requested.
- **Docs**: Do not add new markdown files unless asked. Keep `README.md` (user-facing) and `AGENTS.md` (agent-facing) in sync when adding features or persisted files.
- **Git**: Do not commit, push, or open PRs unless the user explicitly asks. Do not change git config. When committing user-facing work, include the `CHANGELOG.md` update (see **CHANGELOG (required)** above); bump `Cargo.toml` `version` on medium/bigger commits (see **Versioning and releases**) and tag the bump commit with `rustdl-vX.Y.Z` (bump scripts or `-TagOnly`; do not push the tag until release unless asked).
- **Secrets**: Never commit API tokens, config exports, or user `rustdl_config.json` contents.

## Further reading

- [`README.md`](README.md) — features, settings tables, LAN security notes, platform behavior, config file paths.
- [`MIGRATION.md`](MIGRATION.md) — remotes (`github`, `gitlab`), branch policy, monorepo history.
- [`CHANGELOG.md`](CHANGELOG.md) — recent product changes (check latest section before editing user-facing copy).
