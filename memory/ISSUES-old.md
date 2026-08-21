# Known Issues

Comprehensive code review performed across the entire codebase, comparing
implementation against AGENTS.md, README.md, ARCHITECTURE.md, IMPLEMENTATION.md,
ENCRYPTION.md, and SENSITIVE.md. Issues are ordered by severity within each
section.

---

## Section A -- Documentation Discrepancies

Discrepancies between what documentation claims and what the code actually does.

### D1. Docker image tag out of sync with server crate version

- **Status:** FIXED
- **Severity:** Medium
- **Files:** `docker-compose.yml:3`, `server/docker-compose.yml:3`, `server/Cargo.toml:3`, `README.md:111`, `scripts/redeploy-server-docker.sh:20`
- **Description:** `server/Cargo.toml` declares version `0.1.5`, but both
  `docker-compose.yml` (root, line 3) and `server/docker-compose.yml` (line 3)
  default to tag `0.1.4`. README.md line 111 also references `0.1.4`. The
  `redeploy-server-docker.sh` script comment on line 20 shows `0.1.4` as the
  example default. AGENTS.md explicitly says: "Keep compose default image tag
  aligned with the current server crate version."
- **Fix:** Updated all references to `0.1.5`. Created `scripts/sync-version.sh`
  to detect and fix version drift automatically.

### D2. ARCHITECTURE.md lists nonexistent server source files

- **Status:** FIXED
- **Severity:** Medium
- **Files:** `ARCHITECTURE.md:235-250`
- **Description:** The directory structure in ARCHITECTURE.md claims
  `server/src/` contains `models.rs` and `search.rs`. Neither file exists.
  Models are defined in `crates/copywraith-core/src/models.rs`. Search is
  implemented inline within `server/src/storage.rs` (FTS5 + in-memory
  fallback). The file also omits `server/src/crypto.rs`, which is a major
  module implementing all authentication and encryption logic.
- **Fix:** Removed `models.rs` and `search.rs`; added `crypto.rs` with
  description.

### D3. ARCHITECTURE.md missing `sensitive.rs` from shared crate

- **Status:** FIXED
- **Severity:** Low
- **Files:** `ARCHITECTURE.md:253-258`
- **Description:** The directory listing for `crates/copywraith-core/src/` omits
  `sensitive.rs`, which contains all heuristic sensitive data detection logic
  (credit cards, SSNs, API keys, PEM keys, JWTs, etc.). This is a significant
  feature module.
- **Fix:** Added `sensitive.rs` to the directory listing with description.

### D4. ARCHITECTURE.md missing frontend utility files

- **Status:** FIXED
- **Severity:** Low
- **Files:** `ARCHITECTURE.md:206-208`
- **Description:** `src/lib/util/` listing omits `platform.ts` (platform
  detection store, `isMobile` derived store) and `syncStatusStore.ts` (sync
  endpoint state store). Both are actively used by the popup UI.
- **Fix:** Added both files to the directory listing.

### D5. ARCHITECTURE.md claims separate `/api/search?q=` endpoint

- **Status:** FIXED
- **Severity:** Medium
- **Files:** `ARCHITECTURE.md:58`
- **Description:** The API endpoints table lists `GET /api/search?q=` as a
  standalone endpoint. This does not exist. Search is a query parameter on
  `GET /api/entries?search=<term>`. AGENTS.md and API.md correctly document
  this.
- **Fix:** Removed `/api/search?q=`; added note that search is a parameter
  on `GET /api/entries`. Also added auth endpoints and Swagger/OpenAPI docs.

### D6. ARCHITECTURE.md security section is significantly outdated

- **Status:** FIXED
- **Severity:** High
- **Files:** `ARCHITECTURE.md:280-285`
- **Description:** The security section says "Server API can optionally require
  an API key via `Authorization: Bearer <key>` header." The actual system uses
  mandatory password-based authentication with Argon2id key derivation and
  AES-256-GCM at-rest encryption. The `COPYWRAITH_ADMIN_API_KEY` env var was
  removed long ago. AGENTS.md and ENCRYPTION.md correctly describe the current
  system.
- **Fix:** Rewrote security section to describe password auth, encryption at
  rest, sensitive data detection, and reference ENCRYPTION.md.

### D7. ARCHITECTURE.md data model schema is outdated

