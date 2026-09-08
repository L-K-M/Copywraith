# AGENTS.md

This file is a fast context handoff for future agent runs in this repository.

## What this project is

Copywraith is a local-first clipboard manager with:

- Desktop client: Tauri v2 + Svelte 5 (popup UI)
- Server: Rust + Axum + SQLite + blob storage
- Shared crate: `crates/copywraith-core` for models, API types, hashing/content utils

Desktop captures clipboard changes, stores locally, and syncs with server.

- a Tauri desktop client (Svelte 5 + Rust backend), and
- a Rust/Axum server for durable, searchable clipboard history.

The desktop app watches your clipboard, stores entries in a local SQLite cache, and can optionally sync those entries to the server.

## Current status

Implemented today:

- Clipboard capture for `text`, `html`, `rtf`, `image`, and `file`
- Content-hash deduplication (SHA-256)
- Floating popup UI with retro System 7 styling
- Star/unstar, delete, search/filter
- Global shortcuts for opening popup and quick plaintext paste
- Two-way background sync (push local unsynced + pull remote updates)
- Server REST API + Svelte admin web UI
- Password protection with at-rest encryption (Argon2id + AES-256-GCM)
- Android/mobile client support via Tauri mobile entry point, mobile clipboard plugin, platform-aware UI, and `capture_clipboard` on open/resume

Planned later:

- Android/mobile production hardening and device testing

## Architecture at a glance

1. **Clipboard change happens on desktop**
2. The app-owned `NativeClipboard` watcher invokes the Rust capture callback
3. Backend reads clipboard content (files/image/html/rtf/text in priority order)
4. Entry is normalized and deduplicated by hash
5. Entry is stored in local SQLite + blob store
6. UI receives `clipboard-updated` event and refreshes list
7. Background sync loop pushes unsynced entries and pulls entries from other devices

## Repository layout

```text
.
├── crates/copywraith-core/   # Shared models, API types, hashing/content helpers
├── server/                   # Axum API + SQLite/blob persistence + Svelte admin UI
├── scripts/                  # Android bootstrap/env helpers, server redeploy/version sync
├── src-tauri/                # Tauri Rust backend (monitoring, commands, sync, shortcuts)
├── src/                      # Svelte popup frontend
├── ARCHITECTURE.md
├── IMPLEMENTATION.md
└── ENCRYPTION.md
```

## Current architecture (important)

### Clipboard monitoring is Rust-owned (not JS-owned)

Source: `src-tauri/src/clipboard.rs`

- `native_clipboard.rs` owns the clipboard-rs 0.3 context, watcher and locks
- Registers the capture callback before starting the watcher; stops/joins on exit
- Reads typed image/file/flavor payloads; paste writes through the same adapter
- Priority order: `Image > File > Text/HTML/RTF bundle`

Do not re-introduce frontend `startListening()` dependency unless intentionally redesigning.

### Deduplication

- Dedup key is content hash (`content_hash`, SHA-256)
- Client local DB and server DB both deduplicate by unique index on `content_hash`

### Two-way sync (running desktop app)

Sources: `src-tauri/src/lib.rs`, `src-tauri/src/sync.rs`

- Background loop runs every ~5s
- Pushes local unsynced entries to server (`sync_unsynced_entries`)
- Pulls new server entries into local storage (`pull_new_entries`)
- Uses a persisted `(updated_at, id)` watermark to only ingest entries newer than the last pull; comparing the full key (not just an id) keeps the cursor stable when an entry's `updated_at` changes (re-copy/re-star), so newer entries are never skipped
- Emits `clipboard-updated` event when pull imports new entries so UI refreshes
- Sync settings live in local SQLite (`server_url_primary`, `server_url_fallback`, `api_key`) and are edited in `src/lib/components/SettingsDialog.svelte`; sync tries the primary URL first, then falls back to the secondary URL

### Server admin UI

Source: `server/ui/` — a plain Svelte + Vite SPA (not SvelteKit).

- Built output goes to `server/ui/dist/`
- Served by `server/src/main.rs` at `/` via `tower_http::services::ServeDir`
- Uses `@lkmc/system7-ui` components (DataTable, TitleBar, Button, etc.)
- Build with: `cd server/ui && npm run build`
- If UI not built, server shows a fallback HTML page with build instructions

### Paste simulation

Source: `src-tauri/src/paste.rs`

