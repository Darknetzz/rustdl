# Changelog

All notable changes to **rustdl** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

When releasing, bump `version` in `Cargo.toml`, add a dated section below, and tag `rustdl-vX.Y.Z` for GitHub release builds.

**Version history:** **`0.4.7`** is the first GitHub-tagged release. The **`0.1.x`** sections below are the pre-tag development baseline (no `rustdl-v0.1.3` tag). In-tree bumps through **`0.4.6`** were not cut as separate releases—those changes were rolled into **`0.4.7`**, which is why the jump from **`0.4.7`** to **`0.1.3`** looks abrupt.

## [Unreleased]

### Added

- The video / convert queue **auto-undocks** when the main window is resized to the minimum inner size (920×760); it **re-docks** when the window grows again. Manual dock/undock (toolbar or Settings) is remembered until you change it.

### Fixed

- Floating **Activity log** window shows log lines again (scroll area fills the window; toolbar controls stay in a single row at the top).
- Queue card **URL…** and **Remove…** dropdowns no longer clip inside the card (desktop popups render above the scroll area; LAN web UI menus are no longer cut off by card overflow).

### Changed

- **Destination disk** labels (desktop header, LAN web UI header and settings hint) show a storage/hard-drive icon before the text.

- **Show log** / **Hide log** live only in the main header (also removed from the floating/docked log chrome); **Dock log** / **Undock log** stay on the log panel. Hiding the log no longer resets dock preference, so **Show log** restores the log where you left it.
- Downloader queue cards (desktop and LAN web UI): **Copy URL** and **Open URL** are grouped under a **URL…** menu; **Remove** uses danger styling.

## [0.4.7] - 2026-06-08

First tagged GitHub release; rolls up changes since the **0.1.3** baseline below (including in-tree **0.4.x** work that was never tagged separately).

### Documentation

- Release routine scripts: `scripts/bump_version.*` (semver bump in `Cargo.toml`) and `scripts/release.*` (finalize changelog, commit, tag, optional push). Documented in `AGENTS.md` and `README.md`.

### Added

- **Mode panel colors** in Settings → Shared (desktop and LAN web UI): customize the Downloader and Video Converter panel tint / accent (defaults: blue and purple). Use **Default** to restore built-in colors.
- Downloader queue cards (desktop and LAN web UI) include **Copy URL** and **Open URL** on every row that has a saved page link (paste URL, resolved `webpage_url`, or YouTube id).
- Downloader queue thumbnails and URLs are saved when metadata resolves: preview images go to `thumbnails/downloader/` under the rustdl config folder (with a JSON sidecar for `webpage_url`, `thumbnail_url`, and `source_line`), the queue JSON records `thumbnail_path`, and URLs are stored from the moment a link is added.

### Fixed

