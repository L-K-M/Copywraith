# AGENTS.md

This file is a fast context handoff for future agent runs in this repository.

## What this project is

Copywraith is a local-first clipboard manager with:

- Desktop client: Tauri v2 + Svelte 5 (popup UI)
- Server: Rust + Axum + SQLite + blob storage
- Shared crate: `crates/copywraith-core` for models, API types, hashing/content utils

Desktop captures clipboard changes, stores locally, and syncs with server.

- a Tauri desktop client (Svelte 5 + Rust backend), and
- a Rust/Axum server for durable, searchable clipboard history.

The desktop app watches your clipboard, stores entries in a local SQLite cache, and can optionally sync those entries to the server.

## Current status

Implemented today:

- Clipboard capture for `text`, `html`, `rtf`, `image`, and `file`
- Content-hash deduplication (SHA-256)
- Floating popup UI with retro System 7 styling
- Star/unstar, delete, search/filter
- Global shortcuts for opening popup and quick plaintext paste
- Two-way background sync (push local unsynced + pull remote updates)
- Server REST API + Svelte admin web UI
- Password protection with at-rest encryption (Argon2id + AES-256-GCM)
- Android/mobile client support via Tauri mobile entry point, mobile clipboard plugin, platform-aware UI, and `capture_clipboard` on open/resume

Planned later:

- Android/mobile production hardening and device testing

## Architecture at a glance

1. **Clipboard change happens on desktop**
2. Tauri Rust backend receives `plugin:clipboard://clipboard-monitor/update`
3. Backend reads clipboard content (files/image/html/rtf/text in priority order)
4. Entry is normalized and deduplicated by hash
5. Entry is stored in local SQLite + blob store
6. UI receives `clipboard-updated` event and refreshes list
7. Background sync loop pushes unsynced entries and pulls entries from other devices

## Repository layout

```text
.
├── crates/copywraith-core/   # Shared models, API types, hashing/content helpers
├── server/                   # Axum API + SQLite/blob persistence + Svelte admin UI
├── scripts/                  # Android bootstrap/env helpers, server redeploy/version sync
├── src-tauri/                # Tauri Rust backend (monitoring, commands, sync, shortcuts)
├── src/                      # Svelte popup frontend
├── ARCHITECTURE.md
├── IMPLEMENTATION.md
└── ENCRYPTION.md
```

## Current architecture (important)

### Clipboard monitoring is Rust-owned (not JS-owned)

Source: `src-tauri/src/clipboard.rs`

- Starts native monitor via `tauri_plugin_clipboard::Clipboard::start_monitor(...)`
- Listens for single event: `plugin:clipboard://clipboard-monitor/update`
- Reads content through Rust API (`has_*`/`read_*`)
- Priority order: `Image > File > Html > Rtf > Text`

Do not re-introduce frontend `startListening()` dependency unless intentionally redesigning.

### Deduplication

- Dedup key is content hash (`content_hash`, SHA-256)
- Client local DB and server DB both deduplicate by unique index on `content_hash`

### Two-way sync (running desktop app)

Sources: `src-tauri/src/lib.rs`, `src-tauri/src/sync.rs`

- Background loop runs every ~5s
- Pushes local unsynced entries to server (`sync_unsynced_entries`)
- Pulls new server entries into local storage (`pull_new_entries`)
- Uses server entry cursor (`last_seen_server_id`) to only ingest newer remote entries
- Emits `clipboard-updated` event when pull imports new entries so UI refreshes
- Sync settings live in local SQLite (`server_url_primary`, `server_url_fallback`, `api_key`) and are edited in `src/lib/components/SettingsDialog.svelte`; sync tries the primary URL first, then falls back to the secondary URL

### Server admin UI

Source: `server/ui/` — a plain Svelte + Vite SPA (not SvelteKit).

- Built output goes to `server/ui/dist/`
- Served by `server/src/main.rs` at `/` via `tower_http::services::ServeDir`
- Uses `@lkmc/system7-ui` components (DataTable, TitleBar, Button, etc.)
- Build with: `cd server/ui && npm run build`
- If UI not built, server shows a fallback HTML page with build instructions

### Paste simulation

Source: `src-tauri/src/paste.rs`

