#!/usr/bin/env bash
# Enable automatic rustdl-dev publish after git push github dev.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

uninstall=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --uninstall) uninstall=1 ;;
    -h|--help)
      sed -n '2,5p' "$0" | sed 's/^# \?//'
      exit 0
      ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
  shift
done

if [[ "$uninstall" -eq 1 ]]; then
  current="$(git config --local --get core.hooksPath 2>/dev/null || true)"
  if [[ "$current" == .githooks ]]; then
    git config --local --unset core.hooksPath
    echo 'Removed core.hooksPath (.githooks). Automatic dev release publish is disabled.'
  else
    echo "core.hooksPath is '${current:-<unset>}' (not .githooks); left unchanged."
  fi
  exit 0
fi

if [[ ! -f .githooks/pre-push ]]; then
  echo 'Missing .githooks/pre-push' >&2
  exit 1
fi

chmod +x .githooks/pre-push scripts/wait_and_publish_dev.sh 2>/dev/null || true
git config --local core.hooksPath .githooks

echo 'Installed .githooks/pre-push (core.hooksPath = .githooks).'
echo
echo 'After git push github dev, GitLab CI builds and publishes when you also'
echo '  git push gitlab dev'
echo '  (or use ./scripts/push_dev.sh which pushes both remotes).'
echo
echo 'Local compile+gh publish is off by default. Emergency: RUSTDL_LOCAL_PUBLISH=1'
echo 'Disable hook: ./scripts/install_dev_release_hook.sh --uninstall'
