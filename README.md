# Copywraith

Copywraith is a local-first clipboard manager with:

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

Planned later:

- Android/mobile client
- Stronger server auth model (the client can send a bearer token, but server-side validation is not enforced yet)

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
├── src-tauri/                # Tauri Rust backend (monitoring, commands, sync, shortcuts)
├── src/                      # Svelte popup frontend
├── ARCHITECTURE.md
└── IMPLEMENTATION.md
```

## Prerequisites

- Rust toolchain (stable; project currently builds with Rust 1.83+)
- Node.js + npm
- Tauri v2 system dependencies for your OS

Tauri dependency guide:

- https://v2.tauri.app/start/prerequisites/

### UI dependency

`@lkmc/system7-ui` is installed from npm during `npm install`.

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

### 1) Start the server

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
- `RUST_LOG`

### 2) Start the desktop app

From repository root:

```bash
npx tauri dev
```

The popup window starts hidden. Use the hotkeys below to open it.

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

## Server API

Base URL: `/api`

- `GET /health`
- `POST /entries`
- `GET /entries`
- `GET /entries/{id}`
- `PATCH /entries/{id}`
- `DELETE /entries/{id}`
- `GET /entries/{id}/blob`

Notes:

- `GET /entries` supports pagination/filtering/search via query params
  - `limit`, `offset`, `content_type`, `starred_only`, `search`
- Deduplication is based on `content_hash`
- Binary payloads are stored on disk in a blob directory keyed by hash

## Docker (server)

From repo root (recommended):

```bash
docker compose up --build
```

Alternatively from `server/`:

```bash
cd server
docker compose up --build
```

This exposes port `3742` and persists server data in Docker volume `copywraith-data`.

## Data and persistence

- Desktop client keeps its own SQLite + blob cache in Tauri app data directory
- Server keeps SQLite + blobs under `COPYWRAITH_DATA_DIR` (default `./data`)
- Both desktop and server deduplicate by content hash

## Platform notes

- Clipboard monitoring works via `tauri-plugin-clipboard`
- Paste simulation is currently implemented for macOS (via `osascript`)
- On non-macOS platforms, writing to clipboard works, but simulated keystroke paste is not fully implemented yet

## Development notes

- Frontend dev server port is `1420` (Tauri expects this)
- SvelteKit is static-adapter based and runs client-side (`ssr = false`)
- Main docs:
  - `ARCHITECTURE.md`
  - `IMPLEMENTATION.md`

## Quick troubleshooting

- **`npm install` fails with registry/package errors**
  - verify network access and npm registry settings
- **Tauri fails to launch webview**
  - verify OS prerequisites from Tauri docs
- **Entries not syncing**
  - check Settings -> `Primary Server URL` / `Fallback Server URL`
  - verify server is reachable and running on expected port
