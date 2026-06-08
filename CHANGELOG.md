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

- **Show log** moved to the top header (next to Settings) instead of above the download queue.
- Destination disk free space is shown in the top header next to the yt-dlp/ffmpeg/ffprobe checks (removed from Download options); redundant **Queue** label above the action toolbar removed.
- Done download cards show the most recently finished items first (left in the horizontal strip).
- Failed download cards show **Retry** only (not a duplicate **Redo**); **Redo** remains on completed downloads.
- LAN web UI: **Downloader** and **AV1** sections use distinct accent colors (blue / purple) on the nav toggle, panels, and primary actions.
- Desktop GUI: **Downloader** and **AV1** panels use the same muted mode tint and left accent stripe as the LAN web UI (main controls and queue panels); mode nav tabs use blue / purple when active.
- LAN web UI: queue thumbnails load reliably during live SSE updates (debounced refresh, ignore aborted image loads, typed image blobs).

### Fixed

- Floating **Videos** window can be resized again (queue content now fills the window instead of shrink-wrapping).
- Floating **Activity log** window: resizable again, log lines fill the viewport (no empty black area above the toolbar).
- LAN web UI: AV1 **Cancel** is disabled and greyed out when the converter is idle (nothing to cancel).
- LAN web UI: clearing finished downloader queue items (or any action that refreshed status) no longer throws “Cannot access 'shuttingDown' before initialization”.
- **Videos** queue uses a resizable bottom panel in the main window (egui `TopBottomPanel`) with a fixed-height scroll region (fixes infinite layout from unbounded `available_height()`); floating **Videos** window uses the same pattern.
- Docked activity log under **Videos** no longer shrinks the card list below usable size (log height is capped so the queue keeps at least ~160px; log toolbar is not nested twice).
- Main controls no longer leave a large empty gap above the docked **Videos** panel when the queue is pinned to the bottom of the window.
- Undocked **Videos** strip and docked activity log use a resizable bottom panel (same layout model as docked **Videos**), fixing overlapping log controls and empty space below the footer.
- Floating **Videos** window card list fills the space below the toolbar again.
- Log height slider shows whole pixels instead of long floating-point values.
- Main controls scroll fills the central panel (no dead gap above the bottom footer); docked log lines expand to use remaining footer space.
- Docked **Videos** bottom panel respects manual resize (content no longer forces the panel back to full height); card list fills space below the toolbar.
- Docked **Videos** panel resize works again after mode-tint styling (queue height no longer escapes the panel bounds).
- Queue footer toolbar uses a single compact button row (less vertical padding).
- Activity log docked under **Videos** uses one compact control row (no height slider); log lines fill remaining panel space so resizing the panel works reliably.
- Docked **Videos** card list uses `available_height` layout (fixes toolbar floating mid-panel and invisible queue from stale panel state).
- **Downloader** / **AV1 Converter** mode tabs show their labels again (icon + text use separate fonts; tabs no longer rely on oversized min-width buttons).
- Fixed startup crash (`FontFamily::Name("material-icons")`) when drawing the mode tabs.
- Floating **Videos** window: toolbar pins to the top, queue list fills the remaining height, and cards scroll again.
- **Downloader** / **AV1 Converter** mode tabs use full row width with visible icon + label text (layout no longer shrink-wraps inside the main scroll area).
- Docked **Videos** queue fills the bottom panel width/height, pins the card list under the toolbar, and sizes the scroll area from the panel clip rect so cards are not clipped to a thin strip.
- Fixed docked/floating **Videos** toolbar vertically centering in the panel (shrink-wrapped content inside a tall `min_size` region).
- Restored docked/floating **Videos** layout to the proven `available_height` + `set_min_height` pattern (reverts experimental height helpers that broke the queue).
- Floating **Videos** window: toolbar and card list pin to the top of the window (bounded `max_rect` layout; resets stale egui resize state from earlier builds).
- Main window header, mode tabs, and controls respect the right content margin again (no longer sized from the full window clip rect).
- **Videos** queue card list fills the panel below the toolbar (explicit height allocation; scroll region uses space down to the panel bottom, not shrink-wrapped `max_rect`).
- **Videos** / **AV1 queue** panels show cards from the top with dock/hide/action buttons pinned below the list (floating window layout simplified).
- Docked **Videos** panel height is remembered when you drag the resize handle (content fills the panel; height saved in settings).
- **AV1 Converter**: Start/Cancel/Clear batch actions moved to the queue panel footer (same layout as Downloader queue actions).
- Queue cards show a **File missing** badge on the thumbnail when the source file is not on disk (AV1 queue and finished Downloader rows with a missing save).
- Main controls scroll shrinks to content height (removes the large empty gap above the bottom panel).
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
