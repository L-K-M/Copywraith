#!/usr/bin/env bash
set -Eeuo pipefail

# Usage:
#   ./scripts/android-release-bootstrap.sh
# Optional:
#   TAURI_TARGETS="aarch64 armv7" BUILD_APK=1 BUILD_AAB=1 ./scripts/android-release-bootstrap.sh
#   DEBUG=0 INSTALL=0 ./scripts/android-release-bootstrap.sh
#   INSTALL_APK_PATH="src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk" ./scripts/android-release-bootstrap.sh
#   RUN_BUILD=0 ./scripts/android-release-bootstrap.sh

TAURI_TARGETS="${TAURI_TARGETS:-aarch64}"
BUILD_APK="${BUILD_APK:-1}"
BUILD_AAB="${BUILD_AAB:-0}"
INSTALL="${INSTALL:-1}"
if [[ -z "${DEBUG+x}" ]]; then
  if [[ "$INSTALL" == "1" ]]; then
    DEBUG="1"
  else
    DEBUG="0"
  fi
fi
INSTALL_APK_PATH="${INSTALL_APK_PATH:-}"
ADB_INSTALL_ARGS="${ADB_INSTALL_ARGS:--r}"
RUN_BUILD="${RUN_BUILD:-1}"
REPO_ROOT="${REPO_ROOT:-$(pwd)}"

cd "$REPO_ROOT"

if [[ ! -f "src-tauri/Cargo.toml" || ! -f "package.json" ]]; then
  echo "ERROR: Run this from the Copywraith repo root."
  exit 1
fi

if [[ "$BUILD_APK" != "1" && "$BUILD_AAB" != "1" ]]; then
  echo "ERROR: Set BUILD_APK=1, BUILD_AAB=1, or both."
  exit 1
fi

if [[ "$INSTALL" == "1" && "$BUILD_APK" != "1" ]]; then
  echo "ERROR: INSTALL=1 requires BUILD_APK=1."
  exit 1
fi

# Ensure rustup-managed binaries are first.
export PATH="$HOME/.cargo/bin:$PATH"
hash -r

# Required tools.
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

if [[ "$TAURI_TARGETS" == "all" ]]; then
  TARGET_VALUES=(aarch64 armv7 i686 x86_64)
else
  read -r -a TARGET_VALUES <<< "$TAURI_TARGETS"
fi

if [[ ${#TARGET_VALUES[@]} -eq 0 ]]; then
  echo "ERROR: TAURI_TARGETS is empty."
  exit 1
fi

TAURI_TARGET_ARGS=""
RUST_TARGET_ARGS=""

list_contains() {
  local needle="$1"
  local list="$2"
  [[ " $list " == *" $needle "* ]]
}

for target in "${TARGET_VALUES[@]}"; do
  case "$target" in
    aarch64|aarch64-linux-android)
      tauri_target="aarch64"
      rust_target="aarch64-linux-android"
      ;;
    armv7|armv7-linux-androideabi)
      tauri_target="armv7"
      rust_target="armv7-linux-androideabi"
      ;;
    i686|i686-linux-android)
      tauri_target="i686"
      rust_target="i686-linux-android"
      ;;
    x86_64|x86_64-linux-android)
      tauri_target="x86_64"
      rust_target="x86_64-linux-android"
      ;;
    *)
      echo "ERROR: Unknown Android target: $target"
      echo "Use one of: aarch64, armv7, i686, x86_64, all"
      exit 1
      ;;
  esac

  if ! list_contains "$tauri_target" "$TAURI_TARGET_ARGS"; then
    TAURI_TARGET_ARGS="${TAURI_TARGET_ARGS:+$TAURI_TARGET_ARGS }$tauri_target"
  fi
  if ! list_contains "$rust_target" "$RUST_TARGET_ARGS"; then
    RUST_TARGET_ARGS="${RUST_TARGET_ARGS:+$RUST_TARGET_ARGS }$rust_target"
  fi
done

echo "== Diagnostics =="
echo "cargo:        $CARGO_BIN"
echo "rustc:        $RUSTC_BIN"
echo "toolchain:    $ACTIVE_TOOLCHAIN"
echo "tauri target: $TAURI_TARGET_ARGS"
echo "rust target:  $RUST_TARGET_ARGS"
echo "mode:         $([[ "$DEBUG" == "1" ]] && echo debug || echo release)"
echo "apk:          $BUILD_APK"
echo "aab:          $BUILD_AAB"
echo "install:      $INSTALL"
echo

