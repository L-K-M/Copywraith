# Encryption design

This describes how the Copywraith **server** encrypts data at rest. The
authoritative implementation is `server/src/crypto.rs` (with storage wiring in
`server/src/storage.rs`).

> [!NOTE]
> Encryption is a server-side, at-rest feature gated on a configured password.
> The desktop/mobile local SQLite caches are **not** encrypted today; rely on
> OS-level disk encryption for those. Transport is plain HTTP intended to run
> over a trusted LAN/VPN.

## Key hierarchy

```text
password ──Argon2id(salt)──▶ master key (32B)
                               ├─HKDF-SHA256("copywraith-auth")─▶ auth key  (verifies password)
                               └─HKDF-SHA256("copywraith-kek")──▶ KEK
                                                                   │ AES-256-GCM
                                                                   ▼
                                            random DEK (32B) ◀── decrypt(encrypted_dek)
                                                   │
                                                   ▼ AES-256-GCM (per record)
                                         text fields + blob files
```

- **Master key**: derived from the password with **Argon2id** (m=64 MiB, t=3,
  p=4, 32-byte output) and a random 16-byte salt.
- **Auth key**: `HKDF-SHA256(master, info="copywraith-auth")`. Stored so a
  password can be verified without keeping the password itself. Compared in
  constant time.
- **KEK** (key-encryption key): `HKDF-SHA256(master, info="copywraith-kek")`.
- **DEK** (data-encryption key): a random 32-byte key generated at setup,
  encrypted with the KEK (AES-256-GCM) and stored as `encrypted_dek`. All record
  data is encrypted with the DEK. Decoupling the DEK from the password means a
  **password change re-wraps the same DEK** rather than re-encrypting every
  record.

The distinct HKDF `info` strings provide domain separation between the auth key
and the KEK even though both expand the same high-entropy master key.

## On-disk auth config

`{data_dir}/auth.json` (written atomically via temp-file + rename):

| field           | meaning                                            |
| --------------- | -------------------------------------------------- |
| `version`       | format version (currently `1`)                     |
| `argon2_salt`   | base64, 16 bytes                                    |
| `auth_key`      | base64, 32 bytes (password verifier)               |
| `encrypted_dek` | base64 of `nonce(12) ‖ ciphertext ‖ tag(16)`       |
| `dek_nonce`     | base64, 12 bytes (nonce used to wrap the DEK)      |

If `auth.json` is present but unparseable, the server refuses to start rather
than risk overwriting it during a re-setup.

## Record encryption

- **Text fields** (`text_content`, `text_plain`, `text_html`, `text_rtf`,
  `search_text`) are stored as `ENC:1:<base64(nonce(12) ‖ ciphertext ‖ tag(16))>`.
- **Blobs** (image/file bytes) are stored as `ENCB ‖ nonce(12) ‖ ciphertext ‖ tag(16)`
  (a 4-byte magic header).
- Cipher: **AES-256-GCM**, with a fresh random 12-byte nonce per encryption.

Both formats are self-describing, so decryption transparently passes through
values that are *not* encrypted (entries written before a password was set).
Calling `auth/setup` migrates existing plaintext rows and blobs in place.

## Locking / unlocking

- The DEK lives only in memory after a successful unlock; `auth/lock` clears it.
- Every authenticated request supplies the password as `Authorization: Bearer
  <password>`. A fast path compares a SHA-256 of the password against a cached
  hash to avoid re-running Argon2id on every request once unlocked; the slow
  path performs full Argon2id derivation + auth-key comparison.

## Search vs. encryption trade-off

FTS5 indexes plaintext, so when a password is configured FTS is bypassed and
search falls back to loading candidate rows, decrypting in memory, and filtering
by substring. This keeps ciphertext out of the index at the cost of slower
search on large encrypted datasets.

## Known limitations / future work

- No rate limiting on unlock attempts (mitigated by the trusted-network
  requirement and Argon2id cost).
- Local client caches are unencrypted.
- Cached key material is not explicitly zeroized on lock (relies on Rust drop).
</content>
