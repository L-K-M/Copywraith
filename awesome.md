# Copywraith Review: Bugs, Issues, Missing Features & Ideas

A full-codebase review covering the core crate, sync server, Tauri app (desktop +
Android), and both Svelte frontends. Entries marked **[implemented]** have a
companion PR; everything else is documented here for future work.

---

## 1. Bugs

### 1.1 Sensitive entries get duplicated as corrupted "abc•••" entries via sync — **[implemented]**

The server masks sensitive entries on the wire (`mask_sensitive_entry` in
`server/src/api.rs`): list/get responses replace the text with
`first-3-chars + bullets`. But the app's pull path
(`src-tauri/src/sync.rs::ingest_remote_entry`) computes a content hash from the
*masked* flavors, which never matches the original entry's hash. The result:

- Every syncing device — **including the device that created the entry** —
  ingests the masked text as a brand-new entry.
- Pasting that entry on another device pastes literal bullet characters.
- The masked duplicate may itself sync back, multiplying junk entries.

Fix shipped: the pull path now skips entries flagged `sensitive`, since the
server only ever serves a masked (useless) copy of them. The original device
keeps the real content locally. Long-term, see idea 4.6 for real sensitive-entry
sync.

### 1.2 Delete button renders the literal text `\u2715` — **[implemented]**

`src/lib/components/EntryRow.svelte` line 182 puts `\u2715` directly in the
markup. Svelte templates are HTML — JS string escapes are not interpreted there
— so every row's delete button literally displays `\u2715` instead of ✕.
(Line 148 gets it right because the escape lives inside a JS expression.)

### 1.3 `strip_rtf` can panic on malformed RTF — **[implemented]**

In `crates/copywraith-core/src/content.rs`, `depth` is a `usize` decremented on
every `}`. Input like `{\rtf1}}` underflows it: panic in debug builds,
wraparound (and garbage group tracking) in release. Clipboards regularly carry
slightly malformed RTF from third-party apps, so this is reachable from
"user copies something weird". Fixed with a saturating decrement.

### 1.4 `strip_rtf` drops all non-ASCII text (no `\uN` support) — **[implemented]**

