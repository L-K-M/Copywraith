# Copywraith

Copywraith is a clipboard manager with with a server component that allows synchronization and long-term storage of clipboard history.

# Important Security Consideration

Do not expose the server component to the Internet. It is intended to be used on a local network or with a secure VPN (like Tailscale or Netbird) between your devices. The server does not implement any rate limiting or brute-force protection, so it is vulnerable to password guessing attacks if exposed publicly. Always use a strong, unique password and consider additional network-level protections if you need remote access.

## Prerequisites

- Rust toolchain (stable; project currently builds with Rust 1.85+)
- Node.js + npm
- Tauri v2 system dependencies for your OS

Tauri dependency guide:

- https://v2.tauri.app/start/prerequisites/

## Installation

From the repository root:

```bash
npm install
```

Optional sanity checks:

```bash
cargo check --workspace
cargo test --workspace
npm run build
```

## Running Copywraith

### 1.a) Start the server using cargo

From repository root:

```bash
cargo run -p copywraith-server
```

Server defaults:

- API base: `http://localhost:3742/api`
- Admin UI: `http://localhost:3742/`

Environment variables:

- `COPYWRAITH_DATA_DIR` (default `./data`)
- `PORT` (default `3742`)
- `COPYWRAITH_HOST` (default `127.0.0.1`; set `0.0.0.0` for Docker)
- `RUST_LOG`

Tip: copy `.env.example` to `.env` and adjust values for local/docker runs.

### 1.b) Start Server via Docker (server)

From repo root (recommended):

```bash
docker compose up --build
```

```bash
sudo docker compose build --no-cache copywraith-server
sudo docker compose up
```

Or use the helper script (recommended for repeat deploys):

```bash
# from repo root
./scripts/redeploy-server-docker.sh

# on systems where Docker requires sudo
USE_SUDO=1 ./scripts/redeploy-server-docker.sh
```

The script tags the image as `copywraith-server:<server-version>` (from
`server/Cargo.toml`) and uses that same tag for build + up, which helps prevent
stale image reuse between deploys.

Alternatively from `server/`:

```bash
cd server
docker compose up --build
```

This exposes port `3742` and persists server data in Docker volume `copywraith-data`.

Docker note: the server builder image in `server/Dockerfile` must be Rust 1.85+
(`rust:1.85-slim-bookworm` currently). If a build log still shows
`rust:1.83-slim-bookworm`, you are building from an older copy of the file.
Pull latest changes (or update the Dockerfile), then rebuild with:

```bash
docker compose build --no-cache --pull copywraith-server
```

Docker note: the container must run with `COPYWRAITH_HOST=0.0.0.0` (already set
in both compose files). If it is missing, `/api/health` from the host may fail
with connection reset/refused even though the container is running.

Docker note: compose files include
`COPYWRAITH_SERVER_IMAGE_REPO`/`COPYWRAITH_SERVER_IMAGE_TAG` support so you can
pin a deployment image tag explicitly if needed. The default tag in compose is
kept aligned with the server crate version (currently `0.1.5`).



### 2) Start the desktop app

From repository root:

```bash
# Test
npm run tauri dev

# With log output
RUST_LOG=debug npm run tauri dev
```

The popup window starts hidden. Use the hotkeys below to open it.

Build the app:

```bash
npm run tauri build
```

### 3) Configure sync (optional)

In the desktop popup:

- press `Cmd+,` (or `Ctrl+,`) to open Settings
- set `Primary Server URL` to the first address to try (for example `http://192.168.1.5:3742`)
- optionally set `Fallback Server URL` as a backup address (for example a Tailscale IP)
- save

After that, sync runs in both directions (roughly every 5 seconds):

- device -> server: unsynced local entries are uploaded
- server -> device: new entries from other devices are pulled into local history
- popup status bar shows the active sync endpoint (`Primary`/`Fallback`) and when configured endpoints are unreachable

## Hotkeys

- `Cmd/Ctrl + Shift + V` -> toggle popup
- `Cmd/Ctrl + Shift + B` -> popup with starred-only filter enabled
- `Cmd/Ctrl + Shift + Alt + V` -> paste most recent item as plaintext

Inside the list:

- `Click` -> paste selected entry
- `Alt + Click` -> paste as plaintext
- `Double-click` or `Space` on focused row -> open entry preview dialog
- `Enter` on focused row -> paste

## Password protection & encryption

On first visit to the admin UI (or first API call), you are prompted to create
a password. Once set:

- All clipboard text and blob data is encrypted at rest (AES-256-GCM)
- Every API request requires the password as `Authorization: Bearer <password>`
- The web UI stores the password in `sessionStorage` (cleared on tab close)
- The desktop client sends the same password via the Settings "API Key" field

Password can be changed without re-encrypting data (the underlying encryption
key stays the same, only its wrapping changes). If the password is forgotten,
delete `auth.json` from the data directory -- but all encrypted data will be
permanently lost.

## Android app

The same codebase produces an Android app. On Android, tapping an entry copies it
to the clipboard (instead of simulating a paste). Opening the app automatically
captures whatever is on the Android clipboard.

### Android prerequisites

