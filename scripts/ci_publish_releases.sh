#!/usr/bin/env bash
# Publish GitLab CI artifacts to GitHub Releases and GitLab Releases.
# Used by .gitlab-ci.yml (not GitHub Actions).
#
# Env:
#   GH_TOKEN              GitHub PAT with repo scope (optional; GitHub upload skipped if unset)
#   CI_JOB_TOKEN          GitLab job token (set by GitLab)
#   GITLAB_TOKEN          Optional GitLab PAT if job token cannot manage releases
#   CI_API_V4_URL, CI_PROJECT_ID, CI_COMMIT_SHA, CI_COMMIT_SHORT_SHA,
#   CI_COMMIT_BRANCH, CI_COMMIT_TAG, CI_PROJECT_URL
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

github_repo="${GITHUB_REPO:-Darknetzz/rustdl}"
dist_dir="${DIST_DIR:-dist}"
notes_file="$repo_root/ci-release-notes.md"
commit="${CI_COMMIT_SHA:-$(git rev-parse HEAD)}"
short_commit="${CI_COMMIT_SHORT_SHA:-$(git rev-parse --short HEAD)}"
branch="${CI_COMMIT_BRANCH:-}"
ci_tag="${CI_COMMIT_TAG:-}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

if [[ -z "$version" ]]; then
  echo 'Could not read version from Cargo.toml' >&2
  exit 1
fi