- macOS: simulates Cmd+V via `osascript` in a **spawned thread** (must not run synchronously on the Tauri async runtime — doing so blocks the IPC response and races with the async popup hide / focus restoration)
- non-macOS: warns that simulated paste is not implemented
- `simulate_paste` runs on a background thread so the `paste_entry` Tauri command returns immediately after hiding the popup; the thread sleeps 100ms for the hide to complete, then runs osascript to activate the target app and send Cmd+V
- Do **not** change `simulate_paste` to run synchronously (inline) — this was the cause of a regression where the paste keystroke arrived before the previous app had been re-activated

### Global shortcuts

Source: `src-tauri/src/lib.rs`

Default shortcuts (configurable via Settings dialog):
- `CmdOrCtrl+Shift+V`: toggle popup
- `CmdOrCtrl+Shift+B`: popup with starred filter on
- `CmdOrCtrl+Shift+Alt+V`: paste most recent as plaintext

Settings are persisted in local SQLite and shortcuts are re-registered on app start or when settings change on desktop; mobile hides the shortcut fields in `src/lib/components/SettingsDialog.svelte` and skips shortcut re-registration.

### Tauri capability naming gotcha

Desktop clipboard access is Rust-only and needs no plugin capabilities; mobile uses `clipboard-manager:*` permissions in `src-tauri/capabilities/mobile.json`.

## Key files to read first

- `README.md`
- `rust-toolchain.toml`
- `ARCHITECTURE.md`
- `IMPLEMENTATION.md`
- `ENCRYPTION.md`
- `SENSITIVE.md`
- `src/lib/util/platform.ts`
- `src/lib/components/SettingsDialog.svelte`
- `src-tauri/src/clipboard.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/storage.rs`
- `src-tauri/src/sync.rs`
- `src-tauri/capabilities/mobile.json`
- `scripts/android-dev-bootstrap.sh`
- `scripts/android-env-persist.sh`
- `server/src/main.rs`
- `server/src/api.rs`
- `server/src/crypto.rs`
- `server/src/storage.rs`

## Run and test commands

From repo root unless noted.

If `cargo` is missing in this environment, prepend:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Install JS deps:

```bash
npm install
```

Build checks:

```bash
cargo check --workspace
cargo test --workspace
npm run build
```

Run server:

```bash
cargo run -p copywraith-server
```

Run desktop app:

```bash
npm run tauri dev
```

Android mobile dev/build:

```bash
./scripts/android-dev-bootstrap.sh
npx tauri android dev
npx tauri android build
```

Server defaults:

- API: `http://localhost:3742/api`
- Admin UI: `http://localhost:3742/`

## Environment and dependency gotchas

- Rust toolchain is pinned at the repo root via `rust-toolchain.toml` (`1.98.0`).
- Keep the server Docker builder aligned with `rust-toolchain.toml` (1.98.0), CI and release builds. Revalidate dependency MSRVs before changing this pin; edition2024 support alone is insufficient.
- Server binds `127.0.0.1` by default; Docker deployments must set `COPYWRAITH_HOST=0.0.0.0` (set in compose + Dockerfile env) so published port `3742` is reachable.
- Compose supports explicit image tagging via `COPYWRAITH_SERVER_IMAGE_REPO` + `COPYWRAITH_SERVER_IMAGE_TAG`; `scripts/redeploy-server-docker.sh` defaults tag to the server crate version to reduce stale-image confusion.
- Do not expose the server publicly; it is intended for a local network or secure VPN and does not add rate limiting / brute-force protection.
- `@lkmc/system7-ui` is consumed as an npm dependency (not a local sibling path).
- Svelte/TSServer may show false "Cannot find package 'vite'" errors due to bun cache.
  - Confirm actual state with `npm run build`.
- Android builds need `ANDROID_HOME` and `NDK_HOME`; use `scripts/android-env-persist.sh` or `scripts/android-dev-bootstrap.sh` on macOS to set them up.

## Known gaps / pending improvements

- No full desktop end-to-end test automation with real OS clipboard/UI interaction.
- Android/mobile client is partially implemented (platform detection, mobile-specific UI, `capture_clipboard` command, helper scripts) but not yet production-tested.

### Password protection & encryption

Source: `server/src/crypto.rs`, `ENCRYPTION.md`

- Single-user, password-only auth (no login name).
- Password hashed with Argon2id (64 MiB, 3 iterations, 4 parallelism).
- Master key → HKDF splits into auth_key (verification) and KEK (key encryption).
- Random 256-bit DEK encrypted with KEK, stored in `{data_dir}/auth.json`.
- `text_content` encrypted via AES-256-GCM with `ENC:1:` prefix; blobs with `ENCB` header.
- Password change re-wraps the same DEK — no data re-encryption needed.
- If `auth.json` doesn't exist, server shows "Create Password" screen; data endpoints return 403 until setup.
- `COPYWRAITH_ADMIN_API_KEY` env var has been removed; use password auth instead.
- Desktop client sends password as `Authorization: Bearer <password>` (same header, same field).
- Web admin UI stores password in `sessionStorage`; shows setup/unlock screens as needed.
- Auth API: `GET /api/auth/status`, `POST /api/auth/setup`, `POST /api/auth/unlock`,
  `POST /api/auth/change-password`, `POST /api/auth/lock`.