- **Status:** FIXED
- **Severity:** Medium
- **Files:** `ARCHITECTURE.md:63-79`
- **Description:** The SQL schema shown in ARCHITECTURE.md is missing several
  columns that exist in both client and server databases:
  - `content_hash TEXT NOT NULL` with `UNIQUE` index (critical for dedup)
  - `sensitive INTEGER DEFAULT 0`
  - `text_plain TEXT`, `text_html TEXT`, `text_rtf TEXT` (multi-flavor support)
  - `search_text TEXT` (FTS5 source column)
  - `synced INTEGER DEFAULT 0` (client only)
  - `updated_at` index

  The schema also doesn't mention the FTS5 virtual table or triggers.
- **Fix:** Updated schema to match actual `CREATE TABLE` in
  `server/src/storage.rs`, including all columns, indexes, FTS5 table, and
  triggers.

### D8. ARCHITECTURE.md two-window claim is incorrect

- **Status:** FIXED
- **Severity:** Low
- **Files:** `ARCHITECTURE.md:108-113`
- **Description:** Claims the app uses two windows: "1. Main Window (hidden by
  default): Settings, server configuration, preferences. 2. Popup Window: The
  floating paste popup." In reality, `tauri.conf.json` defines only one window
  (`popup`). Settings is a modal `MovableDialog` within the popup, not a
  separate window.
- **Fix:** Updated to describe single popup window with NSPanel conversion on
  macOS, and settings as a modal dialog.

### D9. ARCHITECTURE.md describes Android as "future" but it's partially implemented

- **Status:** FIXED
- **Severity:** Low
- **Files:** `ARCHITECTURE.md:102-104`
- **Description:** Says "Android Client (future) -- Tauri v2 supports mobile
  targets. The same Svelte UI can be compiled for Android with platform-specific
  adaptations." In reality, mobile support is already implemented: platform
  detection, mobile-specific UI (no title bar, larger touch targets, safe area
  insets, "Tap to copy" UX), `capture_clipboard` command, and Android dev
  scripts exist in `scripts/`.
- **Fix:** Updated to describe current partial implementation state.

### D10. ARCHITECTURE.md omits multi-flavor clipboard support

- **Status:** FIXED
- **Severity:** Medium
- **Files:** `ARCHITECTURE.md:265-271`
- **Description:** The content type handling table only shows single-format
  storage. The actual implementation stores multiple clipboard formats
  simultaneously (e.g., HTML + plaintext together) via the `ClipboardFlavors`
  struct with `text_plain`, `text_html`, `text_rtf`, and `file_list` fields.
  This is a significant architectural feature not documented in ARCHITECTURE.md.
- **Fix:** Documented multi-flavor storage model, `ClipboardFlavors` struct,
  and priority-based content_type assignment.

### D11. ARCHITECTURE.md data flow diagram is incomplete

- **Status:** FIXED
- **Severity:** Low
- **Files:** `ARCHITECTURE.md:142-157`
- **Description:** The data flow diagram shows clipboard changes being POSTed
  to the server API as part of the capture flow. In reality, sync is a separate
  background loop (every ~5s with exponential backoff) that pushes unsynced
  entries and pulls remote entries. The capture flow only writes to local
  storage and emits a UI event; server sync happens asynchronously.
- **Fix:** Replaced with two-phase diagram: Phase 1 (capture to local) and
  Phase 2 (background sync), with explanatory text.

### D12. IMPLEMENTATION.md references "API key authentication (optional)"

- **Status:** FIXED
- **Severity:** Low
- **Files:** `IMPLEMENTATION.md:49`
- **Description:** Phase 3.2 says "API key authentication (optional)". The
  actual system uses mandatory password authentication with Argon2id + AES-256-GCM.
- **Fix:** Updated to "Password authentication with at-rest encryption
  (Argon2id + AES-256-GCM)". Also fixed two-window reference and API key
  reference in Settings section.

### D13. AGENTS.md says "Android/mobile client not implemented yet"

- **Status:** FIXED
- **Severity:** Low
- **Files:** `AGENTS.md` (Known gaps section)
- **Description:** Lists "Android/mobile client not implemented yet" as a known
  gap, but mobile support is already partially implemented with platform-specific
  code paths, UI adaptations, and helper scripts.
- **Fix:** Updated to "Android/mobile client is partially implemented ... but
  not yet production-tested."

### D14. Swagger UI / OpenAPI endpoints not documented in AGENTS.md

