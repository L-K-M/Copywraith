# Copywraith Review (`sol.md`)

Independent review of commit `f314806` on 2026-07-11. This covers the shared
Rust crates, server and admin UI, Tauri backend, macOS app behavior, Android
share/Shizuku integration, shared popup UI, build/release automation, tests,
documentation, and product direction.

The review is source-based. macOS focus/paste timing and Android OEM behavior
still require real-device testing. Findings from `awesome.md` were rechecked;
corrections are recorded near the end rather than repeated uncritically.

Severity: **Critical**, **High**, **Medium**, **Low**. Confidence describes
confidence in the diagnosis, not implementation complexity.

## Executive Summary

Copywraith has a good local-first shape, unusually thoughtful macOS window and
paste work, a useful multi-flavor clipboard model, and a coherent visual idea.
It is also carrying several data-integrity and trust problems that should be
fixed before adding many features:

1. The shared incremental sync cursor can permanently miss entries.
2. A first pull reverses remote history and discards its real timestamps.
3. Sensitive-entry masking destroys synced content and can create duplicates.
4. Ordinary captures can unstar an existing server entry.
5. Concurrent sync paths can acknowledge stale data and lose newer changes.
6. Android reports success for image/file operations that did nothing.
7. Large Android shares can exceed the server body limit after base64 expansion,
   then retry every five seconds while consuming substantial memory and battery.
8. Server encryption migration and blob writes are not crash-consistent.
9. An uninitialized LAN server can be claimed remotely, and the master password
   is commonly sent over plain HTTP.
10. Image-heavy histories eagerly move full blobs through Rust, IPC, JavaScript,
    data URLs, and decoded images, creating a credible stutter/OOM path.

The UI builds and the small unit-test suite pass, but the current branch fails
the exact clippy command configured in CI. Almost none of the sync, storage,
API, Tauri, frontend, or Android lifecycle behavior has automated coverage.

## Validation Performed

- `npm run check`: passed, 0 errors and 0 warnings.
- Root `npm run build`: passed. A transient warning appears if build races
  `svelte-kit sync`; CI runs these serially, so this is not a release blocker.
- `server/ui` `npm run build`: passed, but there is no type-check script.
- `cargo fmt --all --check`: passed.
- `cargo test --workspace`: passed, 45 tests total.
- `cargo clippy --workspace --all-targets -- -D warnings`: **failed** with one
  `manual_clamp` error in `api_types.rs` and four `op_ref` errors in
  `content.rs`.
- `bash -n scripts/*.sh`: passed.
- `scripts/sync-version.sh`: reports clean, but does not inspect all versioned
  manifests and exits successfully when its check finds drift.
- `npm audit --omit=dev`: root reports 6 advisories (4 high, 2 moderate); server
  UI reports 4 (2 high, 2 moderate). Reachability varies, but dependency updates
  and an explicit audit policy are needed.

## A. Cross-Device Sync And Data Integrity

### SYNC-01: Mutable ID cursor permanently skips entries

**Critical; high confidence.** `src-tauri/src/sync.rs:257-265,311-321` stores and
stops at one `last_seen_server_id`, while server ordering is mutable
`(updated_at,id)` (`server/src/storage.rs:384-393,656-660,690-696`). If the old
cursor item is recopied or starred and moves above a newer item, the next pull
stops before the newer item and never imports it. This affects macOS and Android,
not Android alone.

Use a real `(updated_at,id)` high-watermark and a server query for rows strictly
newer than it. Serialize pulls and add regression tests for moved/deleted cursor
entries and equal timestamps.

### SYNC-02: Failed-entry handling has two opposite loss modes

**High; high confidence.** Network/blob download errors set
`had_ingest_error`, preventing cursor persistence and causing repeated scans
(`sync.rs:323-350`). Empty blobs and hash mismatches instead return `Ok(false)`
(`sync.rs:542-579`), allowing the cursor to advance past an entry that was never
stored.

Track failed entry IDs as durable retry jobs. Advance only over a successfully
processed contiguous sequence; quarantine permanent corruption visibly.

### SYNC-03: Remote chronology is reversed and provenance is discarded

**High; high confidence.** The server returns newest first, but the client
inserts each remote row through a local-capture path that creates a fresh ID and
`Utc::now()` timestamps (`sync.rs:315-334,584-614`;
`src-tauri/src/storage.rs:244-287`). Older rows are inserted later and therefore
sort above newer ones. "Paste most recent" can paste an old item after initial
sync.

Add a remote upsert preserving canonical ID, `created_at`, `updated_at`, source,
and revision. This is foundational for deletion and conflict resolution too.

### SYNC-04: Sensitive masking corrupts cross-device content

**Critical; high confidence.** Server list/get/create responses replace
sensitive content with bullets and remove useful flavors
(`server/src/api.rs:425-436,730-749`). Native sync recomputes a hash from that
mask and stores it as the real entry (`sync.rs:528-590`). The destination copies
the mask, not the secret, and can upload a duplicate with a different hash.

Mask only at presentation boundaries. Authenticated native sync must either
receive the original payload or an explicit redacted placeholder carrying the
canonical opaque identity and a disabled Copy action. Do not silently turn a
redaction into clipboard content.

### SYNC-05: Ordinary capture can erase starred state

**High; high confidence.** Both normal push and Shizuku direct upload send an
explicit false star value (`sync.rs:218-229`;
`ShizukuClipboardService.kt:139-148`). Server dedup applies it to the existing
record (`server/src/storage.rs:384-393`). Recopying a starred snippet can unstar
it globally.

Create/dedup should preserve mutable metadata unless a versioned mutation was
explicitly requested. Star changes belong in a separate PATCH with revision or
mutation time.

### SYNC-06: Concurrent sync paths lose newer changes

**High; high confidence.** Periodic sync, manual sync, clipboard-triggered push,
and star-triggered push can overlap (`src-tauri/src/lib.rs:674-723`;
`commands.rs:95-100,607-710`). A stale request can finish after a newer local
change and `mark_synced(id)` without checking which revision it sent
(`storage.rs:500-503`). Shared cursor updates can also regress.

Use one sync coordinator/actor. Give local rows a monotonic revision and mark a
row synced only with `UPDATE ... WHERE revision = sent_revision`.

### SYNC-07: A local recopy is not propagated

**Medium; high confidence.** Local dedup updates only `updated_at`; it neither
sets `synced = 0` nor updates source metadata (`storage.rs:208-224`). The local
order changes while other devices never learn about the recopy.

Treat recopy as a versioned update and enqueue it through the coordinator.

### SYNC-08: File synchronization is not portable

**High; high confidence.** Normal macOS file copies upload absolute paths rather
than file bytes (`clipboard.rs:99-139`; `sync.rs:208-230`). Another Mac cannot
paste those paths unless it has the same filesystem, and local paths leak.
Android-imported file bytes are retained, but desktop paste still uses display
paths rather than materializing the blob.

