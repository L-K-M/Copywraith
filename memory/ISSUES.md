# ISSUES

This document captures findings from a full code/documentation review across desktop (`src` + `src-tauri`), server (`server/src`), shared core (`crates/copywraith-core`), and docs.

## High Priority

### 1) Server admin image preview/download is broken when auth is enabled
- **Category:** Bug / UX
- **Status:** Fixed (2026-04-24)
- **Evidence:** `server/ui/src/lib/EntryRow.svelte:130`, `server/ui/src/lib/EntryDetail.svelte:52`, `server/ui/src/App.svelte:224`, `server/src/api.rs:572`
- **What is happening:** The UI renders image/blob URLs directly in `<img src="/api/entries/{id}/blob">` and `<a href="...">` download links. Blob endpoints require `Authorization: Bearer <password>`, but browser image/link requests do not include this custom header.
- **Impact:** Image previews and image downloads fail with `401` in normal password-protected setups.
- **Resolution:**
  1. Added authenticated blob helpers in `server/ui/src/lib/api.ts` (`fetchBlob`, `fetchBlobObjectUrl`) that attach bearer auth headers.
  2. Updated image previews in `server/ui/src/lib/EntryRow.svelte` and `server/ui/src/lib/EntryDetail.svelte` to use authenticated fetch + object URLs.
  3. Updated image downloads in `server/ui/src/App.svelte` to fetch blobs with auth before triggering download.
  4. Added `URL.revokeObjectURL(...)` cleanup to prevent object URL leaks.

### 2) Desktop delete does not sync to server (cross-device inconsistency)
- **Category:** Bug / Behavioral mismatch
- **Evidence:** `src-tauri/src/commands.rs:102`, `src-tauri/src/storage.rs:426`, `src-tauri/src/sync.rs:126`
- **What is happening:** `delete_entry` removes local data only; sync logic only pushes unsynced entries / star updates and pulls remote updates. There is no tombstone/delete propagation.
- **Impact:** A user can delete locally but data remains on server and other devices; behavior is surprising for a “two-way sync” product.
- **Suggested fix:**
  1. Add deletion tombstones (table: `deleted_content_hash` + timestamp) or explicit `deleted` flag.
  2. Push tombstones to server (`DELETE` by content hash or dedicated endpoint).
  3. Pull remote tombstones and apply locally.
  4. Define conflict semantics (e.g., delete wins over star/update).

### 3) Pull sync uses offset pagination over mutable ordering and can miss entries
- **Category:** Bug / Data consistency
- **Status:** Fixed (2026-04-24)
- **Evidence:** `src-tauri/src/sync.rs:205`, `src-tauri/src/sync.rs:265`, `server/src/storage.rs:637`
- **What is happening:** Client paginates `GET /entries?limit&offset` ordered by `updated_at DESC`. If new entries arrive while paging, offsets shift and some rows can be skipped. Cursor update uses top-of-first-page ID, which can make skipped rows permanently unreachable.
- **Impact:** Silent data loss in cross-device sync under concurrent writes.
- **Resolution:**
  1. Added cursor query params (`before_updated_at`, `before_id`) to `ListEntriesParams` in `crates/copywraith-core/src/api_types.rs`.
  2. Updated server list handling in `server/src/api.rs` and `server/src/storage.rs` to support cursor bounds and stable ordering by `(updated_at DESC, id DESC)`.
  3. Updated desktop pull sync in `src-tauri/src/sync.rs` to page via cursor instead of mutable offsets.
  4. Updated API docs note in `API.md` to describe cursor pagination support.

### 4) No rate limiting / lockout on auth endpoints
- **Category:** Security
- **Evidence:** `server/src/api.rs:29`, `server/src/api.rs:210`, `server/src/main.rs:89`
- **What is happening:** `/auth/setup`, `/auth/unlock`, and password-protected endpoints have no request throttling or lockout.
- **Impact:** Online brute-force is possible if server is reachable beyond trusted LAN/VPN.
- **Suggested fix:**
  1. Add per-IP and per-route limits (e.g., `tower-governor`).
  2. Add exponential backoff/temporary lock after repeated failures.
  3. Emit audit logs for repeated failed auth attempts.

### 5) `auth.json` parse/read failure is treated as “uninitialized” (data-loss footgun)
- **Category:** Security / Reliability bug
- **Status:** Fixed (2026-04-24)
- **Evidence:** `server/src/crypto.rs:54`, `server/src/main.rs:78`, `server/src/api.rs:179`
- **What is happening:** If `auth.json` exists but cannot be parsed/read, `CryptoState::load` silently sets `auth_config = None`. Server then behaves like fresh setup.
- **Impact:** Operators may accidentally run setup again and irreversibly orphan previously encrypted data.
- **Resolution:**
  1. Changed `CryptoState::load` in `server/src/crypto.rs` to return `anyhow::Result<CryptoState>`.
  2. Added explicit contextual errors for unreadable/invalid `auth.json`.
  3. Updated server startup in `server/src/main.rs` to fail fast on auth config load errors.