- **Status:** FIXED
- **Severity:** Low
- **Files:** `server/src/main.rs:113-145`
- **Description:** The server serves Swagger UI at `/swagger-ui/` and OpenAPI
  JSON at `/api-docs/openapi.json`. These are documented in `API.md` but not
  in AGENTS.md's server API section. AGENTS.md is the primary reference for
  agents.
- **Fix:** Added Swagger UI and OpenAPI JSON lines to the AGENTS.md server API
  section.

---

## Section B -- Bugs and Correctness Issues

### B1. FTS5 index rebuilt on every server startup

- **Status:** FIXED
- **Severity:** Medium
- **Files:** `server/src/storage.rs` (Storage::new, rebuild_entries_fts)
- **Description:** `rebuild_entries_fts()` is called unconditionally in
  `Storage::new()`. It drops and recreates the FTS5 virtual table, triggers,
  and re-indexes all rows on every server start. For a database with thousands
  of entries, this could cause significant startup delay. The `CREATE VIRTUAL
  TABLE IF NOT EXISTS` in the initial schema is effectively dead code since the
  table is immediately dropped and recreated.
- **Recommendation:** Only rebuild FTS when schema changes are detected (e.g.,
  track a schema version in a metadata table). For routine starts, the existing
  FTS table and triggers should be sufficient.
- **Fix:** Added a `metadata` table with `entries_fts_schema_version` tracking.
  Startup now calls `ensure_entries_fts_schema()` and only rebuilds FTS when
  the stored schema version is missing/outdated or required FTS objects are
  missing.

### B2. Encrypted search stores ciphertext in FTS5, wasting disk space

- **Status:** FIXED
- **Severity:** Low
- **Files:** `server/src/storage.rs`
- **Description:** When encryption is active, `search_text` is stored
  encrypted. The FTS5 triggers still fire and index encrypted ciphertext, which
  is useless for search. The code correctly falls back to in-memory search, but
  the FTS index wastes disk space with encrypted gibberish. After password
  setup, `migrate_existing_data()` encrypts all entries but does not rebuild
  FTS, leaving stale data until the next restart.
- **Recommendation:** Either skip FTS indexing when encryption is active (drop
  triggers or use a conditional trigger), or drop the FTS table entirely when
  a password is configured.
- **Fix:** Updated FTS triggers/rebuild logic to skip encrypted `search_text`
  values (`ENC:1:%`) so ciphertext is never indexed. Bumped
  `entries_fts_schema_version` and rebuilt once on upgrade to purge any
  previously indexed ciphertext rows.

### B3. `backfill_flavor_columns()` runs on every startup without tracking completion

- **Status:** OPEN
- **Severity:** Low
- **Files:** `server/src/storage.rs`, `src-tauri/src/storage.rs`
- **Description:** Both client and server call `backfill_flavor_columns()` on
  every startup. It reads all rows and checks whether each needs migration from
  the legacy `text_content` column to the multi-flavor columns. After the first
  successful run, all subsequent runs are no-ops that still scan every row.
- **Recommendation:** Track whether backfill has been completed (e.g., a
  `schema_version` key in a metadata/settings table). Skip the scan if already
  done.

### B4. TOCTOU race between `ensure_authorized()` and `get_dek()`

- **Status:** OPEN
- **Severity:** Low
- **Files:** `server/src/api.rs`
- **Description:** In handlers like `create_entry`, `ensure_authorized()`
  acquires the crypto mutex, verifies the password, and releases it. Then
  `get_dek()` acquires the mutex again. Between these two calls, another
  request could call `/auth/lock`, clearing the DEK. This would cause
  `get_dek()` to return `None` after `ensure_authorized()` succeeded. In
  practice, `std::sync::Mutex` serialization makes this unlikely, but the
  window exists.
- **Recommendation:** Have `ensure_authorized()` return the DEK directly,
  eliminating the second lock acquisition.

### B5. Swagger UI requires internet access

- **Status:** OPEN
- **Severity:** Low
- **Files:** `server/src/main.rs` (SWAGGER_UI_HTML)
- **Description:** The Swagger UI HTML loads JavaScript and CSS from
  `unpkg.com/swagger-ui-dist@5`. If the server is running on a local network
  without internet (a common deployment scenario per README), Swagger UI will
  not render.
- **Recommendation:** Either bundle Swagger UI assets in the server binary, or
  document that Swagger UI requires internet and point users to the raw OpenAPI
  JSON for offline use.

