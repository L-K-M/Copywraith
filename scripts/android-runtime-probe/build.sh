#!/usr/bin/env bash
set -euo pipefail

# Generate the real app, then overlay only debug/instrumentation sources.
: "${ANDROID_HOME:?Set ANDROID_HOME to an installed SDK}"
: "${JAVA_HOME:?Set JAVA_HOME to Java 17}"
: "${NDK_HOME:?Set NDK_HOME to an installed NDK}"
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
npm run tauri -- android init --ci --skip-targets-install
node scripts/android-runtime-probe/install.mjs
npm run tauri -- android build --debug --target x86_64 --features android-runtime-probe --apk

# Tauri already built and linked the actual cdylib; the test APK reuses it.
src-tauri/gen/android/gradlew -p src-tauri/gen/android --no-daemon \
    -PabiList=x86_64 -ParchList=x86_64 -PtargetList=x86_64 \
    :app:assembleUniversalDebugAndroidTest :copywraith-share-target:testDebugUnitTest \
    -x :app:rustBuildUniversalDebug
