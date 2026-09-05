#!/usr/bin/env bash
set -euo pipefail

# CI and release builds must install the same NDK.
readonly ANDROID_NDK_VERSION="26.1.10909125"
: "${ANDROID_HOME:?ANDROID_HOME must be set}"
: "${GITHUB_ENV:?GITHUB_ENV must be set}"

sdkmanager "ndk;$ANDROID_NDK_VERSION"
printf 'NDK_HOME=%s/ndk/%s\n' "$ANDROID_HOME" "$ANDROID_NDK_VERSION" >> "$GITHUB_ENV"