---

## Section C -- Open Issues (Previously Tracked)

### macOS Window Management

#### W12. Popup always repositioned to cursor on every open, ignoring user's dragged position

- **Status:** OPEN
- **Files:** `src-tauri/src/lib.rs:233, 264-289`
- **Severity:** Medium
- **Description:** Every call to `toggle_popup()` invokes
  `position_popup_near_cursor()`, which moves the popup to the current cursor
  position. If the user has manually dragged the popup to a preferred screen
  location, that position is lost on close/reopen.
- **Recommendation:** Track whether the user has manually repositioned the
  popup. If they have, skip `position_popup_near_cursor()` and restore the last
  known position.

#### W17. No keyboard focus in popup after NSPanel conversion on some macOS versions

- **Status:** OPEN
- **Files:** `src-tauri/src/lib.rs:415`, `src-tauri/tauri.conf.json:21`
- **Severity:** Low
- **Description:** The popup is converted to a non-activating NSPanel
  (`NSWindowStyleMaskNonActivatingPanel`). Non-activating panels do not take
  keyboard focus from the previous app. Some macOS versions may not give the
  webview first-responder status, making keyboard shortcuts (arrow keys, Escape,
  Enter) non-functional.
- **Recommendation:** After `panel.show()`, explicitly call `panel.makeKey()` or
  `NSWindow.makeKeyAndOrderFront()` to force key status.

#### W18. `detect_frontmost_app_name()` returns process name which may differ from Launch Services name

- **Status:** OPEN
- **Files:** `src-tauri/src/paste.rs`
- **Severity:** Low
- **Description:** Uses `System Events` to get the process name (e.g., `"Code"`
  for VS Code). The paste script uses `tell application "X" to activate`, which
  resolves via Launch Services. Some apps have different process and application
  names, causing activation to fail silently.
- **Recommendation:** Use `tell application "System Events" to set frontmost of
  process "X" to true` since the process name is already known.

#### W20. Window position not persisted across app restarts

- **Status:** OPEN
- **Files:** `src-tauri/src/lib.rs:264-289`
- **Severity:** Low
- **Description:** The popup always starts at `visible: false` with no
  remembered position. After a restart, the user's preferred position is lost.
- **Recommendation:** Save popup position/size to the local settings database
  before hiding. Restore on next open.

#### W21. `cursor_position()` may return wrong values on secondary Retina displays

- **Status:** OPEN
- **Files:** `src-tauri/src/lib.rs:267-274`
- **Severity:** Low
- **Description:** On a secondary display with a different scale factor, physical
  coordinates may not correctly map to the popup's position. The 14px fixed
  offset is in physical pixels, translating to only 7 logical pixels on 2x
  Retina.
- **Recommendation:** Convert cursor position to logical coordinates using the
  target display's scale factor.

#### W22. Single-click on entry immediately pastes -- no way to select without pasting

- **Status:** OPEN
- **Files:** `src/lib/components/EntryRow.svelte`
- **Severity:** Low
- **Description:** Clicking an entry row immediately triggers paste, hiding the
  popup. Any accidental click causes an unintended paste. Double-click for
  preview is not discoverable.
- **Recommendation:** Add a "Paste on single-click" toggle in settings.

### Server & Security

#### 1. No rate limiting on authentication endpoints

- **Status:** OPEN
- **Severity:** High
- **Files:** `server/src/api.rs:159-216`
- **Description:** `/auth/setup`, `/auth/unlock`, and `/auth/change-password`
  have no rate limiting. Brute-force attacks are possible, limited only by
  Argon2id's computational cost (~0.5-1s per attempt).
- **Recommendation:** Implement per-IP rate limiting using `tower-governor` or
  similar middleware.

#### 2. Password transmitted in plaintext when server accessed over HTTP

- **Status:** OPEN
- **Severity:** High
- **Files:** `server/src/api.rs`, `src-tauri/src/sync.rs`
- **Description:** Password sent as `Bearer` token in cleartext. README warns
  about not exposing the server to the internet, but no TLS is provided by the
  server itself.
- **Recommendation:** Document TLS requirement prominently. Consider adding
  optional TLS support via rustls.

#### 3. Server storage uses `.ok()` instead of `.optional()` -- masks DB errors

- **Status:** OPEN
- **Severity:** Medium
- **Files:** `server/src/storage.rs`
- **Description:** Several `query_row` calls use `.ok()` which silently converts
  DB errors (corruption, I/O failures) into `None`, masking real problems.