## Medium Priority

### 6) Documentation says clipboard priority is `Image > File > Html > Rtf > Text`, implementation prefers `Text` when plain text exists
- **Category:** Documentation discrepancy / behavior bug
- **Evidence:** `README.md:3` (docs sections describing priority), `ARCHITECTURE.md:316`, `src-tauri/src/clipboard.rs:143`
- **What is happening:** For text flavors, code sets `content_type = Text` whenever `text_plain` exists, even if HTML/RTF are also present. Many HTML copies include plain text, so they get classified as `text`.
- **Impact:** Type filters/labels are misleading; behavior differs from documented format priority.
- **Suggested fix (choose one and document it):**
  1. **Implement documented priority**: prefer `Html` over `Text` when both exist.
  2. **Or** keep current behavior but update docs everywhere to match.

### 7) CORS is fully permissive by default
- **Category:** Security hardening
- **Evidence:** `server/src/main.rs:89`
- **What is happening:** `allow_origin(Any)`, `allow_methods(Any)`, `allow_headers(Any)` for all deployments.
- **Impact:** Easier cross-origin abuse when combined with reachable server and leaked password.
- **Suggested fix:**
  1. Add `COPYWRAITH_ALLOWED_ORIGINS` config.
  2. Default to same-origin in production mode.
  3. Keep permissive CORS only behind explicit opt-in.

### 8) No maximum password length guard on setup/unlock/change
- **Category:** Security / DoS hardening
- **Evidence:** `server/src/api.rs:172`, `server/src/api.rs:250`
- **What is happening:** Only minimum length (8) is checked; very large password payloads are accepted and fed to Argon2.
- **Impact:** Potential memory/CPU abuse.
- **Suggested fix:** Add maximum byte length validation (e.g., 1-4 KiB) for all password-taking endpoints.

### 9) Desktop stores server password in plaintext local SQLite settings
- **Category:** Security
- **Evidence:** `src-tauri/src/storage.rs:531`, `src-tauri/src/storage.rs:586`, `src/lib/components/SettingsDialog.svelte:78`
- **What is happening:** Password (stored in `api_key`) is persisted unencrypted.
- **Impact:** Any local process/user with DB access can read server password.
- **Suggested fix:** Use OS keychain/credential vault via Tauri plugin for secret storage; keep only a keychain reference in SQLite.

### 10) TOCTOU gap between auth check and DEK retrieval
- **Category:** Correctness edge case
- **Evidence:** `server/src/api.rs:333`, `server/src/api.rs:335`, `server/src/api.rs:665`, `server/src/api.rs:709`
- **What is happening:** Handlers call `ensure_authorized()` and later `get_dek()` under separate mutex acquisitions; another request can call `/auth/lock` in between.
- **Impact:** Rare intermittent behavior where a request authenticates but runs without DEK.
- **Suggested fix:** Return DEK (or guard) from `ensure_authorized` and use it directly in the handler.

## Low Priority

### 11) Popup row click immediately pastes; hard to inspect/select safely
- **Category:** UX
- **Evidence:** `src/lib/components/EntryRow.svelte:73`
- **What is happening:** Single click selects and pastes immediately.
- **Impact:** Easy accidental paste; preview workflow is less discoverable.
- **Suggested fix:** Add setting for click behavior (`single-click paste` vs `single-click select, Enter/double-click paste`).

### 12) Startup migration backfill scans full entries table every launch
- **Category:** Performance improvement
- **Evidence:** `server/src/storage.rs:353`, `src-tauri/src/storage.rs:186`
- **What is happening:** `backfill_flavor_columns()` runs each startup on both server and client.
- **Impact:** Unnecessary startup work on large histories.
- **Suggested fix:** Track migration completion/version in metadata/settings and skip if already completed.

### 13) Some UI errors are swallowed instead of surfaced to users
- **Category:** UX / debuggability
- **Status:** Fixed (2026-04-24)
- **Evidence:** `src/lib/util/clipboardStore.ts:103`, `src/lib/components/SettingsDialog.svelte:27`
- **What is happening:** Failures are often only logged to console.
- **Impact:** Users see stale UI with no actionable feedback.
- **Resolution:**
  1. Added user-facing notifications in `src/lib/util/clipboardStore.ts` for failed load/star/delete/paste actions.
  2. Added throttling for repeated load failure notifications to avoid spam.
  3. Added user-facing notification for settings load failure in `src/lib/components/SettingsDialog.svelte`.

### 14) Very large table font sizes in server admin reduce information density
- **Category:** UX
- **Evidence:** `server/ui/src/App.svelte:557`, `server/ui/src/App.svelte:562`
- **What is happening:** Global table headers/cells are forced to `18px/22px !important`.
- **Impact:** Fewer visible rows, more scrolling on common desktop resolutions.
- **Suggested fix:** Use responsive typography scale and avoid hard `!important` overrides.

## Validation Notes

- `cargo test --workspace` passes.
- `npm run build` (root popup app) passes.
- `npm run build` (`server/ui`) passes.
