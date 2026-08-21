# Scaling Improvements - Remaining Work

Extracted from `SCALING.md`. These are the recommended improvements that have not yet been implemented.

---

## Server

### S1. `Mutex<Connection>` serializes all database access
- **Location:** `server/src/storage.rs:11`
- **Problem:** SQLite connection wrapped in `Mutex<Connection>` forces every read and write to acquire the same lock, even though WAL mode supports concurrent readers.
- **Recommendation:** Replace with `r2d2` connection pool or use `tokio_rusqlite` for async access. At minimum, switch to `RwLock<Connection>`.
- **Priority:** High
- **Effort:** Medium

### S2. Full `text_content` returned in list responses
- **Location:** `server/src/api.rs:81-90`
- **Problem:** List responses return full `ClipboardEntry` including `text_content`. Multi-KB HTML documents make JSON responses several MB.
- **Constraint:** Sync client needs full text, so cannot simply remove.
- **Recommendation:** Add optional `preview_length` query parameter. Sync client omits it (gets full text), admin UI sets `preview_length=200`.
- **Priority:** High
- **Effort:** Medium

### S3. Blob endpoint loads entire file into memory
- **Location:** `server/src/api.rs:144-173`
- **Problem:** Reads entire blob file into `Vec<u8>` before sending. 32MB image fully buffered.
- **Recommendation:** Use `tokio::fs::File` with `axum::body::Body::from_stream()` to stream directly.
- **Priority:** Low
- **Effort:** Small

### S4. No request body size limit
- **Location:** `server/src/api.rs:36-66`
- **Problem:** Accepts `blob_base64` with no explicit body size limit.
- **Recommendation:** Add `axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024)` middleware. Consider separate blob upload endpoint with `multipart/form-data`.
- **Priority:** Medium
- **Effort:** Small

### S5. `COUNT(*)` on every list request
- **Location:** `server/src/storage.rs:230-239`
- **Problem:** Runs `SELECT COUNT(*)` on every list request for `total` field. Slow at 1M rows with filters.
- **Recommendation:** Cache total count for no-filter case, or return `total: None` and use cursor-based pagination.
- **Priority:** Medium
- **Effort:** Medium

### S6. Offset-based pagination degrades at high offsets
- **Problem:** `ORDER BY updated_at DESC LIMIT 100 OFFSET 900000` scans 900K rows.
- **Recommendation:** Switch to cursor-based pagination: `WHERE updated_at < ?cursor ORDER BY updated_at DESC LIMIT 100`.
- **Priority:** High
- **Effort:** Medium

---

## Client

### C1. No virtual scrolling in popup UI
- **Location:** `src/lib/components/EntryList.svelte:26`
- **Problem:** Renders all 100 entries as DOM nodes. Would degrade if limit increased.
- **Recommendation:** Use `svelte-virtual-list` to only render visible rows.
- **Priority:** Low
- **Effort:** Small

### C2. Full `text_content` sent to frontend for every entry
- **Location:** `src-tauri/src/commands.rs:46`
- **Problem:** Sends `full_text: e.text_content` for every entry. 1MB HTML sends 1MB over IPC.
- **Recommendation:** Send only `preview` in list. Add `get_entry_full_text(id)` command for preview dialog.
- **Priority:** Medium
- **Effort:** Small

### C3. Client search uses `LIKE '%query%'` (no FTS)
- **Location:** `src-tauri/src/storage.rs:169`
- **Problem:** Full table scan on every search. Slow at 100K+ entries.
- **Recommendation:** Add FTS5 virtual table (server already has FTS).
- **Priority:** Medium
- **Effort:** Small

### C4. `full_text` stays in frontend memory
- **Problem:** Svelte store holds all entries with `full_text` in JS heap.
- **Recommendation:** Same as C2 -- stop sending `full_text` in list response.
- **Priority:** Medium
- **Effort:** Small (covered by C2)

---

## Synchronization

### Y1. First-ever sync still pages through entire server
- **Problem:** New client with no cursor pages through all entries. 1M entries = 10K HTTP requests.
- **Recommendation:** Add bulk export endpoint (`GET /api/entries/export?format=ndjson`) that streams all entries in single response. Or support `since_id` parameter.
- **Priority:** Low
- **Effort:** Large

### Y2. One-at-a-time sequential push
- **Location:** `src-tauri/src/sync.rs:39-41`
- **Problem:** Pushes entries one at a time. 50 unsynced = 50 sequential HTTP requests.
- **Recommendation:** Add batch create endpoint (`POST /api/entries/batch`) accepting array of entries.
- **Priority:** Medium
- **Effort:** Medium

### Y3. Blobs transferred as base64 in JSON
- **Location:** `src-tauri/src/sync.rs:58-66`
- **Problem:** Base64 inflates payload by ~33%. 32MB image becomes ~43MB JSON.
- **Recommendation:** Use `multipart/form-data` for create endpoint when blob present, or add `POST /api/blobs` for raw binary.
- **Priority:** Medium
- **Effort:** Medium

### Y4. Cursor based on entry ID, not timestamp
- **Problem:** Updated entries move to top of list but ID is "behind" cursor. Sync client re-encounters but skips via dedup.
- **Note:** Correct behavior (no data loss) but wasted work.
- **Recommendation:** Use monotonically increasing server-side sequence number (`sync_version`) as cursor.
- **Priority:** Low
- **Effort:** Medium

---

## Summary by Priority

| Priority | Items |
|----------|-------|
| **High** | S1 (Mutex→pool), S2 (preview_length), S6 (cursor pagination) |
| **Medium** | C2 (lazy full_text), C3 (FTS), S4 (body limit), S5 (count cache), Y2 (batch push), Y3 (multipart blobs) |
| **Low** | C1 (virtual scroll), S3 (blob streaming), Y1 (bulk export), Y4 (better cursor) |
