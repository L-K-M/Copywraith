# CI/CD

Copywraith is a cross-platform Tauri app (Rust backend + Svelte frontend) with a companion server. CI validates the frontend and Rust workspace on every change, and the release workflow builds desktop bundles, an Android APK, and a server container image, then publishes a GitHub Release.

## Workflows

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `.github/workflows/ci.yml` | Pull requests, pushes to `main`, and manual `workflow_dispatch` | Type-check & build the frontends; format, lint, and test the Rust workspace. |
| `.github/workflows/release.yml` | Pushing a `v*.*.*` tag | Build desktop/Android/server artifacts and publish them in a GitHub Release. |

## Continuous integration (`ci.yml`)

Frontend, Rust, and Linux packaging jobs run in parallel on `ubuntu-22.04`. Installed-client smoke tests follow on Ubuntu 22.04 and 24.04. In-progress runs for the same ref are cancelled when a new commit is pushed.

**Frontend (check & build)** — uses Node 20 with npm caching:

- `npm ci` for the popup frontend (repo root).
- `npm run check` — Svelte type-check (`svelte-check`).
- `npm run build` — build the popup frontend.
- `npm ci` then `npm run build` in `server/ui` — build the server UI.

**Rust (fmt, clippy, test)** — runs on the Rust `1.85.0` toolchain (with `rustfmt` and `clippy`), cargo build cache enabled:

- Installs Tauri's Linux system dependencies (`libwebkit2gtk-4.1-dev`, `build-essential`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, and others) — the desktop backend in `src-tauri` links against the system webview and GTK.
- Builds the frontend first (`npm ci && npm run build`) because `src-tauri` embeds it via `tauri::generate_context!`, so `build/` must exist before any cargo command.
- `cargo fmt --all --check` — formatting must be clean.
- `cargo clippy --workspace --all-targets -- -D warnings` — clippy warnings are treated as errors.
- `cargo test --workspace` — run the test suite.

**Linux bundle (deb + AppImage)** — builds the desktop bundle the release workflow ships, using the same `tauri-apps/tauri-action@v1` action, so packaging breakage surfaces on a pull request instead of at tag time:

- Installs the same Tauri system dependencies as the Rust job.
- `tauri build --bundles deb,appimage` (the action runs `npm run build` first via `beforeBuildCommand`).
- `scripts/check-linux-bundle.sh` — asserts the `.deb` installs `/usr/bin/copywraith`, ships a `copywraith.png` icon, and execs `copywraith` from its desktop entry. The Ubuntu and KDE docs tell users to bind `copywraith --toggle` to a global shortcut, and the app writes `Icon=copywraith` into its autostart entry, so a renamed binary would silently break both. The binary name comes from `mainBinaryName` in `src-tauri/tauri.conf.json`.
- Uploads the `.deb` and AppImage as a build artifact (7-day retention).

**Ubuntu client smoke** — installs the `.deb` on fresh Ubuntu 22.04 and 24.04 runners, then runs `scripts/smoke-linux-client.py` in isolated D-Bus/Xvfb/Openbox sessions. It checks startup, clipboard capture, single-instance commands, the X11 global shortcut, frontend Escape handling, search-and-paste, and plaintext restoration without duplicate history.

The release workflow also checks package contents and smoke-tests its actual release `.deb` before publication. These tests do not validate a physical GNOME Wayland session, tray rendering, or `/dev/uinput` keystroke injection.

### Running CI checks locally

```sh
# Frontend (popup) — from the repo root
npm ci
npm run check
npm run build

# Server UI
npm ci --prefix server/ui
npm run build --prefix server/ui

# Rust workspace (frontend build/ must exist first; see above)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

```sh
# Linux bundle (needs the Tauri system dependencies below)
npm ci
npm run tauri -- build --bundles deb,appimage
./scripts/check-linux-bundle.sh
```

On Linux you also need the Tauri system dependencies listed above before running the cargo commands.

### GNOME keybinding test

`cargo test --workspace` covers accelerator translation and `gsettings` parsing. CI separately enables the real `gsettings` round-trip — creating, updating, and removing GNOME custom keybindings — in a disposable configuration and D-Bus session. Keep it isolated locally too:

```sh
sudo apt install gnome-settings-daemon-common dconf-gsettings-backend dbus-x11
XDG_CONFIG_HOME=$(mktemp -d) COPYWRAITH_TEST_GSETTINGS=1 \
  dbus-run-session -- cargo test -p copywraith-tauri gnome_keybinding
