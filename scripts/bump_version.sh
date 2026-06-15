#!/usr/bin/env bash
# Bump semver in Cargo.toml (patch by default). Does not edit CHANGELOG — add bullets under [Unreleased] yourself.
# After bumping, creates rustdl-vX.Y.Z on HEAD when that commit already has the new version.
# Run with --tag-only after committing a manual version bump.
# Push dev to github to publish the stable release (pre-push hook / push_dev).
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: ./scripts/bump_version.sh [patch|minor|major] [--tag-only] [--no-tag]

Examples:
  ./scripts/bump_version.sh          # 0.4.6 -> 0.4.7 (+ tag when HEAD matches)
  ./scripts/bump_version.sh minor    # 0.4.6 -> 0.5.0
  ./scripts/bump_version.sh --tag-only
EOF
}

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cargo_toml="$repo_root/Cargo.toml"

cargo_version_from_file() {
  grep -E '^version = ' "$cargo_toml" | head -1 | sed -E 's/version = "(.*)"/\1/'
}

cargo_version_at_head() {
  git -C "$repo_root" show 'HEAD:Cargo.toml' 2>/dev/null |
    grep -E '^version = ' | head -1 | sed -E 's/version = "(.*)"/\1/' || true
}

create_rustdl_version_tag() {
  local version="$1"
  local tag="rustdl-v${version}"
  local head_ver

  if git -C "$repo_root" rev-parse -q --verify "refs/tags/${tag}" >/dev/null 2>&1; then
    echo "Tag ${tag} already exists at $(git -C "$repo_root" rev-parse --short "${tag}")."
    return 0
  fi

  head_ver="$(cargo_version_at_head)"
  if [[ "$head_ver" != "$version" ]]; then
    echo "Cannot tag yet: HEAD has version '${head_ver}', expected '${version}'."
    echo "Commit Cargo.toml with version ${version}, then run: ./scripts/bump_version.sh --tag-only"
    return 0
  fi

  git -C "$repo_root" tag -a "$tag" -m "rustdl ${version}"
  echo "Created annotated tag ${tag} at $(git -C "$repo_root" rev-parse --short HEAD)."
  echo "Push dev to github to publish the stable release (pre-push hook / push_dev), or run ./scripts/publish_stable_release.sh."
}

part="patch"
tag_only=0
no_tag=0

for arg in "$@"; do
  case "$arg" in
    patch | minor | major) part="$arg" ;;
    --tag-only) tag_only=1 ;;
    --no-tag) no_tag=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ "$tag_only" -eq 1 ]]; then
  create_rustdl_version_tag "$(cargo_version_from_file)"
  exit 0
fi

current="$(cargo_version_from_file)"
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

if [[ "$no_tag" -eq 0 ]]; then
  create_rustdl_version_tag "$new"
fi
