# Implementation notes

A module-level map of the codebase, complementing the high-level
[ARCHITECTURE.md](ARCHITECTURE.md). Paths are from the repo root.

## Workspace layout

```text
crates/copywraith-core/        Shared models, API types, hashing, content & sensitive helpers
crates/copywraith-share-target/ Android share-sheet + Shizuku plugin (Rust + Kotlin)
server/                        Axum API, SQLite/blob persistence, encryption, admin UI
src-tauri/                     Tauri Rust backend (desktop + mobile)
src/                           Svelte 5 popup frontend (shared by desktop & mobile)
server/ui/                     Svelte SPA admin UI
scripts/                       Android bootstrap/env + server redeploy/version helpers
```

## Shared core (`crates/copywraith-core`)

- `models.rs` — `ClipboardEntry`, `ClipboardFlavors`, `ContentType`; flavor
  reconciliation (`merge_legacy`, `to_legacy_text_content`, `best_plain_text`)
  and `payload_hash` (the dedup key).
- `content.rs` — SHA-256 hashing, base64, image-format sniffing, and
  HTML/RTF → plain-text stripping used for previews and search.
- `sensitive.rs` — secret detection (see [SENSITIVE.md](SENSITIVE.md)).
- `api_types.rs` — request/response and list-query types shared by server and
  clients, including the cursor pagination params.

## Server (`server/`)

- `main.rs` — process bootstrap: config from env (`COPYWRAITH_HOST`, `PORT`,
  `COPYWRAITH_DATA_DIR`, `COPYWRAITH_UI_DIR`, `COPYWRAITH_MAX_BODY_BYTES`),
  router assembly, static admin UI serving, Swagger UI.
- `api.rs` — Axum handlers for auth (`setup`/`unlock`/`change-password`/`lock`)
  and entries (`create`/`list`/`get`/`patch`/`delete`/`blob`). Auth is a single
  password supplied as `Authorization: Bearer <password>`.
- `storage.rs` — SQLite schema, blob files, FTS5 search, and the cursor/offset
  list query (`ORDER BY updated_at DESC, id DESC`).
- `crypto.rs` — at-rest encryption (see [ENCRYPTION.md](ENCRYPTION.md)).

### List pagination

`GET /api/entries` supports both offset pagination (`limit`/`offset`) and stable
**keyset** pagination via `before_updated_at` + `before_id` (descending by
`(updated_at, id)`). Keyset is what the sync client uses, because it doesn't
drift when rows are inserted/updated concurrently.

## Desktop / mobile backend (`src-tauri/`)

- `lib.rs` — app setup, the Tauri command registry, the background sync loop,
  and (desktop) global-shortcut registration + the macOS NSPanel popup plumbing.
- `clipboard.rs` *(desktop)* — Rust-owned native clipboard monitor. Reads in
  priority order `Image > File > Html > Rtf > Text` and suppresses the monitor
  briefly around our own paste writes so we don't re-capture them.
- `paste.rs` *(desktop)* — writes the chosen entry to the clipboard and
  simulates Cmd+V via `osascript` on a spawned thread, tracking and restoring
  the previously-focused app. **Must stay asynchronous** (running it inline
  races the popup hide / focus restoration).
- `commands.rs` — Tauri commands: `get_entries`, `toggle_star`, `delete_entry`,
  `paste_entry[_plaintext]`, `capture_clipboard` (mobile), share-sheet import
  (Android), `sync_now`, `reset_sync_cursor`, Shizuku controls, settings.
- `storage.rs` — local SQLite cache + blob store, settings, and the persisted
  sync watermark.
- `sync.rs` — the two-way sync client (below).
- `models.rs` — `Settings` and the `EntryForFrontend` projection sent to the UI
  (with sensitive text masked).

### Sync

Source: `src-tauri/src/lib.rs` (loop) and `src-tauri/src/sync.rs`.

- A background loop runs roughly every 5s (backing off to ~120s while a server
  is unreachable), pushing local unsynced entries then pulling remote ones.
- **Push** (`sync_unsynced_entries`/`sync_entry`): uploads entries with
  `synced = 0` to `POST /api/entries`; on success marks them `synced = 1`.
- **Pull** (`pull_new_entries`): walks `GET /api/entries` newest-first using
  keyset pagination and ingests entries the local DB doesn't already have (by
  `content_hash`), downloading blobs as needed. It tracks an `(updated_at, id)`
  high **watermark** so it only processes entries newer than the last pull;
  comparing the full key (not just an id) keeps the stop position stable even
  when an entry's `updated_at` changes (re-copy/re-star moving it back to the
  top). The watermark advances only forward and only after a pass with no
  blocking error; a blob that a reachable server can no longer provide is
  skipped rather than pinning the watermark.
- **Endpoints**: two configurable URLs (`server_url_primary`,
  `server_url_fallback`); sync tries primary first, then the fallback, and
  remembers which one last responded.
- A `clipboard-updated` event tells the UI to refresh after a pull.

## Frontend (`src/`)

- `routes/+page.svelte` — top-level popup; wires events, and on mobile drives
  the capture → import → sync → reload refresh flow with progress UI.
- `lib/util/clipboardStore.ts` — entry list state, paged loading
  (`loadEntries`/`loadMoreEntries`), starred filter, search debounce, selection.
- `lib/components/` — `FilterBar`, `EntryList`, `EntryRow`, `EntryPreview`,
  `SettingsDialog`, `StatusBar`.
- `lib/util/platform.ts` — `platform`/`isMobile` stores that adapt the UI.

The same frontend runs on desktop and mobile; platform branches hide
desktop-only affordances (shortcuts, paste simulation) on mobile, where tapping
an entry copies it to the system clipboard instead of simulating a paste.

## Admin UI (`server/ui/`)

A plain Svelte + Vite SPA (not SvelteKit) built to `server/ui/dist/` and served
by the server at `/`. Uses `@lkmc/system7-ui` components.

## Build & test

See `README.md` and `memory/AGENTS.md` for the full command list. Quick checks:

```bash
cargo check --workspace
cargo test --workspace
npm run check && npm run build
```
</content>
