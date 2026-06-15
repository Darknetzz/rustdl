#!/usr/bin/env bash
# Cut a rustdl release: finalize CHANGELOG, commit, tag rustdl-vX.Y.Z, optionally push.
set -euo pipefail

GITHUB_REPO='Darknetzz/rustdl'
DEFAULT_REMOTE='github'

usage() {
  cat <<'EOF'
Usage: ./scripts/release.sh [options]

Options:
  --dry-run       Show planned version, tag, and changelog edits; do not commit or tag
  --skip-checks   Skip cargo fmt, clippy, and test
  --push          Push dev and the release tag to --remote after tagging
  --remote NAME   Git remote for --push (default: github)
  --yes           Skip confirmation prompt
  -h, --help      Show this help

Routine:
  1. During development: add [Unreleased] bullets; bump Cargo.toml on medium+ changes
     (./scripts/bump_version.sh [patch|minor|major]).
  2. When ready to ship: ./scripts/release.sh --dry-run, then ./scripts/release.sh
  3. Publish: ./scripts/release.sh --push  (or push dev + tag manually)

Pushing tag rustdl-v* triggers .github/workflows/release.yml (multi-platform binaries + GitHub Release).
EOF
}

dry_run=0
skip_checks=0
do_push=0
assume_yes=0
remote="$DEFAULT_REMOTE"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) dry_run=1 ;;
    --skip-checks) skip_checks=1 ;;
    --push) do_push=1 ;;
    --remote)
      remote="${2:-}"
      shift
      ;;
    --yes) assume_yes=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

read_version() {
  grep -E '^version = ' Cargo.toml | head -1 | sed -E 's/version = "(.*)"/\1/'
}

latest_released_version() {
  local from_tags
  from_tags="$(git tag -l 'rustdl-v*' | sed 's/^rustdl-v//' | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -1)"
  if [[ -n "$from_tags" ]]; then
    echo "$from_tags"
    return
  fi
  grep -E '^## \[[0-9]+\.[0-9]+\.[0-9]+\]' CHANGELOG.md \
    | sed -E 's/^## \[([0-9]+\.[0-9]+\.[0-9]+)\].*/\1/' \
    | sort -V \
    | tail -1
}

unreleased_has_content() {
  local body
  body="$(awk '
    /^## \[Unreleased\]/ { in_unreleased=1; next }
    in_unreleased && /^## \[/ { exit }
    in_unreleased { print }
  ' CHANGELOG.md)"
  body="$(printf '%s' "$body" | sed '/^[[:space:]]*$/d')"
  [[ -n "$body" ]]
}

finalize_changelog() {
  local version="$1"
  local date="$2"
  local prev="$3"
  local changelog='CHANGELOG.md'
  local tmp
  tmp="$(mktemp)"

  awk -v ver="$version" -v dt="$date" '
    BEGIN { in_unreleased=0; captured=0 }
    /^## \[Unreleased\]/ {
      print "## [Unreleased]"
      print ""
      print "## [" ver "] - " dt
      in_unreleased=1
      next
    }
    in_unreleased && /^## \[/ {
      in_unreleased=0
      print ""
      print $0
      next
    }
    in_unreleased {
      if (!captured && $0 ~ /[^[:space:]]/) { captured=1 }
      if (captured) { print }
      next
    }
    { print }
  ' "$changelog" >"$tmp"

  if ! grep -q "^## \\[$version\\]" "$tmp"; then
    rm -f "$tmp"
    echo "Failed to insert release section for $version" >&2
    exit 1
  fi

  local unreleased_link="[Unreleased]: https://github.com/${GITHUB_REPO}/compare/rustdl-v${version}...dev"
  local version_link="[${version}]: https://github.com/${GITHUB_REPO}/compare/rustdl-v${prev}...rustdl-v${version}"

  if grep -q '^\[Unreleased\]:' "$tmp"; then
    if [[ "$(uname -s)" == "Darwin" ]]; then
      sed -i '' "s|^\[Unreleased\]:.*|$unreleased_link|" "$tmp"
    else
      sed -i "s|^\[Unreleased\]:.*|$unreleased_link|" "$tmp"
    fi
  else
    printf '\n%s\n' "$unreleased_link" >>"$tmp"
  fi

  if ! grep -q "^\[${version}\]:" "$tmp"; then
    if [[ "$(uname -s)" == "Darwin" ]]; then
      sed -i '' "s|^\[Unreleased\]:|${version_link}\n[Unreleased]:|" "$tmp"
    else
      sed -i "s|^\[Unreleased\]:|${version_link}\n[Unreleased]:|" "$tmp"
    fi
  fi

  mv "$tmp" "$changelog"
}

