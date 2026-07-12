#!/usr/bin/env bash
#
# Update a Copywraith server deployment: pull the latest code and rebuild +
# recreate the Docker stack.
#
#   ./update.sh [branch]
#
# Syncs the current branch by default (pass a branch name to override). Requires:
#   - git with pull access to this repo (the GitHub CLI `gh` is used when
#     present, with a plain `git pull --ff-only` fallback)
#   - Docker with the Compose plugin
#
# For a heavier redeploy with version-tagged images and a health check, see
# scripts/redeploy-server-docker.sh.
set -euo pipefail

cd "$(dirname "$0")"

# The root compose file builds the server image (context is this directory,
# dockerfile server/Dockerfile) and keeps its default image tag aligned with the
# server crate version via scripts/sync-version.sh, so a plain rebuild picks up
# the new version after the sync below.
branch="${1:-$(git rev-parse --abbrev-ref HEAD)}"

# The rebuild uses the checked-out tree, so an explicitly named branch must
# actually be checked out — syncing it alone would redeploy the old branch.
if [[ "$branch" != "$(git rev-parse --abbrev-ref HEAD)" ]]; then
    echo "==> Switching to '$branch'…"
    git checkout "$branch"
fi

echo "==> Syncing '$branch' from the remote…"
if command -v gh >/dev/null 2>&1; then
    if ! gh repo sync --branch "$branch"; then
        echo "    gh repo sync failed; falling back to: git pull --ff-only" >&2
        git pull --ff-only origin "$branch"
    fi
else
    git pull --ff-only origin "$branch"
fi

echo "==> Rebuilding the image and recreating the container…"
docker compose up -d --build --remove-orphans

echo "==> Pruning dangling images…"
docker image prune -f >/dev/null || true

echo "==> Stack status:"
docker compose ps
