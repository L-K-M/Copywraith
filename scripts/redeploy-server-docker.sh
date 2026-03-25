#!/usr/bin/env bash
set -Eeuo pipefail

# Rebuild and redeploy copywraith-server, then print /api/health.
#
# Usage:
#   USE_SUDO=1 COMPOSE_FILE=/mnt/Main/Applications/copywraith/docker-compose.yml ./scripts/redeploy-server-docker.sh
#
# Optional environment variables:
#   COMPOSE_FILE=/path/to/docker-compose.yml
#   SERVICE=copywraith-server
#   USE_SUDO=1
#   NO_CACHE=1
#   PULL=1
#   PORT=3742
#   HEALTH_URL=http://127.0.0.1:3742/api/health
#   HEALTH_RETRIES=20
#   HEALTH_DELAY_SECS=1
#   COPYWRAITH_SERVER_IMAGE_REPO=copywraith-server
#   COPYWRAITH_SERVER_IMAGE_TAG=0.1.3
#
# Notes:
# - If COPYWRAITH_SERVER_IMAGE_TAG is not set, this script uses
#   server/Cargo.toml package version as the image tag.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SERVER_CARGO_TOML="$REPO_ROOT/server/Cargo.toml"

COMPOSE_FILE="${COMPOSE_FILE:-$REPO_ROOT/docker-compose.yml}"
SERVICE="${SERVICE:-copywraith-server}"
USE_SUDO="${USE_SUDO:-0}"
NO_CACHE="${NO_CACHE:-1}"
PULL="${PULL:-1}"
PORT="${PORT:-3742}"
HEALTH_URL="${HEALTH_URL:-http://127.0.0.1:${PORT}/api/health}"
HEALTH_RETRIES="${HEALTH_RETRIES:-20}"
HEALTH_DELAY_SECS="${HEALTH_DELAY_SECS:-1}"

DEFAULT_IMAGE_TAG=""
if [[ -f "$SERVER_CARGO_TOML" ]]; then
  DEFAULT_IMAGE_TAG="$(awk -F '"' '/^version = / { print $2; exit }' "$SERVER_CARGO_TOML" || true)"
fi

IMAGE_REPO="${COPYWRAITH_SERVER_IMAGE_REPO:-copywraith-server}"
IMAGE_TAG="${COPYWRAITH_SERVER_IMAGE_TAG:-${DEFAULT_IMAGE_TAG:-dev}}"
IMAGE_REF="${IMAGE_REPO}:${IMAGE_TAG}"

if ! command -v docker >/dev/null 2>&1; then
  echo "ERROR: docker is not installed or not in PATH."
  exit 1
fi

if [[ ! -f "$COMPOSE_FILE" ]]; then
  echo "ERROR: Compose file not found: $COMPOSE_FILE"
  exit 1
fi

if [[ "$USE_SUDO" == "1" ]]; then
  DOCKER=(sudo docker)
else
  DOCKER=(docker)
fi

COMPOSE=("${DOCKER[@]}" compose -f "$COMPOSE_FILE")

compose() {
  COPYWRAITH_SERVER_IMAGE_REPO="$IMAGE_REPO" COPYWRAITH_SERVER_IMAGE_TAG="$IMAGE_TAG" "${COMPOSE[@]}" "$@"
}

echo "== Copywraith Docker redeploy =="
echo "compose file: $COMPOSE_FILE"
echo "service:      $SERVICE"
echo "image:        $IMAGE_REF"
echo ""

echo "[1/4] Stopping old containers"
compose down --remove-orphans

echo "[2/4] Building image"
if [[ "$NO_CACHE" == "1" && "$PULL" == "1" ]]; then
  compose build --no-cache --pull "$SERVICE"
elif [[ "$NO_CACHE" == "1" ]]; then
  compose build --no-cache "$SERVICE"
elif [[ "$PULL" == "1" ]]; then
  compose build --pull "$SERVICE"
else
  compose build "$SERVICE"
fi

echo "[3/4] Starting container with forced recreate"
compose up -d --force-recreate "$SERVICE"

echo "[4/4] Verifying deployment"
compose ps
"${DOCKER[@]}" ps --filter "publish=${PORT}" --format 'table {{.ID}}\t{{.Image}}\t{{.Names}}\t{{.Status}}\t{{.Ports}}'

container_id=""
while IFS= read -r line; do
  if [[ -n "$line" ]]; then
    container_id="$line"
    break
  fi
done < <(compose ps -q "$SERVICE")

if [[ -n "$container_id" ]]; then
  running_image="$("${DOCKER[@]}" inspect --format '{{.Config.Image}}' "$container_id" 2>/dev/null || true)"
  if [[ -n "$running_image" ]]; then
    echo "running image: $running_image"
  fi
fi

if command -v curl >/dev/null 2>&1; then
  echo ""
  echo "Health: $HEALTH_URL"
  healthy=0
  for ((i = 1; i <= HEALTH_RETRIES; i++)); do
    if curl -fsS "$HEALTH_URL"; then
      echo ""
      healthy=1
      break
    fi
    sleep "$HEALTH_DELAY_SECS"
  done

  if [[ "$healthy" != "1" ]]; then
    echo "WARNING: Health check failed."
    echo "Last container logs:"
    compose logs --tail=120 "$SERVICE" || true
  fi
else
  echo "curl not found; skipping health check."
fi