#### 4. No maximum password length -- DoS via Argon2

- **Status:** OPEN
- **Severity:** Medium
- **Files:** `server/src/api.rs:163-167`
- **Description:** The `auth_setup` and `auth_unlock` endpoints enforce a
  minimum of 8 characters but no maximum. A malicious request with a multi-MB
  password would cause Argon2id to consume significant CPU and memory.
- **Recommendation:** Add a reasonable maximum (e.g., 1024 bytes).

#### 5. CORS allows all origins

- **Status:** OPEN
- **Severity:** Medium
- **Files:** `server/src/main.rs`
- **Description:** `CorsLayer` is configured with `Any` for origins, methods,
  and headers. Any website can make API calls to the server if the user is on
  the same network.
- **Recommendation:** Allow configuring CORS origins, or restrict to same-origin
  in production.

#### 6. Unbounded memory usage in encrypted search

- **Status:** OPEN
- **Severity:** Medium
- **Files:** `server/src/storage.rs`
- **Description:** When encryption is active and a search is performed, the
  server loads ALL entries into memory, decrypts each one, and performs substring
  matching. This is O(n) in the number of entries and could exhaust memory with
  a large history.
- **Recommendation:** Implement pagination within the decryption loop, or add
  a configurable maximum for in-memory search.

#### 7. LIKE pattern special characters not escaped in search

- **Status:** OPEN
- **Severity:** Medium
- **Files:** `src-tauri/src/storage.rs`, `server/src/storage.rs`
- **Description:** The search parameter is interpolated into SQL `LIKE` patterns
  without escaping `%`, `_`, or `[` characters. A search for `100%` would match
  unintended entries.
- **Recommendation:** Escape LIKE special characters or use FTS5 exclusively
  for search.

#### 8. Sync cursor race condition on crash

- **Status:** OPEN
- **Severity:** Medium
- **Files:** `src-tauri/src/sync.rs`
- **Description:** The pull sync cursor is updated after processing entries. If
  the app crashes between ingesting entries and persisting the new cursor,
  entries could be re-ingested on next sync. The dedup-by-hash prevents
  duplicates, but the re-processing wastes resources.
- **Recommendation:** Persist the cursor after each batch of entries rather than
  at the end of the full pull.

#### 9. Paste simulation not implemented on non-macOS platforms

- **Status:** PARTIAL
- **Severity:** Low
- **Files:** `src-tauri/src/paste.rs`
- **Description:** Paste simulation (writing to clipboard + simulating Cmd+V /
  Ctrl+V keystroke) is only implemented for macOS via `osascript`. On Windows
  and Linux, clipboard writing works but the simulated keystroke is not
  implemented. The user must manually paste after clicking an entry.
- **Recommendation:** Implement via `xdotool` on Linux and
  `SendInput`/`keybd_event` on Windows.

#### 10. Temporary file leak on atomic_write failure

- **Status:** OPEN
- **Severity:** Low
- **Files:** `server/src/crypto.rs`
- **Description:** `atomic_write()` writes to a `.tmp` file then renames. If the
  rename fails, the temp file is not cleaned up.
- **Recommendation:** Add cleanup in the error path.

#### 11. No input validation on server URLs in settings

- **Status:** OPEN
- **Severity:** Low
- **Files:** `src/lib/components/SettingsDialog.svelte`
- **Description:** Server URL fields accept any string. Invalid URLs will cause
  sync to fail silently until the user checks status.
- **Recommendation:** Validate URLs on save and show an error for malformed input.

#### 12. Missing Content-Security-Policy headers

- **Status:** OPEN
- **Severity:** Low
- **Files:** `server/src/main.rs`
- **Description:** The server admin UI is served without CSP headers. While the
  current UI is simple and self-contained, adding CSP would prevent XSS vectors.

#### 13. Blob hash validation allows uppercase hex

- **Status:** OPEN
- **Severity:** Low
- **Files:** `crates/copywraith-core/src/content.rs`
- **Description:** `is_valid_hash()` accepts uppercase hex characters. The
  SHA-256 implementation produces lowercase hex. A hash generated externally
  with uppercase could bypass dedup but still be stored, creating inconsistency.
- **Recommendation:** Normalize to lowercase before validation and storage.

#### 14. Argon2 parameters could be strengthened

