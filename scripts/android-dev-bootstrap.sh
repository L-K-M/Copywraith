#!/usr/bin/env bash
set -Eeuo pipefail

# Usage:
#   ./scripts/android-dev-bootstrap.sh
# Optional:
#   TARGET=aarch64-linux-android RUN_TAURI=0 ./scripts/android-dev-bootstrap.sh

TARGET="${TARGET:-aarch64-linux-android}"
RUN_TAURI="${RUN_TAURI:-1}"
REPO_ROOT="${REPO_ROOT:-$(pwd)}"

cd "$REPO_ROOT"

if [[ ! -f "src-tauri/Cargo.toml" || ! -f "package.json" ]]; then
  echo "ERROR: Run this from the Copywraith repo root."
  exit 1
fi

# Ensure rustup-managed binaries are first
export PATH="$HOME/.cargo/bin:$PATH"
hash -r

# Required tools
for cmd in rustup cargo rustc npx; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: Missing required command: $cmd"
    exit 1
  fi
done

CARGO_BIN="$(command -v cargo)"
RUSTC_BIN="$(command -v rustc)"
ACTIVE_TOOLCHAIN="$(rustup show active-toolchain | awk '{print $1}')"

if [[ -z "$ACTIVE_TOOLCHAIN" ]]; then
  echo "ERROR: Could not detect active rustup toolchain."
  exit 1
fi

echo "== Diagnostics =="
echo "cargo:     $CARGO_BIN"
echo "rustc:     $RUSTC_BIN"
echo "toolchain: $ACTIVE_TOOLCHAIN"
echo "target:    $TARGET"
echo

echo "== Ensuring Android Rust target is installed =="
rustup target add --toolchain "$ACTIVE_TOOLCHAIN" "$TARGET"
rustup component add --toolchain "$ACTIVE_TOOLCHAIN" rust-std --target "$TARGET"

TARGET_LIBDIR="$(rustc --print target-libdir --target "$TARGET" 2>/dev/null || true)"
if [[ -z "$TARGET_LIBDIR" || ! -d "$TARGET_LIBDIR" ]]; then
  echo "Target stdlib directory missing; reinstalling target..."
  rustup target remove --toolchain "$ACTIVE_TOOLCHAIN" "$TARGET" || true
  rustup target add --toolchain "$ACTIVE_TOOLCHAIN" "$TARGET"
  TARGET_LIBDIR="$(rustc --print target-libdir --target "$TARGET")"
fi

if [[ ! -d "$TARGET_LIBDIR" ]]; then
  echo "ERROR: target libdir still missing for $TARGET"
  exit 1
fi

# Basic verification that libdir is readable/non-empty
if ! ls "$TARGET_LIBDIR" >/dev/null 2>&1; then
  echo "ERROR: target libdir exists but is not readable: $TARGET_LIBDIR"
  exit 1
fi

echo "Target libdir OK: $TARGET_LIBDIR"
echo

# Android SDK defaults (macOS)
if [[ -z "${ANDROID_HOME:-}" && -d "$HOME/Library/Android/sdk" ]]; then
  export ANDROID_HOME="$HOME/Library/Android/sdk"
fi

# Clear stale NDK_HOME
if [[ -n "${NDK_HOME:-}" && ! -d "${NDK_HOME}" ]]; then
  echo "Stale NDK_HOME detected: $NDK_HOME"
  echo "Unsetting stale NDK_HOME..."
  unset NDK_HOME
fi

# Auto-pick an installed NDK if NDK_HOME is unset
SDK_NDK_DIR="${ANDROID_HOME:-$HOME/Library/Android/sdk}/ndk"
if [[ -z "${NDK_HOME:-}" && -d "$SDK_NDK_DIR" ]]; then
  LATEST_NDK="$(ls -1 "$SDK_NDK_DIR" 2>/dev/null | sort | tail -n 1 || true)"
  if [[ -n "$LATEST_NDK" && -d "$SDK_NDK_DIR/$LATEST_NDK" ]]; then
    export NDK_HOME="$SDK_NDK_DIR/$LATEST_NDK"
  fi
fi

echo "ANDROID_HOME=${ANDROID_HOME:-<unset>}"
echo "NDK_HOME=${NDK_HOME:-<unset>}"
echo

if [[ "$RUN_TAURI" == "1" ]]; then
  echo "== Starting Android dev build =="
  RUSTUP_TOOLCHAIN="$ACTIVE_TOOLCHAIN" npx tauri android dev
else
  echo "RUN_TAURI=$RUN_TAURI, skipping 'npx tauri android dev'."
fi