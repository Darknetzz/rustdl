#!/usr/bin/env bash
# Tag and publish a stable GitHub release for the Cargo.toml version at COMMIT.
#
# Usage:
#   ./scripts/publish_stable_release.sh
#   ./scripts/publish_stable_release.sh --dry-run
#   ./scripts/publish_stable_release.sh --skip-build
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

repo='Darknetzz/rustdl'
remote='github'
commit=''
dry_run=0
skip_build=0
allow_dirty=0
force=0

usage() {
  sed -n '2,8p' "$0" | sed 's/^# \?//'
  exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) dry_run=1 ;;
    --skip-build) skip_build=1 ;;
    --repo) repo="$2"; shift ;;
    --remote) remote="$2"; shift ;;
    --commit) commit="$2"; shift ;;
    --allow-dirty) allow_dirty=1 ;;
    --force) force=1 ;;
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
version="$(git show "${commit}:Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -1)"
tag="rustdl-v${version}"
title="rustdl ${version}"
notes_file="$repo_root/release-notes.md"

if [[ "$allow_dirty" -eq 0 ]] && [[ -n "$(git status --porcelain)" ]]; then
  echo 'Working tree is not clean. Commit/stash or pass --allow-dirty.' >&2
  exit 1
fi

echo 'Stable release'
echo "  Repo:    $repo"
echo "  Tag:     $tag"
echo "  Commit:  $short_commit ($commit)"
echo "  Version: $version"

if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  if [[ "$force" -eq 0 ]]; then
    echo
    echo "GitHub release $tag already exists; skipping stable publish."
    exit 0
  fi
  echo
  echo "Release $tag exists; --force will refresh assets and target."
fi

local_tag_sha="$(git rev-parse -q --verify "refs/tags/${tag}" 2>/dev/null || true)"
if [[ -n "$local_tag_sha" ]]; then
  if [[ "$local_tag_sha" != "$commit" ]]; then
    if [[ "$force" -eq 0 ]]; then
      echo "Tag $tag exists at $(git rev-parse --short "$local_tag_sha") but commit is $short_commit. Pass --force to move the tag." >&2
      exit 1
    fi
    if [[ "$dry_run" -eq 1 ]]; then
      echo "Would move tag $tag to $short_commit"
    else
      git tag -f -a "$tag" "$commit" -m "rustdl $version"
    fi
  fi
else
  if [[ "$dry_run" -eq 1 ]]; then
    echo "Would create tag $tag at $short_commit"
  else
    git tag -a "$tag" "$commit" -m "rustdl $version"
  fi
fi

if [[ "$skip_build" -eq 0 && "$dry_run" -eq 0 ]]; then
  echo
  echo 'Building release binary...'
  ./scripts/build_binary.sh
elif [[ "$skip_build" -eq 0 && "$dry_run" -eq 1 ]]; then
  echo
  echo 'Would run: ./scripts/build_binary.sh'
fi

./scripts/extract_release_notes.sh CHANGELOG.md "$tag" >"$notes_file"

if [[ "$dry_run" -eq 1 ]]; then
  echo
  echo "Dry run: would publish stable release $tag"
  echo "  Push tag: git push $remote $tag"
  echo "  Notes:    $notes_file"
  exit 0
fi

local_tag_sha="$(git rev-parse "refs/tags/${tag}")"
remote_tag="$(git ls-remote "$remote" "refs/tags/${tag}" | awk 'NR==1 {print $1}')"
if [[ -z "$remote_tag" ]]; then
  echo
  echo "Pushing tag $tag to $remote ..."
  git push "$remote" "$tag"
elif [[ "$remote_tag" != "$local_tag_sha" ]]; then
  if [[ "$force" -eq 0 ]]; then
    echo "Remote tag $tag ($remote_tag) differs from local ($local_tag_sha). Pass --force to update." >&2
    exit 1
  fi
  echo
  echo "Force-pushing tag $tag to $remote ..."
  git push --force "$remote" "$tag"
fi

if [[ -f target/release/rustdl ]]; then
  binary=target/release/rustdl
elif [[ -f target/release/rustdl.exe ]]; then
  binary=target/release/rustdl.exe
else
  echo 'Release binary not found. Run ./scripts/build_binary.sh first (or drop --skip-build).' >&2
  exit 1
fi

if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  echo
  echo "Updating existing release $tag ..."
  gh release edit "$tag" \
    --repo "$repo" \
    --title "$title" \
    --notes-file "$notes_file" \
    --target "$commit"
  gh release upload "$tag" "$binary" --repo "$repo" --clobber
else
  echo
  echo "Creating release $tag ..."
  gh release create "$tag" \
    --repo "$repo" \
    --title "$title" \
    --notes-file "$notes_file" \
    --target "$commit" \
    "$binary"
fi

echo
echo "Published stable release: https://github.com/${repo}/releases/tag/${tag}"