echo "== Ensuring Android Rust targets are installed =="
for rust_target in $RUST_TARGET_ARGS; do
  rustup target add --toolchain "$ACTIVE_TOOLCHAIN" "$rust_target"
  rustup component add --toolchain "$ACTIVE_TOOLCHAIN" rust-std --target "$rust_target"

  target_libdir="$(rustc --print target-libdir --target "$rust_target" 2>/dev/null || true)"
  if [[ -z "$target_libdir" || ! -d "$target_libdir" ]]; then
    echo "Target stdlib directory missing for $rust_target; reinstalling target..."
    rustup target remove --toolchain "$ACTIVE_TOOLCHAIN" "$rust_target" || true
    rustup target add --toolchain "$ACTIVE_TOOLCHAIN" "$rust_target"
    target_libdir="$(rustc --print target-libdir --target "$rust_target")"
  fi

  if [[ ! -d "$target_libdir" ]]; then
    echo "ERROR: target libdir still missing for $rust_target"
    exit 1
  fi

  if ! ls "$target_libdir" >/dev/null 2>&1; then
    echo "ERROR: target libdir exists but is not readable: $target_libdir"
    exit 1
  fi

  echo "Target libdir OK for $rust_target: $target_libdir"
done
echo

# Android SDK defaults (macOS).
if [[ -z "${ANDROID_HOME:-}" && -d "$HOME/Library/Android/sdk" ]]; then
  export ANDROID_HOME="$HOME/Library/Android/sdk"
fi

# Clear stale NDK_HOME.
if [[ -n "${NDK_HOME:-}" && ! -d "${NDK_HOME}" ]]; then
  echo "Stale NDK_HOME detected: $NDK_HOME"
  echo "Unsetting stale NDK_HOME..."
  unset NDK_HOME
fi

# Auto-pick an installed NDK if NDK_HOME is unset.
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

if [[ -n "${ANDROID_HOME:-}" ]]; then
  export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"
  hash -r
fi

select_install_apk() {
  local mode="$1"
  local apk_root="src-tauri/gen/android/app/build/outputs/apk"
  local apk

  if [[ -n "$INSTALL_APK_PATH" ]]; then
    printf '%s\n' "$INSTALL_APK_PATH"
    return 0
  fi

  for apk in \
    "$apk_root"/universal/"$mode"/*.apk \
    "$apk_root"/arm64-v8a/"$mode"/*.apk \
    "$apk_root"/aarch64/"$mode"/*.apk \
    "$apk_root"/*/"$mode"/*.apk \
    "$apk_root"/"$mode"/*.apk; do
    if [[ -f "$apk" ]]; then
      printf '%s\n' "$apk"
      return 0
    fi
  done

  printf '\n'
}

if [[ "$RUN_BUILD" == "1" ]]; then
  BUILD_COMMAND=(npx tauri android build --ci)

  if [[ "$DEBUG" == "1" ]]; then
    BUILD_COMMAND+=(--debug)
  fi
  if [[ "$BUILD_APK" == "1" ]]; then
    BUILD_COMMAND+=(--apk)
  fi
  if [[ "$BUILD_AAB" == "1" ]]; then
    BUILD_COMMAND+=(--aab)
  fi
  BUILD_COMMAND+=(--target)
  for tauri_target in $TAURI_TARGET_ARGS; do
    BUILD_COMMAND+=("$tauri_target")
  done

  echo "== Starting Android packaged build =="
  echo "${BUILD_COMMAND[*]}"
  RUSTUP_TOOLCHAIN="$ACTIVE_TOOLCHAIN" "${BUILD_COMMAND[@]}"
  echo
  echo "Build complete. Outputs are under:"
  if [[ "$BUILD_APK" == "1" ]]; then
    echo "  src-tauri/gen/android/app/build/outputs/apk/"
  fi
  if [[ "$BUILD_AAB" == "1" ]]; then
    echo "  src-tauri/gen/android/app/build/outputs/bundle/"
  fi
  if [[ "$DEBUG" != "1" ]]; then
    echo
    echo "Release APK/AAB artifacts may need Android signing config before install or distribution."
  fi

  if [[ "$INSTALL" == "1" ]]; then
    if ! command -v adb >/dev/null 2>&1; then
      echo "ERROR: INSTALL=1 requires adb. Install Android platform-tools or add adb to PATH."
      exit 1
    fi

    apk_mode="$([[ "$DEBUG" == "1" ]] && echo debug || echo release)"
    install_apk="$(select_install_apk "$apk_mode")"

    if [[ -z "$install_apk" || ! -f "$install_apk" ]]; then
      echo "ERROR: Could not find a $apk_mode APK to install."
      echo "Set INSTALL_APK_PATH=/path/to/app.apk if the artifact is in a custom location."
      exit 1
    fi

    if [[ "$DEBUG" != "1" && "$install_apk" == *unsigned*.apk ]]; then
      echo "ERROR: Refusing to install unsigned release APK: $install_apk"
      echo "Use the default debug install for local testing, or configure Android release signing."
      exit 1
    fi

    echo
    echo "== Installing APK on connected Android device =="
    echo "adb install $ADB_INSTALL_ARGS \"$install_apk\""
    if ! adb install $ADB_INSTALL_ARGS "$install_apk"; then
      echo
      echo "Install failed. If a previous build was signed with a different key, uninstall first:"
      echo "  adb uninstall ch.lkmc.copywraith"
      exit 1
    fi
  fi
else
  echo "RUN_BUILD=$RUN_BUILD, skipping 'npx tauri android build'."
fi
