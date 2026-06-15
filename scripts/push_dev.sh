#!/usr/bin/env bash
# Push dev to GitHub, publish rolling dev release, then mirror to GitLab.
#
# Usage:
#   ./scripts/push_dev.sh
#   ./scripts/push_dev.sh --dry-run
#   ./scripts/push_dev.sh --skip-gitlab
#   ./scripts/push_dev.sh --skip-publish
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

dry_run=0
skip_publish=0
skip_gitlab=0
github_remote='github'
gitlab_remote='gitlab'

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) dry_run=1 ;;
    --skip-publish) skip_publish=1 ;;
    --skip-gitlab) skip_gitlab=1 ;;
    --github-remote) github_remote="$2"; shift ;;
    --gitlab-remote) gitlab_remote="$2"; shift ;;
    -h|--help)
      sed -n '2,8p' "$0" | sed 's/^# \?//'
      exit 0
      ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
  shift
done

branch="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$branch" != dev ]]; then
  echo "Warning: not on dev branch (on $branch)." >&2
fi

echo "--- $github_remote ---"
if [[ "$dry_run" -eq 1 ]]; then
  echo "Would run: git push $github_remote dev"
else
  git push "$github_remote" dev
fi

if [[ "$skip_publish" -eq 0 ]]; then
  echo
  echo '--- rolling dev release ---'
  publish_args=()
  [[ "$dry_run" -eq 1 ]] && publish_args+=(--dry-run)
  ./scripts/publish_dev_release.sh "${publish_args[@]}"

  echo
  echo '--- stable release (if new version) ---'
  stable_args=(--skip-build)
  [[ "$dry_run" -eq 1 ]] && stable_args+=(--dry-run)
  ./scripts/publish_stable_release.sh "${stable_args[@]}"
fi

if [[ "$skip_gitlab" -eq 0 ]]; then
  echo
  echo "--- $gitlab_remote ---"
  if [[ "$dry_run" -eq 1 ]]; then
    echo "Would run: git push $gitlab_remote dev"
  else
    if ! git push "$gitlab_remote" dev; then
      echo "Warning: GitLab push failed (GitHub and dev release may already be updated)." >&2
      exit 1
    fi
  fi
fi

if [[ "$dry_run" -eq 1 ]]; then
  echo
  echo 'Dry run complete.'
fi
