#!/usr/bin/env bash
# Extract GitHub release notes from CHANGELOG.md for a rustdl tag.
# Uses the ## [X.Y.Z] section matching the tag (date suffix optional).
# Does not use ## [Unreleased] — finalize CHANGELOG before publishing stable releases.
#
# Usage: ./scripts/extract_release_notes.sh [CHANGELOG.md] TAG
set -euo pipefail

changelog="${1:-CHANGELOG.md}"
tag="${2:?tag required (e.g. rustdl-v0.4.9)}"
include_heading="${EXTRACT_RELEASE_NOTES_INCLUDE_HEADING:-0}"

ver="${tag#rustdl-v}"
ver="${ver#v}"

if [[ ! -f "$changelog" ]]; then
  echo "CHANGELOG not found: $changelog" >&2
  exit 1
fi

extract_version_section() {
  awk -v ver="$ver" '
    BEGIN { in_section = 0; found = 0 }
    { sub(/\r$/, "") }
    $0 ~ "^## \\[" ver "\\]( - [0-9]{4}-[0-9]{2}-[0-9]{2})?$" {
      in_section = 1
      found = 1
      print
      next
    }
    in_section && /^## \[/ { exit }
    in_section { print }
    END { if (!found) exit 1 }
  ' "$changelog"
}

section_has_body() {
  local section="$1"
  local body
  body="$(printf '%s' "$section" | tail -n +2 | sed '/^[[:space:]]*$/d')"
  [[ -n "$body" ]]
}

format_release_body() {
  local section="$1"
  if [[ "$include_heading" == "1" ]]; then
    printf '%s\n' "$section"
    return
  fi
  printf '%s\n' "$section" | awk 'NR==1 && /^## \[/ { next } { print }' | sed '/^[[:space:]]*$/d' | sed -e '$a\'
}

notes=""
if ! notes="$(extract_version_section 2>/dev/null)"; then
  echo "No ## [$ver] section with release notes found in $changelog for tag $tag." >&2
  echo "Add a dated ## [$ver] - YYYY-MM-DD section (move bullets out of [Unreleased]) before publishing." >&2
  exit 1
fi

if ! section_has_body "$notes"; then
  echo "Changelog section ## [$ver] is empty in $changelog" >&2
  exit 1
fi

format_release_body "$notes"
