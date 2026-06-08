#!/usr/bin/env bash
# Bump semver in Cargo.toml (patch by default). Does not edit CHANGELOG — add bullets under [Unreleased] yourself.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: ./scripts/bump_version.sh [patch|minor|major]

Examples:
  ./scripts/bump_version.sh          # 0.4.6 -> 0.4.7
  ./scripts/bump_version.sh minor    # 0.4.6 -> 0.5.0
  ./scripts/bump_version.sh major    # 0.4.6 -> 1.0.0
EOF
}

part="${1:-patch}"
case "$part" in
  patch | minor | major) ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    echo "Unknown part: $part" >&2
    usage >&2
    exit 1
    ;;
esac

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cargo_toml="$repo_root/Cargo.toml"

current="$(grep -E '^version = ' "$cargo_toml" | head -1 | sed -E 's/version = "(.*)"/\1/')"
IFS='.' read -r major minor patch <<<"$current"
major="${major:-0}"
minor="${minor:-0}"
patch="${patch:-0}"

case "$part" in
  patch) patch=$((patch + 1)) ;;
  minor)
    minor=$((minor + 1))
    patch=0
    ;;
  major)
    major=$((major + 1))
    minor=0
    patch=0
    ;;
esac

new="${major}.${minor}.${patch}"

if [[ "$(uname -s)" == "Darwin" ]]; then
  sed -i '' -E "s/^version = \".*\"/version = \"${new}\"/" "$cargo_toml"
else
  sed -i -E "s/^version = \".*\"/version = \"${new}\"/" "$cargo_toml"
fi

echo "Bumped Cargo.toml version: ${current} -> ${new}"
echo "Remember to add a [Unreleased] bullet in CHANGELOG.md for user-visible changes in this commit."
