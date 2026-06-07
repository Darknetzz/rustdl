# Changelog

All notable changes to **rustdl** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

When releasing, bump `version` in `Cargo.toml`, add a dated section below, and tag `rustdl-vX.Y.Z` for GitHub release builds.

## [Unreleased]

### Added

- **LAN web UI** (Settings → Shared): optional HTTP server with token auth, REST API, SSE progress stream, and embedded web pages to control the downloader queue from other devices on the local network.
- LAN web UI: **Quit** button to gracefully shut down rustdl (cancels active jobs, saves queue/settings; closes the desktop app or stops `--web-only`).
- AV1 Converter: undock the encode queue to a floating window (same controls as Downloader **Videos**).

### Changed

- LAN web UI: **Downloader** and **AV1** sections use distinct accent colors (blue / purple) on the nav toggle, panels, and primary actions.
- LAN web UI: queue thumbnails load reliably during live SSE updates (debounced refresh, ignore aborted image loads, typed image blobs).

### Fixed

- LAN web UI: clearing finished downloader queue items (or any action that refreshed status) no longer throws “Cannot access 'shuttingDown' before initialization”.
- **Videos** queue uses a resizable bottom panel in the main window (egui `TopBottomPanel`) with a fixed-height scroll region (fixes infinite layout from unbounded `available_height()`); floating **Videos** window uses the same pattern.
- Docked activity log under **Videos** no longer shrinks the card list below usable size (log height is capped so the queue keeps at least ~160px; log toolbar is not nested twice).
- Main controls no longer leave a large empty gap above the docked **Videos** panel when the queue is pinned to the bottom of the window.
- Undocked **Videos** strip and docked activity log use a resizable bottom panel (same layout model as docked **Videos**), fixing overlapping log controls and empty space below the footer.
- Floating **Videos** window card list fills the space below the toolbar again.
- Log height slider shows whole pixels instead of long floating-point values.
- Header: tool status on a second row so Settings, Web UI, and status badge no longer overlap on narrower widths.
- Header: **Settings** and **Exit** are grouped together on the right.
- Shared `DownloadCore` service state synchronized between the egui app and the web control plane.

## [0.1.1] - 2026-06-02

### Added

- Compact tool-version labels in the header (build date when available; full string on hover).

### Changed

- Main window header: title on the left; yt-dlp/ffmpeg/ffprobe status and Settings / Logs / Exit on one row, right-aligned.
- Restore-session banner and Downloader / AV1 mode bar use the full content width.
- Main panel content inset (horizontal padding) so controls do not sit on the window edge.

### Fixed

- **Windows:** No extra console window when launching the GUI from Explorer (Windows GUI subsystem).
- **Windows:** Child processes (yt-dlp, ffmpeg, PowerShell theme probe, etc.) no longer flash a console.

## [0.1.0] - 2026-03-30

Initial published version: desktop GUI for yt-dlp with queue, previews, settings, and download progress.

### Added

- Downloader setting: enqueue completed video downloads in the AV1 converter queue.
- Shared `ytdlp_download_args` module: CLI and GUI use the same yt-dlp argument builder (with unit tests).
- Headless CLI: `--dry-run`, batch downloads from `@file.txt` or `-` (stdin).
- GUI: **Import queue** from `.txt`, **Open output folder** on completed cards, download speed limit setting (`--limit-rate`).
- `lib.rs` crate surface; integration tests in `tests/` (progress fixtures; optional `RUSTDL_IT=1` subprocess smoke).
- CI: macOS job, `Swatinem/rust-cache`; release workflow on `rustdl-v*` tags (Linux, Windows, macOS x64/arm64).

### Changed

- `app/mod.rs` split: `queue_persist`, `download_control`, `background_spawn`, `events`, `app_parsing`.
- `cargo deny` / `cargo audit` run on pushes to `dev` only (PR CI stays faster).

### Documentation

- README: CLI batch/dry-run, platform drag-and-drop deferral rationale for Linux/macOS.

[Unreleased]: https://github.com/Darknetzz/rustdl/compare/rustdl-v0.1.1...dev
[0.1.1]: https://github.com/Darknetzz/rustdl/compare/rustdl-v0.1.0...rustdl-v0.1.1
[0.1.0]: https://github.com/Darknetzz/rustdl/tree/rustdl-v0.1.0
