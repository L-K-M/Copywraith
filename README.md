# Copywraith

Copywraith is a local-first clipboard manager with an optional sync server for durable, searchable clipboard history across devices.

It has three main pieces:

- [Server](server/README.md): Rust/Axum API, SQLite storage, encrypted blobs, and Svelte admin UI.
- [Mac app](README.mac.md): Tauri v2 + Svelte popup app with clipboard capture, search, hotkeys, and paste helpers.
- [Android app](README.android.md): Tauri mobile app with foreground clipboard capture, share-sheet import, and optional Shizuku listener.

> [!IMPORTANT]
> LLM Disclosure: Much of this code base was written with the help of large language models — AI coding agents working from the [`AGENTS.md`](memory/AGENTS.md) brief in this repo.

## Important Security Note

Do not expose the Copywraith server directly to the Internet. It is intended for a trusted local network or secure VPN such as Tailscale or Netbird. The server is single-user and password-protected, but it does not implement rate limiting or brute-force protection.

## Quick Start

Install shared frontend dependencies from the repository root:

```bash
npm install
```

Run the server:

```bash
cargo run -p copywraith-server
```

Run the Mac desktop app:

```bash
npm run tauri dev
```

Optional repository checks:

```bash
cargo check --workspace
cargo test --workspace
npm run build
```

## Repository Layout

```text
crates/copywraith-core/   Shared models, API types, hashing, and content helpers
server/                   Rust server, persistence, encryption, and admin UI
src-tauri/                Tauri Rust backend for desktop and mobile apps
src/                      Svelte popup frontend
scripts/                  Android setup helpers and server deployment scripts
```

## More Documentation

- [Architecture](ARCHITECTURE.md)
- [Implementation notes](IMPLEMENTATION.md)
- [Encryption design](ENCRYPTION.md)
- [API notes](API.md)
- [Sensitive data notes](SENSITIVE.md)
