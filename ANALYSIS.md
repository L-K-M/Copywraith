# Copywraith Analysis And Roadmap

Backlog last rebuilt 2026-07-25 against `9ca8179`. This is the maintained
backlog — the single document to start from when picking up work.

> **Currency warning.** `main` has moved since that rebuild: 0.3.1 plus the
> Ubuntu/Linux paste, popup and release-gating work of PRs #104, #105, #106 and
> #109, then #110 (dependencies), #114 (features), and #117 (coordinated
> dependency migrations) and #118 (native KDE shortcuts).
> Every finding below was written against `9ca8179`, so its file and line
> references have drifted. **The disposition of every PR from the 2026-07-25
> review is in the Outcome ledger at the bottom** — read it before acting on any
> status claim in between.

Sources, in order of currency:

- `opus.md` — full independent review of `9ca8179` (2026-07-25). Current
  evidence record: file/line references, mechanisms, and measurements.
- `sol.md` — earlier independent review of `f314806`. Still the detailed record
  for findings that predate `opus.md` and remain open.
- `awesome.md` — the first review, of `977745c`. Largely superseded; retained
  for its product/design rationale (sections 5 and 6).

Anything shipped is removed from the backlog and recorded in **Shipped** at the
bottom, so it is neither lost nor accidentally reimplemented. The **Outcome
ledger** above it records what merged, what was rejected, and where the rejected
work is tracked.

---

## Current release position

Copywraith has a sound local-first shape and a distinctive interface. The
2026-07-25 review found the highest-value remaining work is concentrated in
three places:

1. **Sync convergence.** The Android client could not finish a first sync on a
   large history — see *Android sync latency* below. There are eight causes.
   #114 fixes SYNC-A1 and SYNC-A2 and part of SYNC-A5; the other five and the
   remainder of SYNC-A5 are untouched, and each needs a protocol or product
   decision. **SYNC-A3 is the one that matters**: a timed-out `sync_now` discards
   all pull-watermark progress, so a large history never converges at all.
2. **Test coverage of the sync protocol.** Still the single highest-leverage
   piece of missing engineering work. The storage layer now has unit tests on
   both sides, but nothing exercises the protocol end to end.
