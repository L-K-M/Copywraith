# Sensitive Data Detection

## Problem

Copywraith captures everything copied to the clipboard. This inevitably includes
sensitive information such as passwords, credit card numbers, social security
numbers, API keys, and private keys. Displaying this content in plaintext in the
UI (both the desktop popup and the server admin panel) creates a security risk --
anyone glancing at the screen or taking a screenshot can read secrets.

## Goals

1. **Detect** clipboard entries that contain sensitive data at capture time.
2. **Tag** them with a `sensitive` flag stored in the database.
3. **Censor** tagged entries in every UI surface (desktop popup list, preview
   dialog, server admin table, server detail dialog).
4. **Preserve** the actual data: entries are stored and pasted without
   modification. Only the *display* is censored.

## Approach

### Detection strategy: local regex-based heuristic scanning

We use a set of regular expressions applied to text content at capture time.
This runs entirely locally with no network calls or external dependencies.
Detection is best-effort -- it prioritizes avoiding false negatives (missing
real secrets) over false positives (flagging benign text), since the cost of a
false positive is merely a cosmetic mask that the user can mentally dismiss,
while the cost of a false negative is an exposed secret.

### Categories detected

| Category | Detection method |
|---|---|
| **Credit card numbers** | 13-19 digit sequences (with optional separators) passing the Luhn checksum |
| **Social Security Numbers** | `NNN-NN-NNNN` pattern with area/group validity checks |
| **Passwords in assignments** | Patterns like `password=`, `passwd:`, `secret=`, `token=`, etc. in config/env-file style text |
| **API keys / tokens** | Common vendor prefixes (`sk-`, `pk_live_`, `ghp_`, `AKIA`, `xoxb-`, `glpat-`, etc.) |
| **Private keys (PEM)** | `-----BEGIN (RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY-----` markers |
| **JWT tokens** | `eyJ` base64-header pattern with two dots (three-part structure) |
| **AWS access keys** | 20-char uppercase alphanumeric starting with `AKIA` |
| **Generic high-entropy strings** | Long hex/base64 strings assigned to secret-like variable names |

### Where detection runs

Detection is performed in `copywraith-core` so both client and server share the
same logic:

- **Desktop client** (`clipboard.rs`): scans text content immediately after
  capture, before storing. The `sensitive` flag is written into the local
  database row and propagated to the server during sync.
- **Server** (`storage.rs`): scans text content on `POST /api/entries` for
  entries arriving via the sync API. This ensures that even entries created
  before detection was implemented, or entries arriving from older clients,
  get scanned.

### Data model changes

- `ClipboardEntry` struct: add `sensitive: bool` field (default `false`).
- Client SQLite schema: add `sensitive INTEGER DEFAULT 0` column.
- Server SQLite schema: add `sensitive INTEGER DEFAULT 0` column.
- `EntryForFrontend` (desktop): add `sensitive: bool` field.
- `EntryResponse` (API): inherits `sensitive` via `#[serde(flatten)]` on
  `ClipboardEntry`.
- `CreateEntryRequest`: no change needed -- server runs its own detection.

### UI censoring rules

When `sensitive` is true:

- **Preview text**: replaced with `[Sensitive content hidden]`.
- **Full text / detail view**: replaced with the same placeholder. A small
  label like "(sensitive)" appears in the metadata section.
- **Images**: not affected (image content is not scanned for text).
- **Paste behavior**: unchanged -- the real content is pasted.
- **Starred/delete/other actions**: unchanged.

### Migration

Both client and server use `ALTER TABLE ... ADD COLUMN` with a default of 0,
which is safe for SQLite and backward-compatible. Existing entries remain
`sensitive = 0` until they are re-scanned or until the user manually flags
them (future feature).

### Performance

Regex compilation uses `once_cell::sync::Lazy` (or `std::sync::LazyLock` on
recent Rust) to compile patterns once. Scanning a typical clipboard entry
(< 10 KB of text) takes microseconds. There is no measurable impact on the
clipboard capture hot path.

### Future improvements

- User toggle to reveal sensitive content temporarily in the UI.
- User ability to manually mark/unmark entries as sensitive.
- Configurable pattern list.
- Scanning HTML/RTF text content after stripping markup.
