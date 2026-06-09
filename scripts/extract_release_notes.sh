#!/usr/bin/env bash
# Extract GitHub release notes from CHANGELOG.md for a rustdl tag.
# Prefers the dated ## [X.Y.Z] section matching the tag; falls back to ## [Unreleased].
#
# Usage: ./scripts/extract_release_notes.sh [CHANGELOG.md] TAG
set -euo pipefail

changelog="${1:-CHANGELOG.md}"
tag="${2:?tag required (e.g. rustdl-v0.4.9)}"

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

extract_unreleased_section() {
  awk '
    BEGIN { in_section = 0; found = 0 }
    { sub(/\r$/, "") }
    $0 == "## [Unreleased]" {
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

notes=""
if notes="$(extract_version_section 2>/dev/null)" && section_has_body "$notes"; then
  :
elif notes="$(extract_unreleased_section 2>/dev/null)" && section_has_body "$notes"; then
  :
else
  if [[ -z "${notes:-}" ]]; then
    echo "No release notes found for $tag in $changelog" >&2
  else
    echo "Changelog section for $tag is empty in $changelog" >&2
  fi
  exit 1
fi

printf '%s\n' "$notes"