Either mark path-only file entries local-only or store managed file blobs with
name/MIME metadata, then materialize/share them safely on the destination.

### SYNC-09: Deletion is local-only and resurrection-prone

**High; high confidence.** `commands.rs:105-108` deletes only local storage;
the server hard-delete endpoint is unused and the protocol has no tombstones.
Deleted items can return after cursor reset or a later server update.

Use synchronized soft-delete tombstones with revision semantics and eventual
garbage collection. Add Undo in both clients before permanent collection.

### SYNC-10: Status conflates transport, auth, and successful synchronization

**High; high confidence.** Any HTTP response marks an endpoint "responding"
before status validation (`sync.rs:395-417,473-505`), and recent response state
is exposed as online (`sync.rs:171-181`). Wrong passwords, 413s, and 500s can
look healthy.

Expose distinct states: offline, reachable, setup required, unauthorized,
payload rejected, server error, syncing, and synced. Record the endpoint that
actually completed the page/cycle.

### SYNC-11: Fixed polling is wasteful and not Android-reliable

**High on Android, medium on desktop; high confidence.** The process polls every
five seconds (`lib.rs:674-705`), requesting a top page even when unchanged.
Android may suspend this loop in the background, while a foreground session can
make about 720 pulls/hour. Permanent failures also repeat at that cadence.

Push on local changes, use an efficient "after watermark" pull, apply adaptive
backoff, and suspend based on lifecycle/connectivity. Use WorkManager only for
appropriate constrained background work; do not pretend a Tokio timer is an OS
background scheduler.

### SYNC-12: Eager serial blob pulling blocks completion

**High; high confidence.** Remote blobs are downloaded sequentially inside the
metadata loop (`sync.rs:317-334,538-581`). Manual sync has finite phase timeouts,
so a large first sync can repeatedly process the same prefix.

Prefer metadata-first sync and on-demand blob caching. If eager fetching remains,
use bounded concurrency and independently resumable jobs.

### SYNC-13: Primary/fallback cursor semantics are undefined

**Medium; medium confidence.** A single cursor is shared when failing between
two URLs. This is safe only if both URLs expose the same logical database with
the same IDs and ordering. The settings UI does not state or validate that
assumption.

Document that primary/fallback must be aliases for one server, or key cursor and
sync state by server identity.

## B. Server And Shared Core

### SERVER-01: Uninitialized server can be claimed remotely

**Critical in LAN/Docker deployments; high confidence.** `/auth/setup` is
unauthenticated (`server/src/api.rs:157-195`), CORS allows any origin/method/header
(`server/src/main.rs:89-92`), and Compose exposes all interfaces. A reachable
host or browser origin can choose the first password before the owner does.

Require a one-time bootstrap token printed locally, loopback-only setup, or a
CLI setup flow. Restrict CORS to configured origins.

### SERVER-02: Master password and content are encouraged over cleartext HTTP

**High; high confidence.** The actual encryption password is reused as a bearer
credential, while Settings and documentation use `http://` remote examples
(`SettingsDialog.svelte:129-166`; `README.mac.md:64-70`;
`ShizukuClipboardService.kt:151-159`). A trusted LAN is not encrypted.

Require HTTPS for non-loopback endpoints by default, document a TLS reverse
proxy/VPN, warn explicitly for cleartext overrides, and replace the master
password with revocable per-device tokens.

### SERVER-03: Plaintext can be mistaken for ciphertext

**High; high confidence.** Encryption state is inferred from user-controlled
prefixes `ENC:1:` and `ENCB` (`server/src/storage.rs:237-250,784-864`;
`crypto.rs:288-359`). Literal text or binary data beginning with those values
can be skipped during encryption and later fail decryption.

Store encryption state/version as metadata. Always encrypt new plaintext and
add regression tests for prefix-shaped payloads.

### SERVER-04: Initial encryption migration can leave mixed plaintext data

**Critical for upgrades; high confidence.** Setup publishes `auth.json` and
in-memory state before migrating existing rows/blobs (`api.rs:178-195`). Rows are
changed incrementally and blobs in place (`storage.rs:753-865`). Interruption
leaves setup permanently initialized but migration incomplete.

Use a durable pending migration state, transactional metadata updates, temp
files plus atomic rename, verification, startup resume, and only then activate
auth. Address SQLite free-page/WAL plaintext retention in the threat model.

### SERVER-05: Blob and SQLite operations are not crash-consistent

**High; high confidence.** Blob creation writes directly to its final hash path
before inserting a row (`storage.rs:405-428`). A crash can leave a partial file
that a retry trusts because it exists. Delete commits the row first and ignores
file deletion failures (`storage.rs:700-728`).

Write unique temp files, flush/verify/hash, atomically rename, and reconcile
missing/corrupt/orphan blobs on startup or through an integrity command.

### SERVER-06: Server trusts client-supplied content hashes

**High; high confidence.** `CreateEntryRequest.content_hash` is authoritative
(`api_types.rs:8-22`; `storage.rs:372-428`). Different payloads with the same
supplied value silently deduplicate, and content-type/payload consistency is not
validated.

Compute canonical hashes server-side, reject mismatches, and validate invariants
for text/image/file payloads. Make client hashes advisory or remove them.

### SERVER-07: Authorization can race global lock state

**High correctness risk; high confidence.** Handlers call `ensure_authorized`,
release the crypto lock, then separately fetch the DEK (`api.rs:275-282,333-350,
593-609,681-728`). `/auth/lock` can intervene, leading to plaintext writes or
ciphertext responses after authorization.

Authorization should atomically return a per-request DEK snapshot. If auth is
configured, storage must never interpret missing DEK as permission for
plaintext behavior. Clarify whether lock is global server state or UI-session
state.

### SERVER-08: Remote Swagger JavaScript executes on the privileged origin

**High; high confidence.** `main.rs:31-49` loads unpinned Swagger code from
unpkg, and users are told to enter the server password there. Same-origin code
can access API/session data.

Bundle pinned Swagger assets, add a restrictive CSP, and do not solicit
credentials inside remotely supplied JavaScript.

### SERVER-09: Sensitive API responses lack cache/security headers

**Medium; high confidence.** Authenticated metadata/blobs lack `Cache-Control:
no-store`; the server has no CSP, `nosniff`, or referrer policy (`main.rs:97-119`;
`api.rs:625-630`). Browser and intermediary caches may retain clipboard data.

Add response hardening centrally, with strict policy for admin and Swagger.

### SERVER-10: Raw hashes leak guesses despite encryption

**Medium; high confidence.** Unsalted SHA-256 content hashes and metadata remain
plaintext (`storage.rs:283-310`). An offline attacker can test guesses for URLs,
commands, OTPs, and common snippets.

Document this leakage. If the threat model requires stronger confidentiality,
use a DEK-derived HMAC dedup key and explicitly decide which metadata remains
visible.