- Android Studio with SDK Platform 24+ installed
- Android NDK (installed automatically by `tauri android init`, or manually via Android Studio SDK Manager)
- Rust Android targets:

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
```

- `JAVA_HOME` pointing to the JDK bundled with Android Studio (or a standalone JDK 17+)
- `ANDROID_HOME` pointing to your Android SDK (e.g. `~/Library/Android/sdk`)

If you have `NDK_HOME` set in your shell profile, make sure it matches the
installed NDK version. If in doubt, `unset NDK_HOME` and let Tauri auto-detect it.

### Initialize the Android project

From the repository root:

```bash
npx tauri android init
```

This generates the Gradle project under `src-tauri/gen/android/`.

### If CLI NDK install fails

If `npx tauri android init` reports that it installed an NDK version but then
fails with `NDK_HOME ... doesn't point to an existing directory`, install the
NDK using Android Studio instead:

1. Open Android Studio
2. Go to Settings/Preferences -> Android SDK -> SDK Tools
3. Enable `NDK (Side by side)` and apply changes
4. Verify the installed path exists under `~/Library/Android/sdk/ndk/<version>`
5. Point `NDK_HOME` to that exact directory and re-run init

```bash
export NDK_HOME="$HOME/Library/Android/sdk/ndk/<version>"
npx tauri android init
```

### Optional helper scripts

From the repository root:

```bash
# Verifies Rust/Android toolchains, fixes stale NDK_HOME, then runs Android dev
./scripts/android-dev-bootstrap.sh

# Verifies Rust/Android toolchains, fixes stale NDK_HOME, then builds and installs a packaged APK
./scripts/android-release-bootstrap.sh

# Persists ANDROID_HOME/NDK_HOME into your shell profile and macOS launchctl env
./scripts/android-env-persist.sh
```

### Build and run (development)

Connect a device or start an emulator, then:

```bash
npx tauri android dev
```

Android dev builds are not standalone installs. They load the Vite dev server
from your computer on port `1420`, so reopening a dev-installed app later will
fail with a `Failed to request http://<computer-ip>:1420/` error unless the dev
server is still running and reachable from the phone.

For a standalone test install that does not depend on Vite, build a packaged
debug APK instead:

```bash
npx tauri android build --debug
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

If Android rejects the install because an older dev/release build has a different
signature, uninstall first:

```bash
adb uninstall ch.lkmc.copywraith
```

### Android app icon

Android launcher icons are generated from `src-tauri/icons/icon.png`.
If you update the app icon source, regenerate icons with:

```bash
npx tauri icon src-tauri/icons/icon.png
```

Then rebuild the Android app. If the old icon still appears on-device, uninstall
the app once and reinstall (some launchers cache app icons).

### Build a release APK / AAB

```bash
npx tauri android build
```

Or use the release bootstrap helper:

```bash
./scripts/android-release-bootstrap.sh
```

By default this builds a debug-signed packaged APK and installs it with `adb`,
so it is standalone and does not depend on the Vite dev server.

Useful options:

```bash
# Build only, without installing
INSTALL=0 ./scripts/android-release-bootstrap.sh

# Build an unsigned/signed release artifact instead of a debug-signed local install
DEBUG=0 INSTALL=0 ./scripts/android-release-bootstrap.sh

# Build APK and AAB
BUILD_APK=1 BUILD_AAB=1 INSTALL=0 ./scripts/android-release-bootstrap.sh

# Build all Android ABIs instead of only aarch64
TAURI_TARGETS=all ./scripts/android-release-bootstrap.sh
```

The unsigned APK is written to `src-tauri/gen/android/app/build/outputs/apk/`.
For a signed release build, configure signing in
`src-tauri/gen/android/app/build.gradle.kts` per the
[Tauri Android distribution guide](https://v2.tauri.app/distribute/sign/android/).

### Android troubleshooting

- **`Failed to request http://<ip>:1420/` after reopening the app** -- the installed
  app is a dev build from `npx tauri android dev`. Keep the dev command running,
  or install a packaged debug APK with `npx tauri android build --debug`.
- **`NDK_HOME` doesn't point to an existing directory** -- unset the variable
  (`unset NDK_HOME`) or update it to the path printed by `sdkmanager` during init.
  If the printed path does not actually exist, install NDK from Android Studio
  (Android SDK -> SDK Tools -> `NDK (Side by side)`) and point `NDK_HOME` to
  the installed version directory.
- **Gradle sync fails** -- make sure `JAVA_HOME` and `ANDROID_HOME` are set correctly
  and that you have accepted the SDK licenses (`sdkmanager --licenses`).
- **App crashes on launch** -- check `adb logcat` for Rust panics. The most common
  cause is a missing server URL in Settings (sync errors are non-fatal but logged).

## Quick troubleshooting

- **`npm install` fails with registry/package errors**
  - verify network access and npm registry settings
- **Tauri fails to launch webview**
  - verify OS prerequisites from Tauri docs
- **Entries not syncing**
  - check Settings -> `Primary Server URL` / `Fallback Server URL`
  - verify server is reachable and running on expected port
- **Docker build fails with `feature \`edition2024\` is required`**
  - this usually means Cargo 1.83 is being used from an old Docker builder image
  - confirm `server/Dockerfile` uses `FROM rust:1.85-slim-bookworm` (or newer)
  - rebuild with `docker compose build --no-cache --pull copywraith-server`
- **`/api/health` fails after container start (`connection reset`/`refused`)**
  - confirm your compose env includes `COPYWRAITH_HOST=0.0.0.0`
  - redeploy with `./scripts/redeploy-server-docker.sh` (use `USE_SUDO=1` if needed)
- **Server still reports an old version after deploy**
  - redeploy with `./scripts/redeploy-server-docker.sh` (or `USE_SUDO=1 ...`)
  - verify script output `running image:` matches the expected tag (for example `copywraith-server:0.1.5`)
  - if needed, set `COPYWRAITH_SERVER_IMAGE_TAG` to a new value before build/up
