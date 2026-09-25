# Copywraith Analysis And Roadmap

Backlog last rebuilt 2026-09-25 against `a4dc499` (0.3.1). This is the
maintained backlog — the single document to start from when picking up work.

> **Currency note.** The sixteen PRs (#147–#162) implementing the 2026-09-25
> review are **merged to `main` through the integration PR #164**, and the six
> Dependabot updates of that week through #163. Their outcomes are in the
> Outcome ledger under *Integrated — #164*. Deferred follow-ups from their
> review rounds are backlog items below.
>
> **Line numbers.** References in entries citing `tmp.md` are as of
> `a4dc499`. References carried over from `opus.md` (`9ca8179`) or `sol.md`
> (`f314806`) are marked with their commit and have drifted; search for the
> named function rather than trusting the number.

Sources, in order of currency:

- `tmp.md` — full code-first review of `a4dc499` (2026-09-25), including
  dedicated admin-UI, Android and Linux/CI passes. **Most current evidence
  record.** Its finding IDs (SEC-N*, SYNC-N*, SRV-N*, CAP-N*, UX-N*, ADM-N*,
  AND-N*, OPS-N*, TOOL-01, DOC-N1) are quoted in parentheses below so the
  evidence can be traced. It lives on branch `claude/pensive-carson-cobgp9`;
  this document stands without it.
- `opus.md` — full independent review of `9ca8179` (2026-07-25). Evidence record
  for SYNC-A*, SEC-0*, BUG-*, PERF-*, UX-0*, OPS-0* and FEAT-* IDs.
- `sol.md` — earlier independent review of `f314806`. Still the detailed record
  for findings that predate `opus.md` and remain open (SYNC-, SERVER-, MAC-,
  ANDROID-, UI-, ADMIN-, OPS- IDs).
- `awesome.md` — the first review, of `977745c`. Largely superseded; retained
  for its product/design rationale (sections 5 and 6).

Anything shipped is removed from the backlog and recorded in **Shipped** at the
bottom, so it is neither lost nor accidentally reimplemented. The **Outcome
ledger** above it records what merged, what is open, what was rejected, and
where rejected work is tracked.

### How to use a backlog entry

Each entry gives **Where** (files, functions), **Problem** (mechanism),
**Fix**, and **Verify** (the test to write or the command to run). The section
is the priority. Standard checks, run for every change:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check && npm run build && npm run test:frontend   # popup
cd server/ui && npm run build && npx vitest run           # admin UI
python3 -m unittest discover -s scripts -p 'test_*.py'    # tooling
```

Desktop-backend tests that need a real server or clipboard live in
`server/tests/` (`native_clipboard.rs`, `desktop_storage.rs`); the native
clipboard suite runs on Linux (under xvfb), macOS and Windows CI.

---

## Current release position

Copywraith has a sound local-first shape and a distinctive interface. As of
2026-09-25 the highest-value remaining work is:

1. **Sync convergence.** #114 fixed SYNC-A1/A2 and part of A5; #149 (one-entry
   probe, immediate first pass), #150 (gzip), #157 (size-aware timeouts), #148
   (honest error states) and #154 (cursor reset on server change), merged in
   #164, remove most steady-state waste and several silent failure modes.
   **SYNC-A3 is still the one that matters**: a timed-out `sync_now` discards
   all pull-watermark progress, so a large history may never converge. No PR
   addresses it; it needs a resumable cursor.
2. **Test coverage of the sync protocol.** Still the single highest-leverage
   missing engineering work. The 2026-09-25 PRs added unit and mock-socket
   tests for their own paths, but nothing exercises client and server end to
   end.
3. **Delete propagation.** #95 was rejected; there are still no tombstones
   anywhere, so a local delete can be undone by a cursor reset (which #154 now
   triggers on server change) or a later server update. Tracked in **#113**.

No PR from the 2026-09-25 review is pending; see the Outcome ledger.

### Verification baseline

On `a4dc499` (2026-09-25, from `tmp.md`):

| Check | Result |
|---|---|
| `npm run build` (popup) | Pass |
| `cargo test --workspace` | Pass (50 core, 15 server-side desktop, 11 server, 39 tauri, others) |
| `npm run test:frontend` | Pass, **except 1 failure when run by an AI agent** (TOOL-01, fixed by #160) |
| Android, macOS, Plasma runtime | Not run (no devices in the review environment) |

On the #164 integration head (2026-09-25), locally:

| Check | Result |
|---|---|
| `cargo fmt`, `cargo clippy -D warnings` | Pass |
| `cargo test --workspace` | Pass: 53 core, 21 server, 16 desktop storage, 81 tauri, 7 native clipboard plus 4 X11 tests under `xvfb-run` |
| Popup `npm run check`, `check:ts6`, `build` | Pass, 0 errors, 0 warnings |
| `npm run test:frontend` | 63/63, including under an AI agent |
| Admin UI svelte-check (TS7, TS6), vitest, build | Pass, 39/39 |
| Python tooling tests | Pass |
| #151 Kotlin share-plugin functions | Compile against the `android-35` platform jar (extracted into a harness; the full plugin was not built) |
| Docker build context (#162) | Exported with BuildKit: 1.7 MB, no data, `.env`, `node_modules`, `dist`, `target` or tests; the server checks with `--locked` from exactly the files the Dockerfile copies |
| Android, macOS, Plasma runtime | Not run |

Historical baseline on `9ca8179`, before and after the 2026-07-25 PRs:

| Check | Result |
|---|---|
| `npm run build` (popup) | Pass |
| `npm run check` (popup) | Pass, 0 errors, 0 warnings |
| `cd server/ui && npm run build` | Pass |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass |
| `cargo test --workspace` | Pass — 59 baseline, 79 after the PRs |
| Android device build | Not run (no NDK in the review environment) |
| Docker build | Not run (no Docker in the review environment) |

Two stale claims from older revisions of this file, both corrected:

- **GitHub Actions runners work.** The earlier note that Actions "creates jobs
  with zero steps and no assigned runner for every new PR" no longer holds. PRs
  #88–#92 received runners immediately and ran green. Red badges are real
  results again.
- **The old PR ledger is gone.** Every implementation PR it tracked (#32–#36,
  #41–#49) has been merged or closed, and so has every PR of the 2026-07-25
  review. The Outcome ledger is the only current record.

---

## Android sync latency

The headline performance complaint, with a root cause that turned out to be
eight compounding problems (SYNC-A1 … SYNC-A8) rather than one. Detail and
measurements in `opus.md` §1; steady-state cost in `tmp.md` SYNC-N1.

### Fixed in #114

- **SYNC-A1** — remote insert/star/sync writes now commit together rather than
  in two transactions (three when starred). `has_content_hash` remains a read.
  **The gain is batching, not weaker durability.**
  #88 also proposed `synchronous=NORMAL`; that was **rejected** and both
  databases are explicitly `synchronous=FULL`, with `busy_timeout=5000` added.
- **SYNC-A2** — sequential push re-reading settings (7 queries) per entry;
  endpoint config is now resolved once per push batch.
- Partial **SYNC-A5** — the duplicated text parsing and unbounded `full_text` in
  the list projection (PERF-01/02) also cut Android list-load cost.

### Fixed by the 2026-09-25 PRs (merged in #164)

- **SYNC-N1** steady-state polling (newest 100 full entries every 5 s per
  client, ~400 AES-GCM decrypts per poll) — #149 probes with `limit=1` first.
- **SYNC-A7** first-pass sleep — #149 runs the first pass at startup. The
  blocking-storage half of A7 remains open (below).
- **SYNC-A5** compression half — #150 (server `CompressionLayer`, reqwest
  `gzip`). The metadata-only projection remains open (below).
- **SYNC-N3** 30 s whole-request timeout — #157. The Sync Now outer 35 s
  timeout remains open (folded into SYNC-A3).

### Still open

#### SYNC-A3 — a timed-out `sync_now` throws away all pull progress

- **Where:** `src-tauri/src/commands.rs` `sync_now` (35 s timeout around
  `pull_new_entries`, `:698` at `9ca8179`); `src-tauri/src/sync.rs`
  `pull_new_entries` (watermark promoted only after the whole walk, `:376` at
  `9ca8179`; `:278-395` at `a4dc499`).
- **Problem:** cancellation discards the watermark. Ingested rows survive, but
  the next pass re-walks from the top of the server list. **If every rescan
  exceeds 35 s, sync cannot finish** — each attempt repeats the scan and reports
  `pulled: 0`. #157 scales per-request deadlines up to 600 s per page,
  but the Sync Now path still wraps everything in the fixed 35 s, so a
  legitimately slow, size-scaled download is still cancelled. Since #149
  serializes pulls, the 35 s also covers waiting for a pull the sync loop has
  already started, so a manual sync during a long loop pull reports a timeout
  while the loop is still making progress.
- **Fix:** (1) make progress durable per page: add an ascending
  `updated_after=(ts,id)` cursor to `GET /api/entries` so the walk runs forward
  and every committed page advances the watermark (`tmp.md` SYNC-N1 "later
  protocol step"); with the current descending walk, at minimum persist the
  pending top watermark plus the lowest handled boundary and resume there.
  (2) Report a partial pass as progress ("pulled N, more pending"), not failure.
  (3) Replace the 35 s outer timeout with no-progress cancellation (abort only
  when no page or blob completes within the 30 s stall window #157 uses), and
  start it only once the manual pull holds `pull_lock`.
- **Verify:** mock-server test with 5 pages and an artificial per-page delay so
  the total exceeds the outer deadline: the first call persists a watermark past
  the initial one, the second call completes without re-fetching page 1.

#### Push timeout blocks pull (AND-N10)

- **Where:** `src-tauri/src/commands.rs:755-776` (`sync_now` returns without
  pulling when push times out); `src-tauri/src/lib.rs:876` (loop always pushes
  first).
- **Problem:** one stuck upload also stops all downloads.
- **Fix:** run pull regardless of the push outcome, each with its own deadline,
  and report both results; or pull first.
- **Verify:** mock server whose POST hangs and whose GET serves one new entry:
  after one pass the entry is ingested and the push is reported as timed out.

#### SYNC-A4 — the first pull is unbounded

- **Where:** `src-tauri/src/sync.rs` `pull_new_entries`, `ingest_remote_entry`,
  `fetch_blob_data`.
- **Problem:** no bootstrap limit. The client pages the entire server history
  and downloads each blob in its own sequential request inside the ingest loop.
- **Fix:** bound the initial pull (recent N / last M days, setting), backfill
  older pages in the background, and fetch blobs lazily (on view/paste or by a
  bounded background worker) instead of inline.
- **Verify:** mock server with 1,000 entries and 200 images: the first pass
  ingests ≤ N rows and fetches no blobs; a later backfill reaches all rows.

#### SYNC-A5 — the list response ships every text flavour in full

- **Where:** `server/src/api.rs` list handler (`:402`); response DTO in the same
  file; client ingest in `src-tauri/src/sync.rs`.
- **Problem:** `text_content`, `text_plain`, `text_html` *and* `text_rtf` for
  all 100 rows. One rich-text copy is routinely 50–500 KB. Gzip (#150) shrinks
  it ~5–10× but the server still decrypts and ships it.
- **Fix:** add a metadata-only projection (`?view=sync`: id, hashes,
  content_type, timestamps, starred, sensitive, sizes, `blob_url`). The client
  fetches full text via `GET /api/entries/{id}` only for hashes it does not
  have. Old servers ignore the parameter; detect by presence of the text fields.
- **Verify:** server test that `view=sync` omits the text fields; client test
  that a known hash triggers no detail request and an unknown one triggers one.

#### SYNC-A6 — `COUNT(*)` on every page; encrypted search scans everything

- **Where:** `server/src/storage.rs` `list_entries` (`:541` region at
  `a4dc499`; count at `:656` and full scan at `:610` at `9ca8179`).
- **Problem:** a full count with the same WHERE clause runs before every page
  only to derive `has_more`. With encryption (mandatory now) *and* a search
  term, the whole table is loaded, decrypted and filtered per request.
- **Fix:** fetch `limit + 1` rows for `has_more`; make `total` opt-in
  (`?count=true`, used only by the admin UI). Encrypted search: see *Decide the
  fate of FTS5 and encrypted search* below.
- **Verify:** storage tests for `has_more` at exactly `limit`, `limit+1` and a
  multiple of `limit` rows; assert no `COUNT` when `count` is absent.

#### SYNC-A7 (remainder) — blocking storage calls on async threads

- **Where:** client storage (`src-tauri/src/storage.rs`, `rusqlite` behind a
  mutex, `std::fs`) called directly from async commands and the sync loop in
  `commands.rs`, `sync.rs`, `lib.rs`. Server counterpart under *Server
  scalability*.
- **Fix:** wrap storage calls in `tokio::task::spawn_blocking` (or a dedicated
  storage thread with a channel).
- **Verify:** a `current_thread` tokio test in which a 50-entry ingest runs
  while a 10 ms interval timer keeps firing on schedule.

#### SYNC-A8 — focus-driven refresh storm

- **Where:** `src/routes/+page.svelte` (`:123-128` at `a4dc499`).
- **Problem:** a full capture → import → sync → reload runs on every Android
  focus event (keyboard, dialogs, permission prompts). `mobileRefreshInFlight`
  guards concurrency but there is no cooldown, and each refresh calls
  `loadEntries()` up to four times.
- **Fix:** drive refresh from Activity lifecycle resume events (plugin event)
  instead of focus; add a cooldown (e.g. 30 s) after completion; call
  `loadEntries()` once per refresh. When the staged-clipboard event is wired
  (AND-N3), debounce an import+push rather than a full refresh.
- **Verify:** unit test of the extracted refresh scheduler
  (`scripts/tests/*.test.mjs`): 10 focus events in 1 s → one refresh, one
  `loadEntries`.

---

## Priority 0: data integrity and security

### Make sync one revision-safe pipeline (incl. SYNC-N5, SYNC-N6)

Refs: `sol.md` SYNC-02/05/06/07/10; `tmp.md` SYNC-N5, SYNC-N6.

- **Where:** `src-tauri/src/storage.rs` `insert_entry` early return
  (`:248-255`), `toggle_star` (`:500-514`),
  `apply_remote_star_state_by_content_hash` (`:541-545`);
  `src-tauri/src/clipboard.rs:172-181`; `src-tauri/src/sync.rs`
  `ingest_remote_entry` (`:622-625`); `src-tauri/src/lib.rs:861-911` (loop);
  `server/src/storage.rs` `toggle_star` (`:705-713`) and list ordering
  (`:619,674`).
- **Problem:** periodic sync, manual sync, capture-triggered push and
  star-triggered push can overlap; a stale request can mark a newer row synced,
  and dedup requests can erase starred state. `updated_at` does two jobs —
  "last used" (sort order) and "last modified" (sync version) — so:
  - **recency does not sync** (SYNC-N5): a duplicate capture bumps local
    `updated_at` but leaves `synced = 1` and only emits `clipboard-reordered`;
    on pull, a known hash applies only the star state. Copy "foo" Monday and
    again Wednesday on the Mac: the server and phone still show Monday.
  - **starring reorders history everywhere**: `toggle_star` sets
    `updated_at = now` on client and server.
  - a remote star apply stamps local `now()` into `updated_at` (SYNC-N6), so
    the same entry carries different timestamps on different devices.
- **Already merged (#164):** #153 applies a push acknowledgement only if the row
  is unchanged (`mark_synced_if_unchanged`, one transaction) — the revision
  check for one path; #148 pauses a push batch on 401/403.
- **Fix:**
  1. Route all sync through one coordinator/actor (a tokio task owning push and
     pull; commands send it requests).
  2. Add a monotonic local row `revision`; mark synced only when the sent
     revision still matches (generalise #153).
  3. Split the timestamps: `modified_at`/revision is the sync cursor;
     `last_used_at` (or `updated_at`) is ordering only. A recopy bumps both and
     sets `synced = 0`; a star bumps only the revision. The server's star PATCH
     must not change the ordering column. Keep `updated_at` in the wire format
     for old clients.
  4. Separate create/recopy from star mutation; preserve server starred state
     during ordinary dedup. **Star reconciliation stays keyed on
     `content_hash`** (see *Review corrections*).
  5. Track per-entry permanent and retryable failures durably (attempts, last
     error, class, `next_attempt_at` with exponential backoff) so one bad row
     cannot monopolise the loop (`tmp.md` SYNC-N3 fix). Push metadata-only
     changes such as stars ahead of blob uploads: since #153 a star waits for
     the loop's oldest-first batch, and under #157's size-scaled deadlines one
     large pending blob can hold that batch for minutes.
  6. Advance the pull watermark only over a successfully handled contiguous
     range (also the fix for SYNC-A3).
- **Verify:** tests for (a) recopy on device A appears at the top on the server;
  (b) starring an old entry leaves server order unchanged; (c) a remote star
  apply leaves `updated_at` equal to the server value; (d) a stale push ack
  after a local edit leaves `synced = 0`; (e) a row failing with 413 is marked
  permanent and the next rows still push.

### Add end-to-end sync tests before protocol growth

Refs: `sol.md` OPS-04.

**Still the highest-leverage missing work in the repo.** Client storage and
list projection have unit tests and the 2026-09-25 PRs added mock-socket tests, but no
test runs the real client against the real server; everything in *Android sync
latency* and BUG-01 would have been caught by one.

- **Where:** new `server/tests/sync_e2e.rs` (the crate already hosts
  desktop-backend tests). Since #150, the server router is built by
  `build_app`, so the test can serve it on an ephemeral port over a temp data
  dir and point a `SyncClient` at it.
- **Cover:**
  - Cursor item moved, deleted, and tied on timestamp.
  - Empty, missing, corrupt, and hash-mismatched blobs.
  - Sensitive full/masked response identity.
  - Initial pull chronology and preserved timestamps.
  - Concurrent star/capture/manual/periodic updates.
  - Wrong password (401), setup-required (403), 413, 500 and transport failure
    status/retry classes, including the new `unauthorized`/`error` states.
  - Primary/fallback aliases and accidentally distinct servers; URL change
    resets the cursor.
  - Process restart halfway through metadata and blob sync.
  - Probe short-circuit when nothing changed; gzip-encoded pages.
  - Large blob on a throttled link completes (size-aware deadline).
  - Absolute cross-origin `blob_url` is refused (SEC-N4).
- **Verify:** `cargo test --workspace --test sync_e2e` in CI.

### Bound and stream large payloads (incl. CAP-N4)

Refs: `sol.md` ANDROID-01/05/12, SYNC-12, MAC-11, ADMIN-08/09; `tmp.md` CAP-N4,
SYNC-N3.

- **Where:** push encoding `src-tauri/src/sync.rs:243-251` (inline base64);
  capture caps `src-tauri/src/clipboard.rs:193` (32 MiB for file images) and
  Android shares (64 MiB); server body limit in `server/src/main.rs`.
- **Problem:** Android accepts a 64 MiB raw file, then base64/JSON expansion
  (~×1.33) exceeds the server's 64 MiB body limit. `ClipboardPayload::Image` has
  **no cap at all**; a large-display screenshot re-encoded to PNG can exceed it
  too (CAP-N4). The failed row retries every 5 s, re-reading and re-encoding the
  blob, and can allocate several copies. #157 removed the
  whole-request timeout that made large blobs fail on slow links; size limits
  remain.
- **Fix:**
  - Replace base64 JSON blobs with streaming multipart or a separate blob API.
  - Enforce compatible decoded per-entry and cumulative batch limits, including
    a cap on clipboard images at capture.
  - Classify 413/invalid payload as permanent and user-action-required.
  - Add available-space checks, progress, cancellation and bounded workers.
  - Generate thumbnails; avoid full blob transfer through WebView IPC.
- **Verify:** test that an entry above the limit is never pushed and is shown as
  needing action; server returns 413 for an oversize body and the client
  classifies it permanent; memory high-water test for a 60 MiB push.

### Secure first-run server ownership and transport

Refs: `opus.md` SEC-01/02/03, BUG-09; `sol.md` SERVER-01/02/16; `tmp.md` ADM-N1
(later half), AND-N4 (CA half).

- **Where:** `server/src/api.rs` setup (`:168` at `9ca8179`; length check
  `:172`), `ensure_authorized` (`:723-740`), `extract_bearer` (`:756-764`);
  `server/src/main.rs` `CorsLayer::allow_origin(Any)` (`:89` at `9ca8179`);
  `docker-compose.yml`.
- **Problem:** unauthenticated setup plus permissive CORS plus a LAN-bound Docker
  service lets any reachable client claim an uninitialised server. The master
  encryption password doubles as the bearer token and is commonly sent over
  plain HTTP. `ensure_authorized` holds the global `Mutex<CryptoState>` across
  `verify_and_unlock`; the correct-password fast path is a SHA-256 compare, but
  a **cache miss runs Argon2id (64 MiB, t=3, p=4) on a Tokio worker while
  holding the lock**, so any client with a stale password stalls every request,
  repeatedly (BUG-09).
- **Fix:**
  - Require a one-time local bootstrap token, loopback setup, or CLI setup.
  - Restrict CORS to configured admin origins.
  - Require HTTPS for non-loopback URLs by default; document a tested TLS
    reverse-proxy or encrypted-VPN deployment. Trust the platform/user CA store
    so private-CA LAN servers work (see AND-N4).
  - Replace master-password API auth with revocable per-device tokens/scopes.
    When changing the auth header, version it and send
    `base64url(utf8(secret))`, keeping the legacy form (ADM-N1 later half; #152
    only rejects header-unsafe passwords).
  - Run Argon2 in `spawn_blocking` outside the lock; bound unlock/setup
    attempts.
- **Verify:** tests: setup from a non-loopback peer without a token → 403;
  CORS preflight from an unlisted origin rejected; 20 concurrent wrong-password
  requests do not delay a correct-password request beyond one Argon2 round.

### Refuse cross-origin blob URLs (SEC-N4)

- **Where:** `src-tauri/src/sync.rs:823` `resolve_url`; `fetch_blob_data`.
- **Problem:** absolute `http(s)://` `blob_url`s pass through, and
  `fetch_blob_data` attaches `Authorization: Bearer <master password>`. A
  malicious or misconfigured server — or anything answering on a mistyped
  fallback URL — can return `blob_url: "http://elsewhere/..."` and harvest the
  password.
- **Fix:** accept only relative blob URLs, or absolute ones whose origin equals
  the endpoint's; never send the header cross-origin.
- **Verify:** unit test on `resolve_url` with relative, same-origin and
  cross-origin inputs; the cross-origin case returns an error.

### Least-privilege CI and release workflows (OPS-N2)

- **Where:** `.github/workflows/release.yml:8-10` (`contents: write`,
  `packages: write` workflow-wide), `:130-131` (`TAURI_SIGNING_PRIVATE_KEY`),
  `:228` (keystore in workspace), `:234` (keystore password on command line);
  `.github/workflows/ci.yml` (no `permissions:`, inherits the write token when
  called from release).
- **Problem:** build jobs run npm lifecycle scripts, cargo build scripts and
  Gradle with persisted write credentials. The updater signing key is exported
  though no updater is configured.
- **Fix:** default `permissions: contents: read`; grant write per job only where
  publishing; `actions/checkout` with `persist-credentials: false`; drop the
  unused signing key; `--ks-pass env:`; keystore in `$RUNNER_TEMP`;
  `gradlew --stop` after the build.
- **Verify:** `grep -n permissions .github/workflows/*.yml`; an `actionlint`
  run; a release dry-run on a fork tag succeeds.

### Make encryption state and migration crash-safe

Refs: `opus.md` SEC-04/05; `sol.md` SERVER-03/04.

- **Where:** `server/src/crypto.rs` (`ENC:1:` prefix `:292`, `ENCB` `:334` at
  `9ca8179`); `server/src/api.rs` setup (`auth.json` published `:193`);
  `server/src/storage.rs` `encrypt_all_blobs` (`:853`).
- **Problem:** user plaintext beginning with `ENC:1:` or `ENCB` is silently
  treated as ciphertext and stored unencrypted. Setup publishes `auth.json`
  before migration completes, and blobs are rewritten in place.
- **Fix:**
  - Store encryption format/version as schema metadata, not payload prefixes.
  - Always encrypt newly received plaintext.
  - Add durable pending-migration state and startup resume.
  - Use transactions for row metadata and temp-file/atomic-rename for blobs.
  - Verify completion before publishing active auth state.
  - Address SQLite WAL/free-page plaintext retention in the threat model.
- **Verify:** tests that inject a failure at every migration phase and restart;
  a text entry `ENC:1:hello` and a blob starting `ENCB` are stored encrypted.

### Make blob storage crash-consistent

Refs: `opus.md` SEC-06; `sol.md` SERVER-05, OPS-18.

- **Where:** `server/src/storage.rs` blob write (`:432` at `9ca8179`);
  `src-tauri/src/storage.rs` `insert_entry` and remote blob writes.
- **Problem:** final blob paths are written directly and trusted merely because
  they exist; deletion commits DB changes before best-effort file removal. On
  the client, `insert_entry` writes the blob before the row, so an *application*
  crash leaves an orphan rather than a dangling row — but that is ordering, not
  crash consistency: a bare `std::fs::write` with no flush, temp file or rename,
  skipped entirely when the file exists, so power loss can leave a truncated
  blob trusted forever behind a committed row. #114's shared mutex prevents
  concurrent deletion during ingest, not power-loss corruption. The server side
  is unchanged.
- **Fix:** write unique temp files, `fsync`, verify plaintext hash, atomically
  rename; insert rows only after a valid final blob exists; add read-only
  reconciliation for missing, corrupt and orphan blobs, then a repair path once
  diagnostics exist.
- **Verify:** test that a pre-existing truncated file at the final path is
  detected and replaced; reconciliation reports a planted orphan and a planted
  corrupt blob.

### Make the server authoritative for payload identity

Refs: `opus.md` SEC-07; `sol.md` SERVER-06/19; `tmp.md` SYNC-N7.

- **Where:** `server/src/api.rs` create handler (client `content_hash` trusted,
  `:359` at `9ca8179`); `src-tauri/src/sync.rs:606-620` (legacy rows).
- **Problem:** the server trusts the client-provided `content_hash` and accepts
  inconsistent content-type/payload combinations, allowing silent dedup or
  broken rows. Legacy image rows without `blob_hash` force clients to download
  the blob just to compute the hash, even when the entry is already local.
- **Fix:** decode and validate content-specific invariants; compute canonical
  flavour/blob hashes server-side; reject mismatched advisory hashes; document or
  remove externally required hash construction; backfill `blob_hash` for legacy
  rows once (metadata-versioned, like #156) — or at least comment the client
  path.
- **Verify:** API tests: mismatched hash → 400; image without blob → 400; after
  backfill every image row has `blob_hash`.

### Remove authorization/DEK race

Refs: `sol.md` SERVER-07; `opus.md` BUG-08 (overstated).

- **Where:** `server/src/api.rs` handlers (authorize, release lock, later fetch
  DEK); admin Lock button `server/ui/src/App.svelte:139` (at `9ca8179`);
  `server/src/crypto.rs` `verify_and_unlock` (`:144`).
- **Problem:** a global Lock can land between authorization and DEK fetch and
  produce plaintext writes or ciphertext responses — and a plaintext path then
  reaches the latent FTS `MATCH` 500 (SRV-N2). Lock clears the process-wide
  DEK, which is global rather than session-scoped. It does **not** leave
  clients broken: `verify_and_unlock` re-derives on its slow path, so the cost
  is a latency spike and one Argon2id round, not an outage.
- **Fix:** return a DEK snapshot atomically from successful authorization; make
  a missing DEK an error whenever auth is configured; decide whether Lock is a
  global server operation or an admin-session action.
- **Verify:** concurrent lock/create/get/blob test: no plaintext row is ever
  written and no ciphertext is ever returned.

---

## Priority 1: reliability, performance, and UX trust

### Sync correctness and status

#### Key the pull watermark to a server identity (#154 follow-up)

- **Where:** `server/src/api.rs` `/api/health`; `metadata` table in
  `server/src/storage.rs`; watermark storage in `src-tauri/src/sync.rs`.
- **Problem:** #154 resets the cursor when a URL changes, but a server
  wiped, restored or replaced behind an unchanged URL keeps the old
  `(updated_at, id)` watermark, so its older entries are never pulled.
- **Fix:** generate a random per-database `server_id` at first start, return it
  from `/api/health`, store it with the watermark, and reset when it differs.
- **Verify:** e2e test: wipe server data dir, restart on the same port, sync →
  all entries pulled.

#### Validate push responses (found in the #164 review)

- **Where:** `src-tauri/src/sync.rs` `push_entry_with_fallback`.
- **Problem:** any 2xx answer marks the entry synced without reading the body.
  A captive portal or a non-Copywraith server that answers `POST
  /api/entries` with `200` and an HTML page makes entries look synced that
  never reached the server, and they are never pushed again. The pull path
  already reports an unreadable 2xx as `error` (#148). Pre-existing; not
  introduced by the 2026-09-25 PRs.
- **Fix:** parse the created-entry JSON; on failure return
  `Rejection::UnreadableResponse` and keep the entry queued.
- **Verify:** a fake server answering `200` with HTML leaves the entry
  unsynced and reports `error`.

#### Honest sync-status follow-ups (after #148)

- **Where:** `src-tauri/src/sync.rs` status classification;
  `src/lib/util/syncStatusStore.ts`; `StatusBar.svelte`; Sync Details.
- **Problem:** #148 adds `unauthorized` and `error`; `tmp.md` also proposed a
  distinct `needs_setup` for 403 "setup required", which #148 does not add, and
  #148's review deferred tests for legacy `checking`/`disabled` states and the
  `error` summary branch.
- **Fix:** add `needs_setup` if the maintainer wants it distinguished; add the
  deferred tests.
- **Verify:** `scripts/tests/sync-status.test.mjs` cases for each state.

### Deletion, retention, backup, and storage visibility

Refs: `sol.md` SERVER-14/20; Product roadmap (Graveyard, midnight ritual).

#### Synchronized tombstones (#113)

- **Where:** both schemas, server API, all three clients.
- **Problem:** **entirely unbuilt.** Nothing deletes across devices; a cursor
  reset (now also triggered by #154 on server change) or a later server update
  restores a local deletion.
- **Fix:** design first. #95 was rejected for: index created before the column
  migration (existing DBs cannot start), local and server ids conflated,
  recopying a deleted entry suppressed, and a POST in flight during a delete
  acknowledged after it. The design must also define tombstone expiry and
  conflict-safe eventual purge.
- **Verify:** e2e: delete on A, sync A and B, reset B's cursor, sync → still
  deleted; recopy on A after delete → entry returns everywhere.

#### Undo / Graveyard before permanent deletion

- **Where:** `src-tauri/src/commands.rs` delete command; `EntryRow.svelte`.
- **Problem:** client delete is immediate with no confirmation or undo (the admin
  UI confirms, the client does not).
- **Fix:** soft-delete to a 24 h Graveyard with restore, then purge; confirm
  only for starred/sensitive/bulk.
- **Verify:** storage test: deleted row hidden from list, restorable within the
  window, purged after it.

#### Retention limits

- **Problem:** **nothing bounds growth** — DB and blob directory grow forever on
  every device; sensitive entries (and, before #147, copied passwords) are kept
  forever.
- **Fix:** configurable age/count/byte retention with starred exclusion and an
  exact preview; optional shorter expiry for sensitive/OTP entries (see
  *Vanishing spirits*). Needs tombstones to propagate.
- **Verify:** storage test with a clock: entries past the limit are removed,
  starred ones kept; preview count equals removed count.

#### Storage visibility, export/import, recovery warning

- **Problem:** no client shows any storage figure; the server exposes
  `entries_count` on `/api/health` only when authorised and the admin UI does
  not display it. There is no export, and losing `auth.json`/password makes data
  unrecoverable without warning.
- **Fix:** show DB/blob/staging usage with a cleanup preview; add encrypted
  versioned export/import with integrity verification; warn at setup and
  password change that losing `auth.json`/password loses the data.
- **Verify:** export → wipe → import round-trip test with hash verification.

### Portable file semantics (incl. AND-N7 residual, AND-N8)

Refs: `opus.md` UX-07; `sol.md` SYNC-08, SERVER-17, ANDROID-03; `tmp.md` AND-N7,
AND-N8.

- **Where:** `src-tauri/src/commands.rs:190-196` (at `9ca8179`, Android hard
  errors for image/file actions) and `:685-689` (MIME trust);
  `src-tauri/src/native_clipboard.rs` `write_files`.
- **Problem:** macOS syncs absolute source-machine paths; Android retains bytes
  no client can open, share or save — entries sync down, occupy storage, render
  thumbnails, then refuse every action. Desktop paste writes `file://{name}` for
  Android file entries, so a normal share pastes an invalid `file://report.pdf`
  (#151 now reduces the name to a sanitised basename, but the desktop consumer
  was not changed). Android trusts the sender's `image/*` MIME, so HEIC, AVIF and
  SVG become Image entries no client decodes.
- **Fix:** decide whether path-only entries are local-only; store managed bytes
  with safe filename, MIME and size; never treat Android file entries as desktop
  paths — materialise temp files on macOS/Linux for paste and Quick Look; add
  Android FileProvider content-URI Open/Save/Share/Copy; stream server files with
  content-disposition and optional ranges; classify images by magic bytes, fall
  back to File.
- **Verify:** test that pasting a remote Android file entry on desktop writes a
  real temp file URI; HEIC bytes shared as `image/heic` are stored as File.

### Local privacy and secret handling

#### Add a CSP and stop exposing the master password to WebViews (SEC-N2, ADM-N8)

- **Where:** `src-tauri/tauri.conf.json` (`"security": {}`);
  `commands::get_settings` (returns `api_key`, shown in
  `SettingsDialog.svelte:41`); admin `server/ui/src/lib/api.ts:4,59-69`
  (password in `sessionStorage`); `server/src/main.rs:105` (`ServeDir`, no
  security headers).
- **Problem:** the popup renders untrusted clipboard content. Svelte escapes it
  today (no known XSS; admin `{@html}` only in the library's `BalloonHelp` with
  static strings, images via `blob:` URLs). But the first unsandboxed rich
  preview would give any copied page script access to Tauri IPC — paste
  simulation and the password that decrypts the whole server. The admin UI's
  `sessionStorage` copy survives tab duplication.
- **Fix:** strict CSP in `tauri.conf.json` (`default-src 'self'; img-src 'self'
  data: blob:; style-src 'self' 'unsafe-inline'; connect-src ipc:
  http://ipc.localhost`); `get_settings` returns `has_api_key: bool` and save
  accepts an optional new value; admin keeps the password in memory and the
  server sends `Content-Security-Policy: default-src 'self'; frame-ancestors
  'none'`; any future rich preview renders in a sandboxed `iframe srcdoc`
  without `allow-scripts`.
- **Verify:** needs a runtime check on each platform — a wrong CSP blanks the
  window. Unit test that `get_settings` output contains no password; server test
  for the header; manual smoke on macOS, Linux, Windows, Android.

#### Move credentials to the OS keystore

Refs: `sol.md` MAC-10, ANDROID-18, SERVER-10.

- **Where:** `src-tauri/src/storage.rs` settings table (`:542` at `9ca8179`).
- **Problem:** the master password is stored in plain SQLite on every client (and
  AND-N1 showed how that DB could leak); revoking one device means changing the
  password everywhere.
- **Fix:** macOS Keychain, Windows Credential Manager, Secret Service on Linux,
  Android Keystore; migrate and delete the SQLite copy. Pairs with per-device
  tokens (P0).
- **Verify:** after migration the settings table holds no password; sync still
  authenticates.

#### Encrypted local storage, backup rules, app lock

- **Fix:** encrypted local SQLite/blob/staging storage with a wrapped app data
  key; Android backup/data-extraction rules (`android:dataExtractionRules`,
  `fullBackupContent`); app lock/biometric; a sensitive Recents-screen policy
  (`FLAG_SECURE` when sensitive rows are visible); document residual raw-hash and
  metadata leakage on the server.
- **Verify:** assert the generated manifest contains the extraction rules;
  `adb backup` excludes app data.

#### Restrict local file and socket permissions (OPS-N6, OPS-N7)

- **Where:** client data dir created at `src-tauri/src/lib.rs:91` with default
  mode; `auth.json` created by `File::create` in `server/src/crypto.rs:444`
  (0644, as root in the container, inside the tracked `./copywraith-data`);
  Linux socket fallback `src-tauri/src/linux/mod.rs:94-101`
  (`/tmp/copywraith-<uid>.sock`).
- **Problem:** other local users can read history and the server auth file.
  Without `XDG_RUNTIME_DIR`, a pre-created `/tmp` listener makes every launch
  forward and exit; stale-file removal fails silently under the sticky bit,
  bind failure is only logged, and there is no peer-UID check.
- **Fix:** 0700 dirs / 0600 files on the client; `OpenOptions` with mode 0600 for
  `auth.json`; umask 077 or `user:` in compose; socket fallback in a private
  0700 dir, `lstat` ownership check, `SO_PEERCRED` peer-UID check.
- **Verify:** unit test on the created modes (`PermissionsExt`); test that a
  socket owned by another uid is refused.

#### Local-only entries for "no cloud upload" markers (#147 follow-up)

- **Where:** `src-tauri/src/native_clipboard.rs` `NativeClipboard::read`
  (#147 adds the marker check); entry schema; push filter in `sync.rs`.
- **Problem:** Windows `CanUploadToCloudClipboard = 0` means "local history
  allowed, no cloud upload". #147 only skips content that forbids recording
  outright (`ExcludeClipboardContentFromMonitorProcessing`,
  `Clipboard Viewer Ignore`, `CanIncludeInClipboardHistory = 0`); skipping this
  marker would be wrong and ignoring it uploads it.
- **Fix:** add a `local_only` column; capture sets it; push never sends such
  rows; show a small local-only glyph.
- **Verify:** native clipboard test on Windows CI with the format set → entry
  stored with `local_only = 1`; sync test asserts it is never POSTed.

#### Opt-in capture of transient/auto-generated content (SEC-N1 follow-up)

- **Problem:** #147 skips `TransientType`/`AutoGeneratedType` along with
  `ConcealedType`; some users want the former captured.
- **Fix:** a Settings toggle (default off) that exempts only those two markers.
- **Verify:** unit test of the marker predicate with the toggle on and off.

#### Honour Android `EXTRA_IS_SENSITIVE` (AND-N6)

- **Where:** Shizuku helper `ShizukuClipboardService.kt:179-182` (full
  `ClipData`, uploads directly); app-side capture in `CopywraithSharePlugin.kt`.
- **Problem:** neither path checks `ClipDescription.EXTRA_IS_SENSITIVE`
  (Android 13+), the Android counterpart of the desktop markers.
- **Fix:** skip such clips (or store them local-only, masked).
- **Verify:** device test on API 33+: copy with the extra set → nothing staged,
  nothing uploaded. Kotlin cannot be built in the review container.

#### Let users override sensitive detection (SEC-N5)

- **Where:** `crates/copywraith-core/src/sensitive.rs:112-121`;
  `src-tauri/src/commands.rs:146-155` (`get_entry_text` masks too).
- **Problem:** any `token|secret|password\s*[:=]\s*\S+` is flagged, so
  `let token = parse(input);` is masked in list and preview with no reveal and no
  way to clear the flag.
- **Fix:** press-and-hold "Reveal" in the preview; a "Not sensitive" action that
  clears the flag locally.
- **Verify:** command test: clearing the flag makes `get_entry_text` return the
  full text.

#### Per-app capture/sync exclusion

- **Problem:** `source_app` is tracked but unused for exclusion. Pause shipped
  in #161; per-app rules and an incognito mode remain.
- **Fix:** exclusion list in Settings matched against `source_app`/bundle ID
  before storing.
- **Verify:** capture test with an excluded source → no row.

### Server scalability and integrity visibility

Refs: `opus.md` PERF-06/07; `sol.md` SERVER-11..18, MAC-12; `tmp.md` SRV-N2,
SRV-N3, SRV-N4, ADM-N5.

#### Move blocking work off async executors

- **Where:** every handler in `server/src/api.rs`; `/api/health` takes the
  crypto lock and runs `COUNT(*)` (SRV-N4).
- **Problem:** blocking SQLite, file, Argon2 and parser work runs behind one
  global mutex on Tokio worker threads; the natural liveness probe contends with
  it.
- **Fix:** `spawn_blocking` + a small connection pool; `/api/health` liveness
  without lock or count.
- **Verify:** load test: health stays under 50 ms while a large list request
  runs.

#### Decide the fate of FTS5 and encrypted search (SRV-N2, ADM-N7)

- **Where:** `server/src/storage.rs:559-563` (decrypt-and-scan path); FTS
  triggers skip `ENC:1:` rows; `ARCHITECTURE.md` FTS claims; admin search
  `server/ui/src/App.svelte:153`.
- **Problem:** encryption is mandatory (`ensure_authorized` requires a
  password, so a DEK is always present), so `entries_fts` stays empty and search
  always full-scans (SYNC-A6). If the plaintext path is ever reached (e.g. the
  DEK race), the raw term goes into `MATCH`: **verified** on SQLite 3.45 that
  `user@example.com`, `https://x`, `a-b`, `e-mail`, `AND` and a lone `"` raise
  errors → HTTP 500. Admin search is not trimmed; spaces return `total=0` with
  no hint.
- **Fix:** either drop FTS and the doc claims, or keep it for an explicit
  unencrypted mode and quote the term as an FTS5 phrase (`"..."`, doubled
  quotes). Real encrypted search needs client-side search or a blind index.
  Trim search input in the admin UI.
- **Verify:** storage test for each listed term → no error.

#### Make blob responses cacheable (SRV-N3)

- **Where:** `server/src/api.rs:602-646` (only `Content-Type` today).
- **Fix:** blobs are content-addressed and immutable: add
  `Cache-Control: private, max-age=31536000, immutable` and `ETag` = hash;
  honour `If-None-Match` with 304. The admin UI then stops re-downloading
  thumbnails.
- **Verify:** server test for headers and the 304 path.

#### Normalise cursor timestamps (ADM-N5)

- **Where:** `server/src/api.rs` list handler, `before_updated_at`.
- **Problem:** **verified live:** the DB stores `…+00:00` (`to_rfc3339`) but JSON
  returns `…Z`; since `'+' < 'Z'`, a cursor built from a JSON timestamp returns
  the boundary row again. Latent today (admin uses offsets; the native client
  reformats with `to_rfc3339()`), but the ADMIN-10 cursor fix would hit it.
- **Fix:** parse `before_updated_at` as RFC 3339 and re-serialise with the
  storage format before comparing.
- **Verify:** API test paging with a `Z` cursor returns no duplicate row.

#### Parser, error and health hardening

- **Fix:** linear HTML parsing (`strip_html`/`strip_rtf` allocate a `Vec<char>`
  over the whole document); fuzz HTML/RTF/auth decoders (`cargo fuzz`);
  propagate SQLite errors instead of turning corruption into 404/healthy;
  distinguish liveness, readiness, migration and integrity health.
- **Verify:** fuzz targets run in CI for a bounded time; a corrupted DB returns
  5xx and `ready: false`.

### Desktop capture and paste

#### Prefer text when an image is only a rendering (CAP-N3)

- **Where:** `src-tauri/src/native_clipboard.rs:69-74`.
- **Problem:** `Image` wins whenever advertised. Office, LibreOffice Calc and
  some browsers put a rendered picture next to copied text or cells, so history
  stores a picture of the cells. **Confirm on macOS/Windows before changing.**
- **Fix:** if non-empty `text/plain` exists and the HTML is not just a lone
  `<img>`, store the text flavours (ideally keep the image as an extra flavour).
- **Verify:** native clipboard test writing text + HTML + image → text entry.

#### Fire plaintext paste on key release (CAP-N6, OPS-N5)

- **Where:** `src-tauri/src/lib.rs:315` (`ShortcutState::Pressed`; popup
  shortcuts use `Released`); KDE `src-tauri/src/linux/kde.rs:152`; GNOME custom
  bindings; fixed 140 ms sleep in `src-tauri/src/linux/mod.rs:39`.
- **Problem:** with `Cmd/Ctrl+Shift+Alt+V` still held, the synthetic paste
  arrives with modifiers down — "Paste and Match Style" or nothing on macOS,
  Paste Special (Ctrl+Shift+V) in LibreOffice, or the shortcut itself.
- **Fix:** fire on release where the mechanism supports it; otherwise wait until
  modifiers lift (poll with a bound) before injecting.
- **Verify:** device check on macOS, X11, KDE, GNOME; unit test of the
  wait-for-release helper with a fake key-state source.

#### Blank plain/rich fallback, preview image MIME (older follow-ups)

- **Problem:** from #117: an entry whose plain flavour is blank but rich flavour
  is not falls back inconsistently. From #114: image decode errors and the
  hardcoded PNG MIME in the preview dialog are untouched.
- **Fix:** one fallback rule in `models.rs` used by list, preview and paste;
  preview uses the stored MIME and shows a decode-error state.
- **Verify:** model tests for blank-plain + HTML; a JPEG entry previews.

### macOS utility lifecycle and paste quality

Refs: `opus.md` UX-08; `sol.md` MAC-01..09, MAC-14; `tmp.md` CAP-N7, UX-N4.

#### Menu-bar presence and launch at login

- **Where:** `src-tauri/src/lib.rs` tray setup (`:734` at `9ca8179`) is
  `#[cfg(target_os = "linux")]`.
- **Problem:** macOS has **no menu-bar presence and no launch-at-login**; the app
  is a hotkey with no way to quit or reach Preferences without the popup.
- **Fix:** menu-bar item with History, Pause (reusing #161's pause state),
  Preferences, Quit; Dock reopen-to-show; launch at login
  (`SMAppService`/autostart plugin). Put the Pause item in the tray on every
  platform (#161 follow-up).
- **Verify:** manual macOS check; unit test that the tray menu builder includes
  the Pause item on all targets.

#### Permissions, activation and self-event suppression

- **Fix:** preflight Accessibility/Automation permissions with Settings links and
  a test; track PID/bundle ID and use native activation/keystroke APIs where
  practical; suppress only matching self-generated pasteboard events (by change
  count) instead of a 500 ms window.
- **Verify:** unit test that a foreign change inside 500 ms of a self-write is
  captured.

#### Clipboard monitor health and recovery UI (#117 follow-up)

- **Problem:** a failed or stalled monitor is invisible.
- **Fix:** retry with backoff and surface monitor health in the status bar and
  the error banner (UX-N5).
- **Verify:** test with an injected monitor error → status reports it and the
  monitor restarts.

#### Source attribution by workspace notifications (CAP-N7)

- **Where:** `src-tauri/src/paste.rs:23-44`, `detect_frontmost_app_name`.
- **Problem:** polls every second forever and spawns `osascript` whenever the
  native lookup returns `None` — which it does whenever Copywraith is frontmost,
  i.e. a process spawn per second while the popup is focused.
- **Fix:** `NSWorkspaceDidActivateApplicationNotification`; drop the poll and the
  `osascript` fallback.
- **Verify:** no `osascript` child processes while the popup is open.

#### Transactional shortcut registration with a key recorder (UX-N4)

- **Where:** `src/lib/components/SettingsDialog.svelte:284-317` (free text);
  `register_shortcuts` in `src-tauri/src/lib.rs:283` (only `log::warn!`).
- **Fix:** key-recorder field; validate before save; register the new set
  transactionally and keep the last valid set on failure; report in-process
  registration failures through `shortcut_status`.
- **Verify:** unit test: an invalid accelerator leaves the old set registered
  and returns an error.

### Linux desktop integration

Refs: `tmp.md` OPS-N3, N4, N9, N10, N12; #161 follow-up.

#### Single-instance startup race (OPS-N3)

- **Where:** `src-tauri/src/lib.rs:51` (check) vs `:151` (listener bind, after
  webview, storage, shortcuts and tray); `src-tauri/src/linux/mod.rs:146`
  (deletes any existing socket).
- **Problem:** during a cold start, a second `copywraith --toggle` (GNOME Wayland
  shortcuts spawn one) starts a second full instance: two trays, monitors and
  sync loops on one DB; on KDE whichever exits first calls `setInactive`,
  killing shortcuts for the other.
- **Fix:** bind first; on "address in use" try to forward, else remove the stale
  socket and retry once (or `flock` a lockfile).
- **Verify:** integration test launching two instances concurrently → one
  survives, the other forwards.

#### Autostart for AppImage (OPS-N4) and flagless launch (OPS-N9)

- **Where:** `src-tauri/src/linux/mod.rs:223-230` (unquoted `Exec=` from
  `current_exe()`), `:203` (checks only file existence);
  `src-tauri/src/linux/shortcuts.rs:571-576` (`$APPIMAGE` handling);
  `src-tauri/src/lib.rs:780-795` (flagless launch ignored).
- **Problem:** in an AppImage `current_exe()` is a temporary `/tmp/.mount_…`
  path, yet the tray shows autostart checked. Launching from the app menu
  (`.deb` entry `Exec=copywraith`) shows nothing because the popup starts
  hidden.
- **Fix:** reuse `launch_command` and quote per the Desktop Entry spec; treat a
  flagless forwarded launch as "show"; add `--background` for autostart.
- **Verify:** unit tests for the generated `Exec=` with spaces and `$APPIMAGE`.

#### Tray pause toggle (#161 follow-up)

- **Fix:** add the pause state to the Linux tray (checkable item or submenu
  5 min / 1 h / until resumed), synced with the status-bar control.
- **Verify:** unit test that toggling from the tray updates the shared pause
  state.

#### Idle wakeups and notification hygiene (OPS-N10, OPS-N12 minor)

- **Where:** `src-tauri/src/linux/kde.rs:99-103,157-160` (polls
  `GetNameOwner` every 250 ms, ~4 idle wakeups/s);
  `src-tauri/src/linux/mod.rs:57-64` (`notify-send` never waited on — a zombie
  per use).
- **Fix:** subscribe to `NameOwnerChanged` with a slow safety poll; reap the
  `notify-send` child; make the paste-failure notification name the actual
  cause (ydotool missing vs daemon vs `/dev/uinput`) instead of always "Install
  ydotool".
- **Verify:** mock-bus test that an owner change is seen without polling; no
  zombie after a notification.

### Android lifecycle and privileged capture

Refs: `opus.md` SEC-08; `sol.md` ANDROID-06, ANDROID-08..11/15/16/20; `tmp.md`
AND-N2, AND-N3, AND-N4, AND-N5, AND-N9, SEC-N3.

#### Fix the Shizuku helper's calling package (AND-N2, High)

- **Where:** `ShizukuClipboardService.kt:36,195,237` (passes
  `ch.lkmc.copywraith` as `callingPackage` while running as uid 2000/0),
  `:297-299` (transaction codes); `CopywraithSharePlugin.kt:72-79`.
- **Problem:** AOSP `ClipboardService.clipboardAccessAllowed` runs
  `checkPackage(uid, callingPackage)` (bypassed only for root) and checks
  `READ_CLIPBOARD_IN_BACKGROUND` by package, which `com.android.shell` holds.
  API 33+ shell mode: `getPrimaryClip` throws and is swallowed (silent
  listener). API 30–32: the `SecurityException` is delivered **to whichever app
  just copied** via its `setPrimaryClip()`, and persists after Copywraith dies
  because the helper is a daemon. Root mode reads return null. The plugin then
  overwrites the helper's error with `"listening"`. Codes 3/6/7 follow the
  Android 9 layout; on 7.x/8.x (minSdk 24) 6 is **remove**Listener.
- **Fix:** pass `com.android.shell`; do a test read before `addListener` and fail
  `start()` on error; stop overwriting helper status; per-API transaction codes
  or raise minSdk for Shizuku.
- **Verify:** device matrix API 29, 31, 33, 34: copy in another app → captured,
  no exception in that app. Maintain an Android/OEM compatibility matrix for
  private Binder calls.

#### Make Shizuku capture durable and local-first

- **Where:** `src-tauri/src/lib.rs:154` (at `9ca8179`, hands helper URL and API
  key); `ShizukuClipboardService.kt:151` (direct upload).
- **Problem:** the helper uploads clipboard text **directly to the server over
  plain HTTP from a privileged process**, bypassing local storage; if the upload
  fails, the capture is lost. Direct capture can actively unstar server entries.
  Staging is local only while the app callback is alive.
- **Fix:** stage to app storage first with acknowledgement; encrypt before
  upload; bounded durable retry/backoff and process-death recovery; reconfigure
  the running service after URL/password changes; idempotent listener
  registration and binder-death handling.
- **Verify:** device test: server down, copy 3 items, kill app, restore server,
  reopen → all 3 synced, star state unchanged.

#### Fix plugin permissions and wire the staged-clipboard event (AND-N3, SEC-N3)

- **Where:** `src-tauri/build.rs:2-8`, `src-tauri/capabilities/mobile.json`,
  `src-tauri/capabilities/default.json`; `src/routes/+page.svelte:123-128`;
  `package.json`.
- **Problem:** `addPluginListener` needs `register_listener`/`remove_listener`,
  which are not granted, and the page hides the failure, so Shizuku captures
  appear only on next focus. Meanwhile `mobile.json` grants two unused, dangerous
  commands: `startShizukuClipboardListener` (lets a script point the privileged
  daemon at any URL/key) and `readShizukuClipboard` (raw text, bypasses masking).
  On desktop, `default.json` grants `global-shortcut:allow-register`,
  `allow-unregister`, `allow-unregister-all` and `allow-is-registered` although
  shortcuts are registered in Rust and the npm
  `@tauri-apps/plugin-global-shortcut` is unused — a compromised WebView could
  rebind system-wide shortcuts.
- **Fix:** grant the listener permissions, drop the two unused commands and the
  four global-shortcut permissions plus the npm package; wire the event to a
  debounced import+push (SYNC-A8), not the full `refreshMobileEntries`.
- **Verify:** `grep` the capability files; device test that a Shizuku capture
  appears without a focus change.

#### Staged import order and staging hygiene (AND-N5, AND-N9)

- **Where:** Kotlin writes `created_at` and sortable filenames
  (`CopywraithSharePlugin.kt:376-377`); `src-tauri/src/commands.rs:362-367`
  ignores `created_at`, imports in `read_dir` order and stamps `Utc::now()`;
  `coerceToText(null)` on URI clips (`kt:182`); `pending-shares/failed/`.
- **Problem:** staged items import shuffled with import-time timestamps; every
  image/file copy flips status to "error"; failed batches and the files they
  reference (up to 64 MiB each) are never retried or deleted.
- **Fix:** sort by filename and carry `created_at`; guard `coerceToText` for
  URI clips; retry failed batches with a cap, then delete batch and files.
- **Verify:** Rust test importing three staged files in reverse `read_dir`
  order → stored in filename order with their `created_at`.

#### Android HTTP client: confirm compression, trust user CAs (AND-N4)

- **Where:** `src-tauri/Cargo.toml:33` (reqwest `rustls-tls`, bundled webpki
  roots).
- **Problem:** `tmp.md` found no `gzip`/`zstd` feature, so a server
  `CompressionLayer` did nothing for Tauri clients. #150 adds `gzip` to the
  shared reqwest build, which Android compiles too; its review declined the
  claim that Android is unverified, but no device check was made. `rustls-tls`
  trusts only bundled roots, so a LAN server with a private CA (installed as a
  user CA on Android) fails, pushing users to plain HTTP.
- **Fix:** after #150, confirm on a device that requests carry
  `Accept-Encoding: gzip` (optionally add `zstd`); switch to
  `rustls-platform-verifier` or add a pinned-fingerprint setting.
- **Verify:** device capture through a logging proxy; sync against a server with
  a user-installed CA succeeds.

#### Lifecycle, progress and network policy

- **Fix:** replace focus-as-resume with lifecycle events (SYNC-A8); one
  backend-owned sync deadline/progress/cancellation model shared by loop and
  Sync Now; pull-to-refresh; byte/item progress; Wi-Fi/metered/battery policy;
  track or assert final generated Gradle/manifest security settings.
- **Verify:** script in `scripts/` asserting the generated manifest (exported
  components, backup rules, cleartext policy).

### Popup usability, accessibility, and responsiveness

Refs: `opus.md` UX-01/02/04/06, BUG-12/13, PERF-04; `sol.md` UI-07, UI-09..18,
MAC-13, ANDROID-13/14; `tmp.md` UX-N2 remainder, UX-N3, UX-N5, UX-N6.

#### Single-click selects, explicit action pastes (product call)

- **Problem:** clicking a row pastes and hides the popup, so you cannot browse,
  inspect or correct a mis-click (UX-01). *Deliberately not done — it changes
  established muscle memory and is the maintainer's decision.*
- **Fix (if approved):** click selects; double-click/Enter pastes.

#### Persistent sync/monitor error state (UX-N5)

- **Where:** `src/routes/+page.svelte:37` (`errorMessage` declared, `ErrorBanner`
  rendered, never set).
- **Problem:** sync failures are transient toasts; a user who looks away never
  learns sync is broken. #148 adds `unauthorized`/`error` states to the status
  bar but not a persistent banner.
- **Fix:** set `errorMessage` from persistent conditions (unauthorized, monitor
  failed, permanent push failures) with a Retry action; or remove the dead
  banner. The Séance Log idea covers history.
- **Verify:** component test: an `unauthorized` status shows the banner until a
  successful sync.

#### Contextual empty states and first-run onboarding

- **Problem:** "No clipboard entries" is shown whether history is empty, the
  filter matched nothing, starred-only is on, or loading failed; first run gets
  no onboarding at all.
- **Fix:** distinct messages per cause; a first-run box (mascot slot).
- **Verify:** unit test of the empty-state selector for each cause.

#### Loaded vs total count and `has_more`

- **Where:** `StatusBar.svelte` (`$entries.length`); `clipboardStore`
  (`has_more` from `result.length === PAGE_SIZE`).
- **Problem:** 5,000 entries read "100 items"; `has_more` is wrong when the
  total is a multiple of 100.
- **Fix:** return `has_more` (limit+1) and a total from the backend; show
  "100 of 5,000".
- **Verify:** store test with exactly 200 rows → `has_more` false after page 2.

#### Hover distinct from selection (UX-N3)

- **Where:** `src/lib/components/EntryRow.svelte:285-293`.
- **Problem:** `:hover` and `.selected` share the black highlight; with the mouse
  over one row and keyboard selection on another, two rows look selected and
  Enter may paste the unexpected one.
- **Fix:** full inversion for selection only; System 7 dotted outline or 50 %
  grey pattern for hover.
- **Verify:** visual check on each platform.

#### Remaining keyboard coverage (UX-N2 remainder)

- **Where:** `src/routes/+page.svelte:417-471`, `FilterBar.svelte:46-72`.
- **Problem:** #158 added Mod+1..9 (with the digits shown while Mod is held),
  Mod+S, Mod+Y preview, Mod+F filter focus, Mod+Backspace, Shift/Alt+Enter,
  PageUp/Down, Home/End and a status-bar hint with a full tooltip. Still
  missing: a plain `Delete` key, type-to-filter and a `?` cheat sheet. The
  window handler's Enter paste also fires while a status-bar button has focus
  (the sync status, Pause and its menu items), so Enter cannot activate them;
  list shortcuts still act while the Pause menu is open; and the menu stays
  open when the popup is shown again.
- **Fix:** extend #158's shared handler for the filter field and the list.
- **Verify:** extend #158's `scripts/tests` keyboard cases.

#### Accessible list semantics

- **Problem:** `tr role="button"` contains real `<button>` children.
- **Fix:** grid/listbox with roving tabindex.
- **Verify:** `npm run check` a11y warnings clean; screen-reader smoke test.

#### Preview timestamps (UX-N6)

- **Where:** `src/lib/components/EntryPreview.svelte:121-123` (`created_at`)
  vs row (`updated_at` relative time).
- **Fix:** show "First copied" and "Last copied".
- **Verify:** component snapshot shows both.

#### Platform-ready initial shell

- **Problem:** `platform` starts `''`, so Android renders the desktop shell
  (title bar, "Click to paste · Opt+Click…") for the first frames of every cold
  start.
- **Fix:** resolve platform before first render (inject at boot) or render
  nothing until known.
- **Verify:** Android cold start shows no desktop chrome.

#### Client-side search index

- **Problem:** search is `search_text LIKE '%…%'`, an unindexed full scan per
  keystroke; the server has FTS5 (unused under encryption, SRV-N2).
- **Fix:** local FTS5 table (client DB is plaintext today) with trigram
  tokenizer; debounce input.
- **Verify:** storage test for substring and multi-word queries; timing on 50k
  rows.

### Admin usability and responsive management

Refs: `sol.md` ADMIN-02/03, ADMIN-05..17; `tmp.md` ADM-N2, N3, N4, N6, N7, N9,
N10, N11.

#### Mobile layout

- **Problem:** **not a single media query** in the admin UI; the five-column
  fixed-width table overflows on a phone with no fallback.
- **Fix:** stacked-card layout below ~640 px; viewport-safe dialogs.
- **Verify:** Playwright screenshot at 375 px width without horizontal scroll.

#### Sorted by last use, labelled "Created" (ADM-N2)

- **Where:** `server/src/storage.rs:619,674` (`updated_at DESC`);
  `server/ui/src/App.svelte:65`, `EntryRow.svelte:113`, `App.svelte:219`.
- **Problem:** starring an entry on page 3 moves it to the top of page 1 and the
  list looks unsorted.
- **Fix:** label "Last used" (or show both); apply PATCH responses in place
  instead of reloading.
- **Verify:** vitest: starring updates the row without a list request.

#### Downloads: any blob type, never the mask (ADM-N3)

- **Where:** `server/src/api.rs:497` (`get_entry` masks sensitive);
  `server/ui/src/App.svelte:289-306` (`triggerDownload`).
- **Problem:** downloading a sensitive entry saves `sk-•••…` as if it were the
  secret; non-image blobs lack filename/MIME and explicit errors.
- **Fix:** disable or label Download for masked entries (or fetch with
  `include_sensitive`); download any blob type with filename/MIME.
- **Verify:** vitest: Download is disabled for a masked entry.

#### Failed Lock must still lock (ADM-N4)

- **Where:** `server/ui/src/lib/api.ts:136-140`; `App.svelte:141-144`.
- **Problem:** the session is cleared only after `await fetch`; a network error
  skips it and is swallowed, so a reload goes straight back in.
- **Fix:** clear session first, unconditionally; report the server-side failure.
- **Verify:** vitest with a rejecting fetch → `sessionStorage` empty, error
  shown.

#### Request lifecycle: errors, timeouts, double delete (ADM-N9)

- **Where:** `ConfirmDialog` window-capture Enter handler; `App.svelte:314-318`.
- **Problem:** a double Enter sends a second DELETE → 404 → "Failed to delete".
  Unauthorized transitions and API errors are handled ad hoc.
- **Fix:** guard in-flight per row; treat 404 on delete as success; centralise
  unauthorized transitions and typed errors; request timeouts/cancellation and
  per-row operation states.
- **Verify:** vitest: two confirms → one DELETE.

#### Subpath deployment (ADM-N6)

- **Where:** `server/ui/src/lib/api.ts:97-98` (`return resp.json()` without
  `await` inside `try`).
- **Problem:** a 200 HTML catch-all at the root escapes the `try`, so the
  subpath candidate is never tried (new evidence for ADMIN-15).
- **Fix:** `await`, validate the response shape, try the most specific candidate
  first; complete reverse-proxy subpath asset support.
- **Verify:** vitest with a root that returns HTML → subpath chosen.

#### Accessibility (ADM-N10)

- **Where:** `App.svelte:347` (`/` focuses search even with a dialog open);
  `EntryRow.svelte:76-83` (star name is the glyph, no `aria-pressed`).
- **Fix:** ignore `/` when a dialog is open; label star with `aria-pressed`;
  label the search input; announce page range and loading state (live region).
- **Verify:** axe check in vitest/Playwright.

#### Detail dialog cost (ADM-N11)

- **Where:** `server/ui/src/lib/components/EntryDetail.svelte:68`.
- **Problem:** up to 10 MiB of text in one `<pre>` (native client caps at 500k
  chars); opening an image re-downloads the blob, Download fetches it a third
  time.
- **Fix:** truncate with "Show all"; reuse the row's object URL; deduplicate
  blob loading between `EntryRow` and `EntryDetail` — **after** adding a type
  check to `server/ui`, not before.
- **Verify:** vitest: opening detail for a loaded image issues no request.

#### Remaining admin items

- Lightweight list DTOs and stable cursor pagination (ADMIN-10; needs ADM-N5
  first). Verify: paging test with concurrent inserts, no gaps or duplicates.
- Semantic password forms and a Security/password-change section.
- Plain/source/rendered rich tabs and useful file metadata.
- Accessible bulk star/delete/export **after** tombstones are correct.
- Fix auth dialog CSS specificity.

---

## Priority 2: engineering and release hardening

Refs: `opus.md` OPS-01..05, FEAT-15; `sol.md` OPS-03, OPS-05..17; `tmp.md`
OPS-N8, OPS-N11, OPS-N12, DOC-N1; PR follow-ups.

### Toolchain and dependencies

- **Keep `typescript: ~6.0.3`** for Kit's compiler API. #117 resolved #111 with
  TypeScript 7.0.2 under the `@typescript/native` alias and `svelte-check`
  4.7.6's `--tsgo` mode; both compilers run in CI. The direct replacement
  proposed by #63 remains incompatible; do not remove TS6.
- **Triage npm advisories** by reachability; record temporary exceptions in the
  repo. Verify: `npm audit --omit=dev` output matches the recorded list.
- **Supported Node/npm in CI and Docker**; verify the claimed package
  release-age policy. Where: `.github/workflows/*.yml`, `Dockerfile`.
- **Pin `tauri-nspanel` by SHA** — a git dependency on a *branch* in
  `src-tauri/Cargo.toml`, which can move under the build at any time. Pin GitHub
  Actions by SHA and Docker images by digest; add Dependabot for Docker base
  images (OPS-N12). Verify: `grep -n 'branch =' src-tauri/Cargo.toml` is empty.
- **Locked builds and provenance:** `cargo build --locked`/`--frozen`; publish
  checksums, SBOM and provenance.
- **Centralize Rust workspace package metadata** and mark private crates
  (`publish = false`).

### Release gating and signing

- **Release gate runs every suite (OPS-N8).** `release.yml:16-18` calls only
  `ci.yml`; `kde.yml` and `clipboard.yml` lack `workflow_call`; nothing checks
  the tag is on `main`. Fix: add `workflow_call` to both and call them; add a
  `git merge-base --is-ancestor "$GITHUB_SHA" origin/main` step. Add
  `timeout-minutes` to CI and release jobs (OPS-N12).
- **Signed Android production APKs**, verified with `apksigner verify`.
- **macOS notarization and Windows signing** required for stable releases.
- **Version synchronization** exhaustive and nonzero on drift (script test in
  `scripts/test_*.py`).

### Server container and deployment

- **Non-root container** with `HEALTHCHECK`, `no-new-privileges`, dropped
  capabilities and amd64/arm64 output; it runs as root with no `USER` today.
  Set umask 077 or `user:` in compose (OPS-N6). Verify: `docker run --rm
  <image> id -u` is not 0.
- **Narrow the Docker build context further.** #162 re-excludes data,
  env, dist, target and tests; `tmp.md` OPS-N1 also recommends allow-listing
  files and narrowing `COPY server/ server/` (`Dockerfile:20`). Verify with
  `moby/patternmatcher` or `docker build --no-cache` plus a context listing.
- **Vendor the Swagger UI assets.** `server/src/main.rs:38,42` (at `9ca8179`)
  loads them from `unpkg.com` at runtime — an external CDN in an app documented
  as VPN-only.
- **Redeploy** builds before stopping, fails on health mismatch, supports
  rollback and real port variables.

### Small follow-ups from the 2026-09-25 PRs

- **IMMEDIATE transaction for `mark_synced_if_unchanged` (#153).** Where:
  `src-tauri/src/storage.rs`. Under a second instance the deferred transaction
  can hit a noisy busy/upgrade error; it self-heals. Fix:
  `TransactionBehavior::Immediate`. Verify: two-connection test, no busy error.
- **One `MIN_PASSWORD_LENGTH` constant (#152).** The length rule now lives in
  server, admin UI and Settings; share it (one Rust const, one TS const, or
  expose it from the API). Verify: `grep -rn MIN_PASSWORD_LENGTH` finds one
  definition per language.
- **Positive-control gzip test (#150).** Declined in review as covered through
  `build_app`; if wanted, add a server test that a large JSON list *is* gzipped
  when requested (the existing test covers the octet-stream negative case).

### Scripts and docs

- **`android-env-persist.sh` can break shell setup (OPS-N11).** Creating
  `~/.bash_profile` (`:47-48,57`) makes bash login shells stop reading
  `~/.profile`; `mv` over the rc file (`:89`) replaces symlinked dotfiles. Fix:
  append to the existing login file; edit in place preserving symlinks. Verify:
  run in a temp `HOME` with a symlinked `.bashrc`.
- **README repeats the LLM disclosure (DOC-N1).** It appears at the top and
  again after the component list (pointing to `memory/AGENTS.md`). Keep one.
- **Fresh-clone docs:** correct command order, target paths, SDK 36
  requirements, and the missing `PASTE_PROBLEM.md` reference.
- **Remove iOS capability claims** until a real dependency/init/build path
  exists.
- **Add `SECURITY.md`**, contribution/release instructions, a changelog, and
  private vulnerability reporting.
- **FTS claims in `ARCHITECTURE.md`** — see *Decide the fate of FTS5*.

---

## Product roadmap

### Aesthetics

**Maintainer position (2026-07-25, respected by the 2026-09-25 review): the
small, varied type sizes, the mixed accent colours, and the absence of dark mode
are how System 7 worked, not defects.** An earlier revision of this document
framed them as a consistency problem; that framing was wrong and has been
removed. Copywraith is a System 7 pastiche and the retro idiom takes precedence
over modern design-system conventions.

**Decided.** #90 and #91 each proposed a type-scale change that contradicts the
position above. #114 integrated their functional fixes and **dropped both
typography changes**: the popup preview stays 24px desktop / 16px touch, the
badges, mixed accents and admin typography are unchanged,
`server/ui/src/App.svelte` was not touched, and there is still no dark mode.
Only the popup's action column widened, to fit the new preview button. Do not
reopen this as a defect.

Defects on their own terms:

- **`filter: hue-rotate()` for the sync progress tone**
  (`StatusBar.svelte:313` at `9ca8179`) cannot hit a specified colour and forces
  a compositing layer. Whatever colours are wanted, name them directly.
- **The status bar has no graceful narrow layout.** Below 920 px the hint is
  `display: none`, leaving an empty grid column, and the endpoint label
  ellipsizes to uselessness rather than degrading to an icon plus colour.
- **No mobile layout for the admin UI at all** — see *Admin usability*.
- **First-run has no empty state or onboarding** — see *Popup usability*.
- **Hover and selection look identical** — see UX-N3 under *Popup usability*.

Ideas that fit the idiom (no dark mode, no type-scale change):

- **Pixel type icons.** Replace the `TXT/HTML/IMG` badges with 16×16 1-bit
  System 7 icons (document, globe, picture, folder), label in the tooltip.
  Reads faster and looks more authentic.
- **Colour swatches.** A small bordered swatch before text entries that are a
  colour (`#ff8800`, `rgb()`, `hsl()`).
- **URL rows.** For a single-URL entry, host in bold and path dimmed; "Open" in
  the preview. No network fetch.
- **Admin, System 7 style:** a "Welcome to Copywraith" startup box instead of
  bare "Loading…"; wristwatch cursor plus the library's barber-pole
  `ProgressBar`; Finder-style inverted selection for the focused row; a 1-bit
  hatched redaction bar with padlock for masked entries (makes ADM-N3 obvious);
  the classic zoom-rectangle animation from row to detail dialog, disabled under
  `prefers-reduced-motion`.

### Power-user features

- **Transform before paste** — trim, to-plaintext, case, JSON pretty/minify,
  URL/base64, shell-quote, line dedupe, Markdown link. Cheap to build, and the
  feature that makes a clipboard manager sticky.
- **Pinned snippets with aliases.** Starred entries are already first-class;
  naming them turns the app into a text expander for free.
- **Quick-paste by number** — shipped in #158 as Mod+1..9, with the numerals
  shown in the first nine rows while Mod is held.
- **Type and source filters** in the client, with **search operators** parsed
  from the filter text: `is:starred`, `type:image`, `app:Safari`,
  `before:2026-09-01`. The admin UI has a content-type dropdown; the client,
  where it matters most, has only starred-only. The backend already has the
  columns; no new UI needed.
- **Rich preview tabs.** `EntryPreview` shows only plain text, discarding the
  HTML/RTF the app goes to real trouble to preserve. Render HTML only in a
  sandboxed `iframe srcdoc` without `allow-scripts`, and only after the CSP
  lands (SEC-N2).
- Fuzzy/FTS search with type, source app, device, date, sensitivity, size.
- **Type-to-filter:** a printable key with the list focused goes to the filter.
- **Drag out** an entry into another app as text, image or file.
- **Paste stack**, starting with **join on paste**: multi-select rows, paste
  joined by newline.
- **Ectoplasm diff:** select two text entries and show a diff; useful for config
  and code snippets.
- OCR/image text; native Quick Look; tags/groups; workspaces.
- Native updater and clear release channel/version information.

### Distinctive delight

The spooky identity is under-exploited. These are cheap and give the app a
personality no competitor has:

- **The ghost sleeps (pause capture)** — shipped in #161: pause
  for 5 min / 1 h / until resumed, "zzz Paused 4m" in the status bar; pairs with
  SEC-N1. Tray items remain (see *Tray pause toggle*, *Menu-bar presence*).
- **Séance Log** — a sync history where each event has a playful name
  ("Summoned 14 spirits from the local plane", "The VPN plane is silent"), with
  the plain diagnostic under every line. Real observability in a costume; it
  also solves the missing persistent sync-error state.
- **Bound spirits** — starred entries never fade and carry a chain glyph;
  unstarred entries desaturate subtly with age, so recency reads at a glance.
- **The Graveyard** — deleted entries rest in a drawer for 24 h with a headstone
  row and one-tap resurrect, then purge. Solves Undo with charm, not a modal.
- **The Ouija board** — a connection diagnostic that spells its answer out
  letter by letter as it walks DNS → TCP → TLS → auth → metadata → blob. Every
  step is a real assertion; the presentation is the joke.
- **Possession badges** — `source_app` is captured but never shown in the popup.
  "Possessed by Safari" is real information and on-theme.
- **The midnight ritual** — retention cleanup with an exact preview: "At
  midnight, 412 spirits older than 30 days will be released. 18 bound spirits
  will remain." Makes a destructive feature feel safe.
- **Ectoplasm tabs** — Plain / Rich / Source / Image / File, named for the
  flavours they reveal (sandboxed, see *Rich preview tabs*).
- **OTP sense** — `sensitive.rs` already detects secret shapes. Detect 6–8 digit
  one-time codes, offer a digits-only copy, auto-expire after 5 minutes.
  Genuinely useful, and thematically perfect for something that vanishes.
- **Vanishing spirits** — sensitive and OTP entries auto-expire locally after a
  configurable time; the row fades as it ages. Extends OTP sense.
- **Haunting statistics** — an "About this haunting" panel: entries captured,
  most-haunted app, busiest hour, bytes held. Uses data already stored.
- **The mascot** — one small dithered ghost, shown *only* on true first run,
  paused, offline, and empty history. Reserved appearances read as craft;
  ubiquity reads as cheap.
- **Ghost trail** — recent-search chips that fade as they age out.
- **Android:** "Bind to Copywraith" in the text-selection menu
  (`ACTION_PROCESS_TEXT`, also enables in-place transforms); a "Summon" Quick
  Settings tile that opens a transparent activity and reads the clipboard while
  focused (one-swipe capture without Shizuku); a transparent share receiver with
  Direct Share targets ("Send to MacBook", "Bind as starred") and a toast
  "1 spirit bound"; "A spirit arrives from MacBook" notifications via
  UnifiedPush/ntfy with a Copy action; a home-screen widget of recent/starred
  entries with sensitive ones blurred.
- Gate all of it on `prefers-reduced-motion` from day one.

Full rationale in `sol.md` sections H and I, `awesome.md` sections 5 and 6, and
`tmp.md` sections 6b, 6c and 8.

---

## Review corrections that must survive consolidation

- The mutable-ID cursor bug affected macOS and Android (fixed; watermark is now
  `(updated_at, id)`).
- Deleting the cursor causes a full scan; repeated scans need another condition
  preventing cursor persistence.
- Blob hash mismatch returns false and can be skipped permanently; it is not an
  ingest error in the reviewed code.
- Shizuku stages locally only while the app callback is alive; detached service
  failure remains lossy, and direct capture can actively unstar server entries.
- The keyed popup list makes wrong-row image reuse unlikely, but eager
  uncancelled image work was a real problem (now fixed).
- Sensitive presentation masking is good; masking the *native sync contract*
  corrupts functionality.
- Prefix-based plaintext/ciphertext passthrough is unsafe for arbitrary bytes.
- `awesome.md` section 9 described planned work, not changes on `main`.
- **(2026-07-25)** the admin RTF stripper was **not** a ReDoS. That claim was
  made and then disproved by measurement — the pattern stays near-linear because
  its greedy `[^}]*` stops at the first `}`. That same property was the real
  bug: it truncated header stripping and corrupted previews.
- **(2026-07-25)** star reconciliation is keyed on `content_hash`, not on the
  server id, and that is deliberate. Two devices copying the same text mint
  different ids, so an id-keyed lookup would miss the locally-captured row.
  Content identity is the right key; do not "fix" it to use the id.
- **(2026-07-25)** entries pulled before #114 keep their pull-time id and
  timestamps. This is an accepted rollout limitation; no automatic expiry
  exists. A destructive backfill was rejected for this rollout: matching local
  rows to server rows by hash and rewriting primary keys is destructive on the
  one table the user cannot re-derive, and ids will key whatever delete
  propagation #113 lands on.
- **(2026-07-25)** do not use `TextDecoder('windows-1252')` for CP1252. It
  depends on the host's ICU data; a Node build without full ICU decodes
  `0x80`-`0x9F` as Latin-1 and produces invisible C1 control characters. This
  passes locally and fails on CI. Use the explicit table in
  `server/ui/src/lib/text.ts`, which mirrors `cp1252_byte_to_char` in
  `crates/copywraith-core/src/content.rs`.
- **(2026-07-25)** `synchronous=FULL` stays on both databases. A local capture
  can be the only copy and a synced row is excluded from later pushes, so
  re-sync cannot be assumed to repair an acknowledged write.
- **(2026-07-25)** the admin Lock button does not leave clients permanently
  broken; `opus.md` BUG-08 overstates it (see *Remove authorization/DEK race*).
- **(2026-09-25)** single-click-to-paste is a product decision, not a defect.
- **(2026-09-25)** svelte-check switches to machine-readable output when it
  detects an AI agent (`AI_AGENT`/`CLAUDECODE`), so any test that parses its
  human summary must pass `--output human` (TOOL-01, #160).
- **(2026-09-25)** the ignored native clipboard tests *do* run in CI (under xvfb
  on Linux). A review claim that they did not was refuted on #147.
- **(2026-09-25)** Android sync runs the same Rust `SyncClient` and reqwest
  build as desktop, so client-side HTTP features (gzip, timeouts) apply to both
  (#150 review). The Kotlin Shizuku helper is the exception — it has its own
  HTTP path.
- **(2026-09-25)** a reqwest connect timeout is classified `is_connect() = true`
  (measured on #148), so it reports as unreachable, not as an HTTP error.
- **(2026-09-25)** `blob_size = Some(0)` is a real empty blob that downloads
  instantly; it must not be treated as "unknown size" (#157).
- **(2026-09-25)** Windows `CanUploadToCloudClipboard = 0` does not forbid
  recording; it forbids upload. Skipping it would be wrong (#147).
- **(2026-09-25)** with encryption active, text fields are independent
  ciphertexts, so equality checks against a recomputed plaintext form always
  differ. That is why the server flavour backfill rewrote every new row on each
  start (SRV-N1); never use "value differs" as a backfill trigger on encrypted
  columns.
- **(2026-09-25)** server timestamps are stored as `…+00:00` but serialised as
  `…Z`; string comparison of the two is wrong (ADM-N5).

---

## Architectural strengths to preserve

- Shared core/server/Tauri/Android separation, and the right shared crate.
- The multi-flavour clipboard model with legacy-compatible hashing
  (`models.rs:149`), including deliberate preservation of single-flavour legacy
  hashes for migration stability.
- `strip_rtf` (`content.rs:191`) — unusually complete for a hand-rolled parser:
  `\uc` fallback skipping, surrogate-pair recombination, CP1252 hex escapes, and
  saturating depth against unbalanced braces, with tests for each.
- `sensitive.rs` — `LazyLock`-compiled patterns, Luhn validation, SSN range
  exclusions.
- Argon2id → HKDF domain separation → random DEK → rewrap on password change,
  including the deliberate choice to make wrong passwords pay the Argon2 cost
  even when unlocked (`crypto.rs:150-160`).
- The macOS NSPanel work (`lib.rs:623`) — main-thread dispatch, `catch_unwind`,
  collection-behaviour verification, retry on failure. The most carefully
  defensive code in the repository.
- Parameterized SQL and hash-validated blob paths; `is_valid_hash` before every
  path join.
- Request-ID guards against out-of-order list responses in both frontends.
- Coherent server keyset ordering.
- The client's explicit rule against loading and rewriting the whole history
  table on launch (`src-tauri/src/storage.rs:123-126`).
- Android storage-permission restraint, filename sanitization, and the optional
  Shizuku fallback model.
- Linux layer, checked and fine on 2026-09-25: ydotool runs without a shell and
  refuses unknown versions; GNOME keybinding quoting is correct; KDE signals are
  sender-checked; `window_activity.rs`; the socket in `XDG_RUNTIME_DIR` accepts
  only three commands from the same user.
- The System 7 / spooky visual identity.

---

## Outcome ledger

Disposition of every PR from the 2026-07-25 and 2026-09-25 reviews. This
supersedes any status claim earlier in this document, in `opus.md`, or in
`tmp.md`.

### Integrated — #164 (2026-09-25 review)

All sixteen PRs merged to `main` through #164, each as its own merge commit so
its history and review record stay attached. A cross-PR review found no
duplicates: each fixes a different finding, and they overlap only in the files
they touch. None was rejected. Each was reviewed in rounds; the columns record
what survived review and what was declined, deferred or refuted, with reasons.
Deferred follow-ups are backlog items.

| PR | Fixes | Kept after review | Declined / deferred / refuted, and why |
|---|---|---|---|
| #147 | SEC-N1 (password-manager copies captured and synced) | Skip content marked concealed/transient/auto-generated (macOS nspasteboard + legacy types; Windows `ExcludeClipboardContentFromMonitorProcessing`, `Clipboard Viewer Ignore`, `CanIncludeInClipboardHistory`=0; Linux `x-kde-passwordManagerHint`) in `NativeClipboard::read`; documented in `SENSITIVE.md`; cfg-aligned test marker, log text. | **Deferred:** Android `EXTRA_IS_SENSITIVE` (AND-N6); Windows `CanUploadToCloudClipboard`=0 needs a local-only flag (capture, never sync), not a skip; opt-in toggle for transient/auto-generated. **Refuted:** a Wayland claim; "ignored tests not in CI" (they run under xvfb). |
| #148 | SYNC-N2 (401/500/locked shown as green "online") | `unauthorized` and `error` states in backend, store, status bar and Sync Details; push batch pauses on 401/403; queue survives the pause (test). | **Declined:** MSRV concern for `Option::is_none_or` (toolchain pinned to 1.98); connect-timeout claim (measured `is_connect()=true`); stale "online" is bounded by 30 s; extra tests for legacy checking/disabled and the `error` summary branch (nice-to-have). |
| #149 | SYNC-N1, SYNC-A7 (first-pass sleep) | One-entry probe page before a full pull; first pass immediately at startup. | Both reviewer findings **refuted** (no `continue` in the loop; the client has timeouts). |
| #150 | SYNC-A5 compression half (the `CompressionLayer` item) | Server `CompressionLayer` (router moved into testable `build_app`); reqwest `gzip`; `application/octet-stream` blobs and tiny bodies not compressed; negotiation tests both ways; client gzip test. | **Declined:** positive-control test (covered through `build_app`). **Refuted:** an Android doc claim ("Android not verified to send `Accept-Encoding`" — same reqwest build). Device confirmation tracked under AND-N4. |
| #151 | AND-N1 (own DB importable via `file://` share), AND-N7 (display name as desktop path) | Refuse `file://` and own-package/own-provider authorities, fail-closed if provider lookup fails; display name reduced to a final component, control chars and bidi overrides rejected. | **Declined:** set-only authority check (prefix check kept intentionally, fail-closed); a desktop consumer audit (residual tracked in *Portable file semantics*). **Refuted:** a final-round claim that returning `false` on lookup failure is fail-open (the only caller drops the share on `false`; the suggested `true` would import it). Kotlin cannot be built in the container — **needs a device test** before merge. |
| #152 | ADM-N1 (header-unsafe passwords lock clients out) | Server, admin UI and Settings reject non-ASCII and whitespace-edged passwords; recovery (delete `auth.json`) documented. | **Declined:** wording nit. **Deferred:** one `MIN_PASSWORD_LENGTH` constant; versioned base64url header (P0 transport item). |
| #153 | UX-N1 (starring waits on the network) | Optimistic star; push ack applied only if the row is unchanged (`mark_synced_if_unchanged`, one transaction). | **Deferred:** IMMEDIATE transaction (only a noisy error path under a second instance; self-heals). **Declined:** startup `request_sync` (covered by #149). |
| #154 | SYNC-N4 (server change keeps old watermark) | Pull watermark reset, with a generation guard, when endpoints change. | **Declined:** resetting on API-key change; dedup (already handled). **Future:** `server_id` in the watermark. |
| #155 | CAP-N1 (multi-file copy collapses to one image), CAP-N2 (`file://` not percent-decoded; the #117 "URI escaping" follow-up) | Multi-file copies stay file lists; file URIs percent-decoded/encoded; remote `file://` URIs preserved; legacy encoded rows handled. | **Refuted:** a CI claim; write-back of remote URIs (early guard passes `file://` through). |
| #156 | SRV-N1 (startup reads and rewrites the whole table) | Flavour backfill runs once, committed atomically with a version marker in `metadata`; newer markers treated as done. | **Declined:** `debug_assert` autocommit (single caller at open); IMMEDIATE (single-writer server); downgrade-marker test (nice-to-have). |
| #157 | SYNC-N3 (30 s whole-request timeout) | Size-scaled per-request deadlines; 30 s stall (read) timeout; default client deadline kept; page 600 s; unknown blob size budgets for 64 MiB. | **Declined:** builder fallback; treating `Some(0)` blob size as unknown (a real empty blob downloads instantly). **Deferred:** Sync Now's 35 s outer timeout (in SYNC-A3). |
| #158 | UX-N2 (most of it) | Mod+1..9 quick paste, Mod+S star, Mod+Backspace delete, Shift/Alt+Enter plaintext paste, PageUp/Down; auto-repeat ignored for one-shot actions, and a held Enter pastes once from a row, the filter or the popup body. | **Refuted:** Function-constructor "blocker" (it is the test harness). Remainder in *Remaining keyboard coverage*. |
| #159 | CAP-N5 (plaintext paste trims) | Plaintext paste no longer trims (prefers untrimmed text). | Nits declined. |
| #160 | TOOL-01 (tooling test fails under an AI agent) | `--output human` pinned for svelte-check. | Env-scrub hardening declined (the flag suffices). |
| #161 | Idea "The ghost sleeps" | Pause capture 5 min / 1 h / until resumed; status-bar "zzz Paused 4m"; Escape closes only the menu (caught on the window in the capture phase, since WebKit does not focus clicked buttons); unparsable end time shows plain "zzz Paused". | **Deferred:** Linux tray pause toggle; tray/menu-bar item on all platforms. |
| #162 | OPS-N1 (`.dockerignore` re-includes live data, `auth.json`, `node_modules`) | Re-exclude data, env, dist, target and tests after the re-includes. In #164, also `server/data` (the server's default `./data` when run from `server/`) and `auth.json` and `copywraith.db*` by name, after a context export showed a server run from `server/` still leaked both. | **Declined:** `.env.example` exception (root not re-included; Dockerfile never reads it); anchoring `dist` (UI is built in the image). |

**How they were combined.** Merged in the order #160, #162, #147, #152, #151,
#156, #150, #158, #161, then #148, #149, #153, #154, #157, then #155 and #159.
Every conflict was two PRs editing the same spot, and each resolution keeps
both changes:

- #148, #149, #153, #154 and #157 each appended a test module to
  `src-tauri/src/sync.rs`; all are kept.
- #149 moved the loop's sleep after the pass and #153 made it wakeable; the
  wakeable sleep now sits after the pass, so the first sync still starts at
  launch and a star toggle still cuts the wait short.
- #148's `PushOutcome` check wraps #153's `mark_synced_if_unchanged`, and
  #157's size-scaled push deadline joins #148's rejection tracking in
  `push_entry_with_fallback`.
- #147's privacy-marker helpers and #155's file-URI helpers were added at the
  same place in `native_clipboard.rs`; both are kept.
- #150 and #152 both add `tower` as a server dev-dependency; one entry remains.

One semantic fix: #150's gzip test compared the endpoint state with the string
`"online"`, which stops compiling once #148 makes the state a `SyncState` enum.
It now compares against `SyncState::Online`. The four sync test modules that
ran a fake server each had their own request reader, response writer and
temp-storage setup; one shared `test_support` module now serves all of them.

**Not addressed by any PR** (still in the backlog): SEC-N2, SEC-N3, SEC-N4,
SEC-N5, SYNC-N5, SYNC-N6, SYNC-N7, SRV-N2, SRV-N3, SRV-N4, CAP-N3, CAP-N4,
CAP-N6, CAP-N7, UX-N3..N6, ADM-N2..N11, AND-N2..N6, AND-N8..N10, OPS-N2..N12,
DOC-N1, and the resumable pull (SYNC-A3).

### Integrated — #163 (dependencies)

The six Dependabot PRs of 2026-09-13/20, each as its own merge commit:
actions/setup-java 6.0.1 (#140), argon2 0.6.0 (#142), vite 8.3.0 in
`server/ui` (#143) and the popup (#144), vitest 5.0.1 (#145) and @types/node
26.6.1 (#146). #143 and #145 conflicted in `server/ui`; the lockfile was
regenerated with npm to hold exactly the versions both chose. The legacy
`auth.json` fixture still unlocks under argon2 0.6, so existing server
passwords keep working. #144's lockfile drops `esbuild`, an optional vite peer
that nothing installs. vitest 5 requires Node 22.12 or later, so CI, the
release workflow and the Docker UI stage moved from Node 20 (end of life since
April 2026) to Node 22, and `server/ui`'s `engines` now matches vitest's range.

### Integrated — #114 (features)

Six PRs were reviewed, corrected and consolidated into one integration PR rather
than merged individually. The original branches are superseded; do not merge
them.

| PR | What was kept | What was changed or dropped |
|---|---|---|
| [#88](https://github.com/L-K-M/Copywraith/pull/88) | Single-transaction remote ingest, endpoint config resolved once per push batch, `busy_timeout=5000`. | `synchronous=NORMAL` **rejected** — both databases are explicitly `synchronous=FULL`, because a local capture can be the only copy and a synced row is excluded from later pushes, so re-sync cannot be assumed to repair an acknowledged write. The 0.2.1 version bump was dropped; the tree stays **0.3.1**. Remote blob writes moved under the DB mutex, after the duplicate lookup, so a concurrent delete cannot leave a row pointing at a removed file. |
| [#89](https://github.com/L-K-M/Copywraith/pull/89) | Plain text projected once per row, list text bounded, on-demand `get_entry_text` for the preview dialog. | — |
| [#90](https://github.com/L-K-M/Copywraith/pull/90) | Viewport-gated image loading with stale-response guards, single paste per double-click, shared relative-time clock, correct row data-URL MIME, explicit preview action, keyboard-reachable row actions, `viewport-fit=cover`. | **Type scale dropped** (see *Aesthetics*). Two local fixes were required: the preview button's Enter bubbled to row paste, and the image effect refetched on metadata refresh. |
| [#91](https://github.com/L-K-M/Copywraith/pull/91) | RTF stripper rewritten as a linear brace-tracking pass; text helpers extracted to `lib/text.ts`; admin images no longer re-downloaded on every list refresh. | **Admin type scale dropped.** One local fix: numeric ampersand references were decoded twice. |
| [#92](https://github.com/L-K-M/Copywraith/pull/92) | Sync Details read-only again; explicit Sync Now with an in-flight guard. | Sync summaries corrected — a manual sync no longer reports success when the endpoint is unreachable, disabled, or still checking. |
| [#94](https://github.com/L-K-M/Copywraith/pull/94) | Pulled entries keep the server's id and timestamps, fixing reversed history on a fresh install and "paste most recent" picking the oldest item. | Star reconciliation stays keyed on `content_hash`, not id — two devices copying the same text mint independent ULIDs, so an id-keyed lookup would miss the locally-captured row. **No destructive backfill of existing rows.** |

**Regression coverage added, and wired into CI:** 13 popup/sync cases
(`node --test scripts/tests/*.test.mjs`), 30 admin text cases
(`server/ui` vitest), and 5 Python cases
(`python3 -m unittest discover -s scripts -p 'test_*.py'`, of which the new one
asserts both schemas still start at `synchronous=FULL`).

**Honest limitations of what shipped:**

- Blob writes remain non-atomic across crashes. The shared mutex prevents
  concurrent deletion during ingestion, not power-loss corruption. Existing
  files are trusted without re-hashing; see *Make blob storage crash-consistent*.
- On-demand text is capped at 500,000 characters plus an ellipsis. It is bounded,
  not literally complete.
- Lazy image loading still transfers the full blob once a row is encountered.
  This is not thumbnail generation and not an eviction cache.
- The batch settings snapshot can retain a batch's configuration until its
  at-most-50 entries finish. Intentional and bounded.
- Rows pulled before #114 keep their pull-time ids and timestamps.
- Image decode errors and the hardcoded PNG MIME in the preview dialog are
  pre-existing and untouched (backlog: *Blank plain/rich fallback, preview image
  MIME*).
- CI exercises the installed Ubuntu client. No manual Android, macOS, or
  Plasma runtime validation was performed.

### Integrated — #110 (dependencies)

15 compatible dependency PRs consolidated. Eight more were **closed** as not
compatible in isolation — #17, #54, #60, #62, #63, #67, #83, #86 — because each
needs a coordinated migration rather than a version bump. **#117** supplies
those replacements for **#111**. Rust 1.100.0 from #54 remained unpublished
at verification on 2026-09-05; the replacement uses published Rust 1.98.0.

### Integrated — #117 (dependency migrations)

- Rust 1.98.0, rusqlite 0.40.2, aes-gcm 0.11.1, rand 0.10.2 and ULID 3.0.0.
  Legacy ciphertext, authentication, database and identifier fixtures remain
  readable; both databases retain `synchronous=FULL` and existing local keys.
- One private native clipboard adapter owns clipboard-rs 0.3.5 and its watcher.
  Unreadable advertised formats fall back independently. Decoder features,
  rich flavors, Android's separate clipboard plugin and startup logging remain.
- Vite 8.2.2/plugin 7.3.0 with TS7 checking and Kit's TS6 compiler API retained.
  Explicit targets preserve the prior WebView baseline. System7 and 0.3.1 stay.
- CI covers both compiler checks, 15 popup/tooling tests, 30 admin tests,
  workspace checks, installed Ubuntu 22.04/24.04 clients and isolated native
  clipboard tests on Linux/macOS/Windows. Native tests are not full macOS or
  Windows application validation; Wayland and Android runtime remain untested.

Pre-existing URI escaping (now CAP-N2, #155), blank-plain/rich fallback and
monitor recovery UI remain follow-ups (backlog items exist for each). Reentrant
callback lifecycle changes were declined: the private contract assigns
lifecycle calls to the owning app thread. No public storage test layer,
destructive ID backfill or compiler-peer override was added.

### Integrated — #118 (native KDE shortcuts)

The replacement for **#112** uses the existing Linux shortcut dispatcher,
not #97's parallel startup path. Native actions preserve KDE's saved or disabled
assignments, authenticate signal owners and recover after daemon replacement.
Settings reports connection failures and offers command fallbacks without
pretending app-managed accelerators configure KDE. Native paste guidance leaves
the target focused.

Isolated Plasma 5/6 CI exercises real keys, assignments, restart and notification
focus. Mock-bus tests cover hostile signals, partial registration, repeated keys
and cleanup failures. Worker panics report unavailable status and retry;
spawn failure asks for restart. Shutdown wakes retries and bounds individual
cleanup calls. This is not a universal
one-second exit guarantee or physical Wayland-session validation.

Declined speculative callback replacement, extra polling sleeps and notification
threads: dispatch resolves current windows, polling already blocks, and paste
notifications already run off the main thread. dbus-rs invokes its message filters
in Rust, not across the claimed C callback boundary.

### Rejected

| PR | Reason | Tracked in |
|---|---|---|
| [#95](https://github.com/L-K-M/Copywraith/pull/95) Tombstones | An existing server database cannot start on the new schema (the index is created before the column migration), local and server ids are conflated, recopying a deleted entry is suppressed, and a POST in flight during a delete can be acknowledged after it. Protocol change across three clients — needs a design, not a patch. | **#113** |
| [#97](https://github.com/L-K-M/Copywraith/pull/97) Native KDE shortcuts | Registration is incomplete: it calls `doRegister` only, which creates the action but never runs `setShortcutKeys`, so the advertised shortcuts are never initialised and stay excluded from enumeration. It also adds a startup path outside `main`'s existing shortcut-status model and does not filter D-Bus senders. | **#112** |

**Neither original implementation shipped.** #118 supplies the independent KDE
replacement. Delete propagation remains unshipped and tracked in **#113**.

---

## Shipped

Work completed and merged to `main`. Listed so it is not reimplemented. Scope,
corrections and limitations are in the Outcome ledger.

### After 2026-07-25 (0.3.x)

Ubuntu/Linux paste and global-shortcut support, popup operations kept on the
main thread, popup hiding distinguished from client termination, private desktop
portal mounts cleaned up, macOS bundles shipped unsigned, and releases gated on
installed-client checks. PRs #104, #105, #106, #109; releases 0.3.0 and 0.3.1.
These postdate `opus.md`.

Then the integration PRs: **#110** (15 compatible dependency updates),
**#114** (the six reviewed feature PRs), **#117** (coordinated dependency
migrations) and **#118** (native KDE shortcuts). All kept the tree at 0.3.1.

Then the 2026-09-25 review: **#163** (six Dependabot updates) and **#164**
(the sixteen PRs #147–#162: private-marker skipping, honest sync status, the
one-entry probe, gzip, Android share hardening, header-safe passwords,
optimistic starring, cursor reset on server change, multi-file copies and
decoded file URIs, a one-time server backfill, size-aware deadlines, popup
keyboard shortcuts, whitespace-preserving plain paste, the agent-safe tooling
test, pause capture and a trimmed Docker context). Still 0.3.1.

### Earlier (merged before 2026-07-25)

`(updated_at, id)` sync watermark; RTF underflow and CP1252 decoding; settings
URL validation and load/retry/single-flight save state; server field limits and
entry-ID validation; architecture/implementation/encryption/sensitive docs;
honest Android image/file copy errors; Android staging cleanup with atomic JSON
writes; popup filter/selection/preview/Escape consistency; admin request
ordering and last-page clamping; git/Docker runtime-data hygiene; release gating
on CI and matching manifests; sensitive payloads preserved in explicit native
sync while masked by default; full clippy restoration.
