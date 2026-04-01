#!/usr/bin/env bash
# Build a single-file pydl executable via flet pack (recommended for Flet apps).
# Thin wrapper: runs scripts/build_binary.py from the repository root.
# Usage: ./scripts/build_binary.sh [--distpath DIR] [--onedir] [-- ...]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python "$ROOT/scripts/build_binary.py" "$@"
