# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Downloader setting: enqueue completed video downloads in the AV1 converter queue.
- Shared `ytdlp_download_args` module: CLI and GUI use the same yt-dlp argument builder (with unit tests).
- Headless CLI: `--dry-run`, batch downloads from `@file.txt` or `-` (stdin).
- GUI: **Import queue** from `.txt`, **Open output folder** on completed cards, download speed limit setting (`--limit-rate`).
- `lib.rs` crate surface; integration tests in `tests/` (progress fixtures; optional `RUSTDL_IT=1` subprocess smoke).
- CI: macOS job, `Swatinem/rust-cache`; release workflow on `rustdl-v*` tags (Linux, Windows, macOS x64/arm64).

### Changed

- `app/mod.rs` split: `queue_persist`, `download_control`, `background_spawn`, `events`, `app_parsing`.
- `cargo deny` / `cargo audit` run on pushes to `main` only (PR CI stays faster).

### Documentation

- README: CLI batch/dry-run, platform drag-and-drop deferral rationale for Linux/macOS.

## [0.1.0] - 2026-03-30

Initial published version: desktop GUI for yt-dlp with queue, previews, settings, and download progress.

[Unreleased]: https://github.com/Darknetzz/code/commits/main/Rust/rustdl
[0.1.0]: https://github.com/Darknetzz/code/tree/main/Rust/rustdl