- macOS: simulates Cmd+V via `osascript` in a **spawned thread** (must not run synchronously on the Tauri async runtime — doing so blocks the IPC response and races with the async popup hide / focus restoration)
- non-macOS: warns that simulated paste is not implemented
- `simulate_paste` runs on a background thread so the `paste_entry` Tauri command returns immediately after hiding the popup; the thread sleeps 100ms for the hide to complete, then runs osascript to activate the target app and send Cmd+V
- Do **not** change `simulate_paste` to run synchronously (inline) — this was the cause of a regression where the paste keystroke arrived before the previous app had been re-activated

### Global shortcuts

Source: `src-tauri/src/lib.rs`

Default shortcuts (configurable via Settings dialog):
- `CmdOrCtrl+Shift+V`: toggle popup
- `CmdOrCtrl+Shift+B`: popup with starred filter on
- `CmdOrCtrl+Shift+Alt+V`: paste most recent as plaintext

Settings are persisted in local SQLite and shortcuts are re-registered on app start or when settings change on desktop; mobile hides the shortcut fields in `src/lib/components/SettingsDialog.svelte` and skips shortcut re-registration.

### Tauri capability naming gotcha

Desktop uses `clipboard:*` permissions in `src-tauri/capabilities/default.json`; mobile uses `clipboard-manager:*` permissions in `src-tauri/capabilities/mobile.json`.

## Key files to read first

- `README.md`
- `rust-toolchain.toml`
- `ARCHITECTURE.md`
- `IMPLEMENTATION.md`
- `ENCRYPTION.md`
- `SENSITIVE.md`
- `src/lib/util/platform.ts`
- `src/lib/components/SettingsDialog.svelte`
- `src-tauri/src/clipboard.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/storage.rs`
- `src-tauri/src/sync.rs`
- `src-tauri/capabilities/mobile.json`
- `scripts/android-dev-bootstrap.sh`
- `scripts/android-env-persist.sh`
- `server/src/main.rs`
- `server/src/api.rs`
- `server/src/crypto.rs`
- `server/src/storage.rs`

## Run and test commands

From repo root unless noted.

If `cargo` is missing in this environment, prepend:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Install JS deps:

```bash
npm install
```

Build checks:

```bash
cargo check --workspace
cargo test --workspace
npm run build
```

Run server:

```bash
cargo run -p copywraith-server
```

Run desktop app:

```bash
npm run tauri dev
```

Android mobile dev/build:

```bash
./scripts/android-dev-bootstrap.sh
npx tauri android dev
npx tauri android build
```

Server defaults:

- API: `http://localhost:3742/api`
- Admin UI: `http://localhost:3742/`

## Environment and dependency gotchas

- Rust toolchain is pinned at the repo root via `rust-toolchain.toml` (`1.85.0`).
- Do not downgrade the server Docker builder image below Rust 1.85; Cargo 1.83 fails on the current lockfile with `base64ct` due to missing `edition2024` support (`feature \`edition2024\` is required`).
- Server binds `127.0.0.1` by default; Docker deployments must set `COPYWRAITH_HOST=0.0.0.0` (set in compose + Dockerfile env) so published port `3742` is reachable.
- Compose supports explicit image tagging via `COPYWRAITH_SERVER_IMAGE_REPO` + `COPYWRAITH_SERVER_IMAGE_TAG`; `scripts/redeploy-server-docker.sh` defaults tag to the server crate version to reduce stale-image confusion.
- Do not expose the server publicly; it is intended for a local network or secure VPN and does not add rate limiting / brute-force protection.
- `@lkmc/system7-ui` is consumed as an npm dependency (not a local sibling path).
- Svelte/TSServer may show false "Cannot find package 'vite'" errors due to bun cache.
  - Confirm actual state with `npm run build`.
- Android builds need `ANDROID_HOME` and `NDK_HOME`; use `scripts/android-env-persist.sh` or `scripts/android-dev-bootstrap.sh` on macOS to set them up.

## Known gaps / pending improvements

- No full desktop end-to-end test automation with real OS clipboard/UI interaction.
- Android/mobile client is partially implemented (platform detection, mobile-specific UI, `capture_clipboard` command, helper scripts) but not yet production-tested.

### Password protection & encryption

Source: `server/src/crypto.rs`, `ENCRYPTION.md`

