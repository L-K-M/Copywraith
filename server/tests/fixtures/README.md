Compatibility fixtures generated before migration with aes-gcm 0.10.3,
rusqlite 0.32.1, ULID 1.2.1 and rand 0.9.2 on Rust 1.85.0.

`generate.rs` records the generator temporarily appended to `server/src/crypto.rs`.
Run only against those original dependencies, in an empty fixture directory.
Do not regenerate with upgraded dependencies: these bytes test backward compatibility.

- `auth.json`: password `fixture-password`; random salt, DEK and wrapping nonce.
- `dek.bin`: public test key for the ciphertext fixtures; never use in production.
- `text.enc`: `legacy ciphertext 🦀` in the existing `ENC:1:` envelope.
- `blob.enc`: bytes `\x00\xfflegacy blob` in the existing `ENCB` envelope.
- `legacy.db`: SQLite database missing flavor/search/sensitive columns.
- `entry.json`: original ULID and serialized entry corresponding to its row.

The desktop test adds its historical `synced` column before opening this shared
legacy database through the actual desktop storage implementation.
