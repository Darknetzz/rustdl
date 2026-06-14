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
echo 'After every successful:  git push github dev'
echo '  → builds and refreshes https://github.com/Darknetzz/rustdl/releases/tag/rustdl-dev'
echo
echo "Log: \${TMPDIR:-/tmp}/rustdl-dev-release.log"
echo 'Disable: ./scripts/install_dev_release_hook.sh --uninstall'
