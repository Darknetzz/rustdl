# AGENTS.md

Guidance for AI agents and automation working in this repository.

## What this project is

**rustdl** is a desktop application (Rust + [eframe](https://github.com/emilk/egui)/egui) for managing [yt-dlp](https://github.com/yt-dlp/yt-dlp) download queues. It also includes:

- **Downloader mode** — paste URLs, preview cards, queue downloads, activity log, profiles, settings.
- **AV1 Converter mode** — local file/folder transcoding via ffmpeg (desktop only).
- **Optional LAN web UI** — Axum HTTP server + embedded `web-assets/` for remote queue control (downloader only).
- **Headless CLI** — `--download`, `--web-only`, `--list-profiles` (see `README.md`).

The crate library root is `src/lib.rs`; the binary calls `rustdl::main_entry()` from `src/main.rs`.

This repo was split from the monorepo path `Rust/rustdl/` in [Darknetzz/code](https://github.com/Darknetzz/code) (see `MIGRATION.md`). Default branch on GitHub/GitLab is **`dev`**.

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

Before a PR, match CI (`.github/workflows/ci.yml`):

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

MSRV: **Rust 1.76+** (`rust-version` in `Cargo.toml`).

## Repository layout

| Path | Role |
|------|------|
| `src/app/` | egui UI: `eframe_app.rs` (main window), `videos_panel.rs`, `cards.rs`, `settings_panel.rs`, `log_panel.rs`, `av1_panel.rs` |
| `src/app_ui.rs` | Shared UI helpers (buttons, badges, layout, `compute_main_column_split`) |
| `src/service/` | Background core + Tokio; `web/` for LAN API and static assets |
| `src/ytdlp.rs`, `src/ytdlp_download_args.rs` | yt-dlp invocation and argument building |
| `src/config.rs`, `src/models.rs`, `src/profiles.rs` | Settings, queue models, download profiles |
| `src/cli.rs` | CLI / `--web-only` entry |
| `web-assets/` | LAN web UI (`index.html`, `app.js`, `style.css`) |
| `tests/` | Integration tests (ytdlp fixtures, queue perf, subprocess smoke) |
| `scripts/build_binary.ps1`, `scripts/build_binary.sh` | Release binary build |
| `deny.toml` | `cargo deny` policy (CI on `dev` pushes) |

User data (not in repo): `<config_dir>/rustdl/` — `rustdl_config.json`, `rustdl_queue.json`, `rustdl_activity_log.json`. See `README.md` for paths.

## Architecture notes for code changes

- **UI thread vs background work**: `PydlApp` in `src/app/mod.rs` owns egui state; download/encode work runs on a shared Tokio `Runtime` via `src/service/core.rs`. UI updates arrive through channels (`src/app/events.rs`, `core_sync.rs`).
- **Queue persistence**: `src/app/queue_persist.rs` saves/restores the downloader queue; AV1 queue in `src/av1_state.rs`.
- **External tools**: Resolved via `PATH` or custom paths in settings (`src/external_tools.rs`). Requires `yt-dlp`; `ffmpeg` / `ffprobe` optional but needed for many features.
- **Windows-only**: Browser URL drag-and-drop (`src/win_drop_target.rs`), console detach for GUI (`src/cli.rs`).

When editing UI spacing or panels, check both **docked** (main window) and **floating** (`draw_videos_window`, `draw_logs_window`) code paths in `src/app/videos_panel.rs` and related modules.

## Versioning and releases

- App version: `Cargo.toml` `version` field (also `rustdl --version` / About).
- User-facing changes: `CHANGELOG.md` (Keep a Changelog).
- Releases: tag `rustdl-vX.Y.Z`, workflow in `.github/workflows/release.yml`.

## Agent conventions

- **Scope**: Smallest correct diff; match existing naming and patterns in the file you touch.
- **Comments**: Only for non-obvious behavior; prefer clear code.
- **Tests**: Add or extend tests when fixing real behavior bugs; avoid trivial tests unless requested.
- **Docs**: Do not add new markdown files unless asked (this file is the exception the user requested).
- **Git**: Do not commit, push, or open PRs unless the user explicitly asks. Do not change git config.
- **Secrets**: Never commit API tokens, config exports, or user `rustdl_config.json` contents.

## Further reading

- `README.md` — features, settings tables, LAN security notes, platform behavior.
- `MIGRATION.md` — remotes (`github`, `gitlab`), branch policy, monorepo history.
- `CHANGELOG.md` — recent product changes.
