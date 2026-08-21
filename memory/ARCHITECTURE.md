# Copywraith Architecture

## Overview

Copywraith is a clipboard manager with a client-server architecture. The desktop client monitors the system clipboard, captures all copied content (text, images, rich text, files), and syncs entries to a central server for permanent storage. A System 7-themed floating popup provides quick access to clipboard history with filtering, starring, and paste-as-plaintext support.

## System Architecture

```
+---------------------------+          +---------------------------+
|     Desktop Client        |          |       Server              |
|     (Tauri + Svelte)      |  HTTP    |       (Rust + Axum)       |
|                           | <------> |                           |
|  - Clipboard monitoring   |   REST   |  - REST API               |
|  - Local SQLite cache     |   API    |  - SQLite storage         |
|  - Global shortcuts       |          |  - Image/blob storage     |
|  - Floating paste popup   |          |  - Svelte admin UI        |
|  - system7-ui components  |          |  - Docker deployment      |
+---------------------------+          +---------------------------+
        ^                                        ^
        |                                        |
        v                                        v
+---------------------------+          +---------------------------+
|     Android Client        |          |     Web Browser           |
|     (Tauri Mobile)        |          |     (Server UI)           |
+---------------------------+          +---------------------------+
```

## Components

### 1. Server (`server/`)

A standalone Rust HTTP server deployed via Docker that provides permanent clipboard storage.

**Technology:**
- **Rust** with **Axum** web framework
- **SQLite** via `rusqlite` for metadata storage
- **File system** for binary blob storage (images, files)
- **Svelte + Vite** for the admin web UI (served as static files)
- **Docker** for deployment

**Responsibilities:**
- Store clipboard entries with metadata (timestamp, content type, hash, starred status)
- Serve a REST API for CRUD operations on clipboard entries
- Store binary data (images, files) on disk, referenced by content hash
- Provide full-text search across text entries
- Serve a web-based admin UI for browsing/managing entries
- De-duplicate entries by content hash

**API Endpoints:**
```
GET    /api/health           - Health check (always open)
GET    /api/auth/status      - Auth status (open)
POST   /api/auth/setup       - Create password (open)
POST   /api/auth/unlock      - Unlock with password (open)
POST   /api/auth/change-password - Change password (authed)
POST   /api/auth/lock        - Lock server (authed)
POST   /api/entries          - Create new clipboard entry
GET    /api/entries          - List entries (pagination, filtering, search via ?search=)
GET    /api/entries/:id      - Get single entry
PATCH  /api/entries/:id      - Update entry (star/unstar)
DELETE /api/entries/:id      - Delete entry
GET    /api/entries/:id/blob - Get binary content
```

Full-text search is a query parameter on `GET /api/entries?search=<term>`, not a
separate endpoint. Interactive API docs available at `/swagger-ui/` and raw
OpenAPI JSON at `/api-docs/openapi.json`.

**Data Model:**
```sql
CREATE TABLE entries (
    id TEXT PRIMARY KEY,           -- ULID for time-ordered IDs
    content_type TEXT NOT NULL,    -- "text", "image", "html", "rtf", "file"
    text_content TEXT,             -- Legacy column (kept for migration compat)
    text_plain TEXT,               -- Plain-text flavor
    text_html TEXT,                -- HTML flavor
    text_rtf TEXT,                 -- RTF flavor
    search_text TEXT,              -- Denormalized searchable text (FTS5 source)
    blob_hash TEXT,                -- SHA-256 hash for binary content
    blob_size INTEGER,             -- Size in bytes
    content_hash TEXT NOT NULL,    -- SHA-256 of content (dedup key)
    source_app TEXT,               -- Application that produced the copy
    starred INTEGER DEFAULT 0,     -- Boolean: starred entry
    sensitive INTEGER DEFAULT 0,   -- Boolean: contains sensitive data
    created_at TEXT NOT NULL,      -- ISO 8601 timestamp
    updated_at TEXT NOT NULL       -- ISO 8601 timestamp
);

CREATE UNIQUE INDEX idx_entries_content_hash ON entries(content_hash);
CREATE INDEX idx_entries_created_at ON entries(created_at DESC);
CREATE INDEX idx_entries_updated_at ON entries(updated_at DESC);
CREATE INDEX idx_entries_starred ON entries(starred) WHERE starred = 1;
CREATE INDEX idx_entries_content_type ON entries(content_type);

-- Full-text search via SQLite FTS5 with auto-sync triggers
CREATE VIRTUAL TABLE entries_fts USING fts5(
    search_text, content='entries', content_rowid='rowid'
);
-- INSERT/UPDATE/DELETE triggers keep entries_fts in sync
```

