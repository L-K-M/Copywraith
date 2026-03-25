# Copywraith

Copywraith is a clipboard manager with with a server component that allows synchronization and long-term storage of clipboard history.

# Important Security Consideration

Do not expose the server component to the Internet. It is intended to be used on a local network or with a secure VPN (like Tailscale or Netbird) between your devices. The server does not implement any rate limiting or brute-force protection, so it is vulnerable to password guessing attacks if exposed publicly. Always use a strong, unique password and consider additional network-level protections if you need remote access.

## Prerequisites

- Rust toolchain (stable; project currently builds with Rust 1.83+)
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
- `RUST_LOG`
- `COPYWRAITH_ADMIN_API_KEY` (legacy bearer token; ignored when a password is configured)

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

Alternatively from `server/`:

```bash
cd server
docker compose up --build
```

This exposes port `3742` and persists server data in Docker volume `copywraith-data`.



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

## Quick troubleshooting

- **`npm install` fails with registry/package errors**
  - verify network access and npm registry settings
- **Tauri fails to launch webview**
  - verify OS prerequisites from Tauri docs
- **Entries not syncing**
  - check Settings -> `Primary Server URL` / `Fallback Server URL`
  - verify server is reachable and running on expected port
