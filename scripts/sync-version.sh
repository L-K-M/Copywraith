#!/usr/bin/env bash
set -Eeuo pipefail

# Reads the server crate version from server/Cargo.toml and updates every file
# that contains a hardcoded copy of that version.  Run this after bumping the
# version in server/Cargo.toml.
#
# Usage:
#   ./scripts/sync-version.sh          # preview changes (dry-run)
#   ./scripts/sync-version.sh --write  # apply changes

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CARGO_TOML="$REPO_ROOT/server/Cargo.toml"

if [[ ! -f "$CARGO_TOML" ]]; then
  echo "ERROR: $CARGO_TOML not found."
  exit 1
fi

VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$CARGO_TOML")"
if [[ -z "$VERSION" ]]; then
  echo "ERROR: could not parse version from $CARGO_TOML"
  exit 1
fi

WRITE=0
if [[ "${1:-}" == "--write" ]]; then
  WRITE=1
fi

echo "Server crate version: $VERSION"
echo ""

# Portable in-place sed (macOS and GNU)
_sed_i() {
  if sed --version >/dev/null 2>&1; then
    sed -i "$@"
  else
    sed -i '' "$@"
  fi
}

changed=0

update_file() {
  local file="$1"
  local pattern="$2"
  local replacement="$3"
  local rel="${file#"$REPO_ROOT"/}"

  if [[ ! -f "$file" ]]; then
    return
  fi

  if grep -qE "$pattern" "$file"; then
    if [[ "$WRITE" == "1" ]]; then
      _sed_i "s|$pattern|$replacement|g" "$file"
      echo "  updated  $rel"
    else
      echo "  stale    $rel"
      grep -nE "$pattern" "$file" | head -3 | while IFS= read -r line; do
        echo "           $line"
      done
    fi
    changed=1
  else
    echo "  ok       $rel"
  fi
}

SEM="[0-9]+\.[0-9]+\.[0-9]+"

echo "Checking files..."

# docker-compose.yml (root)
update_file "$REPO_ROOT/docker-compose.yml" \
  "COPYWRAITH_SERVER_IMAGE_TAG:-${SEM}" \
  "COPYWRAITH_SERVER_IMAGE_TAG:-${VERSION}"

# server/docker-compose.yml
update_file "$REPO_ROOT/server/docker-compose.yml" \
  "COPYWRAITH_SERVER_IMAGE_TAG:-${SEM}" \
  "COPYWRAITH_SERVER_IMAGE_TAG:-${VERSION}"

# .env.example
update_file "$REPO_ROOT/.env.example" \
  "COPYWRAITH_SERVER_IMAGE_TAG=${SEM}" \
  "COPYWRAITH_SERVER_IMAGE_TAG=${VERSION}"

# README.md -- version in prose (e.g. "currently `0.1.4`")
update_file "$REPO_ROOT/README.md" \
  "currently \`${SEM}\`" \
  "currently \`${VERSION}\`"

# README.md -- example tag in troubleshooting (e.g. "copywraith-server:0.1.4")
update_file "$REPO_ROOT/README.md" \
  "copywraith-server:${SEM}" \
  "copywraith-server:${VERSION}"

# redeploy script comment
update_file "$REPO_ROOT/scripts/redeploy-server-docker.sh" \
  "COPYWRAITH_SERVER_IMAGE_TAG=${SEM}" \
  "COPYWRAITH_SERVER_IMAGE_TAG=${VERSION}"

echo ""
if [[ "$changed" == "0" ]]; then
  echo "All files are already at version $VERSION."
elif [[ "$WRITE" == "0" ]]; then
  echo "Run with --write to apply changes."
fi