### 2. Desktop Client (`src/` + `src-tauri/`)

A Tauri v2 desktop application with a Svelte 5 frontend using system7-ui.

**Technology:**
- **Tauri v2** for the desktop shell
- **Svelte 5** + **SvelteKit** (static adapter) for the UI
- **system7-ui** for System 7-themed components
- **tauri-plugin-clipboard** for clipboard monitoring and read/write
- **tauri-plugin-global-shortcut** for hotkeys
- **SQLite** (via Tauri) for local cache

**Responsibilities:**
- Monitor system clipboard for changes (text, images, HTML, RTF, files)
- Sync clipboard entries to the server
- Provide a floating paste popup triggered by global hotkey
- Filter/search clipboard history
- Star/unstar entries
- Paste entries (with option for plaintext)
- Maintain a local cache for offline access

### 3. Android Client (partially implemented)

Tauri v2 mobile support is partially implemented. The Svelte UI includes platform detection (`src/lib/util/platform.ts`) with mobile-specific adaptations: no title bar, larger touch targets, safe area insets, "Tap to copy" UX instead of paste simulation, and a `capture_clipboard` command. Android dev scripts exist in `scripts/`. Not yet production-tested.

## Desktop Client Architecture

### Window Management

The app uses a single window:

- **Popup Window**: The floating paste popup triggered by global hotkey, converted to an NSPanel on macOS for non-activating behavior

Settings, server configuration, and preferences are accessed via a **modal dialog** (`SettingsDialog.svelte` using `MovableDialog`) within the popup window — not a separate window.

### Global Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd+Shift+V` | Show/hide paste popup (all entries) |
| `Cmd+Shift+B` | Show/hide paste popup (starred only) |
| `Cmd+Shift+Alt+V` | Paste most recent as plaintext |

### Paste Popup UI

```
+-----------------------------------------------+
| [TitleBar: Copywraith]                        |
|-----------------------------------------------|
| [Filter: ________________________________]    |
|-----------------------------------------------|
| [*] Screenshot.png          [img preview] 2m  |
| [ ] Hello world             [text]        5m  |
| [*] <h1>Title</h1>         [html]       12m  |
| [ ] Long text that gets...  [text]       1h   |
| [ ] Another image           [img preview] 2h  |
|-----------------------------------------------|
| Click to paste | Opt+Click for plaintext      |
+-----------------------------------------------+
```

### Data Flow

**Phase 1: Clipboard Capture (immediate)**
```
Clipboard Change
    |
    v
tauri-plugin-clipboard (monitor)
    |
    v
Rust: clipboard_changed event
    |
    +---> Store in local SQLite cache + blob store
    |
    +---> Emit "clipboard-updated" event to frontend (update UI)
```

**Phase 2: Background Sync (every ~5s, async)**
```
Sync loop (background timer)
    |
    +---> Push: find local unsynced entries → POST to server API
    |
    +---> Pull: fetch new server entries (cursor-based) → insert into local DB
    |
    +---> Emit "clipboard-updated" if new entries pulled
```

Sync is completely decoupled from clipboard capture. The capture flow only
writes to local storage and notifies the UI. Server communication happens
asynchronously in the background sync loop with exponential backoff on failure.

### Paste Flow

```
User selects entry in popup
    |
    v
Frontend sends paste command to Rust
    |
    v
Rust: Write to system clipboard
    |
    +---> Hide popup window (async, dispatched to main thread)
    |
    +---> Simulate Cmd+V keystroke (spawned thread: sleep 100ms, then osascript)
```

The paste simulation runs in a **spawned background thread** so the Tauri
command returns immediately after writing to the clipboard and hiding the
popup. The thread sleeps 100ms to allow the popup hide and focus restoration
to complete before sending the Cmd+V keystroke. Running `simulate_paste`
synchronously on the Tauri runtime causes a regression where the keystroke
arrives before the target app is ready.

## Directory Structure