- **Status:** OPEN
- **Severity:** Low
- **Files:** `server/src/crypto.rs`
- **Description:** Current: 64 MiB memory, 3 iterations, parallelism 4. This
  meets OWASP minimums but is on the low end. OWASP recommends 19 MiB / 2
  iterations as minimum and higher for better security.
- **Note:** Current parameters are a reasonable trade-off for a local-first app.

#### 15. No DELETE cascade on entries table

- **Status:** OPEN
- **Severity:** Low
- **Files:** `server/src/storage.rs`, `src-tauri/src/storage.rs`
- **Description:** `PRAGMA foreign_keys=ON` is set but the entries table has no
  foreign key relationships. This is not a bug per se, but blob cleanup relies
  on application logic (`delete_entry` manually removes unreferenced blobs)
  rather than DB constraints.

---

## Section D -- Improvement Ideas

### I1. Unify client and server storage implementations

- **Files:** `src-tauri/src/storage.rs`, `server/src/storage.rs`
- **Description:** Both client and server have independent SQLite storage
  implementations with similar but divergent schemas and logic. Migration code,
  schema definitions, and `row_to_entry` mapping are duplicated. Changes to the
  data model must be made in both places.
- **Suggestion:** Extract common storage logic into `copywraith-core` or a new
  shared `copywraith-storage` crate.

### I2. Add schema version tracking

- **Files:** Both storage implementations
- **Description:** Migrations are done by checking for column existence
  (`ensure_entries_column`) on every startup. There's no `schema_version` table
  to track what migrations have been applied.
- **Suggestion:** Add a `schema_version` table and only run migrations when the
  version is below the expected target.

### I3. Replace `std::sync::Mutex` with `tokio::sync::Mutex` in server

- **Files:** `server/src/main.rs`, `server/src/api.rs`
- **Description:** `std::sync::Mutex` is used for both the SQLite connection and
  `CryptoState` in an async context. This blocks the Tokio runtime thread while
  holding the lock. For a single-user server this is acceptable, but it's a
  scalability concern.
- **Suggestion:** Use `tokio::sync::Mutex` or `spawn_blocking` for DB operations
  to avoid blocking the async runtime.

### I4. Bundle Swagger UI assets for offline use

- **Files:** `server/src/main.rs`
- **Description:** Swagger UI loads from unpkg.com CDN, requiring internet.
  Given the project's local-first philosophy, bundling assets would be more
  consistent.
- **Suggestion:** Use `utoipa-swagger-ui` crate which embeds Swagger UI assets.

### I5. Add entry export/import functionality

- **Description:** There's no way to bulk export or import clipboard history.
  Useful for backup, migration between servers, or sharing.
- **Suggestion:** Add `GET /api/entries/export` (JSON dump) and
  `POST /api/entries/import` endpoints.

### I6. Add configurable entry retention / auto-cleanup

- **Description:** Clipboard history grows indefinitely. For users with heavy
  clipboard usage, the database could become very large over time.
- **Suggestion:** Add configurable retention settings (max entries, max age,
  max total storage size) with automatic cleanup.

### I7. Implement reveal-sensitive-content toggle in UI

- **Files:** `src/lib/components/EntryRow.svelte`, `server/ui/src/lib/EntryRow.svelte`
- **Description:** SENSITIVE.md lists "User toggle to reveal sensitive content
  temporarily in the UI" as a future improvement. Currently, sensitive content
  is permanently masked in the display with no way to view it except by pasting.
- **Suggestion:** Add an eye/reveal icon button on sensitive entries that
  temporarily shows the real content.

### I8. Consider using `content_hash` dedup via UNIQUE constraint error instead of SELECT+INSERT

- **Files:** `server/src/storage.rs`, `src-tauri/src/storage.rs`
- **Description:** The `create_entry` method does a SELECT to check for existing
  hash, then an INSERT. This is a TOCTOU pattern. The `UNIQUE INDEX` on
  `content_hash` would catch duplicates, but the resulting constraint violation
  error is not explicitly handled.
- **Suggestion:** Use `INSERT OR IGNORE` or `INSERT ON CONFLICT` to handle
  dedup atomically at the DB level.

### I9. `row_to_entry` uses fragile positional column indices

- **Files:** `server/src/storage.rs`, `src-tauri/src/storage.rs`
- **Description:** Both `row_to_entry` implementations use positional indices
  (0-12) to extract column values from SQL rows. If `ENTRY_SELECT_COLUMNS` is
  reordered or modified without updating the indices, data will be silently
  misread.