- LAN web UI queue card thumbnails (and in-browser playback) work again: Axum 0.7 path routes used `{id}` syntax from 0.8, so `/api/thumbnail/:id` and `/api/media/:id` never matched and always returned 404.
- Saved downloader thumbnails load in the LAN web UI even when the cache key drifts (e.g. after `local_path` is backfilled on Windows extended-length paths); Windows `\\?\` download paths are recognized under the output folder again.
- Completed downloader cards no longer stay on **Thumbnail unavailable** when the file is on disk: Done/Failed rows always load previews (even in large queues), saved `local_path` is used for ffmpeg frame grabs, and the local file is tried before remote CDN URLs.
- Downloader queue card thumbnails load again when YouTube CDN URLs fail: the desktop UI now tries all preview URL candidates (not only the metadata URL) and falls back to an ffmpeg frame grab from the downloaded file, matching the LAN web UI. Cards show **Thumbnail unavailable** after all sources fail instead of staying on **Fetching thumbnail...** forever; thumbnails retry automatically when a download finishes.

- Activity log under the docked Videos panel no longer renders blank: the queue list reserves the configured log height instead of squeezing it away, and a height slider adjusts `log_dock_height` there too.
- Download progress lines are recorded on the shared core (not only when the GUI event loop handles them), so the activity log fills during batch downloads and survives restarts from `rustdl_activity_log.json`.
- Activity log open/docked state, dock height, undocked footer height, and floating window size are persisted across sessions (`logs_open` now defaults to open when missing from older configs).
- Activity log no longer appears blank after downloads or converts: GUI log lines go through the shared core, the GUI no longer overwrites core logs each frame (which dropped convert/download messages from background work), **Important** filter includes convert/skip messages, and a hint appears when the filter hides all lines.
- Floating activity log window layout uses remaining height correctly so log lines fill the resizable window.
- LAN web UI **Convert** tab no longer fails silently (typo in view switch threw a JavaScript error before the page could open).
- LAN web UI activity log shows an empty-state hint, supports vertical drag-resize, and refreshes every 5 seconds (not only when SSE is disconnected).
- LAN web UI no longer shows **Save API token to load thumbnails** when a token is saved but the preview fetch failed (wrong token, no preview yet, or ffmpeg missing); shows **Thumbnail unavailable** or **Token rejected** instead, and thumbnail 401 responses reopen the Connect screen like other API calls.
- Video Converter batch summary now reports **output growth** (e.g. `output +4.1 GiB (+71.9%)`) when encoded files are larger than the sources, instead of incorrectly showing `saved 0B (0.0%)`.
- Video Converter in-place replacement (delete original + rename to original filename) now encodes next to the source file instead of the shared downloader output folder, so the final file overwrites the original path.

### Added

- **Batch progress bars** on download and converter queues (desktop and LAN web UI): overall percentage includes partial credit for active items, with a second transfer bar on downloads when byte totals are known.
- **Retry skipped** on the Video Converter queue (desktop and LAN web UI): resets skipped rows to Ready so you can lower **Min shrink %** and run **Start Convert batch** without clearing and re-scanning paths.
- **Video Converter** mode (formerly AV1 Converter): session-wide target codec **AV1** (default), **H.265**, or **H.264** with per-codec encoder auto-detect and recommended containers (MKV for AV1, MP4 for H.264/H.265).
- Target codec selector in Settings → Converter (desktop and LAN web UI).

### Changed

- Settings window uses a two-column table layout so labels and controls align across Shared, Downloader, AV1, and Web UI tabs.
- Download and converter batch progress (status counts and progress bars) now appear only in the Videos panel or floating window, not duplicated in the main downloader column or undocked footer strip.
- **Breaking:** AV1 mode rebranded to **Video Converter**; `last_mode` `av1` → `convert`; settings keys `av1_*` → `convert_*` (legacy aliases still load); queue file `rustdl_av1_queue.json` → `rustdl_convert_queue.json` (legacy still read); LAN API `/api/av1/*` → `/api/convert/*`; SSE events `av1_*` → `convert_*`.
- Output filenames use `-AV1`, `-H265`, or `-H264` suffix per target codec.
- Audio defaults: Opus for AV1/MKV/WebM; AAC for H.264/H.265 MP4.

### Added

- Startup warning banner when settings, queue, profiles, or activity log JSON fails to parse (original file renamed to `.bak` when possible); same warnings in activity log, `--web-only` stderr, and LAN `/api/status`.
- Layout QA checklist in `AGENTS.md` for docked/floating queue and log panels.
- **Command palette** (Ctrl/Cmd+K): fuzzy search for Settings tabs, Start/Pause/Resume, log toggle, mode switch, and more.
- **Layout presets** in Settings → Shared: Compact queue, Review mode, and Minimal one-click display bundles (desktop and LAN web UI).
- **Session restore preference** in Settings → Shared: always restore, never restore, or ask each startup (desktop).
- **Max content width** slider in Settings → Shared for ultrawide monitors.
- Desktop **profile rename and delete** for user-defined download profiles; profiles now include cookies, impersonate, speed limit, and verify settings.
- LAN web UI: **queue search**, clickable **status filter chips**, **config warning banner**, **light/dark theme** toggle, **expand log** control, layout presets, and keyboard shortcuts (Ctrl/Cmd+, F, L, Enter, D).
- Settings → **Web UI**: **QR code** for LAN URL + token (scan on this PC; use LAN IP on phone).
- Expanded keyboard shortcuts on desktop: Ctrl/Cmd+, (Settings), F (focus queue search), L (toggle log), K (command palette), Escape (close dialogs).

### Changed

- LAN web UI: output disk space badge moved from the queue status row into the top header (next to tool status).
- Startup asks whether to restore saved downloader and AV1 queues from the previous session instead of loading them automatically (headless `--web-only` still auto-restores).
- LAN web UI: SSE events update download progress and logs in place instead of always refetching every endpoint; queue refresh skips unchanged generations; 5s fallback polling pauses while SSE is connected.
- Desktop download control (Start, Pause, Resume, Retry, Redo, cancel) now goes through the shared `DownloadCore` service (same path as the LAN web UI) instead of duplicate GUI-only logic.
- GUI↔core sync skips full queue clones when core generation is unchanged; patches queue rows in place when the generation bumps.
- README documents LAN AV1 converter support, desktop-only web gaps, expanded Download settings (cookies, archive, proxy, speed limit), and `--download` CLI limitations.
- Tool version labels show **unknown** when a binary is found but its version probe fails.
- Settings → Shared shows a clearer note when LAN bind address uses `0.0.0.0`.
- Queue search and activity log filter persist across restarts (Settings / shared config).
- Settings tab choice persists when switching tabs without changing other settings.
- LAN web UI honors **relative log timestamps** and **autoscroll log** settings; activity log uses the same formatting rules as the desktop app.
- LAN web UI settings (enable, bind address, API token, QR connect helper) moved to their own **Web UI** tab in Settings (was under Shared).
- Main header: Web UI / activity status and PATH tool checks with destination disk space sit on one row next to the title; **Settings** and **Exit** stay on the right.

### Fixed

- Main header **Settings** and **Exit** buttons align to the far right again (header row uses full content width).
- Destination disk progress bar shows **used** percentage (e.g. 80%) instead of free; hover still shows both used and free.
- Destination disk bar color follows **used** space (warning from 75%, critical from 90%); free-space text color still reflects absolute free bytes.
- Main header **Settings** button no longer clips away when disk status is wide (actions reserve a fixed right column).
- Main header **Show log**, **Settings**, and **Exit** sit flush on the right (flex spacer between status row and actions).
- Main header actions stay inside the content right margin (left status row is width-capped so buttons cannot overflow).
- Header status text (tool checks, activity badge, destination disk) is slightly larger (+2px).
- Settings → Shared: **Copy** API token button briefly shows **Copied!** after a successful clipboard copy.
- Main window and docked queue panels keep the right-side content inset again (scroll-area width no longer bleeds to the window edge).
- LAN web UI: queue thumbnails load from a shared cache populated by the desktop app and fixed proxy fetch order (metadata URL before local ffmpeg); thumbnail retries no longer stick after transient failures.
- Floating **Activity log** window is resizable again (explicit scroll height from the window body; resets stale layout state).
- Drag-to-reorder Ready queue items now mark the queue dirty so order changes sync to the LAN web UI.
- Removed duplicate **Effective command preview** block in Settings → Downloader.
- **Cancel all → Ready** and **Cancel all → Remove** moved from Download options into the **Videos** queue toolbar (docked and floating), grouped under a **Cancel all…** dropdown.

## [0.1.3] - 2026-06-02

Pre-tag development baseline (not released or tagged on GitHub).

### Added

- **LAN web UI** (Settings → Shared): optional HTTP server with token auth, REST API, SSE progress stream, and embedded web pages to control the downloader queue from other devices on the local network.
- LAN web UI: **Quit** button to gracefully shut down rustdl (cancels active jobs, saves queue/settings; closes the desktop app or stops `--web-only`).
- AV1 Converter: undock the encode queue to a floating window (same controls as Downloader **Videos**).

### Changed

- **Activity log** controls are on the **Videos** panel toolbar (Show/Hide/Dock log); the header always shows **Show log** or **Hide log** (no longer hidden when the log is open).
- New installs open the activity log by default (dock under the queue); existing saved settings are unchanged.
- **Show log** is its own header button (not fused with Settings/Exit); queue **Videos** dock/hide controls sit on a separate row from Pause/Export/Clear; docked activity log uses the same placement vs. toolbar split as the floating log window.
- Import and export actions are grouped under an **Import/Export** dropdown (URL input, download queue, settings, and profiles) instead of separate toolbar buttons.
- **Show log** moved to the top header (next to Settings) instead of above the download queue.
- Destination disk free space is shown in the top header next to the yt-dlp/ffmpeg/ffprobe checks (removed from Download options); redundant **Queue** label above the action toolbar removed.
- Done download cards show the most recently finished items first (left in the horizontal strip).
- Failed download cards show **Retry** only (not a duplicate **Redo**); **Redo** remains on completed downloads.
- LAN web UI: **Downloader** and **AV1** sections use distinct accent colors (blue / purple) on the nav toggle, panels, and primary actions.
- Desktop GUI: **Downloader** and **AV1** panels use the same muted mode tint and left accent stripe as the LAN web UI (main controls and queue panels); mode nav tabs use blue / purple when active.
- LAN web UI: queue thumbnails load reliably during live SSE updates (debounced refresh, ignore aborted image loads, typed image blobs).

### Fixed

- Header tool checks and destination disk info (including the free-space bar) stay on one line instead of stacking the bar below the text.
- Queue search moved from Download options into the **Videos** panel (docked and floating window).
- **Open output folder** moved into the **Videos** panel action toolbar (removed duplicate from Download options and the session-finished banner).
- Floating **Videos** / **Activity log** windows and the docked queue panel keep their size when resized (content fills the allocated area so egui no longer snaps back to shrink-wrapped height).
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

[0.4.7]: https://github.com/Darknetzz/rustdl/releases/tag/rustdl-v0.4.7
[Unreleased]: https://github.com/Darknetzz/rustdl/compare/rustdl-v0.4.7...dev
[0.1.3]: https://github.com/Darknetzz/rustdl/compare/rustdl-v0.1.1...rustdl-v0.1.3
[0.1.1]: https://github.com/Darknetzz/rustdl/compare/rustdl-v0.1.0...rustdl-v0.1.1
[0.1.0]: https://github.com/Darknetzz/rustdl/tree/rustdl-v0.1.0
