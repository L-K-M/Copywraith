# Pasteboard Management

## Why this document exists

This is a handoff note for implementing proper clipboard flavor fidelity in Copywraith.

The key user expectation is correct: **Copywraith should preserve what was copied and restore it without mangling**.

The recent IntelliJ `&#32;` symptom is a concrete example of format loss caused by the current architecture.

## TL;DR

- Current architecture stores one flavor per entry (`content_type` + `text_content` or `blob_hash`).
- Real clipboards often carry multiple text flavors at once (`text/plain`, `text/html`, `text/rtf`).
- Copywraith currently collapses these into one payload, and normal paste often writes text only.
- This causes fidelity loss (formatting loss, entity artifacts, and behavior differences across target apps).
- Correct fix: store and restore multiple flavors together for each entry.

## What was found

### 1) Root cause of IntelliJ mangling

- IntelliJ copy commonly exposes both plain text and HTML flavors.
- HTML can contain entity-encoded content (for example `&#32;`).
- If Copywraith stores/uses HTML then converts to plain text later, fidelity depends on HTML decoding behavior.
- That is exactly why users saw `Popup&#32;position...` in preview/paste.

### 2) Current capture behavior is lossy by design

Current capture path (`src-tauri/src/clipboard.rs`) picks one representation and discards others.

- Today: `Image > File > Text > HTML > RTF` (temporary mitigation for IntelliJ text fidelity).
- Historically: `Image > File > HTML > RTF > Text` (which made entity problems more likely for text-first workflows).

Even with the text-first mitigation, this is still lossy for rich text workflows.

### 3) Current paste behavior is also lossy

`src-tauri/src/commands.rs` + `src-tauri/src/paste.rs`:

- `paste_entry` writes either image bytes or text.
- HTML entries are converted to plain text before paste.
- RTF is not restored as RTF.
- So "normal paste" does not preserve the original flavor bundle.

### 4) Data model currently cannot represent flavor bundles

`crates/copywraith-core/src/models.rs`:

- `ClipboardEntry` has a single `content_type`.
- `text_content` is one string payload.
- `blob_hash` is one binary payload path.

There is no field set for parallel text flavors.

### 5) Local and server storage mirror the same single-flavor model

- Local DB (`src-tauri/src/storage.rs`) has `content_type`, `text_content`, `blob_hash`, `blob_size`.
- Server DB (`server/src/storage.rs`) has the same shape.
- Sync API (`crates/copywraith-core/src/api_types.rs`) sends one `content_type` + one `text_content` + optional blob.

### 6) Plugin capability is better than current app usage

From `tauri-plugin-clipboard` and `clipboard-rs`:

- Read APIs exist for text/html/rtf/image/files.
- Write APIs include `write_text`, `write_html`, `write_html_and_text`, `write_rtf`, image/files.
- Underlying `clipboard-rs` supports setting multiple flavors in one operation (`set(vec![Text, Rtf, Html, ...])`).

So preserving/restoring a multi-flavor bundle is technically feasible.

### 7) Temporary mitigations already applied

- HTML entity decoding in `copywraith-core::content::strip_html` was improved (named + numeric entities).
- Capture now prefers plain text when plain text and HTML are both present.

These are useful stopgaps, not the final architecture.

## Correct target behavior

For a text/rich-text clipboard item, Copywraith should:

1. Capture all available standard text flavors (`text/plain`, `text/html`, `text/rtf`) together.
2. Persist them together as one logical entry.
3. On normal paste, restore as many flavors as possible in one write operation.
4. On "Paste as plaintext", force plain text only.
5. Avoid destructive transformations on capture (no stripping/sanitizing as storage format).

For image/files, preserve existing behavior and extend only where practical.

## Proposed refactor plan

### Phase 0 - Define model and invariants

Add explicit flavor bundle semantics in core.

Proposed shape (example):

```rust
pub struct ClipboardFlavors {
    pub text_plain: Option<String>,
    pub text_html: Option<String>,
    pub text_rtf: Option<String>,
    pub file_list: Option<Vec<String>>,
}
```

Entry keeps a primary display type (`content_type`) for UI icon/sorting, but payload is bundle-based.

Key invariant: no flavor is generated on capture except optional derived search/preview text.

### Phase 1 - Storage schema upgrades (local + server)

Add columns for parallel text flavors while keeping backward compatibility.

Suggested columns:

- `text_plain`
- `text_html`
- `text_rtf`
- `file_list_json` (if needed)
- keep `blob_hash` / `blob_size`
- keep `content_type` for UI compatibility
- optional `search_text` (derived plaintext for filtering/indexing)

Migration strategy:

- Backfill from legacy rows:
  - `content_type=text` => `text_plain = text_content`
  - `content_type=html` => `text_html = text_content`
  - `content_type=rtf` => `text_rtf = text_content`
  - `content_type=file` => convert existing newline text list to JSON list if adopting `file_list_json`
- Keep legacy `text_content` readable during transition.

### Phase 2 - Capture all flavors

Update `src-tauri/src/clipboard.rs`:

- Read all available formats in one pass.
- Build one flavor bundle.
- Store bundle as a single entry.
- Keep source app attribution logic as-is.

Do not "choose one text flavor" at capture time.

### Phase 3 - Paste flavor bundles correctly

Update paste path (`src-tauri/src/commands.rs`, `src-tauri/src/paste.rs`):

- Normal paste:
  - image -> write image + simulate paste
  - text bundle -> write multiple text flavors together when possible
- Plaintext paste:
  - write only plain text (`text_plain`, fallback strip html/rtf if needed)

Important implementation note:

- For best fidelity, text/html/rtf should be set in one clipboard transaction.
- `write_html_and_text` exists now; RTF may need a new helper that writes all at once.
- If needed, add a small wrapper using `clipboard-rs` multi-content `set(...)`.

### Phase 4 - Sync/API compatibility

Extend API types in a backward-compatible way.

Suggested API evolution:

- Add optional `flavors` object to `CreateEntryRequest` and response types.
- Keep legacy fields (`content_type`, `text_content`, `blob_base64`) during migration.
- New clients send both (legacy + new) until server rollout is complete.
- Old servers ignore unknown `flavors`; new servers prefer `flavors`.

### Phase 5 - Search, preview, and sensitivity

Search and preview should use derived plain text, not mutate stored source formats.

- `preview`: `text_plain` if present, else derived from html/rtf.
- sensitivity detection: run on best plain text representation.
- DB filtering/FTS: index/search a derived plain-text field.

### Phase 6 - Cleanup

After migration confidence:

- remove single-flavor assumptions from code paths.
- deprecate legacy `text_content` usage where safe.
- keep compatibility adapters only at API boundaries if needed.

## Dedupe strategy decision (must decide before implementation)

This is the biggest design choice.

Options:

1. **Semantic dedupe** (plain-text based):
   - Pros: avoids many near-duplicate entries.
   - Cons: loses distinction between formatting variants.

2. **Exact payload dedupe** (hash all flavors + blob):
   - Pros: true fidelity semantics.
   - Cons: more duplicates in history.

3. **Hybrid**:
   - Store both `semantic_hash` and `payload_hash`.
   - Use one for dedupe, one for diagnostics/sync conflict handling.

Recommended starting point: **Hybrid**, with dedupe behavior configurable later.

## Concrete file-level worklist

Core/shared:

- `crates/copywraith-core/src/models.rs` (entry/flavor structs, preview behavior)
- `crates/copywraith-core/src/api_types.rs` (request/response flavor payload)

Desktop client:

- `src-tauri/src/clipboard.rs` (capture all flavors)
- `src-tauri/src/storage.rs` (schema + read/write migrations)
- `src-tauri/src/commands.rs` (paste + plaintext-paste semantics)
- `src-tauri/src/paste.rs` (clipboard write helpers for multi-flavor)
- `src-tauri/src/sync.rs` (serialize/deserialize flavor bundles)

Server:

- `server/src/api.rs` (accept/return flavor payload)
- `server/src/storage.rs` (schema + persistence + list/get behavior)

Frontend:

- `src/lib/types.ts` (if response schema changes)
- `src/lib/components/*` (preview source if needed)

Docs:

- `API.md`, `ARCHITECTURE.md`, and this file

## Testing plan

Automated:

- Unit tests for model conversion and dedupe hash generation.
- Storage migration tests (legacy row -> new columns).
- Sync round-trip tests (client -> server -> client) with multi-flavor entries.
- Paste command tests for flavor selection logic.

Manual matrix (macOS first, then Windows/Linux):

Sources:

- IntelliJ (plain text + HTML)
- Browser rich text
- Word/Pages/Notes style-rich copy

Targets:

- Plain text editor
- Rich text editor
- Browser input/contenteditable

Scenarios:

- normal paste preserves rich formatting where target supports it
- plaintext paste removes formatting
- preview is readable and not entity-mangled
- sync to another device preserves behavior

## Risks and gotchas

- Writing formats in multiple separate clipboard calls can overwrite previous flavors; prefer one multi-format write operation.
- Search/FTS currently assumes one `text_content` field.
- Encryption path on server currently encrypts one `text_content`; all text flavor columns must follow same policy.
- Large HTML/RTF payloads can increase DB and sync size; consider size caps/telemetry.

## Notes for next implementation session

- The immediate user-facing bug is a symptom of lossy single-flavor storage, not just bad entity decoding.
- Keep the current temporary mitigation (prefer plain text over HTML when both exist) until full multi-flavor support lands.
- Do not regress paste timing behavior in `simulate_paste` (it must stay async-threaded on macOS).
- Existing environment note: `npm run check` can fail in this workspace due to missing Node typings; `npm run build`, `cargo check`, and Rust tests are the primary reliable checks here.

## Definition of done for this refactor

- Copy from IntelliJ and paste via Copywraith produces the same text as direct paste.
- Rich text copied from browser/word processors pastes with formatting into rich targets.
- Plaintext paste always outputs plain text only.
- Sync round-trips preserve available flavors.
- No lossy flavor collapse in capture path.
