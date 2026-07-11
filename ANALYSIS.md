# Copywraith Analysis And Roadmap

Updated 2026-07-11 from:

- `sol.md`, the full independent review of commit `f314806`.
- `awesome.md`, the earlier review of commit `977745c`.
- Local build/test results and the implementation PRs listed below.

`sol.md` is the detailed evidence record: file references, impact, suggested
fixes, product ideas, strengths, and corrections to the earlier audit all remain
there. This file is the shorter maintained roadmap. Completed or implemented
work is removed from the active backlog and retained in the PR ledger so it is
not lost or accidentally reimplemented.

## Current Release Position

Copywraith has a sound local-first shape and a distinctive interface, but its
next milestone should prioritize trustworthy synchronization and storage over
new surface area. The highest remaining risks are reversed remote chronology,
concurrent sync lost updates, large-blob behavior, server bootstrap/transport
security, and crash consistency during encryption/blob writes.

Local validation at the reviewed commit:

| Check | Result |
|---|---|
| Root Svelte check | Passed, 0 diagnostics |
| Root frontend build | Passed |
| Server UI build | Passed |
| Rust formatting | Passed |
| Rust tests | Passed, 45 baseline tests |
| Rust clippy CI command | Failed on baseline; fixed in PR #41 |
| Shell syntax | Passed |
| Production npm audit | 6 root and 4 server-UI advisories; triage remains |
| Android target check | Blocked locally by missing Android NDK clang |
| Docker build | Blocked locally because Docker is unavailable |

GitHub Actions currently creates jobs with zero steps and no assigned runner
(`runner_id: 0`) for every new PR. Earlier July jobs did receive runners. This
is consistent with private-repository runner availability/billing rather than a
branch test failure. Restore runner availability before treating red PR badges
as code results.

## Implementation PR Ledger

These scopes have implementation branches and are not repeated in the active
backlog. They are not considered shipped until merged and verified together.

