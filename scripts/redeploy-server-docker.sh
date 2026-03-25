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

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

COMPOSE_FILE="${COMPOSE_FILE:-$REPO_ROOT/docker-compose.yml}"
SERVICE="${SERVICE:-copywraith-server}"
USE_SUDO="${USE_SUDO:-0}"
NO_CACHE="${NO_CACHE:-1}"
PULL="${PULL:-1}"
PORT="${PORT:-3742}"
HEALTH_URL="${HEALTH_URL:-http://127.0.0.1:${PORT}/api/health}"

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

echo "== Copywraith Docker redeploy =="
echo "compose file: $COMPOSE_FILE"
echo "service:      $SERVICE"
echo ""

echo "[1/4] Stopping old containers"
"${COMPOSE[@]}" down --remove-orphans

echo "[2/4] Building image"
BUILD=("${COMPOSE[@]}" build)
if [[ "$NO_CACHE" == "1" ]]; then
  BUILD+=(--no-cache)
fi
if [[ "$PULL" == "1" ]]; then
  BUILD+=(--pull)
fi
BUILD+=("$SERVICE")
"${BUILD[@]}"

echo "[3/4] Starting container with forced recreate"
"${COMPOSE[@]}" up -d --force-recreate "$SERVICE"

echo "[4/4] Verifying deployment"
"${COMPOSE[@]}" ps
"${DOCKER[@]}" ps --filter "publish=${PORT}" --format 'table {{.ID}}\t{{.Image}}\t{{.Names}}\t{{.Status}}\t{{.Ports}}'

if command -v curl >/dev/null 2>&1; then
  echo ""
  echo "Health: $HEALTH_URL"
  if curl -fsS "$HEALTH_URL"; then
    echo ""
  else
    echo "WARNING: Health check failed."
  fi
else
  echo "curl not found; skipping health check."
fi
