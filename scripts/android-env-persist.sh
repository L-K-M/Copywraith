#!/usr/bin/env bash
set -Eeuo pipefail

# Persists Android env vars for local Tauri Android builds.
#
# Usage:
#   ./scripts/android-env-persist.sh
# Optional:
#   RC_FILE="$HOME/.zshrc" ./scripts/android-env-persist.sh
#   ANDROID_HOME="$HOME/Library/Android/sdk" ./scripts/android-env-persist.sh
#   NDK_HOME="$HOME/Library/Android/sdk/ndk/30.0.14904198" ./scripts/android-env-persist.sh

default_sdk="$HOME/Library/Android/sdk"
android_home="${ANDROID_HOME:-$default_sdk}"

if [[ ! -d "$android_home" ]]; then
  echo "ERROR: ANDROID_HOME does not exist: $android_home"
  echo "Install Android SDK first (Android Studio -> SDK Manager)."
  exit 1
fi

ndk_home="${NDK_HOME:-}"
if [[ -z "$ndk_home" ]]; then
  ndk_dir="$android_home/ndk"
  if [[ -d "$ndk_dir" ]]; then
    latest_ndk="$(ls -1 "$ndk_dir" 2>/dev/null | sort | tail -n 1 || true)"
    if [[ -n "$latest_ndk" && -d "$ndk_dir/$latest_ndk" ]]; then
      ndk_home="$ndk_dir/$latest_ndk"
    fi
  fi
fi

if [[ -z "$ndk_home" || ! -d "$ndk_home" ]]; then
  echo "ERROR: Could not find a valid NDK directory."
  echo "Install NDK (Android Studio -> SDK Manager -> SDK Tools -> NDK (Side by side))."
  echo "Then re-run with NDK_HOME set to the installed path."
  exit 1
fi

shell_name="$(basename "${SHELL:-}")"
rc_file="${RC_FILE:-}"
if [[ -z "$rc_file" ]]; then
  case "$shell_name" in
    zsh)
      rc_file="$HOME/.zshrc"
      ;;
    bash)
      rc_file="$HOME/.bash_profile"
      ;;
    *)
      rc_file="$HOME/.profile"
      ;;
  esac
fi

mkdir -p "$(dirname "$rc_file")"
touch "$rc_file"

backup_file="$rc_file.bak.copywraith.$(date +%Y%m%d%H%M%S)"
cp "$rc_file" "$backup_file"

tmp_file="$(mktemp)"
in_block=0
while IFS= read -r line || [[ -n "$line" ]]; do
  if [[ "$line" == "# >>> COPYWRAITH ANDROID ENV >>>" ]]; then
    in_block=1
    continue
  fi

  if [[ "$line" == "# <<< COPYWRAITH ANDROID ENV <<<" ]]; then
    in_block=0
    continue
  fi

  if [[ $in_block -eq 0 ]]; then
    printf '%s\n' "$line" >> "$tmp_file"
  fi
done < "$rc_file"

cat >> "$tmp_file" <<EOF

# >>> COPYWRAITH ANDROID ENV >>>
export ANDROID_HOME="$android_home"
export NDK_HOME="$ndk_home"
export PATH="\$ANDROID_HOME/platform-tools:\$ANDROID_HOME/emulator:\$PATH"
# <<< COPYWRAITH ANDROID ENV <<<
EOF

mv "$tmp_file" "$rc_file"

if command -v launchctl >/dev/null 2>&1; then
  launchctl unsetenv NDK_HOME >/dev/null 2>&1 || true
  launchctl setenv ANDROID_HOME "$android_home" >/dev/null 2>&1 || true
  launchctl setenv NDK_HOME "$ndk_home" >/dev/null 2>&1 || true
fi

echo "Updated Android environment in: $rc_file"
echo "Backup created at: $backup_file"
echo "ANDROID_HOME=$android_home"
echo "NDK_HOME=$ndk_home"
echo
echo "Next steps:"
echo "1) Reload your shell: source \"$rc_file\""
echo "2) Initialize Android project (if not done): npx tauri android init"
echo "3) Run app on device/emulator: npx tauri android dev"
