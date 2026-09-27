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

# A fast-forward pull (and a branch switch) refuses to overwrite locally edited
# tracked files that the incoming commits also change. When git refuses and
# such edits exist, point at the fix instead of leaving only git's bare error.
explain_local_changes() {
    git diff --quiet HEAD -- && return
    echo >&2
    echo "These tracked files have local changes:" >&2
    git diff --name-only HEAD -- | sed 's/^/    /' >&2
    cat >&2 <<'EOF'

If git refused above because of them: deployment tweaks to docker-compose.yml
(ports, volumes, environment, …) belong in docker-compose.override.yml, which is
git-ignored and merged automatically by `docker compose`. Move your edits there,
then discard them from the tracked file:

    git diff docker-compose.yml          # review what you changed
    git checkout -- docker-compose.yml   # discard it once it's in the override
    ./update.sh

Or keep the edits aside with `git stash` and re-apply them with `git stash pop`.
EOF
}

# The rebuild uses the checked-out tree, so an explicitly named branch must
# actually be checked out — syncing it alone would redeploy the old branch.
if [[ "$branch" != "$(git rev-parse --abbrev-ref HEAD)" ]]; then
    echo "==> Switching to '$branch'…"
    git checkout "$branch" || { explain_local_changes; exit 1; }
fi

echo "==> Syncing '$branch' from the remote…"
if command -v gh >/dev/null 2>&1; then
    if ! gh repo sync --branch "$branch"; then
        echo "    gh repo sync failed; falling back to: git pull --ff-only" >&2
        git pull --ff-only origin "$branch" || { explain_local_changes; exit 1; }
    fi
else
    git pull --ff-only origin "$branch" || { explain_local_changes; exit 1; }
fi

echo "==> Rebuilding the image and recreating the container…"
docker compose up -d --build --remove-orphans

echo "==> Pruning dangling images…"
docker image prune -f >/dev/null || true

echo "==> Stack status:"
docker compose ps
