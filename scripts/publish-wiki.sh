#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="$repo_root/docs/wiki"
wiki_url="${WIKI_REMOTE_URL:-https://github.com/vynxc/ratatui-3dmesh.wiki.git}"
work_dir="${WIKI_WORK_DIR:-$repo_root/.wiki}"

if [[ ! -d "$source_dir" ]]; then
  echo "missing wiki source directory: $source_dir" >&2
  exit 1
fi

if [[ ! -d "$work_dir/.git" ]]; then
  rm -rf "$work_dir"
  if ! git clone "$wiki_url" "$work_dir"; then
    cat >&2 <<EOF
failed to clone the GitHub wiki repository:
  $wiki_url

If this is the first wiki publish, open the repo wiki in GitHub, create any
Home page once, then rerun this script. GitHub creates <repo>.wiki.git only
after the wiki has an initial page.
EOF
    exit 1
  fi
fi

git -C "$work_dir" fetch origin
default_branch="$(git -C "$work_dir" symbolic-ref --quiet --short refs/remotes/origin/HEAD)"
default_branch="${default_branch#origin/}"
default_branch="${default_branch:-master}"

git -C "$work_dir" checkout "$default_branch"
git -C "$work_dir" pull --ff-only origin "$default_branch"

find "$work_dir" -mindepth 1 -maxdepth 1 \
  ! -name .git \
  ! -name .gitignore \
  -exec rm -rf {} +

cp "$source_dir"/*.md "$work_dir"/

if git -C "$work_dir" diff --quiet -- .; then
  echo "wiki is already up to date"
  exit 0
fi

git -C "$work_dir" add .
git -C "$work_dir" commit -m "Update wiki"
git -C "$work_dir" push origin "$default_branch"