### SERVER-11: Encrypted search and single-connection locking do not scale

**High for large histories; high confidence.** All DB work shares one
`std::sync::Mutex<Connection>` (`storage.rs:13-16`). Encrypted search loads,
decrypts, lowercases, and filters all candidate rows while holding it
(`storage.rs:599-642`). File and crypto work also run in async handlers.

Move blocking work off async executors, use bounded DB access/pooling, add
retention/quotas, and define a deliberate encrypted-search strategy. Corrupt
records should be reported, not silently omitted.

### SERVER-12: Error swallowing turns corruption into 404/healthy

**Medium; high confidence.** Several `query_row(...).ok()` calls discard all
SQLite errors (`storage.rs:372-383,508-516,703-711`). Health can report zero
entries and "ok" after a DB failure (`api.rs:297-303`).

Use `OptionalExtension::optional()` only for no-row cases and distinguish
liveness from readiness/integrity.

### SERVER-13: Parser work can be quadratic or panic on hostile clipboard data

**High; high confidence.** `strip_html` repeatedly sums prefixes and duplicates
large inputs (`core/content.rs:42-112`). RTF depth decrements below zero and hex
bytes are interpreted as Unicode code points (`content.rs:156-231`). A global
64 MiB request limit still permits expensive single fields.

Use linear streaming parsing, per-field limits, guarded depth, proper RTF code
page decoding, and fuzz/property tests.

### SERVER-14: No content-specific size limits or quotas

**High operational risk; high confidence.** One field can consume nearly the
entire 64 MiB body; history and blob storage have no user/storage quota
(`server/src/api.rs:19-45`).

Set explicit decoded limits per flavor, total entry limits, history/storage
quotas, and clear 413 diagnostics.

### SERVER-15: Mutex poisoning and malformed auth config can disable service

**Medium; high confidence.** Locks are broadly unwrapped. Invalid decoded nonce
length can panic at `Nonce::from_slice` while crypto state is locked
(`crypto.rs:161-180,405-415`). Recovering every poisoned lock blindly is also
unsafe if invariants were broken.

Validate auth file version/field lengths on startup and map expected failures to
controlled errors. Fail readiness or restart on true invariant corruption.

### SERVER-16: Hard-coded auth work can block requests

**Medium; high confidence.** Argon2 work executes while holding crypto state and
without brute-force/rate control. Even on a trusted network, several attempts
can serialize expensive work.

Run password derivation in bounded blocking workers and add modest unlock/setup
rate limiting. Per-device random tokens reduce repeated master-password work.

### SERVER-17: File download behavior is incomplete

**Medium; high confidence.** The API can serve a blob, but metadata/content
disposition are insufficient for preserving an original filename, and the UI
does not use the blob path for ordinary files (`api.rs:601-630`).

Store safe filename, MIME, and size; stream responses; set content disposition;
support ranges where useful.

### SERVER-18: Pagination repeats expensive counts

**Low to medium; high confidence.** Every page computes `COUNT(*)` over the
filtered set (`storage.rs:643-686`). This is minor beside encrypted full-scan
search, but unnecessary for cursor polling.

Return `has_more` by fetching one extra row, or make total count optional.

### SERVER-19: OpenAPI error contracts and canonical hashing are incomplete

**Medium; high confidence.** Wrong old password can become 500 despite documented
400/401 (`api.rs:227-258,637-671`). `API.md` does not define multi-flavor hash
construction even though clients must supply it.

Use typed API errors with contract tests; preferably move hash authority to the
server.

### SERVER-20: Backup and key-loss recovery are absent

**High product risk; high confidence.** Losing `auth.json` makes encrypted data
unrecoverable, yet there is no verified backup/export/restore workflow or setup
warning.

Add an encrypted backup bundle, integrity verification, restore drill, and clear
password/key-loss messaging.

## C. macOS App

### MAC-01: Double-click preview is preempted by single-click paste

**High; high confidence.** A row pastes on `click` and previews on `dblclick`
(`EntryRow.svelte:73-87,127-138`). The first click hides the popup, so the
documented double-click preview is effectively broken.

Use single click to select, Enter/double-click to paste, and Space or an explicit
button for preview. Do not overlap immediate and double-click gestures.

### MAC-02: Star actions wait on network before UI updates

**High UX impact; high confidence.** `toggle_star` awaits a push that can try two
30-second endpoints (`commands.rs:95-102`; `sync.rs:387-437`). The row appears
frozen.

Commit locally and update optimistically; background sync reports eventual
failure/conflict.

### MAC-03: Shortcut replacement can lock users out

**High; high confidence.** Existing shortcuts are unregistered before all new
ones are validated, registration errors are only logged, and the command always
reports success (`lib.rs:163-225`; `commands.rs:829-838`). Duplicate detection
is incomplete.

Parse/preflight every shortcut, detect all conflicts, register transactionally,
retain the last valid set, and show field-level errors. Provide another way to
open the app.

### MAC-04: Hidden-window app lacks normal recovery lifecycle

**Medium to high; high confidence.** The only window starts hidden and there is
no Dock reopen handler, menu-bar item, Preferences menu, launch-at-login option,
or visible pause/quit control (`lib.rs:60-156`; `tauri.conf.json:13-26`).

Add reopen-to-show and a status item with History, Pause Capture, Preferences,
and Quit. Offer launch at login.

### MAC-05: Paste targeting is slow and imprecise

**Medium; high confidence.** Paste sleeps, launches `osascript`, activates by
display name, sleeps again, then sends Cmd+V (`paste.rs:291-298,354-382,
469-526`). App names are ambiguous and AppleScript adds latency/permissions.

Track PID/bundle ID, activate with native APIs, and consider CGEvent/AX for the
keystroke. Preserve the existing background-thread requirement.

### MAC-06: Accessibility onboarding is reactive rather than helpful

**Medium; high confidence.** Trust is checked only after paste failure
(`paste.rs:276-289,395-439`). There is no permission status, explanation, test,
or Settings deep link.

Add first-run permission guidance and a Preferences diagnostics panel with
"Open Accessibility Settings" and "Test paste."

### MAC-07: Time-window suppression can miss real clipboard changes

**Medium; high confidence.** Every monitor event for 500 ms after Copywraith
writes is ignored (`paste.rs:97-99,221-227`; `clipboard.rs:39-49`). A quick real
copy can be dropped; delayed self-events can be recaptured.

Suppress only a matching pasteboard change count/content hash.

### MAC-08: Monitoring failure is permanent and invisible

**Medium; high confidence.** Monitor startup is attempted once after 250 ms;
failure only logs (`lib.rs:101-109`; `clipboard.rs:24-30`).

Retry with backoff, expose capture health, and optionally capture current
clipboard after successful initialization.

### MAC-09: Source application attribution can be stale

