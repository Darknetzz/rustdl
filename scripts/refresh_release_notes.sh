#!/usr/bin/env bash
# Refresh GitHub stable release descriptions from CHANGELOG.md version sections.
#
# Usage: ./scripts/refresh_release_notes.sh [--dry-run] [TAG]
set -euo pipefail

repo='Darknetzz/rustdl'
dry_run=0
tag=''

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) dry_run=1; shift ;;
    -*) echo "Unknown option: $1" >&2; exit 2 ;;
    *)
      tag="$1"
      shift
      ;;
  esac
done

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
notes_file="$repo_root/release-notes.md"
cd "$repo_root"

if ! command -v gh >/dev/null 2>&1; then
  echo 'gh CLI not found. Install GitHub CLI and run: gh auth login' >&2
  exit 1
fi

mapfile -t tags < <(
  if [[ -n "$tag" ]]; then
    printf '%s\n' "$tag"
  else
    gh release list --repo "$repo" --limit 100 --json tagName -q '.[].tagName' |
      grep -E '^rustdl-v[0-9]+\.[0-9]+\.[0-9]+$' || true
  fi
)

if ((${#tags[@]} == 0)); then
  echo 'No rustdl-v* releases found.'
  exit 0
fi

updated=0
skipped=0

for release_tag in "${tags[@]}"; do
  if ! ./scripts/extract_release_notes.sh CHANGELOG.md "$release_tag" >"$notes_file"; then
    echo "Skipped $release_tag (no matching CHANGELOG section)" >&2
    ((skipped += 1)) || true
    continue
  fi

  if ((dry_run)); then
    echo "Would update $release_tag"
    ((updated += 1)) || true
    continue
  fi

  gh release edit "$release_tag" --repo "$repo" --notes-file "$notes_file"
  echo "Updated $release_tag"
  ((updated += 1)) || true
done

echo ""
echo "Done. Updated: $updated  Skipped: $skipped"
