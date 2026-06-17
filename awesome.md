# Copywraith — Code Review & Ideas (`awesome.md`)

A thorough review of Copywraith as of commit `977745c`. The goal here is to be
useful, not exhaustive-for-its-own-sake: real bugs first, then performance,
security, missing features, and finally some fun ideas. Each item has a rough
**confidence** that it's worth doing, so the high-value work is easy to find.

The single most important finding is the **Android sync bug** in
[§1](#1-android-sync-the-headline-bug) — it directly explains both reported
symptoms (slow sync, missing/zero starred entries).

Legend: 🐛 bug · ⚡ performance · 🔒 security · ✨ feature · 🎩 delightful/quirky · 🧹 cleanup · 📄 docs/infra
Confidence that it's worth implementing: **High / Medium / Low**.

---

## 1. Android sync: the headline bug

> Reported symptoms: "syncing is slow" and "lots of copied entries that are on
> the server are missing on Android (e.g. clicking starred yields zero when
> there are about a dozen on the server)".

Both symptoms share a root cause in the pull algorithm
(`src-tauri/src/sync.rs::pull_new_entries`).

### 1.1 🐛 The incremental cursor stops at a single entry **id**, but `updated_at` is mutable — High

The client tracks one value, `last_seen_server_id` = the **id of the newest
entry** from the previous successful pull. On the next pull it walks the
server's descending `(updated_at, id)` list and **stops the moment it sees that
exact id** (`sync.rs:318`):

```rust
if initialized && last_seen_server_id.as_deref() == Some(remote.entry.id.as_str()) {
    reached_cursor = true;
    break;
}
```

The problem: an entry's `updated_at` changes whenever it's re-copied or
re-starred (the server bumps `updated_at` on dedup — `server/src/storage.rs:386`
and `update_entry_starred`). So the cursor entry can move **back to the top** of
the list, and any entries created *in between* are now *below* it and get
**skipped forever**.

Concrete failure (very common for a clipboard manager):

1. Pull completes. Newest entry is `A`. Cursor = `A`.
2. On another device you copy a new thing `B` (now: `B`, `A`, …).
3. You re-copy your frequently-used snippet `A`. Server bumps `A.updated_at`
   → order becomes `A`, `B`, ….
4. Next pull: first row is `A`, `id == cursor` → **stop immediately**. `B` is
   never ingested. Cursor stays `A`.

`B` is now permanently invisible on Android. Star a dozen old entries on the
server/desktop (which bumps their `updated_at` to "now", but the cursor entry
may still be on top from a later copy) and you get exactly the reported
"starred shows zero / many missing" behavior.

A second, related failure: if the cursor entry is ever **deleted** on the
server, the walk never finds it and re-scans the *entire* server on every single
pull (the "slow" symptom).

**Fix (planned, see [§9](#9-what-im-implementing-now)):** replace the single-id
cursor with a proper **high-watermark `(updated_at, id)`**. Pull everything
strictly newer than the watermark and advance the watermark to the max seen.
Bumped entries are simply re-ingested (idempotent dedup), and nothing below the
watermark is skipped. This is correct under mutable `updated_at`.

### 1.2 🐛 One bad entry blocks the cursor forever → permanent full re-scans — High

The cursor is only persisted when **no** ingest error occurred during the whole
pass (`sync.rs:350`, `had_ingest_error`). A single entry whose blob can't be
downloaded (server missing the file, a transient 404, hash mismatch returning an
error) sets `had_ingest_error = true`, so the cursor is never saved and **every**
subsequent pull re-scans the entire server from offset 0. Combined with §1.1 and
§1.3 this is a major contributor to "sync is slow."

**Fix:** advance the watermark over successfully-processed entries regardless of
isolated failures (track and retry failures separately, or at least don't let
one poison the whole cursor). With a `(updated_at, id)` watermark, advancing to
the newest *successfully processed contiguous prefix* is the robust choice.

### 1.3 ⚡ Blobs are downloaded one-at-a-time, inline, during the pull — High

`ingest_remote_entry` awaits each image/file blob download sequentially
(`fetch_blob_data`), inside the same loop that walks pages. On a phone with a
history full of images this serializes the whole sync behind dozens of
round-trips, and the manual `sync_now` path has only a 35s timeout
(`commands.rs:662`) — so a large first sync simply times out and never
completes, leaving the DB (and the cursor) partially populated.

**Fixes (ranked):**
- ⚡ **High** — *Lazy blobs on mobile*: pull entry **metadata** during sync and
  fetch image bytes on demand when the user actually opens/uses the entry. This
  is the biggest single Android speed/data win. (Today `get_entry_image` only
  reads local blobs; it would need an on-demand server fetch + cache.)
- ⚡ **Medium** — Bounded-concurrency blob downloads (e.g. `buffer_unordered(4)`)
  if eager download is kept.

### 1.4 ⚡ Background sync doesn't run when the app is backgrounded (Android) — Medium

`start_sync_loop` is a `tokio::time::sleep` loop (`lib.rs:674`). Android freezes
the process under Doze/App Standby, so cross-device entries only arrive while the
app is in the foreground. There's no `WAKE_LOCK`/WorkManager path.

**Options:** document the limitation clearly; or add a WorkManager-based periodic
sync; or a foreground service tied to the optional Shizuku listener. (Larger
effort — flagged, not slated for immediate implementation.)

### 1.5 🐛 Shizuku direct-upload path can silently lose captures — Medium

In `ShizukuClipboardService.kt` the privileged listener POSTs clipboard text
straight to the server and also stages it for local import. If the upload fails
(network down) there's no guaranteed local persistence, and uploads always send
`"starred": false`. Net effect: captures can be lost, and starred state never
propagates upward from this path. Worth making "persist locally first, sync
later" the invariant for *all* capture paths.

### 1.6 🧹 `pull_new_entries` page `total`/`has_more` is fine, but the per-page `COUNT(*)` is wasteful — Low

The server recomputes `COUNT(*)` over the filtered set for every page
(`server/src/storage.rs:645`). Correct, but for keyset pagination you can skip
the count or compute it once. Minor.

---

## 2. Correctness bugs (non-sync)

### 2.1 🐛 RTF parser can underflow `depth` on unbalanced braces — Medium
`crates/copywraith-core/src/content.rs` decrements `depth` on every `}` without
checking `depth > 0`. Malformed RTF (extra `}`) underflows (panic in debug,
wrap in release). Clipboard RTF is attacker-influenced (any app can put bytes on
the clipboard). Guard with `if depth > 0 { depth -= 1; }`.

### 2.2 🐛 RTF `\'hh` hex escapes are cast `byte as char` — Medium
Same file: a decoded byte `0x80–0xFF` becomes U+0080–U+00FF instead of being
decoded as Windows-1252 (the RTF default) or skipped. Produces mojibake in
previews/search for non-ASCII RTF. Decode via CP1252 or skip non-ASCII bytes.

### 2.3 🐛 Mutex `.unwrap()` everywhere → one panic poisons and crashes the server — Medium
`server/src/api.rs` and both `storage.rs` files use `lock().unwrap()`. A panic
while a lock is held poisons it; every later request then panics too. For a
long-running server, prefer recovering the guard (`lock().unwrap_or_else(|e|
e.into_inner())`) or mapping to a 500. Low individual risk, but cheap insurance.

### 2.4 🐛 EntryRow image load has no stale-request guard — Low
`src/lib/components/EntryRow.svelte` loads image bytes in `onMount` without a
"disposed" flag (unlike `EntryDetail.svelte`, which does). Rapid re-use of the
row component can show the wrong image briefly. Mirror the disposed-flag pattern.

### 2.5 🐛 `regex` compiled with `.unwrap()` in sensitive.rs — Low
All patterns are hard-coded so this won't fire in practice, but `.expect("…")`
with context is friendlier if a pattern is ever edited.

---

## 3. Security

This is a single-user, password-gated server explicitly meant for trusted
LAN/VPN (no rate limiting by design — documented in README). With that scope in
mind:

### 3.1 🔒 No per-field size cap on entries — Medium
`COPYWRAITH_MAX_BODY_BYTES` caps the whole request (default 64 MiB) but a single
text field can be ~64 MiB and is encrypted + indexed. A cap per text flavor
(e.g. 10 MiB) avoids pathological DB bloat. (`server/src/api.rs::create_entry`.)

### 3.2 🔒 Entry IDs from the URL aren't validated as ULIDs — Low
Not exploitable today (parameterized SQL; blob filenames are hashes, not ids),
but validating the path id keeps the surface tidy.

### 3.3 🔒 `api_key`/server password stored in plaintext SQLite settings — Low
On desktop/mobile the sync password lives in the local `settings` table in the
clear. Platform secure storage (Keychain / Android Keystore) would be stronger.
Larger change; flagged.

### 3.4 ℹ️ Things that look scary but are actually fine (calibration)
- **HKDF with `None` salt** (`crypto.rs:388`): acceptable — the IKM is already a
  high-entropy Argon2id output; HKDF-Extract without salt is standard for
  uniformly-random IKM. Domain separation comes from distinct `info` strings.
- **`constant_time_eq` early-returns on length mismatch**: the compared values
  are fixed 32-byte keys; no meaningful length leak.
- **AES-GCM nonces**: freshly random per encryption — no reuse concern at these
  volumes.
- **Server `search_text` is encrypted** when a password is set (FTS is disabled
  and search falls back to in-memory decrypt). Earlier worry about "plaintext
  search column" does **not** apply.
- **Mobile `content_hash` index exists** (`storage.rs:163`), so dedup checks are
  indexed; that is *not* a cause of slow sync.

---

## 4. Performance

- ⚡ **High** — Lazy/concurrent blobs on mobile (see §1.3).
- ⚡ **Medium** — Mobile `get_entries` filters with `search_text LIKE '%q%'`
  (`src-tauri/src/storage.rs:328`), which can't use an index. Fine for small
  histories; consider FTS5 on-device (the server already uses it) if histories
  grow large.
- ⚡ **Low** — `EntryRow` eagerly loads every image even off-screen; an
  `IntersectionObserver` (or virtualized list) would cut memory on image-heavy
  histories.
- ⚡ **Low** — Skip/precompute the per-page `COUNT(*)` for keyset pages (§1.6).

---

## 5. Missing features

- ✨ **High** — **Local retention / size cap.** Nothing ever prunes the local DB
  or blob dir; both grow unbounded. A configurable "keep N days / M MB, never
  delete starred" policy + a "storage used" readout would be very welcome,
  especially on phones.
- ✨ **Medium** — **Delete should propagate.** `DELETE /api/entries/{id}` exists
  on the server, but desktop/mobile delete is local-only (`commands.rs:delete_entry`
  never calls the server) and the pull has no concept of remote deletions, so
  deleted entries reappear and starred-unstar/delete races resurrect content.
  A soft-delete tombstone synced both ways would close the loop.
- ✨ **Medium** — **Bulk actions** in the admin UI (multi-select delete / star /
  export).
- ✨ **Medium** — **Server URL validation** in Settings (today any string is
  accepted and only fails at sync time). Quick win.
- ✨ **Low** — **Sort/filter by type** in the popup (admin UI has type filter;
  popup only has starred + search).
- ✨ **Low** — **Export / import** clipboard history (JSON) for backup/migration.

---

## 6. Delightful / quirky ideas 🎩

- 🎩 **"Wraith mode" auto-expiry / incognito.** A one-tap toggle that captures
  nothing (or captures to a session that self-deletes on close). Fits the
  spooky name and is genuinely useful for sensitive sessions.
- 🎩 **Snippet expansion.** Star an entry and give it a short keyword; typing the
  keyword (desktop) expands to the full snippet. Turns the starred list into a
  lightweight text-expander.
- 🎩 **Smart actions on detected content.** The sensitive-data detector already
  recognizes structured content — extend it to *offer actions*: "looks like a
  URL → open", "looks like a 2FA code → copy digits only and auto-expire in
  30s", "looks like JSON → pretty-print".
- 🎩 **Paste stack / multi-paste.** Queue several entries and paste them in
  sequence with repeated hotkey presses (a classic power-user clipboard trick).
- 🎩 **Quick-look transforms.** In the preview, one-click "trim whitespace",
  "to lowercase", "strip formatting", "base64 decode", "URL-decode" — paste the
  transformed version without mutating history.
- 🎩 **Per-device source labels & icons.** `source_app` is captured but barely
  surfaced; showing "from MacBook / from Pixel" with a tiny glyph makes the
  cross-device story feel alive.
- 🎩 **A literal ghost.** When history is empty, the System-7 styling is begging
  for a little dithered wraith mascot in the empty state. Pure joy, low cost.

---

## 7. Docs & infrastructure 📄

- 📄 **Medium** — README links four docs that **don't exist**: `ARCHITECTURE.md`,
  `IMPLEMENTATION.md`, `ENCRYPTION.md`, `SENSITIVE.md` (`README.md:58–62`).
  `memory/AGENTS.md` even calls them "key files to read first." Either create
  them or drop the links. (Encryption and sensitive-data designs are real and
  worth documenting — see §3.4.)
- 📄 **Medium** — `scripts/sync-version.sh` doesn't update `package.json`,
  `server/ui/package.json`, or `src-tauri/tauri.conf.json`, so a version bump
  silently drifts across those three.
- 📄 **Low** — CI builds/tests desktop + server + frontend but **not** Android;
  at minimum a `cargo check` against `aarch64-linux-android` would catch mobile
  breakage before tags.
- 📄 **Low** — `release.yml` doesn't depend on the CI job, so a red build can
  still ship binaries on a tag. Add `needs: [ci]` (or re-run checks).
- 📄 **Low** — server/ui has `.ts` but no `svelte-check`/type-check step.

---

## 8. What's genuinely good 👏

Worth saying, because it's a lot for an LLM-assisted codebase:

- Clean separation: `copywraith-core` shared models, server, desktop, mobile.
- Thoughtful crypto: Argon2id → HKDF → per-record AES-GCM, atomic `auth.json`
  writes, plaintext/ciphertext passthrough for smooth migration.
- Sensible flavor model (`ClipboardFlavors`) with legacy-compatible hashing.
- Real attention to macOS hard parts: NSPanel for fullscreen Spaces, focus
  restoration, monitor-event suppression around our own paste writes.
- Sensitive-data masking that runs server- *and* client-side so secrets don't
  even reach the JS context.
- Keyset (cursor) pagination already designed into the API — the sync bug is in
  *how the client uses it*, not the API shape.

---

## 9. What I'm implementing now

Picked for high confidence + impact + low mutual conflict (each in its own
branch/PR, touching mostly disjoint files):

| Branch | Items | Files (disjoint) |
|---|---|---|
| `claude/fix-android-sync-watermark` | §1.1, §1.2 — `(updated_at,id)` watermark cursor; don't let one bad entry block it | `src-tauri/src/sync.rs`, `src-tauri/src/storage.rs` |
| `claude/rtf-robustness` | §2.1, §2.2 — RTF depth-underflow guard + CP1252 hex decode | `crates/copywraith-core/src/content.rs` |
| `claude/settings-url-validation` | §5 (URL validation) — validate server URLs in Settings | `src/lib/components/SettingsDialog.svelte` |
| `claude/missing-docs` | §7 — write the four referenced docs | `ARCHITECTURE.md`, `ENCRYPTION.md`, `SENSITIVE.md`, `IMPLEMENTATION.md` |
| `claude/server-field-limits` | §3.1, §3.2 — per-field size cap + ULID id validation | `server/src/api.rs` |

Deferred (bigger/riskier or needs product calls): lazy mobile blobs (§1.3),
WorkManager background sync (§1.4), delete propagation (§5), retention policy
(§5), secure key storage (§3.3), the 🎩 ideas in §6.
</content>
