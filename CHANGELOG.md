# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

- Internal: `app` split into `events`, `log_panel`, and existing UI modules; metadata fetch uses Tokio `spawn_blocking`.
- CI: optional `cargo deny` and `cargo audit` jobs; release workflow for version tags.
- `CHANGELOG` added for release notes.

## [0.1.0] - 2026-03-30

Initial published version: desktop GUI for yt-dlp with queue, previews, settings, and download progress.

[Unreleased]: https://github.com/Darknetzz/code/commits/main/Rust/rustdl
[0.1.0]: https://github.com/Darknetzz/code/tree/main/Rust/rustdl
