# CI/CD

Copywraith is a cross-platform Tauri app (Rust backend + Svelte frontend) with a companion server. CI validates the frontend and Rust workspace on every change, and the release workflow builds desktop bundles, an Android APK, and a server container image, then publishes a GitHub Release.

## Workflows

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `.github/workflows/ci.yml` | Pull requests, pushes to `main`, and manual `workflow_dispatch` | Type-check & build the frontends; format, lint, and test the Rust workspace. |
| `.github/workflows/release.yml` | Pushing a `v*.*.*` tag | Build desktop/Android/server artifacts and publish them in a GitHub Release. |

## Continuous integration (`ci.yml`)

Two jobs run in parallel on `ubuntu-22.04`. In-progress runs for the same ref are cancelled when a new commit is pushed.

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

On Linux you also need the Tauri system dependencies listed above before running the cargo commands.

## Releases (`release.yml`)

To cut a release:

```
git tag v1.2.3
git push origin v1.2.3
```

The tag must match `v*.*.*`. A tag containing `-` (e.g. `v1.2.3-rc.1`) is treated as a prerelease.

The workflow runs as a fan-out:

1. **Create draft release** — creates a draft GitHub Release named `Copywraith <tag>` with auto-generated notes. Every build job uploads into this draft.
2. **Desktop** (`tauri-action`, matrix, `fail-fast: false`) — builds bundles for:
   - macOS Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`) on `macos-latest`,
   - Linux on `ubuntu-22.04` (with Tauri system deps installed),
   - Windows on `windows-latest`.
3. **Android APK** — sets up JDK 17, the Android SDK, NDK `26.1.10909125`, and the four Android Rust targets, then runs `tauri android init` / `android build` to produce a universal APK. The APK is **signed only when an upload keystore is provided via secrets**; otherwise the unsigned release APK is attached and a warning is logged. The asset is named `copywraith-<tag>-android-universal.apk` (or `…-unsigned.apk`) and uploaded with `gh release upload`.
4. **Server Docker image** — builds `server/Dockerfile` and pushes to GHCR at `ghcr.io/<owner>/copywraith-server`, tagged with the semver `{{version}}`, `{{major}}.{{minor}}`, and `latest` (the `latest` tag is skipped for prereleases).
5. **Publish release** — once all build jobs succeed, the draft is flipped to published.

**Signing/notarization caveats:** all builds succeed without any signing secrets. When the optional Apple secrets are present the macOS bundles are code-signed and notarized; otherwise they are unsigned. Likewise, the Android APK is only properly signed when the keystore secrets are set — without them it is unsigned and suitable only for sideloading. The optional `TAURI_SIGNING_*` secrets enable Tauri updater signatures.

## Secrets

`GITHUB_TOKEN` is provided automatically (used to create the release and push to GHCR). Every secret below is **optional — release builds complete without them**, just unsigned / un-notarized.

| Secret | Enables |
| --- | --- |
| `APPLE_CERTIFICATE` | macOS code-signing certificate (base64). |
| `APPLE_CERTIFICATE_PASSWORD` | Password for the certificate. |
| `APPLE_SIGNING_IDENTITY` | Developer ID signing identity. |
| `APPLE_ID` | Apple ID used for notarization. |
| `APPLE_PASSWORD` | App-specific password for notarization. |
| `APPLE_TEAM_ID` | Apple Developer team ID. |
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri updater signing key. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for the Tauri updater key. |
| `ANDROID_KEYSTORE_BASE64` | Android upload keystore (base64); without it the APK is unsigned. |
| `ANDROID_KEYSTORE_PASSWORD` | Keystore password. |
| `ANDROID_KEY_ALIAS` | Key alias within the keystore. |
| `ANDROID_KEY_PASSWORD` | Password for the signing key. |