**Low to medium; medium confidence.** Frontmost app is polled once per second
(`paste.rs:24-43`; `clipboard.rs:72-73`). Rapid switching can attach the prior
app.

Subscribe to workspace activation events and store bundle ID plus display name.

### MAC-10: Local cache and password are plaintext

**High security/privacy impact; high confidence.** Text, rich flavors, paths,
blobs, and API password are stored in local SQLite/files (`storage.rs:142-170,
234-270,590-597`). Server encryption does not protect the Mac cache.

Store credentials in Keychain and offer local DB/blob encryption. At minimum,
state reliance on FileVault and filesystem permissions.

### MAC-11: Image-heavy popup has a credible stutter/OOM path

**High; high confidence.** Up to 100 rows each fetch, read, base64-encode, send,
retain, and decode a full image (`clipboardStore.ts:15`; `EntryRow.svelte:54-65`;
`commands.rs:70-91`). Native image capture has no equivalent small thumbnail
limit.

Generate thumbnails, lazy-load by viewport, use object/protocol URLs rather than
base64 IPC, cap decoded dimensions/bytes, and virtualize long lists.

### MAC-12: Blocking storage work shares async/UI paths

**Medium; high confidence.** Synchronous SQLite, file I/O, sensitive scanning,
and image conversion run from async commands and monitor callbacks behind one
mutex (`commands.rs:19-90`; `clipboard.rs:35-55`; `storage.rs:202-271`).

Use a dedicated storage actor or `spawn_blocking`; avoid holding DB locks during
large file work.

### MAC-13: Desktop layout wastes scarce popup width

**Medium; high confidence.** The normal 560 px window reserves fixed metadata
columns and renders preview text at 24 px (`tauri.conf.json:17-24`;
`EntryRow.svelte:241-246`). Header and row widths disagree. At 400 px, content
becomes a fragment.

Use 13-15 px content type, one source of column sizing, progressive metadata
hiding, and a density preference.

### MAC-14: File URI construction is unsafe

**Low; high confidence.** `paste.rs:197-209` prepends `file://` without proper
escaping. Spaces, `#`, `%`, and Unicode can break.

Use `Url::from_file_path` or native pasteboard file URL APIs.

### MAC-15: No automated macOS behavior coverage

**High process risk; high confidence.** CI tests Linux logic only; NSPanel,
fullscreen Spaces, focus restoration, permissions, source attribution, and paste
timing have no macOS smoke tests.

Add a macOS compile/test job and a repeatable manual/E2E matrix for focus,
Spaces, multiple displays, Accessibility denied/granted, and rapid copies.

## D. Android App And Share/Shizuku Integration

### ANDROID-01: Large shares exceed effective server limits and retry forever

**Critical; high confidence.** Kotlin and Rust accept up to 64 MiB raw per file
(`CopywraithSharePlugin.kt:339-367,410-425`; `commands.rs:545-560`), then JSON
base64 adds about 33% (`sync.rs:208-229`) against a 64 MiB whole-request server
limit. A raw blob above roughly 47 MiB cannot fit. Failure remains unsynced and
retries every five seconds.

Use streaming multipart/blob upload, lower raw limits consistently, cap total
batch bytes, and classify 413/invalid payload as user-action-required rather
than endlessly retryable.

### ANDROID-02: Image tap reports success but copies nothing

**Critical UX bug; high confidence.** Mobile image paste returns success without
writing (`commands.rs:180-193`), while the store always says "Copied to
clipboard" (`clipboardStore.ts:230-240`).

Return an explicit action result. Implement content-URI Copy/Open/Share/Save, or
disable and accurately label unsupported operations.

### ANDROID-03: Imported files consume storage but are not usable

**High; high confidence.** File bytes are retained (`commands.rs:545-603`), but
tap usually copies only a filename and there is no native Open, Save, Share, or
content-URI path (`commands.rs:195-213`).

Implement FileProvider-backed actions and meaningful file detail metadata.

### ANDROID-04: Successful staging JSON keeps plaintext forever

**High privacy risk; high confidence.** Shared/Shizuku text is written verbatim
to JSON (`CopywraithSharePlugin.kt:42-55,333-379`) and successful batches are
moved to `pending-shares/processed` instead of deleted (`commands.rs:366-401`).
Deleting history does not remove this duplicate.

Delete staging data immediately after durable import. Keep only bounded,
redacted diagnostics; encrypt any retry queue.

### ANDROID-05: Share processing can block the main thread and exhaust storage

**High; high confidence.** `onNewIntent` synchronously copies streams up to 64
MiB each, while `ACTION_SEND_MULTIPLE` has no count/cumulative quota
(`CopywraithSharePlugin.kt:115-122,323-367`). Each imported item can launch its
own sync task (`commands.rs:485-495`).

Use a bounded worker, aggregate quota/free-space checks, progress/cancel UI, and
one bounded-concurrency sync after the batch.

### ANDROID-06: Shizuku is not durably local-first when app process is dead

**High; high confidence.** The service calls an app-process callback and then
uploads independently (`ShizukuClipboardService.kt:107-136`). If the callback
is dead and upload fails, there is no durable retry; `lastText` can suppress the
same value.

Require acknowledged durable encrypted persistence before network upload. Use
a bounded retry worker and backoff; redesign IPC if shell/root cannot safely
write app-private storage.

### ANDROID-07: Staged-item failures are marked processed

**High; high confidence.** Per-item import errors become `skipped`, but the
batch is considered successful and moved to processed (`commands.rs:391-412,
475-505`). JSON writes are not temp-file/rename atomic.

Retain only failed items in a bounded retry batch and write metadata atomically.

### ANDROID-08: Running Shizuku service keeps stale settings

**Medium; high confidence.** URL/password are passed only when listener starts
(`commands.rs:764-779`). Saving settings does not reconfigure it, and service
fields retain old credentials (`SettingsDialog.svelte:40-58`;
`ShizukuClipboardService.kt:20-39`).

Atomically rebind/reconfigure after relevant settings changes and display the
effective endpoint without exposing secrets.

### ANDROID-09: Shizuku listener lifecycle can leak callbacks/listeners

**Medium; medium confidence.** Static Shizuku callbacks are added on plugin load
without corresponding cleanup, and service `start()` can install another system
listener (`CopywraithSharePlugin.kt:115-118,179-187`;
`ShizukuClipboardService.kt:28-91`). OEM/Tauri recreation frequency varies.

Make start idempotent, remove callbacks on destruction, and handle binder death.

### ANDROID-10: Resume behavior is tied to window focus

**Medium; medium confidence.** `+page.svelte:104-125` treats WebView focus as
Activity resume. Dialogs, keyboard, split-screen, and OEM behavior can trigger
extra or missed captures/full syncs.

Bridge actual Activity lifecycle, debounce resume work, and separate clipboard
capture from full sync.

### ANDROID-11: Frontend/backend timeout layers overlap sync work

