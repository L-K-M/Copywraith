# Copywraith Mac App

The Mac app is a Tauri v2 + Svelte clipboard popup. It captures clipboard changes locally, stores them in a SQLite cache, and can sync with the Copywraith server.

## Features

- Captures text, HTML, RTF, images, and files.
- Deduplicates entries by SHA-256 content hash.
- Searches and filters local clipboard history.
- Stars, previews, pastes, and deletes entries.
- Syncs with a primary and optional fallback server endpoint.
- Uses a floating retro System 7-style popup UI.

## Prerequisites

- Rust 1.85 or newer; this repository pins the toolchain in `rust-toolchain.toml`.
- Node.js and npm.
- Tauri v2 prerequisites for macOS.

Tauri setup guide: https://v2.tauri.app/start/prerequisites/

## Install

From the repository root:

```bash
npm install
```

Optional checks:

```bash
cargo check --workspace
cargo test --workspace
npm run build
```

## Run In Development

From the repository root:

```bash
npm run tauri dev
```

With Rust logs:

```bash
RUST_LOG=debug npm run tauri dev
```

The popup starts hidden. Use the configured hotkeys to open it.

## Build

```bash
npm run tauri build
```

The bundled output is produced by Tauri under `src-tauri/target/`.

## Configure Sync

Start the server first, then open Settings in the Mac app with `Cmd+,`.

Set:

- `Primary Server URL`: first endpoint to try, for example `http://192.168.1.5:3742`
- `Fallback Server URL`: optional backup endpoint, for example a Tailscale IP
- `API Key`: the server password

After saving, sync runs in both directions roughly every 5 seconds:

- Local unsynced entries are uploaded to the server.
- New server entries from other devices are pulled into local history.
- The status bar shows the active sync endpoint and unreachable endpoint state.

## Hotkeys

Default global shortcuts:

- `Cmd + Shift + V`: toggle popup
- `Cmd + Shift + B`: open popup with starred-only filter enabled
- `Cmd + Shift + Alt + V`: paste most recent item as plaintext

Inside the list:

- `Click`: paste selected entry
- `Alt + Click`: paste as plaintext
- `Double-click`: open entry preview dialog
- `Space` on a focused row: open entry preview dialog
- `Enter` on a focused row: paste

Shortcuts can be changed in Settings.

## Clipboard And Paste Behavior

Clipboard monitoring is owned by the Rust backend. The app listens for native clipboard monitor events and reads clipboard content in this priority order:

```text
Image > File > HTML > RTF > Text
```

On macOS, simulated paste uses `osascript` from a background thread so the popup can hide and focus can return to the previous app before `Cmd+V` is sent.

## Troubleshooting

- Tauri fails to launch the webview: verify macOS prerequisites from the Tauri guide.
- Entries are not syncing: verify the server is reachable and Settings contains the correct server URL and password.
- Paste happens in the wrong app: retry after confirming macOS accessibility permissions for the app or development binary.
- Svelte or Vite package errors: run `npm install` from the repository root, then `npm run build`.