- Single-user, password-only auth (no login name).
- Password hashed with Argon2id (64 MiB, 3 iterations, 4 parallelism).
- Master key → HKDF splits into auth_key (verification) and KEK (key encryption).
- Random 256-bit DEK encrypted with KEK, stored in `{data_dir}/auth.json`.
- `text_content` encrypted via AES-256-GCM with `ENC:1:` prefix; blobs with `ENCB` header.
- Password change re-wraps the same DEK — no data re-encryption needed.
- If `auth.json` doesn't exist, server shows "Create Password" screen; data endpoints return 403 until setup.
- `COPYWRAITH_ADMIN_API_KEY` env var has been removed; use password auth instead.
- Desktop client sends password as `Authorization: Bearer <password>` (same header, same field).
- Web admin UI stores password in `sessionStorage`; shows setup/unlock screens as needed.
- Auth API: `GET /api/auth/status`, `POST /api/auth/setup`, `POST /api/auth/unlock`,
  `POST /api/auth/change-password`, `POST /api/auth/lock`.

## Editing guardrails for future runs

- Preserve SvelteKit static adapter + `ssr = false` for popup app.
- Keep `@lkmc/system7-ui` imports package-based (avoid reintroducing local file path coupling).
- Do not silently change clipboard event model without updating both Rust and UI docs.
- Do not change `simulate_paste` from a spawned thread to synchronous execution — it causes a paste regression (see `PASTE_PROBLEM.md`).
- Keep API behavior stable where possible (`/api/entries*`, `/api/health`).
- After larger changes, bump `server/Cargo.toml` patch version so deployments are easy to verify via `/api/health` (and the admin UI version badge).
- Keep compose default image tag (`COPYWRAITH_SERVER_IMAGE_TAG`) aligned with the current server crate version.
- After bumping `server/Cargo.toml` version, run `scripts/sync-version.sh --write` to update all hardcoded version references (compose files, README, .env.example, redeploy script). Run without `--write` to check for drift.


## Server API

Base URL: `/api`

- `GET /health`
- `GET /auth/status`
- `POST /auth/setup`
- `POST /auth/unlock`
- `POST /auth/change-password`
- `POST /auth/lock`
- `POST /entries`
- `GET /entries`
- `GET /entries/{id}`
- `PATCH /entries/{id}`
- `DELETE /entries/{id}`
- `GET /entries/{id}/blob`

Interactive docs: `/swagger-ui/` (requires internet for CDN assets)
OpenAPI JSON: `/api-docs/openapi.json`

Notes:

- Auth endpoints (`/auth/status`, `/auth/setup`, `/auth/unlock`) do not require a password
- All `/entries*` endpoints require `Authorization: Bearer <password>` when a password is configured
- `/health` is always open
- `GET /entries` supports pagination/filtering/search via query params
  - `limit`, `offset`, `content_type`, `starred_only`, `search`
- Deduplication is based on `content_hash`
- Binary payloads are stored on disk in a blob directory keyed by hash

## Data and persistence

- Desktop client keeps its own SQLite + blob cache in Tauri app data directory
- Server keeps SQLite + blobs under `COPYWRAITH_DATA_DIR` (default `./data`)
- Password auth config stored in `{data_dir}/auth.json`; encrypted entries use `ENC:1:` prefix
- Both desktop and server deduplicate by content hash

## Platform notes

- Clipboard monitoring works via `tauri-plugin-clipboard`
- Paste simulation is currently implemented for macOS (via `osascript`)
- On non-macOS platforms, writing to clipboard works, but simulated keystroke paste is not fully implemented yet
- Mobile builds use `tauri-plugin-clipboard-manager`; tapping an entry copies it, and `capture_clipboard` persists the current clipboard when the app opens or resumes.

## Development notes

- Frontend dev server port is `1420` (Tauri expects this)
- SvelteKit is static-adapter based and runs client-side (`ssr = false`)

## Dependency release-age policy

- This repo now enforces npm package age gating with `min-release-age=3` in:
  - `.npmrc`
  - `server/ui/.npmrc`
- When install/update fails because a dependency is newer than 3 days, do not loop retries.
- Preferred handling order:
  1. wait for the age window to pass,
  2. pin to an older known-good version,
  3. temporarily override with `npm install --min-release-age=0` only for urgent fixes, then restore policy.
