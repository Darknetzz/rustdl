#!/usr/bin/env bash
# Run the full local CI checklist (replaces GitHub Actions for day-to-day dev).
# Usage: ./scripts/ci_local.sh [--skip-deny] [--skip-audit]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

SKIP_DENY=0
SKIP_AUDIT=0
for arg in "$@"; do
  case "$arg" in
    --skip-deny) SKIP_DENY=1 ;;
    --skip-audit) SKIP_AUDIT=1 ;;
    -h|--help)
      echo "Usage: $0 [--skip-deny] [--skip-audit]"
      exit 0
      ;;
    *)
      echo "Unknown option: $arg" >&2
      exit 2
      ;;
  esac
done

echo 'ci_local: cargo fmt --check'
cargo fmt --all -- --check

echo 'ci_local: cargo clippy'
cargo clippy --all-targets --all-features -- -D warnings

echo 'ci_local: cargo test'
cargo test --all-targets --all-features

if [[ "$SKIP_DENY" -eq 0 ]]; then
  if ! command -v cargo-deny >/dev/null 2>&1; then
    echo 'ci_local: installing cargo-deny'
    cargo install cargo-deny --locked
  fi
  echo 'ci_local: cargo deny'
  cargo deny check
fi

if [[ "$SKIP_AUDIT" -eq 0 ]]; then
  if ! command -v cargo-audit >/dev/null 2>&1; then
    echo 'ci_local: installing cargo-audit'
    cargo install cargo-audit --locked
  fi
  echo 'ci_local: cargo audit'
  cargo audit
fi

echo 'ci_local: all checks passed.'