| PR | Scope | Local verification | Merge notes |
|---|---|---|---|
| [#41](https://github.com/L-K-M/Copywraith/pull/41) | Restore all Rust clippy checks | fmt, workspace clippy, 45 tests | Merge first; it restores the common CI gate |
| [#42](https://github.com/L-K-M/Copywraith/pull/42) | Honest Android image/file copy errors | fmt, Tauri check, Svelte check/build | Native content-URI actions remain future work |
| [#43](https://github.com/L-K-M/Copywraith/pull/43) | Delete successful Android staging data, retain failures, atomic JSON staging | fmt, desktop Tauri check, Svelte check | Android compile/device test still required |
| [#44](https://github.com/L-K-M/Copywraith/pull/44) | Popup filter/selection/preview/Escape consistency and filter label | Svelte check/build | Low conflict except shared store/page changes |
| [#45](https://github.com/L-K-M/Copywraith/pull/45) | Admin request ordering and last-page clamping | Server UI build | Add server UI type-checking separately |
| [#46](https://github.com/L-K-M/Copywraith/pull/46) | Git/Docker runtime-data and auth-material hygiene | Git ignore assertions | Docker image build still required |
| [#47](https://github.com/L-K-M/Copywraith/pull/47) | Require CI and matching manifests before tag release | Positive/negative version-script tests | Depends operationally on restoring Actions runners |
| [#48](https://github.com/L-K-M/Copywraith/pull/48) | Settings loading/retry/single-flight save state | Svelte check/build | Combine carefully with URL-validation PR #34 |
| [#49](https://github.com/L-K-M/Copywraith/pull/49) | Preserve sensitive payloads in explicit native sync while masking by default | fmt, 48 tests, workspace check | Supersedes lossy skip behavior in PR #28 |

Useful existing implementation PRs from the prior review:

| PR | Scope | Review note |
|---|---|---|
| [#32](https://github.com/L-K-M/Copywraith/pull/32) | Replace mutable ID sync cursor with `(updated_at,id)` watermark | Direction is correct; coordinate with #49 and add sync regression tests |
| [#33](https://github.com/L-K-M/Copywraith/pull/33) | RTF underflow and CP1252 decoding | Good focused fix with tests |
| [#34](https://github.com/L-K-M/Copywraith/pull/34) | Settings URL validation | Combine with #48; add non-loopback HTTP warning later |
| [#35](https://github.com/L-K-M/Copywraith/pull/35) | Missing architecture/implementation/encryption/sensitive docs | Recheck claims against `sol.md`, especially masking and encryption-prefix caveats |
| [#36](https://github.com/L-K-M/Copywraith/pull/36) | Server field limits and entry-ID validation | Good bounded hardening; server hash authority remains unresolved |

PR #28's "skip all sensitive entries" behavior should not be merged together
with #49. Skipping avoids masked duplicates but leaves sensitive cross-device
clipboard functionality broken. PR #49 provides an explicit full-content native
sync path while retaining presentation-safe defaults.

## Merge And Verification Order

1. Restore GitHub Actions runner availability.
2. Merge #41 so all later branches have a clean common clippy baseline.
3. Rebase and merge focused low-conflict work: #33, #35, #36, #42, #43, #45,
   #46, and #47.
4. Combine #34 and #48 in Settings, preserving validation and hydration states.
5. Reconcile #32 and #49 in `sync.rs`; add the sync contract tests described in
   Priority 0 before merge.
6. Merge #44 after checking any concurrent popup/mobile UI changes.
7. Run root/server UI builds, workspace fmt/clippy/tests, a real Android build,
   a Docker build, and manual macOS/Android smoke matrices on the integrated
   branch.

## Priority 0: Data Integrity And Security

### Preserve remote identity and chronology

The server returns newest-first, but clients insert pulled rows through a local
capture path with new IDs and current timestamps. An initial pull can display
old entries as newest and make "paste most recent" choose the wrong item.

Required work:

- Add a dedicated remote upsert that preserves server ID, `created_at`,
  `updated_at`, source, and revision.
- Define canonical identity across primary/fallback URLs.
- Test multi-page initial pull ordering and restart persistence.

Detailed finding: `sol.md` SYNC-03 and SYNC-13.

### Make sync one revision-safe pipeline

Periodic sync, manual sync, capture-triggered push, and star-triggered push can
overlap. A stale request can mark a newer row synced, and current dedup requests
can erase starred state.

Required work:

- Route all sync through one coordinator/actor.
- Add a monotonic local row revision.
- Mark synced only when the sent revision still matches.
- Separate create/recopy from star mutation.
- Preserve server starred state during ordinary dedup.
- Make local recopy a deliberate synced update.
- Track per-entry permanent and retryable failures durably.
- Advance a pull watermark only over a successfully handled contiguous range.

Detailed findings: SYNC-02, SYNC-05, SYNC-06, SYNC-07, and SYNC-10.

### Add end-to-end sync tests before protocol growth

The most important behavior has no automated coverage. Add mocked-server and
temporary-storage tests for:

- Cursor item moved, deleted, and tied on timestamp.
- Empty, missing, corrupt, and hash-mismatched blobs.
- Sensitive full/masked response identity.
- Initial pull chronology and preserved timestamps.
- Concurrent star/capture/manual/periodic updates.
- Wrong password, 413, 500, and transport failure status/retry classes.
- Primary/fallback aliases and accidentally distinct servers.
- Process restart halfway through metadata and blob sync.

Detailed finding: OPS-04.

### Bound and stream large payloads

Android accepts a 64 MiB raw file, then base64/JSON expansion can exceed the
server's 64 MiB whole-body limit. The failed row retries every five seconds and
can allocate several copies of the payload.

Required work:

- Replace base64 JSON blobs with streaming multipart or a separate blob API.
- Enforce compatible decoded per-entry and cumulative batch limits.
- Classify 413/invalid payload as permanent and user-action-required.
- Add available-space checks, progress, cancellation, and bounded workers.
- Generate thumbnails and avoid full blob transfer through WebView IPC.

Detailed findings: ANDROID-01, ANDROID-05, ANDROID-12, SYNC-12, MAC-11,
ADMIN-08, and ADMIN-09.

### Secure first-run server ownership and transport

Unauthenticated setup plus permissive CORS and a LAN-bound Docker service lets
another reachable client attempt to claim an uninitialized server. The master
encryption password is also reused as a bearer token and commonly sent over
plain HTTP.

Required work:

- Require a one-time local bootstrap token, loopback setup, or CLI setup.
- Restrict CORS to configured admin origins.
- Require HTTPS for non-loopback URLs by default.
- Document a tested TLS reverse-proxy or encrypted-VPN deployment.
- Replace master-password API auth with revocable per-device tokens/scopes.
- Add bounded unlock/setup attempt handling off async executor threads.

Detailed findings: SERVER-01, SERVER-02, and SERVER-16.

### Make encryption state and migration crash-safe

User plaintext beginning with `ENC:1:` or `ENCB` can be mistaken for ciphertext.
Setup activates `auth.json` before migration completes, and blobs are rewritten
in place.

Required work:

- Store encryption format/version as schema metadata, not payload prefixes.
- Always encrypt newly received plaintext.
- Add durable pending migration state and startup resume.
- Use transactions for row metadata and temp-file/atomic-rename for blobs.
- Verify completion before publishing active auth state.
- Address SQLite WAL/free-page plaintext retention in the threat model.
- Test interruption at every migration phase and prefix-shaped payloads.

Detailed findings: SERVER-03 and SERVER-04.

### Make blob storage crash-consistent

Final blob paths are written before DB rows and trusted merely because they
exist. Deletion commits DB changes before best-effort file removal.

Required work:

- Write unique temporary files, flush, verify plaintext hash, and atomically
  rename.
- Insert rows only after a valid final blob exists.
- Add read-only reconciliation for missing, corrupt, and orphan blobs.
- Add a safe repair path only after diagnostics exist.

Detailed findings: SERVER-05 and OPS-18.

### Make the server authoritative for payload identity

The server trusts client-provided `content_hash` and accepts inconsistent
content-type/payload combinations, allowing silent deduplication or broken rows.

Required work:

- Decode and validate content-specific payload invariants.
- Compute canonical flavor/blob hashes server-side.
- Reject mismatched advisory hashes.
- Document or remove externally required hash construction.

Detailed findings: SERVER-06 and SERVER-19.

### Remove authorization/DEK race

Handlers authorize, release crypto state, and later fetch the DEK. Global lock
can intervene and produce plaintext writes or ciphertext responses.

Required work:

- Return a DEK snapshot atomically from successful authorization.
- Make missing DEK an error whenever auth is configured.
- Decide whether "Lock" is global server operation or an admin-session action.
- Test concurrent lock/create/get/blob requests.

Detailed finding: SERVER-07.

## Priority 1: Reliability, Performance, And UX Trust

### Deletion, retention, backup, and storage visibility

- Implement synchronized tombstones and conflict-safe eventual purge.
- Add Undo/Graveyard behavior before permanent deletion.
- Add configurable age/count/byte retention with starred exclusions.
- Show DB/blob/staging usage and cleanup preview.
- Add encrypted versioned export/import with integrity verification.
- Warn clearly that losing `auth.json`/password can make data unrecoverable.

Detailed findings: SYNC-09, SERVER-14, SERVER-20, and the Product Roadmap in
`sol.md`.

### Portable file semantics

macOS currently syncs absolute source-machine paths, while Android can retain
bytes that no client can properly open/share/save.

- Decide whether path-only entries are local-only.
- Store managed bytes with safe original filename, MIME, and size.
- Materialize temporary files on macOS for paste/Quick Look.
- Add Android FileProvider content-URI Open, Save, Share, and Copy actions.
- Stream server files with content disposition and optional ranges.

Detailed findings: SYNC-08, SERVER-17, and ANDROID-03.

### Local privacy

- Move server credentials to macOS Keychain and Android Keystore.
- Offer encrypted local SQLite/blob/staging storage with a wrapped app data key.
- Define Android backup/data-extraction rules.
- Add app lock/biometric and sensitive Recents-screen policy.
- Add per-app capture/sync exclusion and pause/incognito modes.
- Document residual raw-hash and metadata leakage on the server.

Detailed findings: MAC-10, ANDROID-18, SERVER-10, and Product Roadmap.

### Server scalability and integrity visibility

- Move blocking SQLite, file, Argon2, and parser work off async executors.
- Use bounded DB access/pooling and avoid file work under DB locks.
- Replace encrypted full-scan search or explicitly bound it through retention.
- Use linear HTML parsing and fuzz HTML/RTF/auth decoders.
- Propagate SQLite errors instead of converting corruption into 404/healthy.
- Distinguish liveness, readiness, migration, and integrity health.
- Make cursor page counts optional or fetch one extra row for `has_more`.

Detailed findings: SERVER-11 through SERVER-18 and MAC-12.

### macOS utility lifecycle and paste quality

- Add Dock reopen-to-show and a menu-bar home with History, Pause, Preferences,
  and Quit.
- Add launch at login.
- Preflight Accessibility/Automation permissions with Settings links and test.
- Track PID/bundle ID and use native activation/keystroke APIs where practical.
- Suppress only matching self-generated pasteboard events, not a 500 ms window.
- Retry and surface clipboard monitor health.
- Use workspace activation notifications for source attribution.
- Register shortcuts transactionally and retain the last valid set.
- Replace overlapping click/double-click gestures with select/preview/paste
  semantics that work consistently.

Detailed findings: MAC-01 through MAC-09 and MAC-14.

### Android lifecycle and privileged capture

- Replace focus-as-resume with Activity lifecycle events.
- Use one backend-owned sync deadline/progress/cancellation model.
- Make Shizuku persistence acknowledged and encrypted before upload.
- Add bounded durable retry/backoff and process-death recovery.
- Reconfigure the running service after URL/password changes.
- Make listener registration idempotent and handle binder death.
- Maintain an Android/OEM compatibility matrix for private Binder calls.
- Add pull-to-refresh, explicit Sync Now/Cancel, byte/item progress, and
  Wi-Fi/metered/battery policy.
- Track or assert final generated Gradle/manifest security settings.

Detailed findings: ANDROID-06, ANDROID-08 through ANDROID-11, ANDROID-15,
ANDROID-16, ANDROID-20, and the Android Product Roadmap.

### Popup usability, accessibility, and responsiveness

- Change desktop single-click to selection and use explicit paste/preview action.
- Add visible touch preview/copy affordances and 44-48 px targets.
- Add Undo for delete and confirmation for starred/sensitive/bulk cases.
- Replace interactive `tr role="button"` nesting with a proper grid/listbox and
  roving tabindex.
- Expose selected/pressed state and visible focus.
- Add persistent stale/error/Retry state and contextual empty states.
- Distinguish loaded count from total results.
- Make Sync Details passive and add a separate Sync Now action.
- Render the detailed mobile sync phase/error state already calculated.
- Fix async listener registration cleanup and platform-ready initial shell.
- Add a shared relative-time clock and explicit image unavailable/retry state.
- Normalize typography, density, and responsive column sizing.

Detailed findings: UI-07, UI-09 through UI-18, MAC-13, ANDROID-13, and
ANDROID-14.

### Admin usability and responsive management

- Download any blob type with filename/MIME and explicit errors.
- Add mobile stacked-card layout and safe viewport dialogs.
- Centralize unauthorized transitions and typed API errors.
- Add request timeouts/cancellation and per-row operation states.
- Use lightweight list DTOs and stable cursor pagination.
- Add semantic password forms and a Security/password-change section.
- Add plain/source/rendered rich tabs and useful file metadata.
- Complete reverse-proxy subpath asset support.
- Add accessible bulk star/delete/export after tombstones are correct.
- Fix auth dialog CSS specificity.

Detailed findings: ADMIN-02, ADMIN-03, and ADMIN-05 through ADMIN-17.

## Priority 2: Engineering And Release Hardening

- Add server UI Svelte/TypeScript checking to its package scripts and CI.
- Triage current npm advisories by reachability, update safe versions, and record
  temporary exceptions.
- Move CI/Docker to a supported Node/npm combination and verify the claimed
  package release-age policy.
- Require signed Android production APKs and verify with `apksigner`.
- Require/verify macOS notarization and Windows signing for stable releases.
- Pin GitHub Actions, Docker images, and `tauri-nspanel` by immutable SHA/digest.
- Use Cargo locked/frozen builds and publish checksums, SBOM, and provenance.
- Run server container as non-root with healthcheck, no-new-privileges, dropped
  capabilities, and amd64/arm64 output.
- Make redeploy build before stopping, fail on health mismatch, and support
  rollback/real port variables.
- Make version synchronization exhaustive and nonzero on drift.
- Correct fresh-clone command order, target paths, SDK 36 requirements, and
  missing `PASTE_PROBLEM.md` reference.
- Remove iOS capability claims until a real dependency/init/build path exists.
- Add `SECURITY.md`, contribution/release instructions, changelog, and private
  vulnerability reporting.
- Centralize Rust workspace package metadata and mark private crates.

Detailed findings: OPS-03 and OPS-05 through OPS-17.

## Product Roadmap After Trustworthiness

### Power-user features

- Fuzzy/FTS search with type, source app, device, date, sensitivity, and size.
- Named starred snippets and optional text-expansion aliases.
- Paste stack and number-key quick paste.
- Transform-before-paste: plaintext, trim, case, JSON, URL/base64, shell quote,
  line dedupe, and Markdown link conversion.
- OCR/image text, native Quick Look, tags/groups, and project workspaces.
- Native updater and clear release channel/version information.

### Distinctive delight

- Seance Log: playful sync event names paired with plain diagnostics.
- Bound spirits: starred items never fade and get a tiny chain/pin glyph.
- Tombstone Undo and a temporary Graveyard drawer.
- Connection Ouija board for local/VPN/TLS/auth/metadata/blob checks.
- Poltergeist side preview that honors reduced motion.
- Possession badges for source app and device.
- Midnight cleanup ritual with an exact retention preview.
- Format ectoplasm tabs: Plain, Rich, Source, Image, and File.
- Ghost trail filter chips and recent searches.
- OTP digits-only copy and optional short auto-expiry.
- A small dithered ghost mascot for true first-run, paused, offline, and empty
  states.

Full feature and design rationale remains in `sol.md` sections H and I and
`awesome.md` sections 5 and 6.

## Important Review Corrections

These corrections must survive future consolidation:

- The mutable-ID cursor bug affects macOS and Android.
- Deleting the cursor causes a full scan; repeated scans need another condition
  that prevents cursor persistence.
- Blob hash mismatch returns false and can be skipped permanently; it is not an
  ingest error in the reviewed code.
- Shizuku stages locally only while the app callback is alive; detached service
  failure remains lossy, and direct capture can actively unstar server entries.
- The keyed popup list makes the previously claimed wrong-row image reuse
  unlikely, though eager uncancelled image work is still a major problem.
- Sensitive presentation masking is good, but masking the native sync contract
  corrupts functionality.
- Prefix-based plaintext/ciphertext passthrough is unsafe for arbitrary
  clipboard bytes.
- `awesome.md` section 9 described planned work, not changes present on `main`.

## Architectural Strengths To Preserve

- Shared core/server/Tauri/Android separation.
- Multi-flavor clipboard model and legacy-compatible hashing.
- Argon2id, domain-separated HKDF keys, random DEK/nonces, and DEK rewrap on
  password change.
- Parameterized SQL and hash-validated blob paths.
- Coherent server keyset ordering.
- macOS NSPanel/main-thread/fullscreen-Space and multi-monitor work.
- Rich text/HTML/RTF preservation instead of immediate flattening.
- Sensitive values kept out of the default popup/admin projection.
- Root list request-ID protection.
- Android storage-permission restraint, filename sanitization, and optional
  Shizuku fallback model.
- The System 7/spooky visual identity.
