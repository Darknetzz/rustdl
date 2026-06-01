#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release
echo "Built: $(pwd)/target/release/rustdl"
