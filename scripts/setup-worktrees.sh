#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Initialize Git and commit the base project first." >&2
  exit 1
fi
if ! git rev-parse --verify HEAD >/dev/null 2>&1; then
  echo "Create an initial commit before adding worktrees." >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "Commit/stash existing changes before creating worktrees." >&2
  exit 1
fi
for lane in core deployment search-preview ui; do
  dest="${ROOT}-wt-${lane}"
  branch="agent/${lane}"
  if [[ -e "$dest" ]]; then
    echo "Already present: $dest"
    continue
  fi
  if git show-ref --verify --quiet "refs/heads/${branch}"; then
    git worktree add "$dest" "$branch"
  else
    git worktree add -b "$branch" "$dest" HEAD
  fi
  echo "Created: $dest [$branch]"
done
printf '\nWorktrees ready. Give each worker its matching task packet from docs/WORKSTREAMS.md.\n'