## Editing guardrails for future runs

- Preserve SvelteKit static adapter + `ssr = false` for popup app.
- Keep `@lkmc/system7-ui` imports package-based (avoid reintroducing local file path coupling).
- Do not silently change clipboard event model without updating both Rust and UI docs.
- Do not change `simulate_paste` from a spawned thread to synchronous execution — it causes a paste regression (see `PASTE_PROBLEM.md`).
- Keep API behavior stable where possible (`/api/entries*`, `/api/health`).
- After larger changes, bump `server/Cargo.toml` patch version so deployments are easy to verify via `/api/health` (and the admin UI version badge).
- Keep compose default image tag (`COPYWRAITH_SERVER_IMAGE_TAG`) aligned with the current server crate version.
- After bumping `server/Cargo.toml` version, run `scripts/sync-version.sh --write` to update all hardcoded version references (compose files, README, .env.example, redeploy script). Run without `--write` to check for drift.


## Server API

Base URL: `/api`

- `GET /health`
- `GET /auth/status`
- `POST /auth/setup`
- `POST /auth/unlock`
- `POST /auth/change-password`
- `POST /auth/lock`
- `POST /entries`
- `GET /entries`
- `GET /entries/{id}`
- `PATCH /entries/{id}`
- `DELETE /entries/{id}`
- `GET /entries/{id}/blob`

Interactive docs: `/swagger-ui/` (requires internet for CDN assets)
OpenAPI JSON: `/api-docs/openapi.json`

Notes:

- Auth endpoints (`/auth/status`, `/auth/setup`, `/auth/unlock`) do not require a password
- All `/entries*` endpoints require `Authorization: Bearer <password>` when a password is configured
- `/health` is always open
- `GET /entries` supports pagination/filtering/search via query params
  - `limit`, `offset`, `content_type`, `starred_only`, `search`
- Deduplication is based on `content_hash`
- Binary payloads are stored on disk in a blob directory keyed by hash

## Data and persistence

- Desktop client keeps its own SQLite + blob cache in Tauri app data directory
- Server keeps SQLite + blobs under `COPYWRAITH_DATA_DIR` (default `./data`)
- Password auth config stored in `{data_dir}/auth.json`; encrypted entries use `ENC:1:` prefix
- Both desktop and server deduplicate by content hash

## Platform notes

- Desktop clipboard monitoring uses the private `NativeClipboard` adapter over clipboard-rs 0.3
- Paste simulation is currently implemented for macOS (via `osascript`)
- On non-macOS platforms, writing to clipboard works, but simulated keystroke paste is not fully implemented yet
- Mobile builds use `tauri-plugin-clipboard-manager`; tapping an entry copies it, and `capture_clipboard` persists the current clipboard when the app opens or resumes.

## Development notes

- Frontend dev server port is `1420` (Tauri expects this)
- SvelteKit is static-adapter based and runs client-side (`ssr = false`)

## Dependency release-age policy

- This repo now enforces npm package age gating with `min-release-age=3` in:
  - `.npmrc`
  - `server/ui/.npmrc`
- When install/update fails because a dependency is newer than 3 days, do not loop retries.
- Preferred handling order:
  1. wait for the age window to pass,
  2. pin to an older known-good version,
  3. temporarily override with `npm install --min-release-age=0` only for urgent fixes, then restore policy.

<!-- shared-rules:start -->

## Working practices

- Follow explicit task instructions over the default workflow below.
- Before editing, inspect the branch and working tree, fetch remote updates,
  and fast-forward where safe. Never overwrite existing work to update.
- Resolve ambiguity before making consequential changes. State low-risk
  assumptions; ask when scope, safety, or expected behavior is unclear.
- Keep changes focused. Do not modify unrelated code, formatting, or comments.
- Prefer surgical edits over whole-file rewrites when the result is equivalent.
- Stage only intended files. Inspect the diff before committing.

## Communication

- Be concise, factual, and direct. Preserve necessary context and uncertainty.
- Avoid praise, motivational filler, emojis, and em dashes in new prose.
- Address the reader directly in user-facing copy.
- Report what was verified and what remains unverified. Never imply that an
  unavailable check passed.

## Code design

- Prefer early returns and shallow nesting. Separate logical blocks with
  blank lines.
