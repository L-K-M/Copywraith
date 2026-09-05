# Copywraith — Independent Review (opus.md)

Reviewer: Claude Opus 5. Date: 2026-07-25. Reviewed tree: `main` @ `9ca8179`
(working branch `claude/pasteboard-manager-review-w03m9z`).

> **Historical record — read as a snapshot, not as current state.**
>
> This document describes `9ca8179` and is kept as the evidence trail for that
> review. Current `main` is 0.3.1 and has since taken Ubuntu/Linux paste, popup
> and release-gating work (PRs #104, #105, #106, #109) that this review predates,
> so file:line citations here have drifted and some findings may already be moot.
>
> The companion implementation PRs from this review — #88, #89, #90, #91, #92,
> #94, #95 — are **open and under review. None of them has merged.** Nothing
> below has shipped.
>
> **Withdrawn on maintainer direction.** The popup's varied and large type sizes,
> its several accent colours, and the absence of dark mode are how System 7
> worked, not defects. VIS-01 and §6 items 1, 3, 4 and 7 rest on the opposite
> premise and are withdrawn; they are marked in place. §6's framing question came
> from a "high-value iOS app" brief that was included in the original request by
> accident. Do not act on the withdrawn items. Everything else in §4 and §6 —
> `hue-rotate()` as a colour, the narrow status bar, the absent admin mobile
> layout, the missing first-run state — stands on its own terms.

This is a fresh, code-first review of the whole repository: server, macOS/Linux
desktop client, Android client, shared core crate, and admin UI. It is written to
stand on its own — every finding cites the file and the mechanism, so it can be
acted on without re-deriving the analysis.

It deliberately does **not** repeat findings from `sol.md` / `awesome.md` /
`ANALYSIS.md` that are already fixed on `main`. Where a previously-known item is
still live in the code, it is re-stated with current evidence and a new ID.

---

## 0. Verification baseline

Run at review time on this tree, before any change:

| Check | Command | Result |
|---|---|---|
| Popup frontend build | `npm run build` | Pass |
| Popup type check | `npm run check` | Pass, 0 errors |
| Server UI build | `cd server/ui && npm run build` | Pass |
| Rust tests (core) | `cargo test -p copywraith-core` | Pass, 50 tests |
| Rust tests (server) | `cargo test -p copywraith-server` | Pass, 9 tests |
| Rust tests (workspace) | `cargo test --workspace` | Pass (needs GTK dev libs installed first) |

At review time the open PRs on the repo were **Dependabot-only** (21 of them,
#17–#86). Every implementation PR referenced in the `ANALYSIS.md` ledger
(#32–#36, #41–#49) had been merged or closed, so that ledger was stale and was
dropped when `ANALYSIS.md` was rebuilt. That is no longer the repo's state: the
seven implementation PRs opened from this review (#88–#92, #94, #95) and #97
(KDE follow-ups) are open and are not Dependabot's.

`ANALYSIS.md` also claims GitHub Actions "creates jobs with zero steps and no
assigned runner for every new PR". That is **no longer true** — the PRs opened
from this review received runners immediately and CI ran green. Red badges can
be read as real results again.

---

## 1. Headline: why the Android client takes a huge amount of time to sync

This was the user's specific complaint, so it gets its own section. There is no
single cause — there are **eight compounding ones** (SYNC-A1 … SYNC-A8), and
they multiply. The per-entry cost of a pull is roughly `1 read + 2–3 fsynced
writes + 1 sensitive scan + N HTTP round trips`, and the whole thing is serial.

### SYNC-A1 — Every ingested entry costs up to four separate SQLite transactions (dominant cost)

`src-tauri/src/sync.rs:571` `ingest_remote_entry` calls, in sequence:

1. `storage.has_content_hash()` — `src-tauri/src/storage.rs:299`
2. `storage.insert_entry()` — `storage.rs:202`
3. `storage.set_starred()` — `storage.rs:309`
4. `storage.mark_synced()` — `storage.rs:511`

Each of these takes `self.db.lock()` and issues a standalone statement, i.e. an
**implicit transaction each**. `LocalStorage::new` (`storage.rs:146`) sets
`journal_mode=WAL` but **never sets `synchronous`**, so SQLite defaults to
`FULL`: every implicit *write* transaction fsyncs the WAL.

Only writes fsync, and only three of the four calls write. `has_content_hash` is
a `SELECT`, and `set_starred` runs only when the remote entry is starred — so a
typical unstarred new entry costs **two** fsyncs and a starred one **three**. On
Android's flash an fsync is commonly 5–40 ms, so pulling 500 unstarred entries is
~1,000 fsyncs — **5 to 40 seconds of pure disk-flush time**, before any network.
That is still the largest single component of the reported latency.

Fix: set `PRAGMA synchronous = NORMAL` (under WAL this keeps the database
consistent and keeps commits durable across an *application* crash; what it gives
up is durability across an OS crash or power loss, which can roll back any
transaction committed since the last checkpoint — not just the last one. For a
clipboard cache that re-syncs, that is an acceptable trade), add `busy_timeout`,
and collapse the four calls into one transaction-wrapped storage method.

### SYNC-A2 — Push is one sequential HTTP POST per entry, and re-reads settings every time

`sync.rs:193` `sync_unsynced_entries` loops entries and awaits
`sync_entry(...)` one at a time. `sync_entry` (`sync.rs:207`) opens with
`storage.get_settings()` — which locks the DB and runs **seven separate
`query_row` calls** (`storage.rs:517`) — then `configured_server_urls(...)`,
**per entry**.

So a first-run Android push of 200 local entries = 200 sequential round trips
(each up to 30 s timeout) + 1,400 settings queries. On a Wi-Fi RTT of 30 ms
that is 6 s of pure latency minimum; over a VPN fallback, far worse.

Fix (cheap): hoist settings/endpoint resolution out of the loop. Fix (real): a
batch `POST /api/entries:batch` endpoint, or bounded concurrency on the push.

### SYNC-A3 — A timed-out `sync_now` throws away all pull progress, so it never converges

`commands.rs:698` wraps `pull_new_entries` in `tokio::time::timeout(35s, …)`.
The frontend wraps *that* in a 45 s `withTimeout` (`+page.svelte:295`).

`pull_new_entries` only promotes the watermark **after the whole multi-page walk
finishes** (`sync.rs:376`). Cancelling the future at 35 s therefore discards the
watermark advance entirely. Ingested rows survive (ingestion is idempotent), but
the next sync **restarts the page walk from the top of the server list** and
re-does `has_content_hash` for every entry it already has.

Consequence: on a history large enough that one pass exceeds 35 s, Android sync
**can never complete**. Every app open pays the full re-scan and then reports
"Timed out" with `pulled: 0`. This exactly matches the reported symptom.

Fix: persist a resume cursor so a cancelled pass restarts where it stopped, and
stop treating a partial pass as a failure.

### SYNC-A4 — The first pull is unbounded: the entire server history, newest-first, serially

There is no bootstrap bound. `pull_new_entries` (`sync.rs:253`) pages 100 at a
time with `include_sensitive=true` and ingests **everything** the server has, and
`fetch_blob_data` downloads each image/file blob in its own sequential request
inside the ingest loop. A server with a few thousand entries and a few hundred
images means hundreds of serial HTTP GETs on a phone.

Fix: bound the initial bootstrap (e.g. most recent N or last M days), then
backfill in the background; and fetch blobs lazily/on-demand rather than inline
during the metadata walk.

### SYNC-A5 — The list response ships every text flavor in full

`api.rs:402` `list_entries` returns full `ClipboardEntry`s: `text_content`,
`text_plain`, `text_html`, **and** `text_rtf` for all 100 rows. A single rich-text
copy from a word processor is routinely 50–500 KB of RTF/HTML. A 100-row page can
therefore be tens of megabytes of JSON that the phone must download, parse, and
re-serialize.

There is no metadata-only projection for sync, and no compression negotiated.

Fix: a `fields=` / `projection=metadata` mode on the list endpoint, plus
`tower_http::compression::CompressionLayer` on the server (a one-line addition
that would cut text payloads ~5–10×).

### SYNC-A6 — `COUNT(*)` on every page

`storage.rs:656`: the non-search path runs a full `SELECT COUNT(*)` with the same
WHERE clause **before** every page query, only to compute `has_more`
(`api.rs:436`). For keyset pagination this is pure waste — fetching `limit + 1`
rows answers `has_more` for free.

With encryption **and** a search term active, `list_entries` takes the
`memory_search` branch (`storage.rs:610`) and **loads, decrypts, and filters the
entire table on every request**. That is O(whole database) per keystroke in the
admin UI.

### SYNC-A7 — The sync loop sleeps *before* its first pass

`lib.rs:807`: `loop { sleep(current_interval).await; …sync… }`. The sleep comes
first, so there is a guaranteed 5 s dead window at startup before anything syncs
— on Android, where the process is killed and restarted constantly, this is paid
on every launch.

Also `AppState`/`LocalStorage` are used from `async` Tauri commands but every
storage call is **blocking** `rusqlite` + `std::fs` executed directly on the
async runtime's worker threads (`commands.rs` throughout). On a phone with a
small runtime thread pool, a slow blocking read stalls unrelated IPC.

### SYNC-A8 — Focus-driven refresh storm

`+page.svelte:125`: `if (mobile && focused) void refreshMobileEntries(...)`.
Android fires focus changes for keyboard show/hide, dialogs, permission prompts,
notification shade, etc. `mobileRefreshInFlight` guards *concurrent* runs but
there is **no cooldown after completion**, so a user tapping in and out of the
settings dialog triggers a full capture→import→sync→reload cycle each time.

And each `refreshMobileEntries` calls `loadEntries()` **up to four times**
(lines 250, 283, 314, and after import) — four full 100-row IPC round trips per
refresh.

> **Estimated effect of fixing A1–A3 alone: first-sync wall time should drop by
> roughly an order of magnitude.** A1 removes most of the disk time, A2 removes
> most of the push latency, A3 makes the pull actually converge.

---

## 2. Correctness bugs

### BUG-01 — Pulled entries lose their identity and chronology *(high)*

`sync.rs:631` ingests remote rows through `LocalStorage::insert_entry`, which
mints a **fresh ULID** and sets `created_at = updated_at = Utc::now()`
(`storage.rs:252`). The server's `id`, `created_at`, and `updated_at` are
discarded.

Because the server returns newest-first and the client walks pages in that order,
the **oldest** server entry is inserted **last** and therefore gets the
**newest** local timestamp. After a fresh install the Android history is
displayed in *reverse* chronological order, and "paste most recent" picks the
wrong item.

This is `sol.md` SYNC-03 and it is still fully present. Fixing it needs a
dedicated remote-upsert path that preserves server id/timestamps.

### BUG-02 — Double-clicking a row pastes twice, then opens the preview *(medium)*

`src/lib/components/EntryRow.svelte:132-134` binds both `onclick` and
`ondblclick` to the same `<tr>`. A double-click emits `click, click, dblclick`.
`handleClick` calls `pasteEntry` — which writes the clipboard, hides the popup,
and simulates Cmd+V. So a double-click *pastes the entry twice into the target
app* and only then tries to show a preview over a hidden window.

### BUG-03 — The admin UI's RTF stripper produces wrong previews *(medium)*

`server/ui/src/lib/EntryRow.svelte:120`:

```js
.replace(/\{\\(?:fonttbl|colortbl|stylesheet|info|\*\\)[^}]*(?:\{[^}]*\}[^}]*)*\}/g, '')
```

`[^}]*` is greedy but **cannot cross a `}`**, so this pattern always terminates
at the *first* closing brace — it only ever removes the first nested group of a
header. Real word processors emit nested font and colour tables, and the
remainder falls through to the blunt `\\[a-z]+\d*\s?` and `[{}]` passes.
Measured outputs:

| input | output |
|---|---|
| `{\fonttbl{\f0 Helvetica;}{\f1 Times New Roman;}}Actual text` | `"Times New Roman;Actual text"` |
| `Line one\par Line two` | `"Line oneLine two"` |
| `Escaped \\{braces\\} here` | `"Escaped \\braces\\ here"` |
| `{\colortbl;...;}Body copy` | `"copy"` |

> **Correction.** My first pass called this a ReDoS, on the shape of
> `(?:\{[^}]*\}[^}]*)*\}`. That was wrong, and I verified it: measured against
> several adversarial inputs up to 3,200 repetitions the pattern stays
> near-linear — precisely *because* the greedy `[^}]*` stops at the first `}`
> and the match succeeds immediately. The same property is what causes the
> correctness bug. Replace it for wrong output, not for a hang.

### BUG-04 — Opening "Sync Details" performs a full sync *(medium)*

`StatusBar.svelte:99` — `toggleSyncDetails` calls `refreshSyncStatusFromDetails`,
which calls `TauriService.syncNow()`. A read-only status panel triggers the most
expensive operation in the app. On Android this means "I wanted to see why sync
is slow" starts another slow sync. There is no separate explicit *Sync Now*
control anywhere in the UI.

### BUG-05 — `viewport-fit=cover` is missing, so all safe-area CSS is dead *(medium)*

`src/app.html:6` is `content="width=device-width, initial-scale=1"`. Without
`viewport-fit=cover`, `env(safe-area-inset-*)` resolves to **`0px`** in the
WebView. That silently disables:

- `.window-frame.mobile` `--safe-area-top` / `--safe-area-bottom`
  (`+page.svelte:607`)
- the entire `.s7-backdrop` padding and dialog `max-width`/`max-height` clamp in
  `src/app.css:8-37`

The Android `max(env(...), 24px)` fallback (`+page.svelte:613`) masks the top
inset, but the **bottom** gesture bar and dialog viewport clamping are unprotected
— dialogs can render under the navigation bar. One attribute unblocks work that
is already written.

### BUG-06 — Admin blob re-fetch on every list refresh *(medium)*

`server/ui/src/lib/EntryRow.svelte:33` — the `$effect` reads `entry.content_type`
and `entry.blob_url`, but since `entry` is a reassigned prop object, every
`loadEntries()` (which happens after **every star toggle**, `App.svelte:219`)
re-runs the effect, revokes the object URL, and re-downloads every image blob on
the page. Starring one row re-downloads up to 50 images.

### BUG-07 — Data URLs hardcode `image/png` *(low)*

`EntryRow.svelte:154`, `EntryPreview.svelte:92` both emit
`data:image/png;base64,{imageData}` regardless of actual format. Browsers sniff
`<img>` sources so it usually renders, but it is wrong, it defeats any future
CSP/`img-src` tightening, and the admin download path already has correct MIME
handling (`App.svelte:259`) that the popup does not share.

### BUG-08 — `handleLock` in the admin UI locks the whole server *(low, sharp edge)*

`App.svelte:139` → `POST /api/auth/lock` → `crypto.lock()` clears the process-wide
DEK (`crypto.rs:260`). A user clicking "Lock" in a browser tab to end *their*
session silently breaks **every** connected desktop and Android client until
someone unlocks the server again. Lock is presented as a session action but is a
global server operation.

### BUG-09 — Wrong password stalls the entire server *(low, but a real DoS)*

`api.rs:723` `ensure_authorized` holds `state.crypto.lock()` (a
`std::sync::Mutex`) across `verify_and_unlock`. On a cache miss that runs
**Argon2id at 64 MiB / t=3 / p=4** (`crypto.rs:371`) on a Tokio worker thread
while holding the global lock. Any client with a stale password blocks every
other request for the duration, repeatedly. The correct-password fast path
(SHA-256 compare, `crypto.rs:155`) is fine; it is the miss path that is dangerous.

### BUG-10 — Local delete is never propagated *(medium)*

`commands.rs:106` `delete_entry` only touches local storage. The row stays on the
server. It will not immediately come back (the watermark has passed it), but
`Reset Sync Cursor` — which the Settings dialog offers to users as "Mobile Sync
Repair" (`SettingsDialog.svelte:252`) — **resurrects every entry the user ever
deleted on any device**. There are no tombstones.

### BUG-11 — Relative timestamps never update *(low)*

`EntryRow.svelte:9` `formatTime` runs once at render. A row that said "now" says
"now" an hour later, until something forces a re-render. Every row also
constructs a fresh `Date` and `toLocaleString()` for its `BalloonHelp` tooltip on
mount — 100 rows × 2 date formattings on every list load.

### BUG-12 — Platform flash on startup *(low)*

`platform.ts:4` initialises `platform` to `''`, so `isMobile` is `false` for the
first frames. On Android the app therefore renders the **desktop** shell first —
System 7 title bar, "Click to paste · Opt+Click plaintext · ↑/↓ select · Enter
paste" hint — and then swaps. It is a visible, cheap-looking flash on every cold
start.

### BUG-13 — Loaded count is presented as total *(low)*

`StatusBar.svelte:19` `entryCount = $entries.length`. With `PAGE_SIZE = 100`, a
5,000-entry history reads "100 items". There is no total, and `has_more` is
inferred from `result.length === PAGE_SIZE` (`clipboardStore.ts:110`) which is
wrong by one page whenever the total is an exact multiple of 100.

---

## 3. Performance (beyond the Android sync path)

### PERF-01 — `get_entries` does all the expensive text work twice, per entry *(high)*

`commands.rs:39-41`:

```rust
let preview = e.preview(200);       // -> best_plain_text() -> resolved_flavors() -> strip_html/strip_rtf
let plain_text = e.best_plain_text(); // -> resolved_flavors() -> strip_html/strip_rtf   AGAIN
```

`ClipboardEntry::resolved_flavors()` (`models.rs:255`) **clones every flavor
string**, and `best_plain_text()` runs `strip_html`/`strip_rtf`
(`content.rs:43`, `content.rs:191`) — both of which allocate a
`Vec<char>` **over the entire document** (4 bytes per char) and walk it.

`row_to_entry` (`storage.rs:21`) already did a `merge_legacy` clone before that.
So a 100-row page containing rich text does, per row: 3 full flavor clones, 2
full `Vec<char>` expansions, and 2 complete HTML/RTF parses.

This is on the critical path of **every popup open** and every Android list
refresh. It is the single cheapest large win in the codebase — compute once.

### PERF-02 — `full_text` ships the complete text of all 100 rows over IPC *(high)*

`models.rs:10` `EntryForFrontend.full_text` is the entry's **entire** plain text,
untruncated, for every row in the list. It is consumed by exactly one component —
`EntryPreview.svelte:97`, which shows one entry at a time.

Tauri IPC serialises this to JSON and hands it to the WebView. On Android that is
a string copy across the JNI/WebView boundary for content the user will almost
certainly never open. `loadEntries()` runs up to four times per mobile refresh
(§SYNC-A8).

### PERF-03 — Every image in the list is decoded and base64'd eagerly *(high)*

`EntryRow.svelte:55` — `onMount` unconditionally calls `getEntryImage(entry.id)`
for every image row. `commands.rs:71` reads the whole blob and base64-encodes it
(+33 % size), returns it as a JSON string, and the row renders it as a
`data:` URL.

There is no viewport gating, no cancellation, no thumbnailing, and no size cap.
A page containing 20 screenshots at 2 MB each means ~53 MB of base64 pushed
through IPC and held as data URLs in the DOM — on a phone. The 48 px-tall
rendered thumbnail needs roughly 0.02 % of those bytes.

### PERF-04 — Client-side search is an unindexed `LIKE '%…%'` *(medium)*

`storage.rs:336`: `search_text LIKE ?N ESCAPE '\\'` with a leading `%`. This is a
full table scan of the whole clipboard history on every keystroke (150 ms
debounce, `clipboardStore.ts:185`). The **server** has a proper FTS5 index
(`server/src/storage.rs:321`); the client has none, despite storing the same
`search_text` column.

### PERF-05 — `contains_sensitive_data` runs nine regex passes per insert

`sensitive.rs:25-33` chains nine checks, each a full scan. Correct and
well-written (patterns are `LazyLock`-compiled), but during a bulk pull it runs
once per ingested entry over the entire text. Worth an early-exit budget on very
large payloads.

### PERF-06 — No HTTP compression on the server

`server/src/main.rs:97` assembles the router with CORS + Trace only. Clipboard
payloads are highly compressible text. Adding
`tower_http::compression::CompressionLayer` is a few lines and is probably the
single highest ratio-of-benefit-to-risk change on the server side.

### PERF-07 — Blocking work on the async executor (server)

Every `api.rs` handler calls into `storage.rs`, which does blocking `rusqlite` and
`std::fs` behind a `std::sync::Mutex`, directly on Tokio worker threads. Combined
with the single global connection, the server serialises all DB work and can
starve its own runtime under concurrent sync clients. `spawn_blocking` + a small
connection pool is the standard fix.

---

## 4. Visual and layout issues

### VIS-01 — Wildly inconsistent type scale *(WITHDRAWN)*

> **Withdrawn.** Per the maintainer, small and varied type sizes are System 7's
> idiom, not leftover debugging, and the large popup preview is deliberate. This
> finding and the priority it carried are void; the table below is retained only
> as a record of the values as they stood at `9ca8179`. VIS-02 (column widths
> declared twice and disagreeing) is a separate, still-valid finding.

The popup and the admin UI both contain oversized values that read as
leftover debugging:

| Location | Value | Neighbouring values |
|---|---|---|
| `src/lib/components/EntryRow.svelte:245` `.text-preview` | **24 px** | badge 9 px, time 11 px |
| `server/ui/src/App.svelte:623` `td` | **22 px `!important`** | — |
| `server/ui/src/App.svelte:618` `th` | **18 px `!important`** | — |
| `server/ui/src/App.svelte:645` `.page-info` | 18 px | version 14 px |
| `server/ui/src/App.svelte:543` `.auth-description` | 16 px | heading 22 px |

The popup content column renders at 24 px while its own type badge is 9 px — a
2.7× ratio inside a single row. The mobile override drops it to 16 px
(`EntryRow.svelte:333`), which strongly suggests 24 px was never intended for
desktop. `!important` on the admin table cells means nothing downstream can
correct it.

~~Nothing else in the app will look considered until this is normalised to a real
scale.~~ (Withdrawn — see above.)

### VIS-02 — Column widths declared twice and disagreeing

`EntryList.svelte:17-23` declares the `DataTable` columns as star 32 px,
type 78 px, time 72 px, actions 36 px. `EntryRow.svelte` then styles the same
cells as star 24 px, type 40 px, time 36 px, actions 20 px
(`EntryRow.svelte:210, 266, 279, 288`). The colgroup wins, so the `td` rules are
dead weight that will mislead the next person editing this file — and the
mobile media query overrides *some* of them (`:317-347`), producing a third set.

### VIS-03 — Selection, hover, and focus are visually identical

`EntryRow.svelte:193-207` — `.entry-row:hover`, `.entry-row.selected`, and
`.entry-row:focus` all set the same background and text colour. Keyboard
navigation is therefore invisible whenever the mouse happens to rest on a
different row, and there is no focus ring at all (`outline: none`).

### VIS-04 — The delete button is invisible on touch and hover-gated on desktop

`.delete-btn { opacity: 0 }` revealed only by `.entry-row:hover`
(`EntryRow.svelte:300-306`). On a touch device there is no hover; the mobile block
sets `opacity: 0.4` (`:349`), but the desktop rule means a keyboard-only user can
never see the control they can nonetheless tab to.

### VIS-05 — Interactive `<tr role="button">` with nested `<button>`s

`EntryRow.svelte:128-138` — a `<tr>` with `role="button"`, `tabindex="0"`,
click/dblclick/keydown handlers, containing two real `<button>` elements. A
button may not contain interactive descendants; screen readers announce the row
as a single button and the star/delete controls become unreachable or
mis-announced. The right shape is `role="grid"` / `role="listbox"` with roving
tabindex.

### VIS-06 — No mobile layout for the admin UI

`server/ui/src/App.svelte:571` — `.window` is `width: min(1180px, 100vw-32px)`
with a five-column fixed-width table (created column alone is 190 px). On a phone
the table overflows horizontally with no stacked-card fallback. There is not a
single media query in the admin UI.

### VIS-07 — Dialog width vs. small screens

`SettingsDialog` is `width="380px"`, `EntryPreview` `420px`, `EntryDetail`
`600px`. The `app.css` clamp that would rescue these depends on `env()` insets
that are currently always zero (BUG-05), and `server/ui` has no equivalent clamp
at all.

### VIS-08 — `filter: hue-rotate()` used to colour the progress bar

`StatusBar.svelte:313-319` tints the sync progress bar for success/error by
hue-rotating the whole element. It is unpredictable across themes, cannot hit a
specified colour, and forces a compositing layer. Success/error should be real
tokens.

### VIS-09 — Status-bar grid collapses badly

`StatusBar.svelte:218` `grid-template-columns: auto minmax(48px, 1fr) auto` with
the endpoint button capped at `min(280px, 42vw)`. Below 920 px the hint is
`display: none` (`:321`), leaving an empty middle column. On a narrow popup the
"Sync: local (192.168.1.5:3742)" label ellipsises to uselessness rather than
degrading to an icon + colour.

---

## 5. User experience

### UX-01 — There is no way to select a row without pasting it

`EntryRow.svelte:73` — single click selects **and immediately pastes and hides
the popup**. You cannot browse, you cannot inspect, you cannot correct a
mis-click. Preview is bound to double-click, which (BUG-02) pastes twice first.

The honest fix is: click selects, `Enter`/double-click pastes, `Space` previews,
and a visible "Paste" affordance on the selected row. That is a behaviour change
and should ship behind a clear release note.

### UX-02 — No undo for delete

`clipboardStore.ts:234` deletes immediately, no confirmation, no undo. The delete
button sits 20 px from the row body that pastes on click. The admin UI *does*
confirm (`App.svelte:513`); the client does not.

### UX-03 — No explicit "Sync Now", and the only sync trigger is a status label

Covered in BUG-04. Mobile users additionally have no pull-to-refresh; the only
way to force a sync is to background and foreground the app, or open Sync Details.

### UX-04 — Sync failures are transient toasts

`clipboardStore.ts:117` and `+page.svelte:308` surface errors as notifications
that disappear. There is no persistent "last sync failed — Retry" state in the
status bar, so a user who looks away never learns sync is broken.

### UX-05 — Settings has no "Test connection"

`SettingsDialog.svelte` validates URL *syntax* (`:152`) but never checks that the
server is reachable or that the password is accepted. Saving fires a background
`syncNow()` whose result is deliberately discarded (`:77`). The single most
common setup failure — wrong password — is invisible until the user goes hunting
in Sync Details.

### UX-06 — Empty states are generic

`EntryList.svelte:78` — `emptyText="No clipboard entries"` regardless of whether
the history is genuinely empty, the filter matched nothing, starred-only is on, or
loading failed. First-run gets no onboarding at all.

### UX-07 — Android cannot do anything useful with images or files

`commands.rs:190-196` returns hard errors: *"Copying images is not supported on
Android yet"*, *"Copying files is not supported on Android yet"*. The entries
sync down, occupy storage, and render as thumbnails — and then refuse every
action. There is no Share, Save, or Open. An Android `FileProvider` content-URI
path would make these entries real.

### UX-08 — macOS has no menu-bar presence and no launch-at-login

Linux gets a full tray with Show/Starred/Paste-plain/Autostart/Quit
(`lib.rs:734`). macOS gets nothing — `#[cfg(target_os = "linux")]` guards the
whole tray block. On macOS the app is a hotkey with no discoverable surface: no
menu bar item, no Dock reopen, no way to quit or reach Preferences without the
popup, and no launch at login.

### UX-09 — `Reset Sync Cursor` is offered to users as a repair tool, and it is a trap

`SettingsDialog.svelte:247` labels it "Mobile Sync Repair". It clears the
watermark (`sync.rs:169`) and forces a **full re-scan of the entire server
history** — the single most expensive operation available, offered as the fix for
slowness (§SYNC-A4). It also resurrects deleted entries (BUG-10).

---

## 6. Aesthetics: the "high-value app" question

> **Partly withdrawn.** The "high-value iOS app" brief this section answers was
> included in the original request by accident, and the maintainer has since
> ruled that varied/large type, multiple accents, and the absence of dark mode
> are intentional System 7 fidelity. **Items 1, 3, 4 and 7 below are withdrawn.**
> Items 2, 5, 6 and 8 do not depend on that premise and still stand. The
> section's own conclusion — *do not re-skin* — was and remains correct.

The user asked for aesthetics closer to a high-value iOS app than a mid Android
app. There is a real tension worth naming before acting on it.

**Copywraith's identity is deliberately System 7.** `@lkmc/system7-ui`, the
1-bit chrome, the pixel icons, the spooky name — that is the product's
personality and `ANALYSIS.md` correctly lists it as a strength to preserve.
Replacing it with iOS translucency and SF Pro would not make it a high-value app;
it would make it a generic one.

What actually distinguishes premium software from cheap software is **craft
consistency**, not a particular visual language. A meticulously executed System 7
app reads as expensive. The current build does not, for reasons that are all
fixable without touching the identity:

1. ~~**A real type scale.**~~ **(Withdrawn — the varied scale is intentional.)**
   Today: 9, 10, 11, 12, 13, 14, 16, 18, 22, 24 px, several with `!important`,
   several contradicting each other in the same component (VIS-01). Premium
   software uses 4–5 sizes with deliberate ratios. This is the single biggest
   change in perceived quality.
2. **A spacing grid.** Padding values in the popup alone: `2px 2px`, `2px 4px`,
   `3px 6px`, `4px 8px`, `5px 6px`, `6px 8px`, `8px`. Snap to a 4 px grid.
3. ~~**One accent, used consistently.**~~ **(Withdrawn — the several accents are
   intentional.)** Currently `#f5a623` for stars, `#ffd700` on hover,
   `#2f6d35`/`#e7f4e7` for online, `#b35a00`/`#fff3e6` for unreachable, `#c44`
   for sensitive, `#a01717` for field errors — six unrelated hues chosen ad hoc.
   Define tokens.
4. ~~**Deliberate density and rhythm.**~~ **(Withdrawn — rests on VIS-01.)** Rows
   should have one consistent height, and the type/time/actions columns should
   align to a shared baseline. Right now the 24 px preview forces the row taller
   than its own metadata.
5. **Motion with intent.** There is essentially none, except a 0.1 s opacity
   transition on the delete button. A 120–160 ms ease on selection change, row
   insertion, and dialog entry — honouring `prefers-reduced-motion` — is what
   makes an interface feel responsive rather than instant-and-jarring.
6. **Icons instead of glyph characters.** `★`, `☆`, `✕` render
   differently on every platform and at every weight. The admin UI already uses
   proper `@lkmc/system7-ui` icons (`server/ui/src/lib/EntryRow.svelte:2-12`);
   the popup should too.
7. ~~**Dark mode.**~~ **(Withdrawn — System 7 had no dark mode; its absence is
   intentional.)** Every colour in the popup is a hardcoded light-mode hex.
   `prefers-color-scheme` is not referenced anywhere in `src/`. On a phone in
   the evening this is the most obviously "not a premium app" signal there is.
8. **First-run polish.** No onboarding, no empty-state illustration, no sense that
   anyone considered the moment the app is opened for the first time.

The recommendation is therefore: **do not re-skin.** The "systematise 1–3 first"
advice that followed is withdrawn with items 1 and 3; what is left worth doing is
2, 5, 6 and 8.

---

## 7. Missing features

Ordered roughly by value-to-effort.

- **FEAT-01 — Explicit Sync Now + pull-to-refresh on mobile.** (See UX-03.)
- **FEAT-02 — Tombstones.** Deletes must propagate. Everything else about
  multi-device trust depends on this. (BUG-10.)
- **FEAT-03 — Retention policy.** No age/count/byte limits anywhere. The DB and
  blob directory grow without bound on every device, forever. Starred entries
  should be excluded.
- **FEAT-04 — Storage visibility.** Nothing shows DB size, blob-directory size, or
  entry count on any client. The server exposes `entries_count` on `/api/health`
  only when authorised (`api.rs:299`) and the admin UI does not display it.
- **FEAT-05 — Transform before paste.** Trim, to-plaintext, upper/lower/title case,
  JSON pretty/minify, URL/base64 encode/decode, shell-quote, line dedupe, join
  lines, Markdown-link from URL+title. Cheap to implement, enormously useful, and
  it is the feature that makes a clipboard manager sticky.
- **FEAT-06 — Pinned snippets with aliases.** Starred entries are already
  first-class; giving them a name and optional expansion trigger turns the app
  into a text-expander for free.
- **FEAT-07 — Quick-paste by number.** `Cmd+Shift+V` then `1`–`9` pastes the Nth
  entry without ever looking at the list.
- **FEAT-08 — Type and source filters.** The admin UI has a content-type dropdown
  (`App.svelte:52`); the client, where it matters most, has only starred-only.
- **FEAT-09 — Encrypted export/import.** No backup path exists. Losing
  `auth.json` or the password makes server data unrecoverable and nothing warns
  the user.
- **FEAT-10 — Per-device tokens.** The master encryption password doubles as the
  API bearer token (`api.rs:730`) and is stored in plain SQLite on every client
  (`storage.rs:542`) — not the macOS Keychain, not the Android Keystore.
  Revoking one device means changing the password everywhere.
- **FEAT-11 — Pause / incognito.** No way to temporarily stop capture, and no
  per-app exclusion list, despite `source_app` already being tracked.
- **FEAT-12 — macOS menu-bar item and launch-at-login.** (UX-08.)
- **FEAT-13 — Android share-out.** FileProvider content URIs for Open/Save/Share
  on image and file entries. (UX-07.)
- **FEAT-14 — Rich preview tabs.** `EntryPreview` shows only plain text
  (`:96-99`), discarding the HTML/RTF the app went to real trouble to preserve.
  Plain / Rich / Source tabs would surface it.
- **FEAT-15 — Server UI type checking in CI.** `server/ui/package.json` has no
  `check` script and `.github/workflows/ci.yml` only builds it. The popup
  frontend is type-checked; the admin UI is not.

---

## 8. Novel, delightful, and quirky ideas

The spooky identity is under-exploited. These are cheap and give the app a
personality no competitor has.

- **The Séance Log.** A scrollable sync history where each event has a playful
  name — *"Summoned 14 spirits from the local plane"*, *"The VPN plane is
  silent"*, *"A spirit refused to manifest (blob 404)"* — with the plain
  diagnostic underneath every line. Real observability wearing a costume; it
  solves UX-04 and is genuinely fun.
- **Bound spirits.** Starred entries never fade and carry a tiny chain glyph.
  Unstarred entries very subtly desaturate with age, so the list has visible depth
  and recency reads at a glance without a timestamp.
- **The Graveyard.** Deleted entries go to a drawer for 24 h with a headstone row
  and one-tap resurrect, then are purged. Solves UX-02 with charm instead of a
  modal.
- **The Ouija board.** A connection diagnostic that spells out its answer letter
  by letter as it walks the checks: DNS → TCP → TLS → auth → metadata → blob.
  Every step is a real assertion with a real error; the presentation is the joke.
  Solves UX-05.
- **Possession badges.** `source_app` is already captured but never shown in the
  popup. A tiny app glyph per row — "possessed by Safari" — is real information
  and on-theme.
- **The midnight ritual.** Retention cleanup (FEAT-03) presented as a nightly
  ritual with an exact preview: *"At midnight, 412 spirits older than 30 days
  will be released. 18 bound spirits will remain."* Makes a scary destructive
  feature feel safe and considered.
- **Ectoplasm tabs.** The Plain / Rich / Source / Image / File tabs of FEAT-14,
  named for the flavours they reveal.
- **OTP sense.** `sensitive.rs` already detects secret shapes. Detect 6–8 digit
  one-time codes specifically, offer a digits-only copy, and auto-expire the entry
  after 5 minutes — genuinely useful, and thematically perfect for something that
  vanishes.
- **The mascot.** One small dithered ghost, shown only in four states: true
  first-run, paused, offline, and empty history. Reserved appearances make a
  mascot feel like craft; ubiquity makes it feel cheap.
- **Ghost trail.** Recent search chips under the filter field that fade as they
  age out.
- **Reduced-motion honesty.** Whatever motion is added, gate it on
  `prefers-reduced-motion` from day one — including the mascot.

---

## 9. Security and operations

Mostly confirming that previously-identified issues remain live; not re-derived
at length here.

- **SEC-01** — Unauthenticated first-run setup (`api.rs:168`) combined with
  `CorsLayer::allow_origin(Any)` (`main.rs:89`) and a LAN-bound Docker service
  lets any reachable client claim an uninitialised server. Needs a loopback-only
  or bootstrap-token setup path.
- **SEC-02** — CORS is fully permissive with `allow_headers(Any)`, on a service
  that authenticates with a bearer header.
- **SEC-03** — Plain HTTP is the documented default; the master password crosses
  the network on every request.
- **SEC-04** — Prefix-based ciphertext detection: user plaintext starting with
  `ENC:1:` (`crypto.rs:292`) or `ENCB` (`crypto.rs:334`) is silently treated as
  ciphertext and passed through unencrypted. Encryption state belongs in schema
  metadata, not in the payload.
- **SEC-05** — `auth_setup` (`api.rs:193`) publishes `auth.json` *before*
  `migrate_existing_data` completes, and `encrypt_all_blobs` (`storage.rs:853`)
  rewrites blobs in place. A crash mid-migration leaves mixed state with no resume.
- **SEC-06** — Blob writes are not atomic on either side
  (`server/src/storage.rs:432`, `src-tauri/src/storage.rs:245`): final path
  written directly, and existence alone is treated as validity.
- **SEC-07** — The server trusts the client's `content_hash` verbatim
  (`api.rs:359`) and never validates payload/content-type consistency.
- **SEC-08** — The Shizuku helper is handed the server URL and API key
  (`lib.rs:154`) and uploads clipboard text **directly to the server over plain
  HTTP from a privileged process** (`ShizukuClipboardService.kt:151`), bypassing
  local storage entirely. If the upload fails, the capture is lost — there is no
  local durability and no retry beyond `endpoints.any { … }`.
- **OPS-01** — The Docker image runs as **root** (`server/Dockerfile`), with no
  `HEALTHCHECK`, no `USER`, no dropped capabilities.
- **OPS-02** — Swagger UI loads from `unpkg.com` at runtime (`main.rs:38, 42`) —
  an external CDN dependency on an app documented as air-gapped/VPN-only.
- **OPS-03** — `tauri-nspanel` is a **git dependency on a branch**
  (`src-tauri/Cargo.toml`), not pinned to a SHA. The branch can move under the
  build at any time.
- **OPS-04** — There is still **no automated test of the sync protocol at all**.
  Everything in §1 and BUG-01 would have been caught by a mocked-server
  integration test. This remains the highest-leverage missing engineering work in
  the repo.
- **OPS-05** — No `SECURITY.md`, no changelog, no private vulnerability reporting.

---

## 10. What is genuinely good

Worth recording so it survives refactoring:

- The `copywraith-core` / server / Tauri / Android split is clean, and the shared
  crate is the right shared crate.
- The multi-flavour clipboard model with legacy-compatible hashing
  (`models.rs:149`) is thoughtfully done, including the deliberate preservation of
  single-flavour legacy hashes for migration stability.
- `strip_rtf` (`content.rs:191`) is unusually complete for a hand-rolled parser:
  `\uc` fallback skipping, surrogate-pair recombination across escapes, CP1252
  hex escapes, and saturating depth to survive unbalanced braces — with tests for
  each.
- `sensitive.rs` is well-built: `LazyLock`-compiled patterns, Luhn validation,
  SSN range exclusions.
- The crypto design is sound: Argon2id → HKDF domain separation → random DEK →
  rewrap on password change. The deliberate choice to make wrong passwords pay the
  Argon2 cost even when unlocked (`crypto.rs:150-160`) shows real care.
- The macOS NSPanel work (`lib.rs:623`) — main-thread dispatch, `catch_unwind`
  around the conversion, collection-behaviour verification, retry on failure — is
  the most carefully defensive code in the repository.
- Parameterised SQL and hash-validated blob paths throughout; `is_valid_hash`
  before every path join.
- Request-ID guards against out-of-order list responses in both frontends.

---

## 11. Implementation plan

Items I have high confidence in, scoped as independent low-conflict branches.
All of these were opened as PRs #88–#92, #94 and #95; **all are still open and
none has merged**, so treat the table as proposed work, not delivered work.

| # | Branch | Scope | Findings |
|---|---|---|---|
| 1 | `claude/sync-throughput-…` | SQLite pragmas (`synchronous=NORMAL`, `busy_timeout`), single-transaction remote ingest, hoist settings out of the push loop | SYNC-A1, SYNC-A2 |
| 2 | `claude/entry-projection-…` | Compute preview + plain text once; bound list `full_text`; on-demand full text for the preview dialog | PERF-01, PERF-02 |
| 3 | `claude/list-image-loading-…` | Viewport-gated, cancellable image loading; correct MIME in data URLs | PERF-03, BUG-07 |
| 4 | `claude/viewport-typography-…` | `viewport-fit=cover`; ~~normalise the type scale~~ (withdrawn); reconcile column widths | BUG-05, ~~VIS-01~~, VIS-02 |
| 5 | `claude/interaction-fixes-…` | Fix double-click double-paste; make Sync Details passive + add explicit Sync Now | BUG-02, BUG-04, UX-03 |
| 6 | `claude/admin-ui-fixes-…` | Replace the incorrect RTF regex; stop re-fetching blobs on every reload | BUG-03, BUG-06 |
| 7 | `claude/live-timestamps-…` | Shared relative-time clock so rows age correctly | BUG-11 |

Deliberately **not** implemented without a product decision:

- **UX-01** (click-to-select instead of click-to-paste) — changes established
  muscle memory; needs an explicit call.
- **BUG-01** (preserve remote identity/chronology) — needs a schema change and
  a remote-upsert path; should land together with OPS-04's sync tests.
- **FEAT-02** (tombstones) — protocol change across three clients.
- **SEC-01/02/03** — deployment-model decisions, not code decisions.
- Any re-skin. See §6.