**Medium; high confidence.** Rust permits about 35 seconds per phase while the
frontend abandons the whole invocation at 45 seconds
(`commands.rs:631-666`; `+page.svelte:289-323,391-398`). `Promise.race` does not
cancel Rust, so another refresh can overlap.

Let one backend coordinator own deadlines, cancellation, progress, and dedupe.

### ANDROID-12: List payloads and images overuse WebView memory

**High; high confidence.** List calls send full non-sensitive text for up to 100
rows; every image row immediately fetches full base64 bytes, then hardcodes PNG
MIME (`commands.rs:36-91`; `EntryRow.svelte:54-65,152-155`).

Use lightweight list DTOs, full content on action/detail, thumbnails, binary
URLs/content URIs, correct MIME, and virtualization.

### ANDROID-13: Destructive actions are small and immediate

**High UX/accessibility impact; high confidence.** Delete has no confirmation or
Undo, and coarse-pointer buttons remain below recommended 44-48 dp
(`EntryRow.svelte:109-119,312-354`).

Use 48 dp targets, overflow/swipe actions, and snackbar Undo. Confirm only
starred/sensitive/bulk destruction.

### ANDROID-14: Touch users lack a practical preview path

**High UX impact; high confidence.** Preview is double-click/Space while tap
copies (`EntryRow.svelte:83-103,127-138`).

Make content tap open/select and provide a distinct Copy action, or expose a
visible info button/long-press menu.

### ANDROID-15: Final app security config is not reproducible from Git

**Medium; high confidence.** `src-tauri/gen/android/` is ignored and regenerated,
so final target SDK, cleartext policy, backup/data extraction rules, launch mode,
`FLAG_SECURE`, R8, and signing config cannot be audited from the repository.

Track a reproducible generated-project strategy or scripted patches/tests that
assert the merged manifest and Gradle configuration.

### ANDROID-16: README confuses min SDK with compile SDK

**Medium contributor impact; high confidence.** Plugin Gradle requires compile
SDK 36 (`build.gradle.kts:6-12`), while README says SDK Platform 24 or newer.
Release CI does not explicitly install platform 36.

Document and install Platform 36/build tools; keep min SDK 24 as a separate fact.

### ANDROID-17: Mobile capability exposes unused privileged commands

**Medium security hardening; high confidence.** Mobile JS capability allows
direct Shizuku read/start/stop/status, including privileged current clipboard
text (`capabilities/mobile.json:6-14`; `CopywraithSharePlugin.kt:165-177`), while
the frontend uses Rust wrappers.

Remove unused direct permissions, especially privileged read, and expose narrow
validated Rust commands.

### ANDROID-18: Local history and credentials are plaintext

**High privacy impact; high confidence.** SQLite, blobs, settings, and staging
files are plaintext. Android sandbox/device encryption helps but does not cover
root, backups, forensic extraction, or a compromised account.

Use Keystore-backed credential storage and an app data key for DB/blob/queue
encryption. Define backup exclusion or encrypted backup behavior.

### ANDROID-19: Broad share filters clutter every chooser

**Low; high confidence.** Manifest accepts `*/*` for single and multiple shares
(`AndroidManifest.xml:13-29`) even though arbitrary files currently have no
useful destination actions.

Narrow MIME types until full file handling exists, or keep broad support only
with clear file import/open/share behavior.

### ANDROID-20: OEM/version risk in private clipboard Binder calls

**High reliability uncertainty; medium confidence.** Shizuku uses hard-coded
transaction IDs/argument layouts (`ShizukuClipboardService.kt:190-300`). These
are inherently Android-version/OEM sensitive.

Maintain a tested API/device compatibility matrix, feature-detect failures, and
degrade visibly to foreground/share capture. Do not present Shizuku as guaranteed.

## E. Shared Popup UI

### UI-01: Settings can overwrite user input before hydration completes

**High; high confidence.** Defaults render immediately and async settings later
replace them; Save is active throughout (`SettingsDialog.svelte:10-58`). A fast
user can lose edits or save blank/default configuration.

Show a loading state, disable editing/Save until hydration succeeds, and ignore
late responses once editing begins.

### UI-02: URL and shortcut inputs are not meaningfully validated

**High; high confidence.** Custom Save bypasses native form URL validation;
normalization only trims slashes (`SettingsDialog.svelte:40-60,121-166`).
Shortcut errors are not surfaced.

Parse HTTP(S) URLs, block credentials/invalid schemes, offer Test Connection,
validate all shortcuts, and show saving/testing states.

### UI-03: Enter can paste a stale hidden result

**High; high confidence.** Filter text changes immediately but old results and
selection remain for the debounce interval; Enter invokes paste
(`FilterBar.svelte:21-63`; `clipboardStore.ts:173-201`).

Invalidate selection immediately on query change and permit Enter only after
the matching request commits.

### UI-04: Unstar under starred-only leaves invalid row visible

**Medium; high confidence.** Store mutation updates the object but does not
remove it under the active filter (`clipboardStore.ts:203-213`).

Remove optimistically and move selection predictably, or reload current query.

### UI-05: Preview state is stale after star changes

**Medium; high confidence.** Preview holds the original object while the store
replaces its row (`EntryPreview.svelte:78-110`; `+page.svelte:39,547-549`).

Store preview ID and derive live state; disable repeated in-flight actions.

### UI-06: Escape handling ignores interaction layers

**Medium; high confidence.** Filter Escape clears text, then bubbles to a global
handler that hides the popup; dialog Escape can also close the whole window
(`FilterBar.svelte:65-67`; `+page.svelte:400-448`).

Handle topmost layer first: confirmation, dialog, filter, then popup; stop
propagation once consumed.

### UI-07: Table interaction semantics are invalid and tab-heavy

**High accessibility impact; high confidence.** `tr role="button"` contains
buttons, lacks selection state, removes focus outline, and creates up to 300 tab
stops. Delete can be focusable while visually hidden
(`EntryRow.svelte:127-207,293-309`).

Use a proper grid/listbox with roving tabindex, `aria-selected`, labeled
`aria-pressed` actions, and visible `:focus-visible` styling.

### UI-08: Search has placeholder-only labeling

**Medium accessibility impact; high confidence.** `FilterBar.svelte:71-81` has
no persistent accessible label and result changes are not announced.

Add a label/`aria-label` and a restrained live result status.

### UI-09: Load failures leave stale results without persistent explanation

**Medium; high confidence.** Existing rows survive a failed query, while only a
transient notification appears; page error state is unused
(`clipboardStore.ts:88-123`; `+page.svelte:37,476-478`).

Represent query snapshot, loading, stale, and error states explicitly with Retry.

### UI-10: Empty states are generic

**Medium product quality; high confidence.** The same message covers first run,
no search matches, no stars, failed sync, and stale page.

Use contextual copy and actions: clear filters, open Settings, sync/retry, or a
friendly first-capture explanation.