- **Suggestion:** Use named column access (`row.get("column_name")`) or derive
  the mapping from the `ENTRY_SELECT_COLUMNS` order with compile-time checks.

---

## Section E -- Previously Fixed Issues

### macOS Window Management (Fixed)

1. **W1** -- NSPanel initialization flag set before async conversion completes; non-panic failures permanently disable panel mode. **FIXED**: Flag reset in `Ok(Err(e))` branch.
2. **W2** -- `popup_open` atomic bool desynchronizes from actual window/panel state. **FIXED**: Added `popup.is_visible()` reconciliation.
3. **W3** -- Popup position not validated against screen bounds. **FIXED**: Monitor-aware clamping with logical pixel offset.
4. **W4** -- No focus restoration on Escape or close-button dismissal. **FIXED**: Added `paste::restore_previous_focus()`.
5. **W5** -- Popup fails to appear in fullscreen Spaces when NSPanel conversion fails silently. **FIXED**: Removed conflicting `visibleOnAllWorkspaces`; NSPanel exclusively controls workspace visibility.
6. **W6** -- Race between `popup.show()` and async `request_panel_show_on_main_thread()`. **FIXED**: Consolidated into single `run_on_main_thread` closure.
7. **W7** -- `emit_paste_failed()` re-shows popup without restoring NSPanel. **FIXED**: Uses `show_popup_and_panel_on_main_thread()`.
8. **W8** -- `remember_frontmost_app()` can capture Copywraith's own process name. **FIXED**: Moved to shortcut handler callbacks with native NSWorkspace lookup.
9. **W9** -- Global shortcut callback may fire on non-main thread. **FIXED**: Split `toggle_popup()` into dispatcher + `toggle_popup_impl`; macOS dispatches to main thread.
10. **W10** -- Tauri `alwaysOnTop` and `visibleOnAllWorkspaces` may conflict with NSPanel settings. **FIXED**: Removed from `tauri.conf.json`; NSPanel exclusively controls.
11. **W11** -- No auto-hide when popup loses focus. **FIXED**: 500ms grace-period auto-hide.
12. **W13** -- Debounce and timing guards create dead zones. **FIXED**: Reduced to 100ms debounce + 200ms open-protection.
13. **W14** -- `remember_frontmost_app()` spawns osascript synchronously. **FIXED**: Native NSWorkspace lookup + background cache thread.
14. **W15** -- Multiple concurrent osascript processes contend during paste. **FIXED**: Timestamp-based suppression window + cached source app.
15. **W16** -- EntryPreview close-and-paste race. **FIXED**: Async `await` before `onclose()`.
16. **W19** -- Panel `order_out` via `run_on_main_thread` races with `popup.hide()`. **FIXED**: Consolidated into single `run_on_main_thread` closure.

### Server & General (Fixed)

17. `preview()` panics on multi-byte UTF-8 content -- Fixed with char-aware slicing.
18. Server binds to `0.0.0.0` by default -- Now defaults to `127.0.0.1`.
19. Path traversal in blob storage -- Hash validation added.
20. No HTTP timeouts on sync client -- 10s connect, 30s request timeout.
21. `delete_entry` does not clean up blob files -- Now removes unreferenced blobs.
22. `get_entry` and `get_most_recent_entry` swallow DB errors (client) -- Fixed with `.optional()`.
23. ContentType deserialization fragile and duplicated -- `FromStr` impl and `row_to_entry` helper.
24. `save_settings` is not transactional -- Wrapped in BEGIN/COMMIT.
25. Non-atomic `auth.json` writes -- Uses atomic write with temp file + rename.
26. `/api/health` exposes entry count without auth -- Count only shown when authenticated.
27. Empty `search.rs` module -- Removed.
28. Clipboard monitor feedback loop during paste -- Timestamp-based suppress window.
29. Invisible paste errors -- `.output()` + stderr parsing + `paste-failed` event.
30. No Accessibility permission check -- `AXIsProcessTrusted()` FFI preflight warning.
31. Short post-activate delay -- Increased to 140ms.
32. Activation failure aborting paste -- `try ... end try` around `activate`.
33. Failure feedback hidden while popup closed -- Re-show popup on paste failure.
34. Literal keystroke flakiness -- Fallback `key code 9` retry path.
