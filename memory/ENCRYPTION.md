# Encryption & Password Protection

Single-user, password-only protection with at-rest encryption for clipboard data.

## Threat model

**Protects against:**

- Unauthorized access to the server API (requires password)
- Data exposure if the server filesystem is compromised (entries and blobs encrypted at rest)
- Data exposure if the server is powered off or restarted (DEK not in memory)

**Does not protect against:**

- An attacker with access to the running server process memory
- Network eavesdropping without HTTPS (use a reverse proxy with TLS in production)
- Client-side compromise (desktop client stores password locally)

## Architecture

### Key hierarchy

```
Password (user-memorized)
    |
    v
Argon2id(password, salt) --> master_key (32 bytes)
    |
    +-- HKDF-SHA256(master_key, info="copywraith-auth") --> auth_key (32 bytes)
    |       Used to verify password. Stored in auth.json.
    |
    +-- HKDF-SHA256(master_key, info="copywraith-kek") --> KEK (32 bytes)
            Used to encrypt/decrypt the DEK. Never stored.
                |
                v
            AES-256-GCM(KEK, nonce) encrypts DEK
                |
                v
            DEK (Data Encryption Key, 32 random bytes)
                Encrypts all clipboard entry content.
                Stored encrypted in auth.json.
```

One Argon2id computation derives the master key. HKDF cheaply splits it into
an authentication key and a key-encryption key, keeping the two concerns
separated without doubling the cost.

### Why a separate DEK?

The Data Encryption Key is a random 256-bit key that actually encrypts clipboard
data. It is itself encrypted by the password-derived KEK. This indirection means:

- **Password change is instant** -- re-encrypt only the 32-byte DEK, not every entry
- **Key material is high-entropy** -- the DEK is random, not derived from a potentially weak password
- **Clean separation** -- authentication and encryption use independent keys

### Argon2id parameters

| Parameter   | Value  | Rationale                                      |
|-------------|--------|------------------------------------------------|
| Memory      | 64 MiB | Resists GPU/ASIC attacks; reasonable for a CLI  |
| Iterations  | 3      | OWASP recommended minimum for 64 MiB           |
| Parallelism | 4      | Matches typical core count                      |
| Output      | 32 B   | 256-bit master key                              |
| Salt        | 16 B   | Random, stored in auth.json                     |

Unlock takes ~0.5-1s on modern hardware. Subsequent requests use a cached
SHA-256 fast-check of the password against the in-memory session, so only the
first request after a server restart is slow.

## Data format

### auth.json

Stored in `{data_dir}/auth.json` alongside the SQLite database.

```json
{
  "version": 1,
  "argon2_salt": "<base64, 16 bytes>",
  "auth_key": "<base64, 32 bytes>",
  "encrypted_dek": "<base64, 32 bytes ciphertext + 16 bytes tag>",
  "dek_nonce": "<base64, 12 bytes>"
}
```

### Encrypted text content

Entries with encrypted `text_content` use a self-describing format:

```
ENC:1:<base64( nonce[12] || ciphertext || tag[16] )>
```

The `ENC:1:` prefix allows the server to distinguish encrypted from plaintext
entries (used during migration of pre-encryption data).

### Encrypted blobs

Blob files on disk use the same scheme: the file contents are
`nonce[12] || ciphertext || tag[16]`. A 4-byte magic header `ENCB` precedes the
nonce to distinguish encrypted blobs from raw files.

## API endpoints

### Auth endpoints (no password required)

| Method | Path                      | Purpose                              |
|--------|---------------------------|--------------------------------------|
| GET    | `/api/auth/status`        | `{ initialized, unlocked }`          |
| POST   | `/api/auth/setup`         | Create password (only if !initialized)|
| POST   | `/api/auth/unlock`        | Unlock server with password          |

### Auth endpoints (password required)

| Method | Path                      | Purpose                              |
|--------|---------------------------|--------------------------------------|
| POST   | `/api/auth/change-password`| Change password (old + new required)|
| POST   | `/api/auth/lock`          | Clear DEK from memory                |

### Data endpoints (password required via `Authorization: Bearer <password>`)

All existing `/api/entries*` endpoints require the password as a Bearer token.
`/api/health` remains open (no auth).

