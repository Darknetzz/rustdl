<h1 align="center">rustdl</h1>

<p align="center">
  <img src="assets/rustdl-icon.png" alt="rustdl icon" width="128" />
</p>

<p align="center">Desktop GUI for <a href="https://github.com/yt-dlp/yt-dlp">yt-dlp</a> built in Rust with <code>eframe/egui</code>.</p>

<p align="center">Source: <a href="https://github.com/Darknetzz/code">github.com/Darknetzz/code</a> (this crate lives at <code>Rust/rustdl/</code> in that repo).</p>

## Requirements

- Rust stable toolchain (`cargo`)
- `yt-dlp` on `PATH` (or set custom executable path in Settings)
- Optional: `ffmpeg` and `ffprobe` on `PATH` (or set custom executable paths in Settings)

## Run

From this directory:

```bash
cargo run
```

From the monorepo root (`code`):

```bash
cargo run --manifest-path Rust/rustdl/Cargo.toml
```

## Development checks

Run these before opening a PR (from `Rust/rustdl/`, or prefix each with `cargo ... --manifest-path Rust/rustdl/Cargo.toml` from the monorepo root):

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
- Queue search, bulk selection, pause/resume downloads, and export URLs to `.txt`.
- Desktop notification when a download session finishes (where supported by the OS).

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
| Fast download | Prioritizes speed/retries with extra args `--concurrent-fragments 4 --retries 10`, enables ignore errors |
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
| Persistence timing | Settings are persisted automatically whenever values change |

## Settings file location

- Preferred: `<config_dir>/rustdl/rustdl_config.json`
- Windows example: `C:\Users\<you>\AppData\Roaming\rustdl\rustdl_config.json`
- Fallback: `./rustdl_config.json` (if config dir is unavailable)

Also in the same folder:

- `rustdl_queue.json` — saved download queue
- `rustdl_activity_log.json` — persisted activity log (survives restarts)

Open **Settings**, **Logs**, or **About** → **Open config folder** to reveal this directory in your file manager.

## Platform notes

- **Browser drag-and-drop** for URLs (from Chrome, Firefox, etc.) is supported on **Windows** only. On Linux and macOS, paste URLs or drop `.url` / `.txt` / shortcut files onto the window.

## Build release binary

```bash
cargo build --release
```

Binary output:

- Windows: `target/release/rustdl.exe`
- Linux/macOS: `target/release/rustdl`

GitHub releases ship **Linux**, **Windows**, and **macOS** binaries. If you use macOS and prefer not to download a release build, run `cargo build --release` locally as above.
