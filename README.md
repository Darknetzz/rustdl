<h1 align="center">rustdl</h1>

<p align="center">
  <img src="assets/rustdl-icon.png" alt="rustdl icon" width="128" />
</p>

<p align="center">Desktop GUI for <a href="https://github.com/yt-dlp/yt-dlp">yt-dlp</a> built in Rust with <code>eframe/egui</code>.</p>

<p align="center">Source: <a href="https://github.com/Darknetzz/rustdl">github.com/Darknetzz/rustdl</a> · mirror: <a href="https://gitlab.roste.org/kriss/rustdl">gitlab.roste.org/kriss/rustdl</a></p>

## Requirements

- Rust stable toolchain (`cargo`)
- `yt-dlp` on `PATH` (or set custom executable path in Settings)
- Optional: `ffmpeg` and `ffprobe` on `PATH` (or set custom executable paths in Settings)

## Run

From this directory:

```bash
cargo run
```

Headless download (no GUI):

```bash
cargo run -- --download "https://..." [--profile "Best quality"] [--output-dir "C:\path"] [--dry-run]
cargo run -- --download @urls.txt          # batch from file (one URL per line)
cargo run -- --download -                  # batch from stdin
cargo run -- --list-profiles
```

`--download` runs yt-dlp directly (no shared download queue, activity log persistence, or LAN web UI). Use the GUI or `--web-only` for full queue/history behavior.

Headless web UI (no GUI window; uses saved queue, settings, and profiles):

```bash
cargo run -- --web-only --host 0.0.0.0 --port 8765
cargo run -- --web-only                    # bind address from Settings → Web UI
```

On first run without a saved API token, rustdl generates one and prints it. Open the local URL from another device on your LAN with that token (same security notes as the desktop LAN web UI).

## Versioning and changelog