RTF encodes non-ASCII characters as `\uN` unicode escapes (e.g. `\u8217?` for
’), and macOS/Word emit these constantly. `strip_rtf` treated `u` as an unknown
control word and skipped it, so RTF-only entries lost every accented character,
em-dash, curly quote, emoji, and all CJK text in previews and plain-text paste.
Fixed: `\uN` is now decoded (including the standard "skip the fallback
character after the escape" rule and surrogate-pair handling).

### 1.5 `strip_html` misaligns on non-ASCII uppercase text — **[implemented]**

`strip_html` lowercases the whole input and walks the original and lowercase
strings with a *shared char index*. Unicode lowercasing can change the char
count (e.g. `İ` → `i̇`), after which the `<style>`/`<script>` detection reads
the wrong offsets. Harmless for ASCII HTML, but the detection silently breaks
for some international content. The `&lower[..].starts_with(..) == &true`
construction is also a readability red flag. A cleaner approach: compare tag
names with `eq_ignore_ascii_case` on a char-slice window (HTML tag names are
ASCII by spec), no parallel lowercase copy needed.

### 1.6 `\'xx` hex escapes in RTF decode as Latin-1, not the document codepage

`strip_rtf` does `result.push(byte as char)` for `\'xx`, which is only correct
for Latin-1. Windows RTF commonly uses CP1252 where e.g. `\'93`/`\'94` are
curly quotes but decode to control characters under Latin-1. Low impact (most
writers emit `\uN` too — supported as of 1.4), noting for completeness.

### 1.7 Local search treats `%` and `_` as wildcards — **[implemented]**

`LocalStorage::get_entries` builds `search_text LIKE '%<query>%'` without
escaping. Searching for `100%` matches everything starting with `100`;
searching for `_` matches every single-character position. Fixed by escaping
`%`, `_`, and the escape character with `ESCAPE '\'`.

### 1.8 Wrong-password attempts are cheap while the server is unlocked — **[implemented]**

`CryptoState::verify_and_unlock` has a fast path: once unlocked, incoming
passwords are checked with a bare SHA-256 against a cached hash, and a
*mismatch returns immediately*. The Argon2id work factor (64 MiB, t=3) only
applies while the server is locked — i.e. exactly when an attacker would not be
brute-forcing a live server. Since the server is essentially always unlocked in
normal operation, brute-force attempts run at SHA-256 speed, defeating the
point of the KDF. The README's "no brute-force protection" caveat covers some
of this, but the fix is cheap: on fast-path mismatch, fall through to the full
Argon2 verification so wrong guesses always pay the KDF cost.

### 1.9 `Storage::get_entry` (server) swallows database errors — **[implemented]**

`server/src/storage.rs::get_entry` uses `.ok()` on the query, so a real SQLite
error (corruption, I/O) is reported to clients as `404 Not Found` instead of
`500`. Same pattern in `create_entry`'s dedup lookup. Fixed with
`.optional()?` so only "no rows" maps to `None`.

### 1.10 Pull cursor can skip entries when an old entry is touched remotely

`SyncClient::pull_new_entries` walks pages newest-first and stops at
`last_seen_server_id`. The list is ordered by `updated_at DESC`, and the
"last seen" entry's `updated_at` moves whenever it is re-copied or re-starred
on another device. If that happens, the cursor entry jumps to the top of the
list and the scan stops immediately — entries created *after* the previous
sync but *below* the moved entry are silently skipped until "Reset sync
cursor" is used. A timestamp-based cursor (`pull everything with
updated_at > last_pulled_at`, with a small overlap window) would be more
robust than an ID-based one.

---

## 2. General Issues

### 2.1 Server masking vs. encrypted-at-rest design is incoherent

Every data endpoint already requires the full password — the same password that
derives the DEK. A caller who can list entries can, by definition, decrypt
everything. Masking sensitive entries on those same authenticated responses
adds no confidentiality against that caller; it only breaks sync (bug 1.1).
Either masking should be a presentation concern (client-side, as the Tauri app
already does for its own UI), or sensitive entries deserve real end-to-end
treatment (idea 4.6).

### 2.2 Every API request re-verifies the password

`ensure_authorized` runs on each request. With the cache this is one SHA-256
per request (fine), but it also means `/api/auth/lock` is largely symbolic: the
very next authenticated request silently re-unlocks the DEK. If "lock" is meant
to be a real protective state (e.g. before a backup or while away), requests
arriving with the correct password should perhaps still be rejected until an
explicit `/api/auth/unlock`.

### 2.3 Encrypted search loads and decrypts the entire table

When encryption is enabled (the normal case), any search in the admin UI does
`SELECT *` over all entries, decrypts each row, and substring-filters in
memory. Fine at 1k entries; painful at 100k. Options: a searchable token index
(trigram HMACs), client-side search over a cached window, or simply
documenting the limit. Related: with encryption enabled, FTS triggers and the
`entries_fts` table sit unused forever.

### 2.4 CORS is `Any` on a password-protected API

`server/src/main.rs` allows any origin, method, and header. Combined with
bearer-token auth this is not catastrophic (browsers won't attach the token
cross-origin), but it does let any website probe `/api/health` reachability of
a user's LAN server from inside their browser. Tightening to the admin UI's
origin (or making it configurable) would be more hygienic.

### 2.5 Unbounded clipboard history

Neither the app's SQLite DB nor the server prunes anything, and every image
blob is kept forever. A clipboard manager that captures *everything* grows
fast — screenshots especially. See idea 4.1; at minimum the README should warn
about disk growth.

### 2.6 `atomic_write` doesn't fsync — **[implemented]**

`server/src/crypto.rs::atomic_write` writes the temp file and renames it
without `File::sync_all` (or fsyncing the directory). On power loss the rename
can survive while the data doesn't, leaving a truncated `auth.json` — which the
loader then (correctly) refuses to start on. One `sync_all()` before the rename
makes the crash-safety claim true.

### 2.7 Settings round-trip races in `set_shizuku_clipboard_enabled`

`commands.rs` does `get_settings` → mutate → `save_settings` with no lock
around the read-modify-write. A concurrent `update_settings` from the UI can be
silently overwritten. Low likelihood, but the settings writer would be safer as
a single `UPDATE ... WHERE key=` per field or guarded by one mutex.

### 2.8 Duplicated storage/model plumbing between app and server

`row_to_entry`, `ensure_entries_column`, `backfill_flavor_columns`, blob
read/write/GC, and the LIKE/paging query builders are near-copies in
`src-tauri/src/storage.rs` and `server/src/storage.rs`. They have legitimately
diverged a little (encryption, FTS), but a shared `copywraith-storage` crate
with a `TextTransform` hook would remove a whole class of "fixed it in one
place" bugs — this review found several issues that exist on only one side of
the copy.

### 2.9 `now_rfc3339` in sync.rs reinvents `Utc::now().to_rfc3339()`

It goes through `SystemTime → duration → DateTime` with a fallback that would
report 1970 on clock error. `chrono::Utc::now().to_rfc3339()` is equivalent
and simpler.

### 2.10 Swagger UI loads from unpkg CDN

`/swagger-ui` pulls JS/CSS from `unpkg.com` at runtime. On a LAN/VPN-only
deployment (the documented threat model!) this breaks without internet and is
an odd external dependency for an otherwise self-contained binary. Vendoring
the two files (or using `utoipa-swagger-ui`'s embedded mode) keeps it offline.

---

## 3. Missing Features

### 3.1 Delete does not propagate

`delete_entry` is local-only on the app and standalone on the server. Deleting
an entry on the Mac leaves it on the server and on every other device — and a
"deleted" secret quietly lives on in the admin UI. Proper fix is tombstones
(deleted_at + sync of deletions); a cheap interim fix is "Delete everywhere"
in the UI calling the server's `DELETE /api/entries/{id}` (requires mapping
local↔server IDs, e.g. by content_hash).

### 3.2 No retention / pruning controls

"Keep last N entries", "keep images for 7 days", "always keep starred" — none
exist on either side. (See 2.5.)

### 3.3 No pause/incognito toggle

Every clipboard manager eventually needs a "stop watching for 15 minutes"
switch (screen-sharing, password managers that don't mark their clipboard as
transient, etc.). The tray/menubar would be the natural home. Related: on
macOS, respecting the `org.nspasteboard.ConcealedType` /
`TransientType` pasteboard markers would auto-skip password managers.

### 3.4 PATCH can't toggle `sensitive`

The detection heuristics will inevitably have false positives (masked forever)
and false negatives. `UpdateEntryRequest` only supports `starred`; the app UI
has no "mark as sensitive / not sensitive" either. Both sides should allow
overriding the flag.

### 3.5 Windows/Linux paste simulation

`simulate_paste` is macOS-only; on other desktops Copywraith silently degrades
to copy-without-paste with only a log line. Even without synthetic keystrokes,
a user-visible "copied — press Ctrl+V" toast would beat silence. (`enigo` or
platform `SendInput`/`xdotool` would close the gap properly.)

### 3.6 Server admin UI cannot delete or star

The Svelte admin UI is read-only beyond unlock. The API supports
PATCH/DELETE; wiring up star/delete/bulk-delete in the admin table is cheap
and makes the server useful as a management surface.

### 3.7 No export/import

Clipboard history is valuable data living in an undocumented SQLite schema.
A `copywraith export --format jsonl` (and matching import) would cover backup,
migration, and "grep my clipboard from last month" in one stroke.

### 3.8 No image support on Android clipboard write

`write_to_clipboard_mobile` silently does nothing for images. Android supports
image clipboard via `ClipData` content URIs; until then, a toast ("images
can't be copied on Android yet") would avoid the current silent no-op.

---

## 4. Novel / Cool / Delightful Ideas

### 4.1 Paste stack mode

A classic power feature that fits Copywraith's popup perfectly: select several
entries (or "stack" the last N copies), then each Cmd+V pastes the next item
off the stack. Great for transferring forms field-by-field. The popup already
has selection plumbing; the paste-simulation code already exists.

### 4.2 The wraith deserves an idle animation

The app is called Copywraith, has a ghost icon with toe-notches… and the UI
never haunts. A tiny ghost that drifts across the (empty) entry list, or a
"👻 *whoosh*" micro-animation when an entry is pasted/synced, would give it
the personality the name promises. System 7 aesthetic + ghost = Casper-era
charm, basically free.

### 4.3 Clipboard time machine ("what did I copy around 3pm Tuesday?")

A date-jump control in the filter bar (or `@tuesday`, `@yesterday 15:00`
filter syntax) that scrolls the history to a point in time. The data model
already has precise timestamps; this is pure frontend.

### 4.4 Smart paste transforms

A small transform menu (or `Cmd+Shift+T` on selection): paste as UPPER/lower/
slug-case, JSON-pretty-printed, base64-decoded, URL-decoded, with-smart-quotes-
stripped, etc. The transforms run locally on `best_plain_text()` — twenty
lines each, huge daily value. Power version: user-defined transform scripts.

### 4.5 QR code handoff

For "send this to my phone *without* setting up the sync server": click an
entry → show a QR code of its text. Useful at the LAN-party / friend's-laptop
moments when sync isn't configured. Pure frontend (qr generation lib), no new
backend surface.

### 4.6 True end-to-end sensitive sync

Instead of masking sensitive entries (1.1/2.1), encrypt them client-side with
a key derived from the password *before* upload, and let other devices decrypt
locally. The server stores an opaque payload it can't even theoretically leak;
sync fidelity is restored; the masked-preview behavior stays purely
presentational. The crypto building blocks (Argon2 → HKDF → AES-GCM) already
exist in `server/src/crypto.rs` and would just move into `copywraith-core`.

### 4.7 Source-app icons and filtering

`source_app` is already captured on macOS but only shows as text in the
preview dialog. Show the app icon per row, and make it clickable to filter
("show me everything I copied from Slack"). The filter side is one extra WHERE
clause; icons are a lookup via `NSWorkspace`/app bundle.

### 4.8 Duplicate-aware "frequent clips" view

The dedup logic already counts re-copies implicitly (every duplicate bumps
`updated_at`). Track an explicit `copy_count`, then offer a "Frequently
copied" sort — your top-20 clips are effectively your personal snippet
library, generated automatically. Pairs well with starring suggestions
("you've copied this 9 times — star it?").

### 4.9 Shizuku status surfacing

The Android Shizuku listener can die silently (status only refreshes when the
settings dialog opens). A small status dot in the mobile status bar — green
listening / grey off / red error — would make the most fragile part of the
Android story observable at a glance.

### 4.10 `/api/health` as a tiny status page

The server already serves an admin UI; a `?pretty` health view with uptime,
entry count, blob-store size, DB size, and last-sync-seen timestamps would
make the "is my Tailscale route up?" check humane instead of `curl | jq`.

---

## Companion PRs

| Item | Fix | PR |
|------|-----|----|
| 1.3 / 1.4 / 1.5 RTF panic, RTF unicode, HTML misalignment | rewritten strippers + tests | #27 |
| 1.1 Sensitive sync corruption | skip masked sensitive entries on pull | #28 |
| 1.2 `\u2715` delete button | render ✕ via JS expression | #28 |
| 1.7 LIKE wildcard escaping | escaped local search | #28 |
| 1.8 cheap brute-force fast path | fall through to Argon2 on mismatch | #29 |
| 1.9 swallowed DB errors | `.optional()?` in server storage | #29 |
| 2.6 non-durable `atomic_write` | fsync before rename | #29 |
