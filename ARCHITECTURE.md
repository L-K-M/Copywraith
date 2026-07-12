# Architecture

Copywraith is a local-first clipboard manager with an optional sync server. This
document gives a high-level map of how the pieces fit together. For module-level
detail see [IMPLEMENTATION.md](IMPLEMENTATION.md); for the crypto design see
[ENCRYPTION.md](ENCRYPTION.md); for sensitive-data handling see
[SENSITIVE.md](SENSITIVE.md).

## Components

```text
┌─────────────────────┐      ┌─────────────────────┐
│  Desktop app (macOS) │      │   Android app        │
│  Tauri v2 + Svelte 5 │      │  Tauri mobile + Svelte│
│                      │      │                      │
│  clipboard monitor   │      │  foreground capture  │
│  paste simulation    │      │  share-sheet import  │
│  global shortcuts    │      │  optional Shizuku     │
│  local SQLite + blobs│      │  local SQLite + blobs│
└──────────┬──────────┘      └──────────┬──────────┘
           │  HTTP (Bearer password)    │
           └─────────────┬──────────────┘
                         ▼
              ┌─────────────────────┐
              │  Server (Rust/Axum)  │
              │  SQLite + blob store  │
              │  at-rest encryption   │
              │  Svelte admin UI      │
              └─────────────────────┘
```

All three share the [`copywraith-core`](crates/copywraith-core) crate: the
`ClipboardEntry`/`ClipboardFlavors`/`ContentType` models, API request/response
types, content hashing, HTML/RTF-to-text helpers, and sensitive-data detection.

## Data model

A clipboard entry (`copywraith_core::models::ClipboardEntry`) carries:

- `id` — a ULID (lexicographically sortable, time-ordered).
- `content_type` — `text | html | rtf | image | file`.
- `flavors` (`ClipboardFlavors`) — optional `text_plain`, `text_html`,
  `text_rtf`, and `file_list`. A single clipboard event can carry several
  flavors at once (e.g. rich-text copy → plain + HTML). `text_content` is a
  legacy single-field representation kept for backward compatibility; helpers
  reconcile the two (`merge_legacy`, `to_legacy_text_content`).
- `blob_hash` / `blob_size` — for images and files, the payload is stored as a
  content-addressed blob (filename = SHA-256 hex of the bytes), not inline.
- `starred`, `sensitive`, `created_at`, `updated_at`.

### Deduplication

Every entry has a `content_hash`. Identical payloads collapse to one row via a
unique index on `content_hash` (client and server both). Re-copying existing
content updates `updated_at` (bringing it to the top) instead of inserting a
duplicate. See `ClipboardFlavors::payload_hash` for how the hash is derived
(single-flavor entries keep a stable legacy hash; multi-flavor entries hash a
serialized payload).

## Capture → store → sync flow

1. **Capture.** Desktop owns a native clipboard monitor in Rust
   (`src-tauri/src/clipboard.rs`); content is read in priority order
   `Image > File > Html > Rtf > Text`. Android captures on app open/resume, via
   the share sheet, or via the optional Shizuku listener.
2. **Normalize & dedup.** The entry is normalized into flavors and hashed.
3. **Store locally.** Written to a local SQLite DB plus a blob directory.
4. **Sync.** A background loop pushes locally-unsynced entries to the server and
   pulls entries created on other devices into local storage. See
   [the sync section of IMPLEMENTATION.md](IMPLEMENTATION.md#sync).

## Server

A Rust/Axum service (`server/`) exposing `/api/*` (see [API.md](API.md)) backed
by SQLite + a blob directory, with FTS5 full-text search over plaintext entries.
It is single-user and password-protected; when a password is configured, text
and blobs are encrypted at rest ([ENCRYPTION.md](ENCRYPTION.md)). A Svelte SPA
admin UI is served at `/`.

> [!IMPORTANT]
> The server has no rate limiting or brute-force protection. Run it only on a
> trusted LAN or a VPN such as Tailscale/Netbird — never directly on the public
> Internet.

## Security posture (summary)

- Transport: plain HTTP is expected to run over a trusted network/VPN.
- At rest (server): Argon2id-derived key wrapping a random data key; AES-256-GCM
  for text and blobs.
- Sensitive content (API keys, tokens, card numbers, etc.) is detected and
  masked before it reaches the UI, on both server and client.
</content>
