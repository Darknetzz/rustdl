#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release
bin="$(pwd)/target/release/rustdl"
echo "Built: $bin"
echo "Run:   $bin"