### UI-11: Counts mean loaded rows, not total matches

**Low to medium; high confidence.** Status uses `$entries.length`, capped at 100
(`StatusBar.svelte:19-20,144-147`; `clipboardStore.ts:15`).

Return total separately or label "100 loaded."

### UI-12: Sync Details unexpectedly performs a full sync

**Medium; high confidence.** Opening an informational affordance invokes
`syncNow()` (`StatusBar.svelte:94-140`).

Keep details passive and provide a distinct Sync Now button with progress.

### UI-13: Detailed mobile sync messages are computed but not shown

**Medium; high confidence.** Page state contains phase/detail/error text, but
StatusBar receives only value/tone (`+page.svelte:45-49,270-365,482-487`).

Show concise phase, last sync, persistent actionable failure, and Retry.

### UI-14: Async listener setup can leak and miss early events

**Medium; high confidence.** Async `onMount` registers listeners sequentially;
destruction during awaits can leave later registrations without cleanup
(`+page.svelte:66-181`).

Keep lifecycle callback synchronous, use an inner initializer with destroyed
flag, register critical listeners first, and immediately unlisten late results.

### UI-15: Mobile briefly renders desktop chrome

**Low; high confidence.** Platform starts unknown but is treated as desktop
until async detection resolves (`platform.ts:3-7`; `+page.svelte:66-85,456-470`).

Use an explicit platform-ready shell or safe synchronous hint.

### UI-16: Relative times never advance and future times show "now"

**Low; high confidence.** Timestamps are calculated only on render and negative
differences satisfy the "now" branch (`EntryRow.svelte:9-23`).

Use one low-frequency shared clock and handle invalid/future timestamps.

### UI-17: Image errors are silent or stuck loading

**Medium; high confidence.** Row failures collapse to a placeholder; preview
swallows failure and can show "Loading image..." forever
(`EntryRow.svelte:54-64`; `EntryPreview.svelte:12-17,90-96`).

Expose unavailable/retry state and cancel stale work after unmount.

### UI-18: Visual typography and hierarchy are inconsistent

**Medium design issue; high confidence.** 24 px row content competes with 9 px
badges and 11 px timestamps; admin rows similarly mix 22 px and 11 px.

Create typography/density tokens. Preserve System 7 character without making
history less scannable.

## F. Server Admin UI

### ADMIN-01: List requests race and display the wrong query/page

**High; high confidence.** `loadEntries()` has no request token or abort signal;
search, filters, refresh, mutation reloads, and pagination overlap
(`server/ui/src/App.svelte:147-199,291-313,413-471`).

Capture the full query snapshot and use a monotonic token/AbortController.
Disable or coalesce pagination while loading.

### ADMIN-02: File Download silently does nothing

**High; high confidence.** Only images use `blob_url`; file rows enter the text
branch and commonly return without feedback (`App.svelte:253-284`;
`EntryRow.svelte:191-201`; `EntryDetail.svelte:101-109`).

Download any blob with preserved filename/MIME and show explicit unsupported or
failure feedback.

### ADMIN-03: Authentication card CSS is overridden

**Medium visual bug; high confidence.** Later `.window` rules override equal-
specificity `.auth-window` width, margin, and height (`App.svelte:498-555`).

Use `.window.auth-window` after generic rules and content-driven height.

### ADMIN-04: Last-page delete can strand an invalid empty page

**Medium; high confidence.** Offset remains after deletion; labels can show
"51-50 of 50" (`App.svelte:291-300,463-470`).

Clamp offset after mutations and reload the valid previous page.

### ADMIN-05: Mobile admin layout is effectively desktop-only

**High UX impact; high confidence.** Fixed columns consume more than a phone
width, dialogs are 600 px without safe viewport caps, and controls do not adapt
(`App.svelte:60-66,544-621`; `EntryRow.svelte:248-313`).

Use stacked cards below about 700 px, hide secondary metadata, wrap actions, and
cap dialogs to safe viewport dimensions.

### ADMIN-06: Unauthorized handling is inconsistent

**Medium; high confidence.** List/download return to unlock; view/star/delete
show generic action errors after the wrapper clears session
(`App.svelte:162-233,291-300`; `api.ts:175-216`).

Centralize typed auth failure handling.

### ADMIN-07: No request timeout or cancellation

**Medium; high confidence.** Fetches can leave startup/list/action loading
forever (`api.ts:88-238`).

Add per-operation abortable timeouts, offline messaging, and Retry.

### ADMIN-08: Full blobs are eagerly fetched for image rows

**High performance risk; high confidence.** Up to 50 mounted images each fetch
full authenticated blobs (`EntryRow.svelte:33-66`; `App.svelte:446-454`).

Use visibility loading, thumbnail endpoint/cache, dimensions, cancellation, and
bounded concurrency.

### ADMIN-09: List API sends/decrypts full content

**High for large histories; high confidence.** Every list response includes full
text/flavors despite a 200-character row (`server/ui/types.ts:8-21`;
`server/api.rs:414-438`).

Create a lightweight list DTO and fetch full detail only when requested.

### ADMIN-10: Offset pagination is unstable under mutable ordering

**Medium; high confidence.** UI uses offsets while starring/sync mutates
`updated_at`, causing page duplicates/skips (`api.ts:161-181`;
`storage.rs:656-696`).

Use the API cursor or a stable immutable sort.

### ADMIN-11: Auth forms lack semantic/password-manager support

**Medium accessibility/usability; high confidence.** Inputs rely on placeholders,
manual Enter handlers, and lack labels/autocomplete (`App.svelte:326-401`).

Use semantic forms, labels, `current-password`/`new-password`, focus-on-error,
busy states, and optional reveal.

### ADMIN-12: Row actions lack per-item operation feedback

**Medium; high confidence.** View/star/delete/download remain repeatable while
requests are in flight (`App.svelte:193-300`; `EntryRow.svelte:178-213`).

Track operation state by ID and disable conflicting actions.

### ADMIN-13: Rich/file detail is incomplete

**Medium; high confidence.** Files show no useful metadata and HTML/RTF lack
plain/source/rendered choices (`EntryDetail.svelte:90-105`).

Add safe format tabs and file name/type/size/action details. Render rich content
only in a sandboxed/sanitized boundary.

### ADMIN-14: Password change exists only in API wrapper

**Medium product gap; high confidence.** `api.ts:144-155` implements it, but the
UI has no Security section or key-loss warning.

Expose password change, session behavior, and backup/recovery implications.

### ADMIN-15: Reverse-proxy subpath support is partial

**Low to medium; medium confidence.** API base discovery is prefix-aware, but
Vite assets/favicon remain root-relative (`api.ts:33-52`; `index.html:6`;
`vite.config.js:4-19`).

Use a relative/deployment-aware Vite base and integration test a subpath.

### ADMIN-16: Errors discard useful server messages