- App version comes from `Cargo.toml` (shown in **About** and `rustdl --version`).
- User-facing changes are recorded in [CHANGELOG.md](CHANGELOG.md) ([Keep a Changelog](https://keepachangelog.com/) style).
- For a release: bump `version` in `Cargo.toml`, finalize `CHANGELOG.md`, commit, and push tag `rustdl-vX.Y.Z` (see [.github/workflows/release.yml](.github/workflows/release.yml); [GitHub releases](https://github.com/Darknetzz/rustdl/releases)).

## Development checks

Run these before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Features

- Paste URLs (one per line), then click **Add URLs** for metadata preview cards.
- Duplicate URL dedupe and playlist preview capping.
- Queue downloads with per-item progress, status, size, and live log output.
- About dialog with app version and update check.
- Settings persistence in user config directory.
- Persisted activity log with timestamps; optional docked log panel under the queue.
- Queue search, bulk selection, pause/resume downloads, drag-to-reorder Ready items (list layout), import/export queue URLs to `.txt`.
- Named download profiles (built-in + user-defined), quality presets, output filename template, download archive, proxy, speed limit, and SponsorBlock options.
- Light / dark / system theme; last mode (Downloader vs Video Converter) remembered across restarts.
- Desktop notification when a download session finishes (where supported by the OS).
- **Video Converter** mode for local file/folder transcoding to AV1 (default), H.265, or H.264 with queue progress, dry-run, cancel, and encoder auto-detect.
- Optional: enqueue each completed video download into the Video Converter queue (Settings → Downloader).
- Optional **LAN web UI**: control the downloader queue from a phone or another PC on your home network (Settings → **Web UI**).

## LAN web UI

When enabled in **Settings → Web UI**, rustdl serves a built-in web interface on the configured bind address (default `0.0.0.0:8765`). Open `http://<this-pc-ip>:8765/` from another device on the same network, paste the **API token** shown in Settings, then use the page to control the **Downloader** queue (add URLs, start/pause, settings, activity log) and the **Video Converter** queue (scan paths, start/cancel batch encode).

**Still desktop-only:** drag-to-reorder Ready items, queue/settings file import-export dialogs, desktop notifications on session complete, update check (About), browser URL drag-and-drop (Windows only).

**Security notes:**

- Traffic is plain **HTTP** (no TLS). Anyone who can reach the bind address and knows the token can control downloads and read queue metadata.
- Use only on a **trusted home LAN**. Do not expose the port to the public internet without a reverse proxy, TLS, and stronger authentication.
- Generate a new token if you suspect it was leaked. Disabling the web UI stops the HTTP server on the next settings save (or when you restart the app).

## Modes

`rustdl` now has two top-level modes:

- **Downloader**: the original yt-dlp workflow (URL preview cards and downloads).
- **Video Converter**: local file/folder conversion to AV1, H.265, or H.264 (session-wide target) from a dedicated in-app panel.

Switch modes from the **Mode** toggle near the top of the main window.

### Video Converter notes

- Input accepts file and folder paths (one per line).
- Output goes to the current **Output folder**.
- **Target codec** (Settings → Converter): AV1 (default), H.265, or H.264.
- Supports recursive scan, dry-run, overwrite, delete original, rename to original filename, and optional re-encode when input already matches the target codec.
- Queue items are remembered between sessions until you click **Clear** (disable in Settings → Converter → *Remember Convert queue between sessions* to start fresh each launch).
- Encoder auto-detect per target: AV1 → `av1_nvenc` → `av1_amf` → `libsvtav1`; H.265 → `hevc_nvenc` → `hevc_amf` → `libx265`; H.264 → `h264_nvenc` → `h264_amf` → `libx264`.
- Uses the shared **ffmpeg** and **ffprobe** paths from Settings → Shared.

## Settings

Open **Settings** from the main toolbar.

Settings are split into tabs:

### General

| Setting | Description |
| --- | --- |
| Show thumbnails in cards | Enables/disables thumbnail loading and display on video cards |
| Use compact cards | Uses denser card layout for larger queues |
| Hide card subtitle/uploader | Hides secondary subtitle text on cards |
| UI scale | Global UI zoom factor (`0.85..=1.5`), useful for larger/smaller display density |
| Auto-add pasted URLs after a short delay | When enabled, valid pasted URLs are auto-queued for metadata fetch; when disabled, use **Add URLs** manually |
| Auto-start downloads when new items become ready | Optional. When enabled, starts downloads automatically after metadata resolution completes |
| **LAN web UI** (enable, bind address, API token) | Settings → **Web UI** tab; see [LAN web UI](#lan-web-ui) |
| Enqueue completed downloads in Video Converter queue | After a successful video download, adds the output file to the converter queue (skipped for audio-only / MP3 extraction) |
| Autoscroll log to latest line | Keeps the log viewer pinned to the newest lines while logs are appended |
| Parallel downloads | Number of concurrent worker queues used when starting downloads (`1..=6`) |
| Max log chars | Maximum in-memory log buffer length before older characters are trimmed |
| Dock activity log under video queue | Shows the log in a resizable panel below cards instead of a floating window |
| Relative timestamps in activity log | Shows ages like `5 min ago` instead of full local time in the log viewer |
| List layout for queue cards | Compact list rows instead of horizontal preview cards |

### Executables

Leave each field empty to use normal `PATH` resolution.

| Setting | Description |
| --- | --- |
| yt-dlp path | Custom executable name or absolute path for `yt-dlp` |
| ffmpeg path | Custom executable name or absolute path for `ffmpeg` |
| ffprobe path | Custom executable name or absolute path for `ffprobe` |

These paths are also used by dependency checks shown at the top of the app.

### Download

#### Presets

Presets are quick-start bundles in **Settings -> Download** that set multiple toggles at once:

| Preset | What it does |
| --- | --- |
| Best quality | Disables audio-only/remux toggles, enables faststart, sets extra args to `--merge-output-format mp4` |
| Audio only | Enables MP3 extraction and disables remux-to-mp4 |
| Fast download | Prioritizes speed with `--concurrent-fragments 4` and ignore errors (HTTP retries remain unlimited by default) |
| Archive mode | Enables writing metadata artifacts (`info.json`, subtitles, embedded metadata) and sets extra args to `--write-description` |

Presets update current settings immediately, and you can still tweak any individual fields afterward.

#### Pasting multiple URLs

- Paste one URL per line.
- Input lines are validated and deduped before queueing.
- Duplicate lines in the same paste are ignored.
- URLs that are already present in the queue are skipped.
- Invalid URL lines are not queued.

#### yt-dlp options

| Setting | Description | Added flag(s) |
| --- | --- | --- |
| Extra args | Raw extra arguments appended to every `yt-dlp` download command (space-separated) | User-provided |
| Embed thumbnail | Embed thumbnail into output when supported | `--embed-thumbnail` |
| Embed metadata | Embed metadata into output when supported | `--embed-metadata` |
| Ignore errors | Continue processing when an item fails | `--ignore-errors` |
| Restrict filenames | Safer ASCII-like filenames | `--restrict-filenames` |
| Write info JSON | Save metadata as JSON file | `--write-info-json` |
| Write auto subtitles | Download auto-generated subtitles | `--write-auto-subs` |
| Cookies | Netscape `cookies.txt` path or browser name for `--cookies-from-browser` (e.g. `firefox`) | `--cookies` / `--cookies-from-browser` |
| Impersonate browser | `--impersonate` target (e.g. `chrome`) for cookie-backed sites | `--impersonate` |
| Output filename template | yt-dlp output template (default includes title and id) | `-o` template |
| Download archive | Skip already-archived videos; file path written by `--download-archive` | `--download-archive` |
| Proxy | HTTP/HTTPS/SOCKS proxy URL | `--proxy` |
| Speed limit | Max download rate (e.g. `500K`, `1M`) | `--limit-rate` |

### Post-process

| Setting | Description | Added flag(s) |
| --- | --- | --- |
| Post-processor args | Passed to `yt-dlp` postprocessing | `--postprocessor-args "<value>"` |
| Enable faststart | Appends faststart flags to postprocessor args | `-movflags +faststart` (inside postprocessor args) |
| Remux video to mp4 | Remux output to MP4 container | `--remux-video mp4` |
| Extract audio as mp3 | Extract audio and encode MP3 | `--extract-audio --audio-format mp3` |

| Note | Details |
| --- | --- |
| Option precedence | If both **Extract audio as mp3** and **Remux video to mp4** are selected, mp3 extraction takes precedence |
| ffmpeg path usage | `ffmpeg path` is passed through `--ffmpeg-location` during downloads |
| Persistence timing | Settings are saved immediately when changed in the Settings window or toolbar |

## Settings file location

- Preferred: `<config_dir>/rustdl/rustdl_config.json`
- Windows example: `C:\Users\<you>\AppData\Roaming\rustdl\rustdl_config.json`
- Fallback: `./rustdl_config.json` (if config dir is unavailable)

Also in the same folder:

- `rustdl_queue.json` — saved download queue
- `rustdl_activity_log.json` — persisted activity log (survives restarts)

Open **Settings**, **Logs**, or **About** → **Open config folder** to reveal this directory in your file manager.

## Platform notes

### Browser drag-and-drop

Browser URL drag-and-drop (from Chrome, Firefox, etc.) is supported on **Windows** only via a custom shell `IDropTarget` ([`win_drop_target.rs`](src/win_drop_target.rs)).

**Linux and macOS (deferred):** winit/eframe exposes file drops (`.url`, `.txt`, shortcuts) but not browser URI-list payloads in a cross-platform way. Implementing parity would require platform-specific code (GTK/Wayland `text/uri-list` on Linux; `NSPasteboard` / `NSDragging` on macOS). Until then, use **paste**, **Import file**, or **Import queue** from a `.txt` export.

On all platforms you can paste URLs or drop `.url` / `.txt` / shortcut files onto the window.

## Build release binary

```bash
cargo build --release
```

Binary output:

- Windows: `target/release/rustdl.exe`
- Linux/macOS: `target/release/rustdl`

GitHub releases ship **Linux**, **Windows**, and **macOS** binaries. If you use macOS and prefer not to download a release build, run `cargo build --release` locally as above.
