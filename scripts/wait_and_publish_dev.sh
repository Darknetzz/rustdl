#!/usr/bin/env bash
# Wait until github/dev matches COMMIT, then publish rustdl-dev (background from pre-push).
set -euo pipefail

remote="${1:-github}"
commit="${2:?usage: wait_and_publish_dev.sh [remote] commit}"
timeout_sec="${RUSTDL_DEV_RELEASE_TIMEOUT:-180}"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
log_file="${TMPDIR:-/tmp}/rustdl-dev-release.log"

log() {
  printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*" >>"$log_file"
}

cd "$repo_root"

log "Waiting for ${remote} dev to reach ${commit} ..."
deadline=$((SECONDS + timeout_sec))
matched=0

while [ "$SECONDS" -lt "$deadline" ]; do
  remote_sha="$(git ls-remote "$remote" refs/heads/dev | awk 'NR==1 {print $1}')"
  if [ "$remote_sha" = "$commit" ]; then
    matched=1
    break
  fi
  sleep 2
done

if [ "$matched" -eq 0 ]; then
  log "Timed out after ${timeout_sec}s (remote dev never reached ${commit})."
  exit 1
fi

log "Remote matched; publishing rustdl-dev ..."
if ! ./scripts/publish_dev_release.sh --commit "$commit"; then
  log "publish_dev_release.sh failed."
  exit 1
fi

log "Published rustdl-dev for ${commit}"

log "Publishing stable release (if Cargo version is new) ..."
if ! ./scripts/publish_stable_release.sh --commit "$commit" --skip-build; then
  log "publish_stable_release.sh failed."
  exit 1
fi

log "Stable release step finished for ${commit}"