## Password verification flow

### First request after server start (slow path)

1. Extract password from `Authorization: Bearer <password>` header
2. Load `auth.json` from data directory
3. Compute `master_key = Argon2id(password, salt)` (~0.5-1s)
4. Derive `auth_key = HKDF-SHA256(master_key, "copywraith-auth")`
5. **Constant-time compare** `auth_key` with stored value -- reject if mismatch
6. Derive `kek = HKDF-SHA256(master_key, "copywraith-kek")`
7. Decrypt `DEK = AES-256-GCM-Decrypt(kek, nonce, encrypted_dek)`
8. Cache `DEK` and `SHA-256(password)` in server memory
9. Proceed with request

### Subsequent requests (fast path)

1. Extract password from header
2. Compute `SHA-256(password)`, constant-time compare with cached hash
3. If match, use cached DEK -- proceed
4. If mismatch, return 401

### Server restart

All cached state (DEK, password hash) is lost. First request triggers the slow
path again.

## Entry encryption

### What is encrypted

| Field          | Encrypted? | Reason                                    |
|----------------|------------|-------------------------------------------|
| `text_content` | Yes        | Primary sensitive data                    |
| Blob files     | Yes        | Images, files are sensitive               |
| `content_hash` | No         | Needed for deduplication                  |
| `content_type` | No         | Needed for filtering                      |
| `source_app`   | No         | Metadata, useful for filtering            |
| `starred`      | No         | Needed for filtering                      |
| `sensitive`    | No         | UI display flag                           |
| `created_at`   | No         | Needed for sorting                        |
| `updated_at`   | No         | Needed for sorting                        |

### Search with encryption

SQLite FTS5 cannot index encrypted text. When encryption is active:

- The FTS triggers and table are kept but operate on ciphertext (effectively
  non-functional for user-facing search)
- Server-side search decrypts entries in memory and performs substring matching
- This is O(n) but acceptable for a personal clipboard manager (typically <100k entries)
- The desktop client searches its own local (unencrypted) database and is unaffected

## Password change

1. Verify old password (full Argon2id path)
2. Generate new salt
3. Derive new `master_key`, `auth_key`, `kek` from new password
4. Re-encrypt the **same DEK** with the new KEK
5. Write updated `auth.json`
6. Update in-memory cache

No data re-encryption needed -- only the 32-byte DEK wrapper changes.

## Password reset (destructive)

If the password is forgotten:

1. Delete `auth.json` from the data directory
2. All encrypted entries and blobs become **permanently unrecoverable**
3. Delete `copywraith.db` and the `blobs/` directory to start fresh
4. Restart the server -- it will show the "Create Password" screen

There is no backdoor or recovery mechanism by design.

## Migration of existing data

When a password is first set up on a server with existing unencrypted entries:

1. The setup endpoint creates `auth.json` with the new keys
2. All existing `text_content` values are encrypted in-place (within a DB transaction)
3. All existing blob files are encrypted in-place
4. The `ENC:1:` prefix / `ENCB` header distinguishes encrypted from plaintext,
   providing crash-safety during migration

## Client integration

### Desktop client (Tauri)

- The user enters the server password in Settings (same field currently used for API key)
- The sync module sends it as `Authorization: Bearer <password>` with every request
- No client-side changes to the protocol -- only the semantics of the Bearer value change

### Web admin UI

1. On load: `GET /api/auth/status`
2. If `!initialized`: show "Create Password" form
3. If `initialized && !unlocked`: show "Enter Password" form
4. On setup/unlock: store password in `sessionStorage` (cleared on tab close)
5. All subsequent API calls include `Authorization: Bearer <password>` from session
6. "Lock" button clears `sessionStorage` and reloads
7. 401 responses clear session and show unlock form

## Security notes

- `auth_key` comparison uses constant-time equality to prevent timing attacks
- Each encrypted value uses a unique random 12-byte nonce (AES-256-GCM)
- The `COPYWRAITH_ADMIN_API_KEY` env var has been removed; password auth is the
  sole authentication mechanism
- Password setup is mandatory — the server will not serve data endpoints until
  a password has been configured via the setup screen
- CORS remains permissive -- rely on HTTPS + password for security, not origin restrictions