**Medium; high confidence.** Most API methods throw only status text while auth
methods parse JSON (`api.ts:108-232`).

Normalize typed status/code/message errors and provide actionable UI copy.

### ADMIN-17: No bulk management tools

**Medium product gap; high confidence.** There is no multi-select star/delete,
export, cleanup preview, or storage management.

Add selection with safe bulk actions after deletion/tombstone semantics are
correct.

## G. Build, Release, Deployment, Tests, And Documentation

### OPS-01: Current main fails its CI clippy command

**High; verified.** `api_types.rs:62` triggers `manual_clamp`; four expressions in
`content.rs:58-74` trigger `op_ref`. `.github/workflows/ci.yml:94-95` denies
warnings, so current `main` should fail Rust CI with current resolved tooling.

Fix the five diagnostics and keep the exact command as a local/release gate.

### OPS-02: Tag releases bypass the CI workflow

**High; high confidence.** CI triggers on main/PR/manual, while release triggers
directly on tags and does not rerun formatting, clippy, tests, root type-check,
or server UI build (`ci.yml:3-7`; `release.yml:3-6`).

Use reusable CI via `workflow_call`; require release validation and version/tag
checks before draft creation or publication.

### OPS-03: Release can publish unsigned artifacts

**High; high confidence.** Android intentionally uploads unsigned APK when
secrets are absent (`release.yml:173-211`); macOS signing/notarization is optional
and Windows has no signing path.

Fail production releases without signing, verify signatures, publish checksums,
and mark any unsigned development build unmistakably as a prerelease artifact.

### OPS-04: Critical behavior has almost no integration coverage

**Critical process risk; high confidence.** Forty-five tests cover core helpers
and crypto only. Server routes/storage, sync, Tauri commands, clipboard/paste,
frontends, Android Kotlin/share/process-death behavior, and migrations have no
tests.

Prioritize mocked sync regression tests, temporary SQLite/blob tests, Axum API
tests, frontend store/component tests, and Android share/lifecycle tests before
large refactors.

### OPS-05: Server UI is built without type-checking

**Medium; high confidence.** `server/ui/package.json` has only dev/build/preview;
CI only builds it.

Add TypeScript/Svelte check configuration and a CI step.

### OPS-06: Dependency advisories are present and policy is incomplete

**High maintenance risk; verified.** Production audit reports advisories in
Svelte/devalue/linkification/markdown and the clipboard API dependency chain.
Actual reachability differs between static Tauri UI and server admin UI.

Update within compatibility constraints, document accepted exceptions, and add
scheduled `npm audit`/Rust advisory review. Do not blindly auto-fix major versions.

### OPS-07: Claimed npm release-age gate is not reliably enforced

**Medium; high confidence.** `.npmrc` uses `min-release-age`, while CI/Docker pin
Node 20/npm 10, which does not implement the npm 11 option. Node 20 is also EOL
by the review date.

Pin a supported Node/npm via `engines`/`packageManager`, assert versions in CI,
and test the policy or remove the claim.

### OPS-08: Deployment data/auth material are insufficiently ignored

**High secret-hygiene risk; high confidence.** Compose writes to tracked
`copywraith-data`; ignore rules omit `auth.json` and there is no `.dockerignore`
(`docker-compose.yml:9-10`; `.gitignore:36-41`).

Ignore all runtime data except a placeholder README and add a restrictive
`.dockerignore` so `.git`, `.env`, runtime data, and build outputs never enter a
remote build context.

### OPS-09: Supply-chain inputs are mutable

**Medium; high confidence.** Actions use major tags, Docker bases use mutable
tags, Docker Cargo build omits `--locked`, and `tauri-nspanel` follows a branch
(`release.yml`; `server/Dockerfile`; `src-tauri/Cargo.toml:40-41`).

Pin actions/images/git dependencies, use locked/frozen builds, and publish SBOM,
provenance, and checksums.

### OPS-10: Docker runs as root and is single-architecture

**Medium; high confidence.** Runtime image has no non-root user or healthcheck;
Compose lacks capability/security hardening; release Buildx does not specify
ARM64 (`server/Dockerfile:31-47`; `release.yml:243-252`).

Run non-root, add health/readiness, no-new-privileges/capability drop, and publish
amd64+arm64 images.

### OPS-11: Redeploy stops service before proving replacement

**Medium; high confidence.** `redeploy-server-docker.sh:77-89` tears down before
build; failed health/version checks only warn, and `PORT` does not reconfigure
Compose.

Build first, replace only after success, fail nonzero on health mismatch, and
support rollback/real port variables.

### OPS-12: Version sync script gives false assurance

**Medium; verified.** It omits root/server UI package manifests and lockfiles,
Tauri config/Cargo packages, and documentation values; check mode exits zero on
drift (`scripts/sync-version.sh:4-6,82-117`).

Choose one authoritative version, update all consumers structurally, and make
check mode exhaustive and nonzero on drift.

### OPS-13: Documentation has broken links and inaccurate commands

**Medium; high confidence.** README links missing `ARCHITECTURE.md`,
`IMPLEMENTATION.md`, `ENCRYPTION.md`, and `SENSITIVE.md`; agent notes also cite
missing `PASTE_PROBLEM.md`. Fresh-clone docs run Cargo before the required
frontend build. Mac bundle path is documented under `src-tauri/target` rather
than workspace `target`.

Create accurate design docs or remove links; provide one verified check command;
add Markdown link validation.

### OPS-14: Android release prerequisites are under-specified

**Medium; high confidence.** CI initializes a fresh project but does not
explicitly install compile SDK 36; signing output is not verified.

Pin platform/build-tools/NDK/JDK, run `apksigner verify`, and add at least a
Gradle compile/test smoke job outside tag releases.

### OPS-15: iOS is advertised in capability/platform code but not buildable

**Medium; high confidence.** Mobile capability includes iOS and frontend treats
it as mobile, but clipboard-manager dependency/init is Android-only while mobile
commands reference it (`capabilities/mobile.json`; `commands.rs:180-268`;
`Cargo.toml:43-45`).

Remove iOS claims/capability until implemented, or add a real iOS path and CI.

### OPS-16: Repository governance is thin for sensitive software

**Low to medium; high confidence.** No `SECURITY.md`, contribution guide,
changelog, release checklist, or private disclosure instructions exist.

At minimum add security reporting/supported versions and a tested release
procedure.

### OPS-17: Dependency metadata is duplicated

**Low; high confidence.** Versions/edition repeat across Rust packages without
`workspace.package`, `rust-version`, license/repository, or explicit
`publish=false` for private applications.

Centralize shared metadata and mark private crates.

### OPS-18: No integrity/diagnostic command exists

**Medium product/operations gap; high confidence.** Users cannot verify DB rows,
blob hashes, encryption readability, cursor state, orphan files, or endpoint
auth independently.

Add read-only diagnostics and an exportable redacted report before automated
repair features.