- Use descriptive constants or enums for meaningful or repeated values.
  Use existing standard definitions for protocol/specification constants.
  Keep obvious, one-off values inline.
- Use enums for behavioral modes that would otherwise require ambiguous
  boolean arguments.
- Default members to private. Widen visibility only for required consumers,
  and review the change as an API design decision.
- Follow the repository's declared dependency boundaries. UI and controllers
  must use application services rather than directly accessing databases,
  subprocesses, sockets, or other low-level mechanisms.
- Encapsulate low-level mechanics behind domain-oriented interfaces.
- Reuse genuinely shared logic. Avoid speculative abstractions and layers
  that only forward calls.
- Prefer pure functions for business rules and immutable data where practical.
  Isolate side effects; document non-obvious state ownership or synchronization.
- Explain non-obvious intent, constraints, and tradeoffs in comments.
  Do not narrate obvious code. Add examples or diagrams when they clarify it.

## Validation and errors

- Validate untrusted input at entry points. Where practical, represent valid
  states in types and enforce persistent invariants in database schemas.
- Represent absence and failure explicitly.
- Use assertions for internal programming invariants, not external-input
  validation or required runtime error handling.
- Prefer explicit, actionable errors over silent failure or undocumented
  fallback. Document intentional recovery behavior.
- Never report a skipped or failed operation as successful.

## Bug fixes

1. Identify the root cause and define an observable success criterion.
2. Add a regression test and observe the relevant failure before fixing it.
3. Implement the fix and observe the test passing.
4. Check surrounding behavior for regressions and architectural consistency.

If an automated regression test is impractical, document the reproduction
and verification procedure. State any inability to reproduce the failure.

## Verification

- Run relevant tests and lint after changes.
- Choose coverage by affected behavior and risk, not patch size.
- Use integration or end-to-end tests for critical workflows and boundaries;
  test isolated business rules at the lowest effective level.
- Run broader suites for cross-cutting or high-risk changes, and the full
  required release checks before releasing.
- Validate the requested command, options, platform, and configuration.
  Unrelated green CI is not proof that the reported problem is fixed.
- Recheck after the final edit. Distinguish local checks from CI results.

## Commit messages

- Use a capitalized, imperative subject without a final period.
- Target 50 characters; never exceed 72.
- Separate the subject and body with one blank line.
- Wrap body text at 72 characters.
- Explain what changed and why. Leave implementation mechanics to the code.

## Implementation and review

Unless explicitly instructed otherwise:

1. Work on a focused branch and open a PR against main.
2. Inspect CI results and completed review feedback for the latest commit.
   A successful reviewer job does not mean the review found no problems.
3. Address important findings or explain why they do not apply. Handle minor
   findings according to the stopping rules below.
4. Evaluate each fix in the surrounding project, add regression coverage,
   and rerun affected checks before pushing.
5. Repeat until a stopping criterion is met.
6. Merge without asking again once the stopping criterion is met, required
   checks pass on the latest commit, and no unresolved blockers or required
   human review requests remain.

### Automated review stopping rules

Judge findings by verified impact, not the reviewer's severity label.
Important findings concern correctness, security, data loss, broken builds,
or materially degraded behavior/performance.

Track completed review rounds and consecutive rounds without important
findings. Reruns of the same revision and integration failures do not count.

- No applicable actionable feedback: finish immediately.
- First minor-only round: optionally fix worthwhile, low-risk findings.
  Do not manufacture another push merely to obtain another review.
- Two consecutive rounds without important findings: stop responding to
  automated nitpicks, even if actionable minor suggestions remain.
  Defer worthwhile leftovers rather than continuing the cycle.
- A confirmed important finding resets the minor-only streak. Address it
  and verify the fix before continuing.

After ten completed rounds, enter stabilization:

- Stop optional cleanup, refactoring, and nitpick fixes.
- One completed review without confirmed important findings is sufficient
  to finish, even if minor suggestions remain.
- Continue only for confirmed important defects. If resolving them stalls,
  report the blockers rather than continuing indefinitely.

These limits end optional automated-feedback work. They do not waive
confirmed blockers, unresolved human review requests, or required checks.

### Reviewer integration failures

After two consecutive reviewer-integration failures, stop and report the
review gap. Do not treat failures as approval. An explicit user instruction
may waive review; report that waiver rather than claiming review passed.

## Completion checklist

- The requested behavior is implemented without unrelated changes.
- Relevant checks pass for the latest code.
- Important review findings are addressed or rejected with reasons.
- Deferred suggestions, remaining risks, and validation gaps are disclosed.
- The final response accurately states whether work is committed, pushed,
  and merged.

<!-- shared-rules:end -->