```

## Releases (`release.yml`)

To cut a release:

```
git tag v1.2.3
git push origin v1.2.3
```

The tag must match `v*.*.*`. A tag containing `-` (e.g. `v1.2.3-rc.1`) is treated as a prerelease.

The workflow first runs CI (including Ubuntu runtime checks) and validates that the tag matches all manifests. It then fans out:

1. **Create draft release** — creates a draft GitHub Release named `Copywraith <tag>` with auto-generated notes. Every build job uploads into this draft.
2. **Desktop** (`tauri-action`, matrix, `fail-fast: false`) — builds bundles for:
   - macOS Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`) on `macos-latest`,
   - Linux on `ubuntu-22.04` (with Tauri system deps installed),
   - Windows on `windows-latest`.
3. **Android APK** — sets up JDK 17, the Android SDK, NDK `26.1.10909125`, and the four Android Rust targets, then runs `tauri android init` / `android build` to produce a universal APK. The APK is **signed only when an upload keystore is provided via secrets**; otherwise the unsigned release APK is attached and a warning is logged. The asset is named `copywraith-<tag>-android-universal.apk` (or `…-unsigned.apk`) and uploaded with `gh release upload`.
4. **Server Docker image** — builds `server/Dockerfile` and pushes to GHCR at `ghcr.io/<owner>/copywraith-server`, tagged with the semver `{{version}}`, `{{major}}.{{minor}}`, and `latest` (the `latest` tag is skipped for prereleases).
5. **Publish release** — once all build jobs succeed, the draft is flipped to published.

**Signing/notarization caveats:** the **macOS bundles are unsigned and un-notarized by design** — `release.yml` deliberately does not pass any `APPLE_*` secrets to `tauri-action`. macOS will warn on first launch. Users approve the app under **System Settings → Privacy & Security → Open Anyway** (which appears for about an hour after the first blocked launch attempt), or clear the quarantine attribute. The older Control-click → Open bypass no longer works — Apple removed it in macOS Sequoia (15) — so don't document it; see [`README.mac.md`](README.mac.md) for the steps given to users.

This is not an oversight. `tauri-action` attempts code-signing whenever `APPLE_CERTIFICATE` is set, and a certificate it cannot import fails the build outright (`failed codesign application: failed to run command security import: failed to import keychain certificate`). That sank both macOS jobs on v0.2.0 and v0.3.0, and since `publish-release` requires the whole `build-desktop` matrix, both releases were left as drafts. Not passing the secrets makes the result independent of whatever happens to be in the secret store. To sign again, restore the `APPLE_*` environment entries on the "Build and upload Tauri bundles" step and verify the certificate imports (`security import`) before relying on it.

The Android APK is only properly signed when the keystore secrets are set — without them it is unsigned and suitable only for sideloading. The optional `TAURI_SIGNING_*` secrets enable Tauri updater signatures.

## Secrets

`GITHUB_TOKEN` is provided automatically (used to create the release and push to GHCR). Every secret below is **optional — release builds complete without them**, just unsigned.

| Secret | Enables |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater signing key. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for the Tauri updater key. |
| `ANDROID_KEYSTORE_BASE64` | Android upload keystore (base64); without it the APK is unsigned. |
| `ANDROID_KEYSTORE_PASSWORD` | Keystore password. |
| `ANDROID_KEY_ALIAS` | Key alias within the keystore. |
| `ANDROID_KEY_PASSWORD` | Password for the signing key. |

The `APPLE_*` secrets are **not** read by any workflow — see the signing caveats above. Any that remain in the repository's secret store are inert; deleting them avoids the impression that macOS builds are signed.
