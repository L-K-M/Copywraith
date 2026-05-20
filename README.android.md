# Copywraith Android App

The Android app is built from the same Tauri + Svelte codebase. It stores clipboard entries locally and can sync with the Copywraith server.

Android does not simulate a system paste action. Tapping an entry copies it to the Android clipboard. Opening or resuming the app captures the current clipboard.

## Clipboard Capture Paths

- Foreground capture: opening or resuming Copywraith imports the current clipboard when available.
- Share-sheet import: share text, images, or files from another Android app to Copywraith.
- Optional Shizuku/Sui listener: advanced mode for privileged text clipboard monitoring.

Normal Android clipboard capture works without Shizuku. If Shizuku is missing, stopped, or denied, Copywraith falls back to foreground capture and share-sheet import.

## Prerequisites

- Android Studio with SDK Platform 24 or newer installed.
- Android NDK, installed through Android Studio SDK Manager or by Tauri init.
- Rust Android targets.
- `JAVA_HOME` pointing to Android Studio's JDK or another JDK 17+.
- `ANDROID_HOME` pointing to the Android SDK, for example `~/Library/Android/sdk` on macOS.

Install Rust Android targets:

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
```

If `NDK_HOME` is set, make sure it points to an installed NDK directory. If unsure, unset it and let Tauri auto-detect the NDK.

## Initialize Android Project

From the repository root:

```bash
npx tauri android init
```

This generates the Gradle project under `src-tauri/gen/android/`.

## Helper Scripts

From the repository root:

```bash
./scripts/android-dev-bootstrap.sh
```

This verifies Rust and Android toolchains, fixes stale `NDK_HOME`, then runs Android dev.

For packaged debug APK builds and local install:

```bash
./scripts/android-release-bootstrap.sh
```

To persist Android environment variables into your shell profile and macOS `launchctl` environment:

```bash
./scripts/android-env-persist.sh
```

## Development Run

Connect a device or start an emulator, then run:

```bash
npx tauri android dev
```

Android dev builds load the Vite dev server from your computer on port `1420`. Reopening a dev-installed app later can fail with `Failed to request http://<computer-ip>:1420/` unless the dev server is still running and reachable from the phone.

## Standalone Debug APK

For a standalone test install that does not depend on Vite:

```bash
npx tauri android build --debug
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

If Android rejects the install because another build has a different signature, uninstall first:

```bash
adb uninstall ch.lkmc.copywraith
```

## Release Build

```bash
npx tauri android build
```

Or use the release bootstrap helper:

```bash
./scripts/android-release-bootstrap.sh
```

Useful options:

```bash
INSTALL=0 ./scripts/android-release-bootstrap.sh
DEBUG=0 INSTALL=0 ./scripts/android-release-bootstrap.sh
BUILD_APK=1 BUILD_AAB=1 INSTALL=0 ./scripts/android-release-bootstrap.sh
TAURI_TARGETS=all ./scripts/android-release-bootstrap.sh
```

The unsigned APK is written under `src-tauri/gen/android/app/build/outputs/apk/`. For signed releases, configure signing in `src-tauri/gen/android/app/build.gradle.kts` following the Tauri Android distribution guide: https://v2.tauri.app/distribute/sign/android/

## Optional Shizuku Clipboard Listener

Shizuku lets Copywraith bind a `UserService` that runs as ADB shell or root/Sui instead of the normal app UID. That helper registers a privileged clipboard listener through Android's system clipboard Binder API.

When clipboard text changes, the helper stages the text for local import when the app is alive and also tries to upload it directly to the configured Copywraith server using the same server password from Settings.

To use it:

1. Install and start Shizuku, or install Sui on a rooted/Magisk device.
2. Open Copywraith on Android and configure server URL and password in Settings.
3. In Settings, enable `Advanced Android Clipboard` > `Enable Shizuku Listener`.
4. Grant Copywraith permission in the Shizuku prompt.

Notes:

- Non-root Shizuku usually runs as ADB shell, `uid 2000`, and may need to be restarted after reboot.
- Sui/root runs as root, `uid 0`, and is more persistent on rooted devices.
- OEM Android builds can differ; shell/root clipboard access is best-effort.
- The helper only handles text clipboard payloads. Use the share sheet for images and files.
- Disabling the setting stops and removes the Shizuku user service.
- Keep the server on a trusted LAN or VPN because the helper sends the configured password as `Authorization: Bearer <password>` for direct uploads.

## Android App Icon

Android launcher icons are generated from `src-tauri/icons/icon.png`.

After changing the source icon, regenerate icons with:

```bash
npx tauri icon src-tauri/icons/icon.png
```

Then rebuild the Android app. If the old icon still appears, uninstall and reinstall the app because some launchers cache app icons.

## NDK Install Issues

If `npx tauri android init` reports that it installed an NDK version but then fails with `NDK_HOME ... doesn't point to an existing directory`, install the NDK in Android Studio:

1. Open Android Studio.
2. Go to Settings or Preferences > Android SDK > SDK Tools.
3. Enable `NDK (Side by side)` and apply changes.
4. Verify the installed path exists under `~/Library/Android/sdk/ndk/<version>`.
5. Point `NDK_HOME` to that exact directory and re-run init.

```bash
export NDK_HOME="$HOME/Library/Android/sdk/ndk/<version>"
npx tauri android init
```

## Troubleshooting

- `Failed to request http://<ip>:1420/` after reopening: install a packaged debug APK or keep `npx tauri android dev` running.
- `NDK_HOME` does not point to an existing directory: unset it or update it to an installed NDK path.
- Gradle sync fails: verify `JAVA_HOME`, `ANDROID_HOME`, and accepted SDK licenses with `sdkmanager --licenses`.
- App crashes on launch: check `adb logcat` for Rust panics and verify Settings contains a valid server URL when sync is enabled.