```
Copywraith/
├── ARCHITECTURE.md
├── IMPLEMENTATION.md
├── LICENSE
├── Cargo.toml                    # Rust workspace root
├── package.json                  # Node project root
├── svelte.config.js
├── vite.config.js
├── tsconfig.json
├── src/                          # SvelteKit frontend
│   ├── app.html
│   ├── routes/
│   │   ├── +layout.svelte
│   │   ├── +layout.ts
│   │   └── +page.svelte          # Main popup page
│   └── lib/
│       ├── tauri.ts              # TauriService wrapper
│       ├── types.ts              # Shared types
│       ├── windowManager.ts      # Window operations
│       ├── util/
│       │   ├── notifications.ts
│       │   ├── windowState.ts
│       │   ├── clipboardStore.ts   # Reactive clipboard state
│       │   ├── platform.ts         # Platform detection (isMobile store)
│       │   └── syncStatusStore.ts  # Sync endpoint status store
│       └── components/
│           ├── FilterBar.svelte
│           ├── EntryList.svelte
│           ├── EntryRow.svelte
│           ├── EntryPreview.svelte
│           ├── SettingsDialog.svelte
│           └── StatusBar.svelte
├── src-tauri/                    # Tauri desktop client
│   ├── tauri.conf.json
│   ├── Cargo.toml
│   ├── capabilities/
│   │   └── default.json
│   └── src/
│       ├── main.rs
│       ├── lib.rs                # Plugin setup, commands
│       ├── commands.rs           # Tauri command handlers
│       ├── clipboard.rs          # Clipboard monitoring logic
│       ├── models.rs             # Data models
│       ├── storage.rs            # Local SQLite cache
│       ├── sync.rs               # Server sync logic
│       └── paste.rs              # Paste simulation
├── server/                       # Standalone server
│   ├── Cargo.toml
│   ├── Dockerfile
│   ├── docker-compose.yml
│   ├── src/
│   │   ├── main.rs               # Server bootstrap, routes, Swagger UI
│   │   ├── api.rs                # Axum route handlers
│   │   ├── crypto.rs             # Password auth, encryption (Argon2id + AES-256-GCM)
│   │   └── storage.rs            # SQLite + blob storage + FTS5 search
│   └── ui/                       # Server admin UI (Svelte + Vite SPA)
│       ├── package.json
│       ├── vite.config.js
│       ├── index.html
│       └── src/
│           ├── main.js
│           ├── App.svelte
│           └── lib/
│               ├── api.ts
│               ├── types.ts
│               ├── EntryRow.svelte
│               └── EntryDetail.svelte
├── crates/
│   └── copywraith-core/          # Shared library
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── models.rs         # Shared data models
│           ├── api_types.rs      # API request/response types
│           ├── content.rs        # Content hashing, type detection
│           └── sensitive.rs      # Heuristic sensitive data detection
└── static/
    └── favicon.png
```

## Content Type Handling

When content is copied to the clipboard, the system captures **all available
formats simultaneously** via the `ClipboardFlavors` struct. A single clipboard
event may produce entries with multiple flavor columns populated.

| Clipboard Content | content_type | Primary Storage | Additional Flavors |
|-------------------|-------------|-----------------|-------------------|
| Plain text | `text` | `text_plain` column | — |
| HTML | `html` | `text_html` column | `text_plain` (fallback) |
| Rich Text (RTF) | `rtf` | `text_rtf` column | `text_plain` (fallback) |
| Image (PNG/JPEG) | `image` | Blob storage (file system) | — |
| File reference | `file` | `text_plain` (file path) | — |

The `content_type` field reflects the highest-priority format captured (priority:
Image > File > HTML > RTF > Text). The `search_text` column stores a
denormalized plain-text representation for FTS5 indexing.

## Deduplication

Entries are deduplicated by content hash:
- Text content: SHA-256 of the UTF-8 bytes
- Binary content: SHA-256 of the raw bytes
- When a duplicate is detected, the existing entry's timestamp is updated (moved to top) rather than creating a new entry

## Security

- **Password authentication**: Single-user, password-only auth (no usernames). Password
  hashed with Argon2id (64 MiB memory, 3 iterations, parallelism 4).
- **At-rest encryption**: Master key derived via HKDF splits into auth key (verification)
  and KEK (key encryption key). Random 256-bit DEK encrypted with KEK, stored in
  `{data_dir}/auth.json`. Text entries encrypted with AES-256-GCM (`ENC:1:` prefix);
  blobs encrypted with `ENCB` header. Password change re-wraps DEK — no data
  re-encryption needed.
- **Transport**: Password sent as `Authorization: Bearer <password>` header. No built-in
  TLS; production deployments should use a reverse proxy with HTTPS.
- **Local storage**: Client cache is stored in the Tauri app data directory.
- **Sensitive data detection**: Heuristic detection of credit cards, SSNs, API keys,
  PEM keys, JWTs, etc. (see `SENSITIVE.md`). Flagged entries are masked in UI.
- See `ENCRYPTION.md` for full cryptographic design details.