version="$(read_version)"
tag="rustdl-v${version}"
date="$(date +%Y-%m-%d)"
prev="$(latest_released_version)"

if [[ -z "$prev" ]]; then
  echo "Could not find a previous released version in CHANGELOG.md" >&2
  exit 1
fi

if ! unreleased_has_content; then
  echo "[Unreleased] in CHANGELOG.md is empty; nothing to release." >&2
  exit 1
fi

if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "Tag $tag exists (from version bump); will move to the release commit."
fi

branch="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$branch" != "dev" ]]; then
  echo "Warning: not on dev branch (on $branch)." >&2
fi

if [[ -n "$(git status --porcelain)" && "$dry_run" -eq 0 ]]; then
  echo "Working tree is not clean. Commit or stash changes before releasing." >&2
  exit 1
fi

echo "Release plan"
echo "  Version:     $version"
echo "  Tag:         $tag"
echo "  Date:        $date"
echo "  Previous:    $prev"
echo "  Compare:     rustdl-v${prev}...rustdl-v${version}"
echo "  Remote push: $([ "$do_push" -eq 1 ] && echo "$remote (dev + tag)" || echo "no (local commit + tag only)")"

if [[ "$skip_checks" -eq 0 && "$dry_run" -eq 0 ]]; then
  echo
  echo "Running pre-release checks (fmt, clippy, test)..."
  cargo fmt --all -- --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-targets --all-features
elif [[ "$skip_checks" -eq 0 && "$dry_run" -eq 1 ]]; then
  echo
  echo "Would run: cargo fmt --check, clippy, test"
fi

if [[ "$dry_run" -eq 1 ]]; then
  echo
  echo "Dry run: no files modified, no commit, no tag."
  exit 0
fi

if [[ "$assume_yes" -eq 0 ]]; then
  read -r -p "Proceed with release commit and tag? [y/N] " reply
  case "$reply" in
    y | Y | yes | YES) ;;
    *)
      echo "Aborted."
      exit 1
      ;;
  esac
fi

finalize_changelog "$version" "$date" "$prev"

git add CHANGELOG.md
git commit -m "release: v${version}"

if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null 2>&1; then
  echo "Tag ${tag} exists (from a version bump); moving to release commit."
  git tag -f -a "$tag" -m "release: rustdl ${version}"
else
  git tag -a "$tag" -m "rustdl ${version}"
fi

notes_file="${repo_root}/release-notes.md"
./scripts/extract_release_notes.sh CHANGELOG.md "$tag" >"$notes_file"

echo
echo "Created commit and tag $tag."
echo "Release notes: $notes_file (gitignored temp file for gh release create)"

if [[ "$do_push" -eq 1 ]]; then
  git push "$remote" dev
  git push "$remote" "$tag"
  echo "Pushed dev and $tag to $remote."
else
  echo "Next:"
  echo "  git push $remote dev"
  echo "  git push $remote $tag"
  echo "  ./scripts/build_binary.sh"
  echo "  gh release create $tag --title \"rustdl $version\" --notes-file release-notes.md target/release/rustdl"
fi
