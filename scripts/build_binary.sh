#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release
bin="$(pwd)/target/release/rustdl"
echo "Built: $bin"
echo "Run:   $bin"
echo "Note:  if no window appears, run from a system terminal (not Cursor)."
echo "       On Linux/Wayland stuck GUIs: killall rustdl && ./target/release/rustdl"