## H. Product And Feature Roadmap

These are missing capabilities, not proof that the current implementation is
wrong. Ordering reflects value and prerequisites.

### High-value fundamentals

- **Retention and storage budget:** keep by age/count/bytes, never auto-delete
  starred entries, show storage usage, and preview cleanup.
- **Pause/incognito capture:** global pause, timed pause, and "Wraith Mode" that
  captures nothing or auto-destroys a session.
- **Per-app exclusions:** denylist password managers, banking apps, terminals, or
  user-selected bundle/package IDs; support "never sync from this app."
- **Secure local storage:** Keychain/Keystore credentials and optional encrypted
  local history.
- **Real delete/Undo:** tombstones, Graveyard, undo snackbar, and eventual purge.
- **Export/import/backup:** encrypted, versioned, verifiable, and portable.
- **Sync diagnostics:** independently test local URL, fallback URL, TLS, auth,
  push, pull, blobs, and last successful watermark.
- **Device identity:** revocable tokens, device names/icons, last-seen status,
  and source device on entries.

### Power-user workflow

- Fuzzy/FTS search with type, app, device, date, sensitive, and size filters.
- Named starred snippets and optional text expansion aliases.
- Paste stack: queue several clips and paste them in order with one hotkey.
- Number-key quick paste for the first 9 results.
- Transform-before-paste: trim, plaintext, case, JSON pretty-print, URL/base64
  decode, line dedupe, shell quote, and Markdown link conversion.
- OCR/image text extraction and native Quick Look/file preview.
- Bulk select/star/delete/export in admin and clients.
- Pin groups/tags and temporary project workspaces.
- Native updater and clear version/channel information.

### Android-specific product work

- Pull-to-refresh and explicit Sync Now/Cancel with item/byte progress.
- Open/Save/Share/Copy actions backed by content URIs.
- Wi-Fi-only blobs, metered/roaming/battery controls.
- App lock/biometric gate, hide sensitive previews in Recents, and optional
  `FLAG_SECURE`.
- Proper lifecycle/background scheduling and robust process-death recovery.

### macOS-specific product work

- Menu-bar home, launch at login, permission onboarding, and monitor health.
- Quick Look, source app icons, Services/Share integration, and native updater.
- A "paste without formatting" transform menu and multi-paste stack.
- Window/panel preference persistence without sacrificing cursor-relative popup
  placement.

## I. Delightful And Distinctive Ideas

The retro/spooky identity is an asset. Delight should decorate successful state
changes, not add animation to scrolling or routine loading.

- **Seance Log:** a compact sync event log with both playful and plain text:
  "Contacting Local Spirit," "VPN apparition found," "3 memories materialized."
- **Bound spirits:** starred clips get a tiny chain/pin glyph and never visually
  fade; old unstarred clips fade gently by age.
- **Tombstone Undo:** deleted entries briefly move to a Graveyard drawer before
  final collection.
- **Connection Ouija board:** a diagnostic panel that tests local, VPN, TLS,
  authentication, metadata, and blobs as separate lights.
- **Poltergeist side preview:** keyboard movement reveals a low-cost side preview
  without changing focus; honor reduced motion.
- **Possession badges:** tiny monochrome source app/device badges show where an
  entry last came from.
- **Midnight cleanup ritual:** retention preview says exactly what will be laid
  to rest and why, with starred exclusions visible.
- **Format ectoplasm:** Plain, Rich, Source, Image, and File manifestations as
  tabs in one preview.
- **Ghost trail filters:** recent searches and active type/device chips remain
  visible and easy to clear.
- **One-time-code behavior:** detect likely OTPs, offer digits-only copy, and
  optionally auto-expire after 30-120 seconds.
- **A literal pixel ghost:** a dithered mascot in genuine first-run/empty states,
  with different expressions for paused, offline, and all-clear states.

## J. What Is Already Strong

- Clear workspace separation between shared model, server, Tauri client, and
  Android bridge.
- Good multi-flavor hashing model with legacy single-flavor compatibility.
- Thoughtful crypto primitives: Argon2id, domain-separated HKDF outputs, random
  DEK, random AES-GCM nonces, and password changes that rewrap rather than
  re-encrypt everything.
- Parameterized SQL and hash-validated blob paths.
- Server keyset ordering itself is coherent; the client cursor is the broken
  part.
- macOS NSPanel/main-thread/fullscreen-Space and multi-monitor positioning work
  shows attention to real platform constraints.
- Rich clipboard capture/paste preserves text, HTML, and RTF rather than flattening
  immediately.
- Sensitive values are kept out of the popup WebView projection; the mistake is
  applying destructive masking to the native sync contract too.
- Root list loading already uses request IDs to reject stale responses.
- Android avoids broad storage permissions and sanitizes shared filenames.
- Opt-in Shizuku behavior degrades to normal foreground/share paths rather than
  making the basic app depend on privileged access.
- Current frontend builds, formatting, and existing unit tests are clean.

## K. Corrections To `awesome.md`

- The sync cursor bug affects macOS and Android, not Android alone.
- Deleting the cursor causes one full scan; it repeats forever only when another
  failure also prevents cursor persistence.
- Blob hash mismatch does not currently raise an ingest error. It returns false
  and may be skipped permanently, which is a different and serious bug.
- Shizuku stages locally while the app callback is alive; loss remains when the
  daemon is detached/dead or staging fails. It also actively unstars existing
  entries, which the prior review understated.
- The claimed stale-row wrong-image bug is unlikely with the current keyed list.
  Uncancelled work is still wasteful, and preview error/disposal handling remains
  weak.
- The server UI row already has a disposal guard; the popup path remains eager.
- Server-side sensitive masking is not an unqualified strength because it breaks
  the native sync data contract.
- Prefix-based plaintext/ciphertext passthrough is not safe for arbitrary
  clipboard data.
- `awesome.md` section 9 described planned branches; none of those fixes is in
  the audited tree.

## L. Recommended Implementation Order

1. Add regression tests around sync ordering, cursor movement, failed entries,
   sensitive entries, and star dedup semantics.
2. Fix server/native sync contracts together: identity/timestamps, watermark,
   non-destructive sensitive transfer, explicit metadata mutations.
3. Serialize sync and add revision-safe acknowledgement/retry classification.
4. Fix Android false-success actions, staging retention, quotas, and permanent
   failure loops.
5. Fix bootstrap/CORS/HTTP credential risks and server crash consistency.
6. Replace eager full-image paths with thumbnails/lazy binary delivery.
7. Repair direct UI trust bugs: settings hydration, stale selection, double-click,
   admin request races, deletion Undo, and accessibility semantics.
8. Add retention, secure local credential storage, diagnostics, and verified
   backup before expanding advanced features.
9. Harden CI/releases and build a real cross-platform test matrix.
10. Add the power-user and spooky-delight features on top of trustworthy data
    behavior.
