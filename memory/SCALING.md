# Scaling Analysis

How Copywraith behaves when the user has hundreds of thousands or millions of clipboard entries, and what has been done (and still needs to be done) about it.

Constraint: **the server must never delete data**.

---

## Current state after improvements

Several scaling issues have been fixed in this pass. The remaining items are documented below as future recommendations.

### What was fixed

**1. Missing server index on `updated_at` (server/src/storage.rs)**

The server's `list_entries` query sorts by `ORDER BY updated_at DESC`, but the only index was on `created_at`. At 1M rows this meant a full table sort on every list request. Added `idx_entries_updated_at`.

**2. Missing client index on `synced` column (src-tauri/src/storage.rs)**

`get_unsynced_entries()` filters `WHERE synced = 0` but had no index. As entries accumulate and the vast majority become synced, SQLite had to scan the full table to find the few unsynced ones. Added a partial index `idx_entries_synced ON entries(synced) WHERE synced = 0`.

**3. Server API limit clamped to 200 (crates/copywraith-core/src/api_types.rs, server/src/api.rs)**

`ListEntriesParams.limit` was `u32` with no maximum. A client could request `limit=4294967295` and the server would attempt to materialize the entire database into a single JSON response. Now clamped to 200 via `clamp_limit()`.

**4. Image blobs no longer loaded inline in entry list IPC (src-tauri/src/commands.rs)**

Previously, `get_entries` read every image blob from disk, base64-encoded it (up to ~5MB each), and included it in the IPC response for all 100 entries. With 20 screenshots visible, that was ~133MB of data serialized over the Tauri bridge on every popup open.

Now `get_entries` returns a lightweight `has_image: bool` flag, and a new `get_entry_image` command lets the frontend load images lazily per-row. Each row component fetches its own image on mount, so the initial list loads instantly and images appear progressively.

**5. Sync cursor persisted to SQLite (src-tauri/src/sync.rs, src-tauri/src/storage.rs)**

The pull sync cursor (`last_seen_server_id`) was stored only in memory. Every app restart triggered a full re-scan of the entire server entry list. With 1M entries at 100 per page, that's 10,000 HTTP requests before the client catches up.

The cursor is now saved to the `settings` table and restored on startup. Normal restarts only pull entries newer than the cursor.

**6. Sync loop uses exponential backoff on failure (src-tauri/src/lib.rs)**

The sync loop previously ran every 5 seconds unconditionally, even when the server was unreachable. Now it backs off exponentially (5s -> 10s -> 20s -> ... -> 120s max) on consecutive failures and resets to 5s on success.

---

## Remaining concerns and recommendations

### Server

#### S1. `Mutex<Connection>` serializes all database access

`server/src/storage.rs:11` wraps the SQLite connection in `Mutex<Connection>`. Even though WAL mode is enabled (which supports concurrent readers), the Rust mutex forces every read and write to acquire the same lock. Under concurrent load from multiple API clients syncing simultaneously, this becomes a bottleneck.

**Recommendation:** Replace `Mutex<Connection>` with an `r2d2` connection pool or use `tokio_rusqlite` for async access. At minimum, switch to `RwLock<Connection>` so readers don't block each other (though SQLite's threading model makes a pool the better choice).

#### S2. Full `text_content` returned in list responses

`server/src/api.rs:81-90` returns the full `ClipboardEntry` (including `text_content`) in list responses via `#[serde(flatten)]`. When listing 50 entries that are each multi-KB HTML documents, the JSON response can be several MB.

The sync client needs full text content to store locally, so this cannot be simply removed without breaking sync. However, the admin UI only uses a 200-char preview.

**Recommendation:** Add an optional `preview_length` query parameter. When set, `text_content` is truncated to that length in the response. The sync client omits it (gets full text), the admin UI sets `preview_length=200`. Alternatively, add a `fields` parameter to control which columns are returned.

#### S3. Blob endpoint loads entire file into memory

`server/src/api.rs:144-173` reads the entire blob file into `Vec<u8>` and sends it as the response body. A 32MB image is fully buffered in memory.

**Recommendation:** Use `tokio::fs::File` with `axum::body::Body::from_stream()` to stream the file directly to the response without buffering. This caps memory usage at a small read buffer regardless of file size.

#### S4. No request body size limit

`server/src/api.rs:36-66` accepts `Json<CreateEntryRequest>` with `blob_base64: Option<String>` and no body size limit middleware. Axum's default is 2MB but this should be explicitly configured.

**Recommendation:** Add `axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024)` as middleware to explicitly cap request bodies. Consider adding a separate blob upload endpoint that accepts `multipart/form-data` instead of base64-in-JSON.

#### S5. `COUNT(*)` on every list request

`server/src/storage.rs:230-239` runs a `SELECT COUNT(*)` query on every list request for the `total` field. At 1M rows with filters, this can be slow.

**Recommendation:** For the common no-filter case, cache the total count and invalidate on insert/delete. For filtered queries, consider returning `total: None` and using cursor-based pagination instead of offset-based.

#### S6. Offset-based pagination degrades at high offsets

`ORDER BY updated_at DESC LIMIT 100 OFFSET 900000` requires SQLite to scan through 900,000 rows before returning the requested page.

**Recommendation:** Switch to cursor-based pagination using `WHERE updated_at < ?cursor_timestamp ORDER BY updated_at DESC LIMIT 100`. This is O(1) regardless of how deep into the list the client is.

### Client

#### C1. No virtual scrolling in popup UI

`src/lib/components/EntryList.svelte:26` renders all entries (up to 100) as DOM nodes using `{#each}`. While 100 rows is manageable, this means the DOM holds 100 table rows with potential image thumbnails. If the limit were ever increased, this would degrade.