3. **Delete propagation.** Remote identity/chronology (#94) shipped in #114.
   Delete propagation did not: **#95 was rejected** — see the Outcome ledger —
   so there are still no tombstones anywhere in the product, and a local delete
   can be undone by a cursor reset or a later server update. Tracked in
   **#113**; delete propagation remains Priority 0.

### Verification baseline

Measured on `9ca8179` before any change, and again with the original PR branches
applied to that tree. This is the historical baseline, **not** a measurement of
`main` after #110 and #114; for what those integrate and how they were checked,
see the Outcome ledger:

| Check | Result |
|---|---|
| `npm run build` (popup) | Pass |
| `npm run check` (popup) | Pass, 0 errors, 0 warnings |
| `cd server/ui && npm run build` | Pass |
| `cargo fmt --all --check` | Pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass |
| `cargo test --workspace` | Pass — 59 baseline, 79 after the PRs below |
| Android device build | Not run (no NDK in the review environment) |
| Docker build | Not run (no Docker in the review environment) |

Two stale claims from the previous revision of this file, both now corrected:

- **GitHub Actions runners work.** The earlier note that Actions "creates jobs
  with zero steps and no assigned runner for every new PR" no longer holds. PRs
  #88–#92 received runners immediately and ran green. Red badges are real
  results again.
- **The old PR ledger is gone.** Every implementation PR it tracked (#32–#36,
  #41–#49) has been merged or closed, and so has every PR of the 2026-07-25
  review. Do not read any open/closed count from this section — the Outcome
  ledger is the only current record.

---

## Android sync latency

The headline performance complaint, with a root cause that turned out to be
eight compounding problems (SYNC-A1 … SYNC-A8) rather than one. Detail and
measurements in `opus.md` §1.

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

### Still open

**SYNC-A3 — a timed-out `sync_now` throws away all pull progress.**
`commands.rs:698` wraps `pull_new_entries` in a 35 s timeout; the watermark is
promoted only after the whole multi-page walk finishes (`sync.rs:376`), so
cancellation discards it entirely. Ingested rows survive, but the next pass
re-walks from the top of the server list. **If every rescan exceeds 35 s, sync
cannot finish** — each attempt repeats the scan and reports `pulled: 0`.
Needs a durable resume cursor and for a partial
pass to stop being treated as failure.

**SYNC-A4 — the first pull is unbounded.** No bootstrap limit: the client pages
the entire server history and downloads each blob in its own sequential request
inside the ingest loop. Bound the initial pull (recent N / last M days), backfill
in the background, and fetch blobs lazily rather than inline.

**SYNC-A5 — the list response ships every text flavor in full.** `api.rs:402`
returns `text_content`, `text_plain`, `text_html`, *and* `text_rtf` for all 100
rows. One rich-text copy is routinely 50–500 KB. Needs a metadata-only
projection for sync clients, plus `tower_http::compression::CompressionLayer`
(a few lines, ~5–10× on text).

**SYNC-A6 — `COUNT(*)` on every page.** `storage.rs:656` runs a full count with
the same WHERE clause before every page query, only to derive `has_more`. Fetch
`limit + 1` rows instead. Worse: with encryption *and* a search term,
`list_entries` loads, decrypts, and filters the **entire table** per request
(`storage.rs:610`).

**SYNC-A7 — the sync loop sleeps before its first pass**, guaranteeing 5 s of
dead time at startup — paid on every Android launch. Separately, all storage
calls are blocking `rusqlite`/`std::fs` executed directly on async runtime
threads.

**SYNC-A8 — focus-driven refresh storm.** `+page.svelte:125` runs a full
capture→import→sync→reload on every Android focus event (keyboard, dialogs,
permission prompts). `mobileRefreshInFlight` guards concurrency but there is no
cooldown after completion, and each refresh calls `loadEntries()` up to four
times.

---

## Priority 0: data integrity and security

### Make sync one revision-safe pipeline

Periodic sync, manual sync, capture-triggered push, and star-triggered push can
overlap. A stale request can mark a newer row synced, and dedup requests can
erase starred state.

- Route all sync through one coordinator/actor.
- Add a monotonic local row revision; mark synced only when the sent revision
  still matches.
- Separate create/recopy from star mutation; preserve server starred state
  during ordinary dedup.
- Track per-entry permanent and retryable failures durably.
- Advance the pull watermark only over a successfully handled contiguous range
  (this is also the fix for SYNC-A3).

`sol.md` SYNC-02, SYNC-05, SYNC-06, SYNC-07, SYNC-10.

### Add end-to-end sync tests before protocol growth

**Still the highest-leverage missing work in the repo.** The sync protocol has
no automated coverage at all; everything in *Android sync latency* and BUG-01
would have been caught by a mocked-server test. Cover:

- Cursor item moved, deleted, and tied on timestamp.
- Empty, missing, corrupt, and hash-mismatched blobs.
- Sensitive full/masked response identity.
- Initial pull chronology and preserved timestamps.
- Concurrent star/capture/manual/periodic updates.
- Wrong password, 413, 500, and transport failure status/retry classes.
- Primary/fallback aliases and accidentally distinct servers.
- Process restart halfway through metadata and blob sync.

`sol.md` OPS-04.

> Client storage and list projection have unit tests; the HTTP sync protocol
> still lacks integration coverage.

### Bound and stream large payloads

Android accepts a 64 MiB raw file, then base64/JSON expansion can exceed the
server's 64 MiB body limit. The failed row retries every five seconds and can
allocate several copies of the payload.

- Replace base64 JSON blobs with streaming multipart or a separate blob API.
- Enforce compatible decoded per-entry and cumulative batch limits.
- Classify 413/invalid payload as permanent and user-action-required.
- Add available-space checks, progress, cancellation, and bounded workers.
- Generate thumbnails and avoid full blob transfer through WebView IPC.

`sol.md` ANDROID-01/05/12, SYNC-12, MAC-11, ADMIN-08/09.

### Secure first-run server ownership and transport

Unauthenticated setup (`api.rs:168`) plus `CorsLayer::allow_origin(Any)`
(`main.rs:89`) and a LAN-bound Docker service lets any reachable client claim an
uninitialised server. The master encryption password doubles as the bearer token
and is commonly sent over plain HTTP.

- Require a one-time local bootstrap token, loopback setup, or CLI setup.
- Restrict CORS to configured admin origins.
- Require HTTPS for non-loopback URLs by default.
- Document a tested TLS reverse-proxy or encrypted-VPN deployment.
- Replace master-password API auth with revocable per-device tokens/scopes.
- Add bounded unlock/setup attempt handling off async executor threads.

`opus.md` SEC-01/02/03; `sol.md` SERVER-01/02/16.

**Related, newly recorded:** `ensure_authorized` (`api.rs:723`) holds the global
`Mutex<CryptoState>` across `verify_and_unlock`. The correct-password fast path
is a SHA-256 compare and is fine, but a **cache miss runs Argon2id at 64 MiB /
t=3 / p=4 on a Tokio worker thread while holding that lock** — any client with a
stale password stalls every other request, repeatedly. (`opus.md` BUG-09.)

### Make encryption state and migration crash-safe

User plaintext beginning with `ENC:1:` (`crypto.rs:292`) or `ENCB`
(`crypto.rs:334`) is silently treated as ciphertext and passed through
unencrypted. Setup publishes `auth.json` before migration completes
(`api.rs:193`), and `encrypt_all_blobs` (`storage.rs:853`) rewrites blobs in
place.

- Store encryption format/version as schema metadata, not payload prefixes.
- Always encrypt newly received plaintext.
- Add durable pending migration state and startup resume.
- Use transactions for row metadata and temp-file/atomic-rename for blobs.
- Verify completion before publishing active auth state.
- Address SQLite WAL/free-page plaintext retention in the threat model.
- Test interruption at every migration phase and prefix-shaped payloads.

`opus.md` SEC-04/05; `sol.md` SERVER-03/04.

### Make blob storage crash-consistent

Final blob paths are written directly (`server/src/storage.rs:432`,
`src-tauri/src/storage.rs`) and trusted merely because they exist. Deletion
commits DB changes before best-effort file removal.

- Write unique temporary files, flush, verify plaintext hash, atomically rename.
- Insert rows only after a valid final blob exists.
- Add read-only reconciliation for missing, corrupt, and orphan blobs, then a
  safe repair path once diagnostics exist.

`opus.md` SEC-06; `sol.md` SERVER-05, OPS-18.

> Partially mitigated on the client only: `insert_entry`
> (`src-tauri/src/storage.rs`) writes the blob file before inserting the row, so
> an *application* crash between the two leaves an orphan blob rather than a row
> pointing at a missing one. That is ordering, not crash consistency — the write
> is a bare `std::fs::write` with no flush, temp file, or rename, and the path is
> skipped entirely when the file already exists, so an OS crash or power loss can
> leave a truncated blob that is then trusted forever behind a committed row. The
> server side is unchanged.

### Make the server authoritative for payload identity

The server trusts client-provided `content_hash` (`api.rs:359`) and accepts
inconsistent content-type/payload combinations, allowing silent deduplication or
broken rows.

- Decode and validate content-specific payload invariants.
- Compute canonical flavor/blob hashes server-side.
- Reject mismatched advisory hashes.
- Document or remove externally required hash construction.

`opus.md` SEC-07; `sol.md` SERVER-06/19.

### Remove authorization/DEK race

Handlers authorize, release crypto state, and later fetch the DEK. A global lock
can intervene and produce plaintext writes or ciphertext responses.

- Return a DEK snapshot atomically from successful authorization.
- Make a missing DEK an error whenever auth is configured.
- Decide whether "Lock" is a global server operation or an admin-session action
  — the admin UI's Lock button (`App.svelte:139`) clears the process-wide DEK,
  which is global rather than session-scoped. It does **not** leave clients
  permanently broken: `verify_and_unlock` (`server/src/crypto.rs:144`) re-derives
  and re-caches the DEK on its slow path, so the next native request carrying a
  valid password unlocks the server again. The cost is a latency spike and an
  Argon2id round, not an outage. `opus.md` BUG-08 overstates this.
- Test concurrent lock/create/get/blob requests.

`sol.md` SERVER-07.

---

## Priority 1: reliability, performance, and UX trust

### Deletion, retention, backup, and storage visibility

- Implement synchronized tombstones and conflict-safe eventual purge. **Still
  entirely unbuilt.** #95 attempted it and was rejected (upgrade ordering, local
  vs. server ids, recopy suppression, and an in-flight-POST acknowledgment race);
  the replacement is tracked in **#113**. Nothing in the product deletes across
  devices today; a cursor reset or later server update can restore a local
  deletion. The replacement must define tombstone expiry too.
- Add Undo/Graveyard behaviour before permanent deletion.
- Add configurable age/count/byte retention with starred exclusions. **Nothing
  bounds growth today** — the DB and blob directory grow forever on every device.
- Show DB/blob/staging usage and a cleanup preview. No client shows any storage
  figure; the server exposes `entries_count` on `/api/health` only when
  authorised, and the admin UI does not display it.
- Add encrypted versioned export/import with integrity verification.
- Warn clearly that losing `auth.json`/password can make data unrecoverable.

`sol.md` SERVER-14/20 and the Product Roadmap.

### Portable file semantics

macOS syncs absolute source-machine paths; Android retains bytes no client can
open, share, or save — `commands.rs:190-196` returns hard errors for both image
and file entries, which still sync down, occupy storage, and render as
thumbnails before refusing every action.

- Decide whether path-only entries are local-only.
- Store managed bytes with safe original filename, MIME, and size.
- Materialize temporary files on macOS for paste/Quick Look.
- Add Android FileProvider content-URI Open, Save, Share, and Copy.
- Stream server files with content disposition and optional ranges.

`opus.md` UX-07; `sol.md` SYNC-08, SERVER-17, ANDROID-03.

### Local privacy

- Move server credentials to macOS Keychain and Android Keystore. Today the
  master password is stored in plain SQLite on every client (`storage.rs:542`),
  and revoking one device means changing the password everywhere.
- Offer encrypted local SQLite/blob/staging storage with a wrapped app data key.
- Define Android backup/data-extraction rules.
- Add app lock/biometric and a sensitive Recents-screen policy.
- Add per-app capture/sync exclusion and pause/incognito modes — `source_app` is
  already tracked but unused for this.
- Document residual raw-hash and metadata leakage on the server.

`sol.md` MAC-10, ANDROID-18, SERVER-10.

### Server scalability and integrity visibility

- Move blocking SQLite, file, Argon2, and parser work off async executors
  (`spawn_blocking` + a small connection pool). Every handler currently does
  blocking work behind one global mutex on Tokio worker threads.
- Add `tower_http::compression::CompressionLayer` — highest benefit-to-risk
  change available on the server.
- Replace or explicitly bound encrypted full-scan search.
- Use linear HTML parsing and fuzz HTML/RTF/auth decoders. `strip_html` and
  `strip_rtf` both allocate a `Vec<char>` over the entire document.
- Propagate SQLite errors instead of converting corruption into 404/healthy.
- Distinguish liveness, readiness, migration, and integrity health.
- Make cursor page counts optional or fetch one extra row for `has_more`.

`opus.md` PERF-06/07; `sol.md` SERVER-11..18, MAC-12.

### macOS utility lifecycle and paste quality

macOS currently has **no menu-bar presence and no launch-at-login**. Linux gets
a full tray (`lib.rs:734`) behind `#[cfg(target_os = "linux")]`; on macOS the app
is a hotkey with no discoverable surface — no way to quit or reach Preferences
without the popup.

- Add a menu-bar home with History, Pause, Preferences, Quit; Dock reopen-to-show.
- Add launch at login.
- Preflight Accessibility/Automation permissions with Settings links and a test.
- Track PID/bundle ID; use native activation/keystroke APIs where practical.
- Suppress only matching self-generated pasteboard events, not a 500 ms window.
- Retry and surface clipboard monitor health.
- Use workspace activation notifications for source attribution.
- Register shortcuts transactionally and retain the last valid set.

`opus.md` UX-08; `sol.md` MAC-01..09, MAC-14.

### Android lifecycle and privileged capture

- Replace focus-as-resume with Activity lifecycle events (this is SYNC-A8).
- Use one backend-owned sync deadline/progress/cancellation model.
- Make Shizuku persistence acknowledged and encrypted before upload. The helper
  is handed the server URL and API key (`lib.rs:154`) and uploads clipboard text
  **directly to the server over plain HTTP from a privileged process**
  (`ShizukuClipboardService.kt:151`), bypassing local storage entirely — if the
  upload fails, the capture is lost.
- Add bounded durable retry/backoff and process-death recovery.
- Reconfigure the running service after URL/password changes.
- Make listener registration idempotent and handle binder death.
- Maintain an Android/OEM compatibility matrix for private Binder calls.
- Add pull-to-refresh, byte/item progress, and Wi-Fi/metered/battery policy.
- Track or assert final generated Gradle/manifest security settings.

`opus.md` SEC-08; `sol.md` ANDROID-06, ANDROID-08..11/15/16/20.

### Popup usability, accessibility, and responsiveness

- **Change single-click from paste to selection** and use an explicit paste
  action. Today clicking a row pastes and hides the popup, so you cannot browse,
  inspect, or correct a mis-click (`opus.md` UX-01). *Deliberately not done in
  this pass — it changes established muscle memory and is a product call.*
- Add Undo for delete, and confirmation for starred/sensitive/bulk cases. Delete
  is immediate with no confirmation; the admin UI confirms, the client does not.
- Replace the interactive `tr role="button"` nesting (which contains real
  `<button>` children) with a proper grid/listbox and roving tabindex.
- Add persistent stale/error/Retry state — sync failures are transient toasts
  today, so a user who looks away never learns sync is broken.
- Add contextual empty states: the list says "No clipboard entries" whether the
  history is empty, the filter matched nothing, starred-only is on, or loading
  failed. First run gets no onboarding at all.
- **Distinguish loaded count from total.** `StatusBar` shows `$entries.length`,
  so a 5,000-entry history reads "100 items"; `has_more` is inferred from
  `result.length === PAGE_SIZE`, wrong whenever the total is a multiple of 100.
- Add pull-to-refresh on mobile.
- Fix the platform-ready initial shell: `platform` starts `''`, so Android
  renders the **desktop** shell — title bar and "Click to paste · Opt+Click…"
  hint — for the first frames of every cold start.
- Add a client-side search index. Search is `search_text LIKE '%…%'`, an
  unindexed full scan per keystroke, while the server has proper FTS5.

`opus.md` UX-01/02/04/06, BUG-12/13, PERF-04; `sol.md` UI-07, UI-09..18,
MAC-13, ANDROID-13/14.

### Admin usability and responsive management

- Add a mobile stacked-card layout and safe viewport dialogs. **There is not a
  single media query in the admin UI**; the five-column fixed-width table
  overflows on a phone with no fallback.
- Download any blob type with filename/MIME and explicit errors.
- Centralize unauthorized transitions and typed API errors.
- Add request timeouts/cancellation and per-row operation states.
- Use lightweight list DTOs and stable cursor pagination.
- Add semantic password forms and a Security/password-change section.
- Add plain/source/rendered rich tabs and useful file metadata.
- Complete reverse-proxy subpath asset support.
- Add accessible bulk star/delete/export after tombstones are correct.
- Fix auth dialog CSS specificity.
- Deduplicate the blob-loading logic now shared by `EntryRow` and `EntryDetail`
  — **after** adding a type check to `server/ui`, not before.

`sol.md` ADMIN-02/03, ADMIN-05..17.

---

## Priority 2: engineering and release hardening

- Keep `typescript: ~6.0.3` for Kit's compiler API. **#117 resolves #111**
  with TypeScript 7.0.2 under the `@typescript/native` alias and
  `svelte-check` 4.7.6's `--tsgo` mode. Both compilers run in CI. The direct
  replacement proposed by #63 remains incompatible; do not remove TS6.
- Triage current npm advisories by reachability; record temporary exceptions.
- Move CI/Docker to a supported Node/npm combination; verify the claimed
  package release-age policy.
- Require signed Android production APKs, verified with `apksigner`.
- Require/verify macOS notarization and Windows signing for stable releases.
- **Pin `tauri-nspanel` by SHA** — it is currently a git dependency on a
  *branch* (`src-tauri/Cargo.toml`), which can move under the build at any time.
  Same for GitHub Actions and Docker images.
- Use Cargo locked/frozen builds; publish checksums, SBOM, and provenance.
- **Run the server container as non-root** with a healthcheck,
  no-new-privileges, dropped capabilities, and amd64/arm64 output. It currently
  runs as root with no `HEALTHCHECK` and no `USER`.
- **Vendor the Swagger UI assets.** `main.rs:38,42` loads them from `unpkg.com`
  at runtime — an external CDN dependency in an app documented as VPN-only.
- Make redeploy build before stopping, fail on health mismatch, support
  rollback/real port variables.
- Make version synchronization exhaustive and nonzero on drift.
- Correct fresh-clone command order, target paths, SDK 36 requirements, and the
  missing `PASTE_PROBLEM.md` reference.
- Remove iOS capability claims until a real dependency/init/build path exists.
- Add `SECURITY.md`, contribution/release instructions, a changelog, and private
  vulnerability reporting.
- Centralize Rust workspace package metadata and mark private crates.

`opus.md` OPS-01..05, FEAT-15; `sol.md` OPS-03, OPS-05..17.

---

## Product roadmap

### Aesthetics

**Maintainer position (2026-07-25): the small, varied type sizes, the mixed
accent colours, and the absence of dark mode are how System 7 worked, not
defects.** An earlier revision of this document framed them as a consistency
problem; that framing was wrong and has been removed. Copywraith is a System 7
pastiche and the retro idiom takes precedence over modern design-system
conventions.

What is left here is only what is defective on its own terms:

- **`filter: hue-rotate()` for the sync progress tone**
  (`StatusBar.svelte:313`) cannot hit a specified colour and forces a
  compositing layer. Whatever colours are wanted, name them directly.
- **The status bar has no graceful narrow layout.** Below 920 px the hint is
  `display: none`, leaving an empty grid column, and the endpoint label
  ellipsizes to uselessness rather than degrading to an icon plus colour.
- **No mobile layout for the admin UI at all** — see the Admin section above.
  The five-column fixed-width table simply overflows on a phone.
- **First-run has no empty state or onboarding.** Independent of visual idiom.

**Decided.** #90 and #91 each proposed a type-scale change that contradicts the
position above. #114 integrated their functional fixes and **dropped both
typography changes**: the popup preview stays 24px desktop / 16px touch, the
badges, mixed accents and admin typography are unchanged, `server/ui/src/App.svelte`
was not touched, and there is still no dark mode. Only the popup's action column
widened, to fit the new preview button. Do not reopen this as a defect.

### Power-user features

- **Transform before paste** — trim, to-plaintext, case, JSON pretty/minify,
  URL/base64, shell-quote, line dedupe, Markdown link. Cheap to build, and the
  feature that makes a clipboard manager sticky.
- **Pinned snippets with aliases.** Starred entries are already first-class;
  naming them turns the app into a text expander for free.
- **Quick-paste by number** — `Cmd+Shift+V` then `1`–`9`, without looking.
- **Type and source filters** in the client. The admin UI has a content-type
  dropdown; the client, where it matters most, has only starred-only.
- **Rich preview tabs.** `EntryPreview` shows only plain text, discarding the
  HTML/RTF the app goes to real trouble to preserve.
- Fuzzy/FTS search with type, source app, device, date, sensitivity, size.
- Paste stack; OCR/image text; native Quick Look; tags/groups; workspaces.
- Native updater and clear release channel/version information.

### Distinctive delight

The spooky identity is under-exploited. These are cheap and give the app a
personality no competitor has:

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
  flavours they reveal.
- **OTP sense** — `sensitive.rs` already detects secret shapes. Detect 6–8 digit
  one-time codes, offer a digits-only copy, auto-expire after 5 minutes.
  Genuinely useful, and thematically perfect for something that vanishes.
- **The mascot** — one small dithered ghost, shown *only* on true first run,
  paused, offline, and empty history. Reserved appearances read as craft;
  ubiquity reads as cheap.
- **Ghost trail** — recent-search chips that fade as they age out.
- Gate all of it on `prefers-reduced-motion` from day one.

Full rationale in `sol.md` sections H and I and `awesome.md` sections 5 and 6.

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
- **New (2026-07-25):** the admin RTF stripper was **not** a ReDoS. That claim
  was made and then disproved by measurement — the pattern stays near-linear
  because its greedy `[^}]*` stops at the first `}`. That same property was the
  real bug: it truncated header stripping and corrupted previews.
- **New (2026-07-25):** star reconciliation is keyed on `content_hash`, not on
  the server id, and that is deliberate. Two devices copying the same text mint
  different ids, so an id-keyed lookup would miss the locally-captured row.
  Content identity is the right key; do not "fix" it to use the id.
- **New (2026-07-25):** entries pulled before #114 keep their pull-time id and
  timestamps. This is an accepted rollout limitation; no automatic expiry
  exists. A destructive backfill was rejected for this rollout:
  matching local rows to server rows by hash and rewriting primary keys is
  destructive on the one table the user cannot re-derive, and ids will key
  whatever delete propagation #113 lands on.
- **New (2026-07-25):** do not use `TextDecoder('windows-1252')` for CP1252.
  It depends on the host's ICU data; a Node build without full ICU decodes
  `0x80`-`0x9F` as Latin-1 and produces invisible C1 control characters. This
  passes locally and fails on CI. Use the explicit table in
  `server/ui/src/lib/text.ts`, which mirrors `cp1252_byte_to_char` in
  `crates/copywraith-core/src/content.rs`.

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
- Android storage-permission restraint, filename sanitization, and the optional
  Shizuku fallback model.
- The System 7 / spooky visual identity.

---

## Outcome ledger

Final disposition of every PR from the 2026-07-25 review. This supersedes any
status claim earlier in this document or in `opus.md`.

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
  pre-existing and untouched.
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

Pre-existing URI escaping, blank-plain/rich fallback and monitor recovery UI
remain follow-ups. Reentrant callback lifecycle changes were declined: the
private contract assigns lifecycle calls to the owning app thread. No public
storage test layer, destructive ID backfill or compiler-peer override was added.

### Integrated — #118 (native KDE shortcuts)

The replacement for **#112** uses the existing Linux shortcut dispatcher,
not #97's parallel startup path. Native actions preserve KDE's saved or disabled
assignments, authenticate signal owners and recover after daemon replacement.
Settings reports connection failures and offers command fallbacks without
pretending app-managed accelerators configure KDE. Native paste guidance leaves
the target focused.

Isolated Plasma 5/6 CI exercises real keys, assignments, restart and notification
focus. Mock-bus tests cover hostile signals, partial registration, repeated keys
and cleanup failures. Worker panics become unavailable status and retry; shutdown
wakes retries and bounds individual cleanup calls. This is not a universal
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

Work completed and merged to `main`. Listed so it is not reimplemented.

### After 2026-07-25 (0.3.x)

Ubuntu/Linux paste and global-shortcut support, popup operations kept on the
main thread, popup hiding distinguished from client termination, private desktop
portal mounts cleaned up, macOS bundles shipped unsigned, and releases gated on
installed-client checks. PRs #104, #105, #106, #109; releases 0.3.0 and 0.3.1.
These postdate `opus.md` and are not reflected in the findings above.

Then the two integration PRs: **#110** (15 compatible dependency updates) and
**#114** (the six reviewed feature PRs). Both kept the tree at 0.3.1. Scope,
corrections and limitations are in the Outcome ledger.

### Earlier (merged before 2026-07-25)

`(updated_at, id)` sync watermark; RTF underflow and CP1252 decoding; settings
URL validation and load/retry/single-flight save state; server field limits and
entry-ID validation; architecture/implementation/encryption/sensitive docs;
honest Android image/file copy errors; Android staging cleanup with atomic JSON
writes; popup filter/selection/preview/Escape consistency; admin request
ordering and last-page clamping; git/Docker runtime-data hygiene; release gating
on CI and matching manifests; sensitive payloads preserved in explicit native
sync while masked by default; full clippy restoration.
