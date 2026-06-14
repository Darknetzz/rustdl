#!/usr/bin/env bash
# Build and publish a rolling pre-release on GitHub (tag rustdl-dev).
#
# Usage:
#   ./scripts/publish_dev_release.sh
#   ./scripts/publish_dev_release.sh --dry-run
#   ./scripts/publish_dev_release.sh --skip-build
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

repo='Darknetzz/rustdl'
tag='rustdl-dev'
commit=''
dry_run=0
skip_build=0
allow_dirty=0

usage() {
  sed -n '2,8p' "$0" | sed 's/^# \?//'
  exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) dry_run=1 ;;
    --skip-build) skip_build=1 ;;
    --repo) repo="$2"; shift ;;
    --tag) tag="$2"; shift ;;
    --commit) commit="$2"; shift ;;
    --allow-dirty) allow_dirty=1 ;;
    -h|--help) usage 0 ;;
    *) echo "Unknown option: $1" >&2; usage 1 ;;
  esac
  shift
done

if ! command -v gh >/dev/null 2>&1; then
  echo 'gh CLI not found. Install GitHub CLI and run: gh auth login' >&2
  exit 1
fi

gh auth status >/dev/null

if [[ -z "$commit" ]]; then
  commit="$(git rev-parse HEAD)"
fi
short_commit="$(git rev-parse --short "$commit")"
branch="$(git rev-parse --abbrev-ref HEAD)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
notes_file="$repo_root/dev-release-notes.md"
title='rustdl dev (rolling)'

if [[ "$allow_dirty" -eq 0 ]] && [[ -n "$(git status --porcelain)" ]]; then
  echo 'Working tree is not clean. Commit/stash or pass --allow-dirty.' >&2
  exit 1
fi

unreleased_body() {
  awk '
    /^## \[Unreleased\]/ { in_u=1; next }
    in_u && /^## \[/ { exit }
    in_u { print }
  ' CHANGELOG.md | sed '/./,$!d'
}

write_notes() {
  local built unreleased compare
  built="$(date '+%Y-%m-%d %H:%M %z')"
  unreleased="$(unreleased_body || true)"
  compare="https://github.com/${repo}/compare/${tag}...${short_commit}"

  {
    echo '# rustdl dev (rolling)'
    echo
    echo "Pre-release build from the tip of \`${branch}\`. Stable builds: [GitHub Releases](https://github.com/${repo}/releases)."
    echo
    echo '| | |'
    echo '| --- | --- |'
    echo "| **Commit** | [${short_commit}](https://github.com/${repo}/commit/${commit}) |"
    echo "| **Cargo version** | ${version} |"
    echo "| **Built** | ${built} |"
    echo
    echo '## [Unreleased] (from CHANGELOG)'
    echo
    if [[ -n "${unreleased// }" ]]; then
      printf '%s\n' "$unreleased"
    else
      echo '_No bullets under `[Unreleased]` yet._'
    fi
    echo
    echo '---'
    echo "Compare to previous dev build: ${compare}"
  } >"$notes_file"
}

binary_path() {
  if [[ -f target/release/rustdl ]]; then
    echo target/release/rustdl
  elif [[ -f target/release/rustdl.exe ]]; then
    echo target/release/rustdl.exe
  else
    echo 'Release binary not found. Run ./scripts/build_binary.sh first (or drop --skip-build).' >&2
    exit 1
  fi
}

echo 'Rolling dev release'
echo "  Repo:    $repo"
echo "  Tag:     $tag"
echo "  Commit:  $short_commit ($commit)"
echo "  Version: $version"

if [[ "$skip_build" -eq 0 && "$dry_run" -eq 0 ]]; then
  echo
  echo 'Building release binary...'
  ./scripts/build_binary.sh
elif [[ "$skip_build" -eq 0 && "$dry_run" -eq 1 ]]; then
  echo
  echo 'Would run: ./scripts/build_binary.sh'
fi

write_notes

if [[ "$dry_run" -eq 1 ]]; then
  binary='target/release/rustdl (or rustdl.exe on Windows)'
  echo
  echo "Dry run: would publish $tag to $repo"
  echo "  Binary:  $binary"
  echo "  Notes:   $notes_file"
  exit 0
fi

binary="$(binary_path)"

if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  echo
  echo "Updating existing release $tag..."
  gh release edit "$tag" \
    --repo "$repo" \
    --title "$title" \
    --notes-file "$notes_file" \
    --prerelease \
    --target "$commit"
  gh release upload "$tag" "$binary" --repo "$repo" --clobber
else
  echo
  echo "Creating release $tag..."
  gh release create "$tag" \
    --repo "$repo" \
    --title "$title" \
    --notes-file "$notes_file" \
    --prerelease \
    --target "$commit" \
    "$binary"
fi

echo
echo "Published rolling dev release: https://github.com/${repo}/releases/tag/${tag}"
