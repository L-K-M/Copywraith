//! Server envelope validation and repair, layered above durable file publication.
use std::path::Path;

use copywraith_core::blob_store::BlobStore;
use copywraith_core::content::hash_bytes;

use crate::crypto;

pub(super) struct ServerBlobs {
    files: BlobStore,
}

impl ServerBlobs {
    pub(super) fn open(path: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            files: BlobStore::open(path)?,
        })
    }

    pub(super) fn put(&self, bytes: &[u8], dek: Option<&[u8; 32]>) -> anyhow::Result<String> {
        let hash = hash_bytes(bytes);
        let encoded = match dek {
            Some(key) => crypto::encrypt_blob(key, bytes)?,
            None => {
                // Missing keys must never downgrade an existing authenticated file.
                if self
                    .read(&hash)?
                    .is_some_and(|stored| crypto::is_encrypted_blob(&stored))
                {
                    anyhow::bail!("Cannot replace an encrypted blob without its key");
                }
                bytes.to_vec()
            }
        };
        self.ensure(&hash, &encoded, dek)?;
        Ok(hash)
    }

    pub(super) fn ensure_existing(&self, hash: &str, dek: Option<&[u8; 32]>) -> anyhow::Result<()> {
        let stored = self
            .read(hash)?
            .ok_or_else(|| anyhow::anyhow!("Missing blob {hash}"))?;
        let plaintext = decode(&stored, dek)?;
        anyhow::ensure!(hash_bytes(&plaintext) == hash, "Blob hash mismatch");
        self.put(&plaintext, dek)?;
        Ok(())
    }

    fn ensure(&self, hash: &str, encoded: &[u8], dek: Option<&[u8; 32]>) -> anyhow::Result<()> {
        self.files.ensure(hash, encoded, |stored| {
            // Reuse only the requested representation, after authentication/hash checks.
            if dek.is_some() != crypto::is_encrypted_blob(stored) {
                return false;
            }
            decode(stored, dek).is_ok_and(|plaintext| hash_bytes(&plaintext) == hash)
        })?;
        Ok(())
    }

    pub(super) fn read(&self, hash: &str) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.files.read(hash)?)
    }

    pub(super) fn remove(&self, hash: &str) {
        // DB deletion already committed; failed reclamation leaves an orphan.
        let _ = self.files.remove(hash);
    }

    pub(super) fn encrypt_all(&self, dek: &[u8; 32]) -> anyhow::Result<()> {
        for hash in self.files.hashes()? {
            self.ensure_existing(&hash, Some(dek))?;
        }
        Ok(())
    }
}

fn decode(stored: &[u8], dek: Option<&[u8; 32]>) -> anyhow::Result<Vec<u8>> {
    if let Some(key) = dek {
        // decrypt_blob fails on invalid ENCB envelopes; never reinterpret them.
        return crypto::decrypt_blob(key, stored);
    }
    anyhow::ensure!(
        !crypto::is_encrypted_blob(stored),
        "Encrypted blob requires a key"
    );
    Ok(stored.to_vec())
}