shopt -s nullglob
assets=("$dist_dir"/*)
if [[ ${#assets[@]} -eq 0 ]]; then
  echo "No files in $dist_dir" >&2
  exit 1
fi

gitlab_auth_args() {
  if [[ -n "${GITLAB_TOKEN:-}" ]]; then
    echo --header "PRIVATE-TOKEN: ${GITLAB_TOKEN}"
  else
    echo --header "JOB-TOKEN: ${CI_JOB_TOKEN:?CI_JOB_TOKEN or GITLAB_TOKEN required}"
  fi
}

unreleased_body() {
  awk '
    /^## \[Unreleased\]/ { in_u=1; next }
    in_u && /^## \[/ { exit }
    in_u { print }
  ' CHANGELOG.md | sed '/./,$!d'
}

write_dev_notes() {
  local unreleased compare
  unreleased="$(unreleased_body || true)"
  compare="https://github.com/${github_repo}/compare/rustdl-dev...${short_commit}"
  {
    echo '# rustdl dev (rolling)'
    echo
    echo "Pre-release build from GitLab CI on \`${branch:-dev}\`."
    echo
    echo '| | |'
    echo '| --- | --- |'
    echo "| **Commit** | [${short_commit}](https://github.com/${github_repo}/commit/${commit}) |"
    echo "| **Cargo version** | ${version} |"
    echo "| **Pipeline** | ${CI_PIPELINE_URL:-n/a} |"
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

write_stable_notes() {
  local tag="$1"
  if ! ./scripts/extract_release_notes.sh CHANGELOG.md "$tag" >"$notes_file"; then
    echo "No changelog section for $tag; skipping stable publish." >&2
    return 1
  fi
  return 0
}

github_upload() {
  local tag="$1"
  local title="$2"
  local prerelease="$3"
  if [[ -z "${GH_TOKEN:-}" ]]; then
    echo "GH_TOKEN unset; skipping GitHub release $tag"
    return 0
  fi
  export GH_TOKEN
  if gh release view "$tag" --repo "$github_repo" >/dev/null 2>&1; then
    echo "Updating GitHub release $tag ..."
    local extra=()
    if [[ "$prerelease" == "1" ]]; then
      extra+=(--prerelease)
    fi
    gh release edit "$tag" \
      --repo "$github_repo" \
      --title "$title" \
      --notes-file "$notes_file" \
      --target "$commit" \
      "${extra[@]}"
    gh release upload "$tag" "${assets[@]}" --repo "$github_repo" --clobber
  else
    echo "Creating GitHub release $tag ..."
    local extra=()
    if [[ "$prerelease" == "1" ]]; then
      extra+=(--prerelease)
    fi
    gh release create "$tag" \
      --repo "$github_repo" \
      --title "$title" \
      --notes-file "$notes_file" \
      --target "$commit" \
      "${extra[@]}" \
      "${assets[@]}"
  fi
  echo "GitHub: https://github.com/${github_repo}/releases/tag/${tag}"
}

gitlab_pkg_ver() {
  local tag="$1"
  if [[ "$tag" == rustdl-dev ]]; then
    echo dev
  else
    echo "${tag#rustdl-v}"
  fi
}

gitlab_upload_packages() {
  local pkg_ver="$1"
  local file name url
  for file in "${assets[@]}"; do
    name="$(basename "$file")"
    url="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/packages/generic/rustdl/${pkg_ver}/${name}"
    echo "Uploading GitLab package ${pkg_ver}/${name} ..."
    curl -fsS --request PUT \
      $(gitlab_auth_args) \
      --upload-file "$file" \
      "$url" >/dev/null
  done
}

gitlab_upsert_release() {
  local tag="$1"
  local title="$2"
  local pkg_ver
  pkg_ver="$(gitlab_pkg_ver "$tag")"
  gitlab_upload_packages "$pkg_ver"

  local links_json
  links_json="$(
    python3 - "$pkg_ver" "${assets[@]}" <<'PY'
import json, os, sys
pkg_ver = sys.argv[1]
api = os.environ["CI_API_V4_URL"]
pid = os.environ["CI_PROJECT_ID"]
links = []
for path in sys.argv[2:]:
    name = os.path.basename(path)
    links.append({
        "name": name,
        "url": f"{api}/projects/{pid}/packages/generic/rustdl/{pkg_ver}/{name}",
        "link_type": "package",
    })
print(json.dumps(links))
PY
  )"

  local payload
  payload="$(
    python3 - "$tag" "$title" "$commit" "$notes_file" "$links_json" <<'PY'
import json, sys
tag, title, commit, notes_path, links_raw = sys.argv[1:]
with open(notes_path, encoding="utf-8") as f:
    description = f.read()
print(json.dumps({
    "name": title,
    "tag_name": tag,
    "ref": commit,
    "description": description,
    "assets": {"links": json.loads(links_raw)},
}))
PY
  )"

  local api="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/releases"
  local code
  code="$(curl -sS -o /tmp/gl-release-body.json -w '%{http_code}' --request POST \
    $(gitlab_auth_args) \
    --header 'Content-Type: application/json' \
    --data "$payload" \
    "$api")"
  if [[ "$code" == "201" || "$code" == "200" ]]; then
    echo "Created GitLab release $tag"
  elif [[ "$code" == "409" ]]; then
    echo "Updating GitLab release $tag ..."
    local update
    update="$(
      python3 - "$title" "$notes_file" "$links_json" <<'PY'
import json, sys
title, notes_path, links_raw = sys.argv[1:]
with open(notes_path, encoding="utf-8") as f:
    description = f.read()
print(json.dumps({
    "name": title,
    "description": description,
    "assets": {"links": json.loads(links_raw)},
}))
PY
    )"
    curl -fsS --request PUT \
      $(gitlab_auth_args) \
      --header 'Content-Type: application/json' \
      --data "$update" \
      "${api}/${tag}" >/dev/null
    echo "Updated GitLab release $tag"
  else
    echo "GitLab release $tag failed (HTTP $code):" >&2
    cat /tmp/gl-release-body.json >&2 || true
    echo >&2
    return 1
  fi
  echo "GitLab: ${CI_PROJECT_URL}/-/releases/${tag}"
}

publish_dev() {
  write_dev_notes
  github_upload rustdl-dev 'rustdl dev (rolling)' 1
  gitlab_upsert_release rustdl-dev 'rustdl dev (rolling)'
}

publish_stable() {
  local tag="$1"
  local from_tag_pipeline="${2:-0}"
  local ver="${tag#rustdl-v}"
  if ! write_stable_notes "$tag"; then
    return 0
  fi
  if [[ "$from_tag_pipeline" != "1" ]]; then
    if [[ -z "${GH_TOKEN:-}" ]]; then
      echo "Skipping stable publish on branch pipeline (GH_TOKEN unset)."
      return 0
    fi
    if gh release view "$tag" --repo "$github_repo" >/dev/null 2>&1; then
      echo "GitHub release $tag already exists; skipping stable publish on branch pipeline."
      return 0
    fi
  fi
  github_upload "$tag" "rustdl ${ver}" 0
  gitlab_upsert_release "$tag" "rustdl ${ver}"
}

echo 'CI publish'
echo "  GitHub:  $github_repo"
echo "  Version: $version"
echo "  Commit:  $short_commit"
echo "  Assets:  ${assets[*]}"

if [[ -n "$ci_tag" ]]; then
  publish_stable "$ci_tag" 1
elif [[ "$branch" == "dev" ]]; then
  publish_dev
  publish_stable "rustdl-v${version}" 0
else
  echo "Nothing to publish for branch='$branch' tag='$ci_tag'"
fi