**Recommendation:** Consider a virtual scrolling library (e.g., `svelte-virtual-list`) to only render rows visible in the viewport. This would also enable increasing the entry limit beyond 100 without DOM overhead.

#### C2. Full `text_content` sent to frontend for every entry

`src-tauri/src/commands.rs:46` sends `full_text: e.text_content` for every entry in the list. An entry with 1MB of HTML content sends that entire 1MB over IPC, even though the UI only displays a 200-char preview. The full text is only needed when the user opens the preview dialog.

**Recommendation:** Send only the `preview` in list responses. Add a separate `get_entry_full_text(id)` command that loads the full text on demand when the preview dialog opens.

#### C3. Client search uses `LIKE '%query%'` (no FTS)

`src-tauri/src/storage.rs:169` uses `WHERE text_content LIKE '%query%'` which is a full table scan. At 100K+ entries, search will become noticeably slow.

**Recommendation:** Add an FTS5 virtual table to the client database (matching what the server already has). The client schema already has the `entries` table; adding FTS would make search sub-millisecond regardless of table size.

#### C4. `full_text` stays in frontend memory

The `entries` Svelte store holds all 100 entries including their `full_text` in JavaScript memory. With large HTML entries, this can consume significant heap space.

This is partially mitigated by the 200-char preview approach, but `full_text` is still transferred and stored.

**Recommendation:** See C2 above -- stop sending `full_text` in the list response entirely.

### Synchronization

#### Y1. First-ever sync still pages through entire server

When a new client connects for the first time (no persisted cursor), it must page through the entire server entry list to build its local copy. With 1M entries at 100 per page, this is 10,000 HTTP round trips, each returning ~50-100KB of JSON.

**Recommendation:** Add a bulk export endpoint on the server (e.g., `GET /api/entries/export?format=ndjson`) that streams all entries in a single response using newline-delimited JSON. The client can consume this as a stream without buffering the entire dataset. Alternatively, support a `since_id` query parameter so the client can request only entries after a given ID.

#### Y2. One-at-a-time sequential push

`src-tauri/src/sync.rs:39-41` pushes entries one at a time in a `for` loop. With 50 unsynced entries, that's 50 sequential HTTP requests per sync cycle.

**Recommendation:** Add a batch create endpoint (`POST /api/entries/batch`) that accepts an array of entries. This reduces 50 round trips to 1. For image entries, consider a separate bulk blob upload or use multipart.

#### Y3. Blobs transferred as base64 in JSON

`src-tauri/src/sync.rs:58-66` converts blob data to base64 before embedding it in the JSON request body. This inflates the payload by ~33%. A 32MB image becomes ~43MB of JSON.

**Recommendation:** Use `multipart/form-data` for the create endpoint when a blob is present, or add a dedicated `POST /api/blobs` endpoint that accepts raw binary and returns a blob hash. The entry creation then references the blob by hash without embedding the data.

#### Y4. Cursor based on entry ID, not timestamp

The sync cursor stores the `id` of the most recent entry seen. If an old entry is updated (changing its `updated_at`), it moves to the top of the `ORDER BY updated_at DESC` list but its ID is "behind" the cursor. The sync client will re-encounter it (since it sorts above the cursor position) but skip it via `has_content_hash` dedup.

This is correct behavior (no data loss) but causes wasted work proportional to how many entries have been updated since the last sync.

**Recommendation:** For a more robust approach, use a monotonically increasing server-side sequence number (e.g., auto-increment `rowid` or a `sync_version` counter) as the cursor instead of entry ID. This makes the cursor immune to reordering.

### Database growth projections

Assuming no data is ever deleted:

| Entries | Estimated SQLite size | Estimated blob storage | Notes |
|---------|----------------------|----------------------|-------|
| 10K | ~20 MB | ~2 GB (if 20% images) | Comfortable |
| 100K | ~200 MB | ~20 GB | Queries remain fast with proper indexes |
| 1M | ~2 GB | ~200 GB | `COUNT(*)` starts to slow; offset pagination degrades |
| 10M | ~20 GB | ~2 TB | Single SQLite file becomes unwieldy; need sharding or archival |

SQLite handles databases up to ~280 TB and performs well up to tens of GB with proper indexing. The main bottleneck at scale is blob storage disk space (screenshots average 1-5MB each) and offset-based pagination.

### Summary priority matrix

| Priority | Issue | Impact at 1M entries | Effort |
|----------|-------|---------------------|--------|
| Done | Missing `updated_at` index (server) | Every list query does full sort | Trivial |
| Done | Missing `synced` index (client) | Every sync check scans full table | Trivial |
| Done | No API limit cap | Single request can OOM server | Trivial |
| Done | Image blobs loaded inline in list IPC | ~133MB per popup open with 20 images | Medium |
| Done | Sync cursor lost on restart | 10K HTTP requests to re-sync | Small |
| Done | No sync backoff on failure | CPU/network waste when server down | Small |
| High | Mutex\<Connection\> on server | All requests serialize | Medium |
| High | Offset pagination on server | Deep pages scan millions of rows | Medium |
| High | Full text\_content in list responses | Multi-MB JSON per page | Medium |
| Medium | Base64 blob transfer in sync | 33% bandwidth overhead | Medium |
| Medium | Sequential one-at-a-time push | 50 round trips per sync cycle | Medium |
| Medium | No client-side FTS | Search scans full table | Small |
| Medium | full\_text sent in client IPC | Large entries waste IPC bandwidth | Small |
| Low | No blob streaming on server | Multi-MB files buffered in RAM | Small |
| Low | First sync pages entire server | 10K requests for new client | Large |
| Low | No virtual scrolling in popup | DOM limited to 100 rows anyway | Small |
