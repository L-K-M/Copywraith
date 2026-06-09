use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use anyhow::Context;
use argon2::Argon2;
use hkdf::Hkdf;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// Encrypted text prefix: `ENC:1:<base64(nonce||ciphertext||tag)>`
const ENC_TEXT_PREFIX: &str = "ENC:1:";
/// Encrypted blob magic header (4 bytes)
const ENC_BLOB_MAGIC: &[u8; 4] = b"ENCB";

// Argon2id parameters
const ARGON2_M_COST: u32 = 65_536; // 64 MiB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 4;
const ARGON2_OUTPUT_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12; // AES-256-GCM nonce
const DEK_LEN: usize = 32;

/// On-disk auth configuration stored as `{data_dir}/auth.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub version: u32,
    /// Base64-encoded 16-byte Argon2 salt.
    pub argon2_salt: String,
    /// Base64-encoded 32-byte auth verification key.
    pub auth_key: String,
    /// Base64-encoded (nonce[12] || ciphertext || tag[16]) of the DEK.
    pub encrypted_dek: String,
    /// Base64-encoded 12-byte nonce used to encrypt the DEK.
    pub dek_nonce: String,
}

/// In-memory crypto state, behind a Mutex in AppState.
pub struct CryptoState {
    data_dir: PathBuf,
    auth_config: Option<AuthConfig>,
    /// Cached DEK after successful unlock.
    dek: Option<[u8; DEK_LEN]>,
    /// SHA-256 of the verified password for fast subsequent checks.
    password_hash_cache: Option<[u8; 32]>,
}

impl CryptoState {
    /// Load auth config from disk. Does NOT unlock (no DEK in memory yet).
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let auth_path = data_dir.join("auth.json");
        let auth_config = if auth_path.exists() {
            let json = std::fs::read_to_string(&auth_path).with_context(|| {
                format!("Failed to read auth config at {}", auth_path.display())
            })?;
            Some(serde_json::from_str::<AuthConfig>(&json).with_context(|| {
                format!(
                    "Invalid auth config JSON at {}. Refusing to start to avoid data-loss during re-setup.",
                    auth_path.display()
                )
            })?)
        } else {
            None
        };

        Ok(CryptoState {
            data_dir: data_dir.to_path_buf(),
            auth_config,
            dek: None,
            password_hash_cache: None,
        })
    }

    pub fn is_initialized(&self) -> bool {
        self.auth_config.is_some()
    }

    pub fn is_unlocked(&self) -> bool {
        self.dek.is_some()
    }

    /// Get a copy of the DEK if unlocked.
    pub fn get_dek(&self) -> Option<[u8; DEK_LEN]> {
        self.dek
    }

    /// Set up a new password. Fails if already initialized.
    pub fn setup_password(&mut self, password: &str) -> anyhow::Result<()> {
        if self.auth_config.is_some() {
            anyhow::bail!("Password already configured");
        }

        // Generate random salt
        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);

        // Derive master key via Argon2id
        let master_key = derive_master_key(password, &salt)?;

        // Derive auth key and KEK via HKDF
        let auth_key = hkdf_derive(&master_key, b"copywraith-auth")?;
        let kek = hkdf_derive(&master_key, b"copywraith-kek")?;

        // Generate random DEK
        let mut dek = [0u8; DEK_LEN];
        rand::thread_rng().fill_bytes(&mut dek);

        // Encrypt DEK with KEK
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&kek)
            .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let encrypted_dek = cipher
            .encrypt(nonce, dek.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to encrypt DEK: {}", e))?;

        let config = AuthConfig {
            version: 1,
            argon2_salt: base64_encode(&salt),
            auth_key: base64_encode(&auth_key),
            encrypted_dek: base64_encode(&encrypted_dek),
            dek_nonce: base64_encode(&nonce_bytes),
        };

        // Write auth.json atomically (crash-safe)
        let auth_path = self.data_dir.join("auth.json");
        let json = serde_json::to_string_pretty(&config)?;
        atomic_write(&auth_path, json.as_bytes())?;

        // Cache in memory
        self.auth_config = Some(config);
        self.dek = Some(dek);
        self.password_hash_cache = Some(sha256_hash(password.as_bytes()));

        Ok(())
    }

    /// Verify password and unlock (derive DEK). Returns Ok(true) on success.
    pub fn verify_and_unlock(&mut self, password: &str) -> anyhow::Result<bool> {
        let config = match &self.auth_config {
            Some(c) => c.clone(),
            None => anyhow::bail!("No password configured"),
        };

        // Fast path: if already unlocked, compare against cached password hash.
        // On a mismatch we deliberately fall through to the slow path below so
        // that wrong-password attempts always pay the Argon2id cost — returning
        // early would let attackers brute-force at SHA-256 speed whenever the
        // server is unlocked (i.e. almost always).
        if let Some(cached_hash) = &self.password_hash_cache {
            let incoming_hash = sha256_hash(password.as_bytes());
            if constant_time_eq(&incoming_hash, cached_hash) {
                return Ok(true);
            }
        }

        // Slow path: full Argon2id verification
        let salt = base64_decode(&config.argon2_salt)?;
        let stored_auth_key = base64_decode(&config.auth_key)?;

        let master_key = derive_master_key(password, &salt)?;
        let auth_key = hkdf_derive(&master_key, b"copywraith-auth")?;

        if !constant_time_eq(&auth_key, &stored_auth_key) {
            return Ok(false);
        }

        // Password verified -- derive KEK and decrypt DEK
        let kek = hkdf_derive(&master_key, b"copywraith-kek")?;
        let encrypted_dek = base64_decode(&config.encrypted_dek)?;
        let nonce_bytes = base64_decode(&config.dek_nonce)?;

        let cipher = Aes256Gcm::new_from_slice(&kek)
            .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let dek_bytes = cipher
            .decrypt(nonce, encrypted_dek.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to decrypt DEK: {}", e))?;

        let mut dek = [0u8; DEK_LEN];
        if dek_bytes.len() != DEK_LEN {
            anyhow::bail!("Decrypted DEK has wrong length: {}", dek_bytes.len());
        }
        dek.copy_from_slice(&dek_bytes);

        // Cache
        self.dek = Some(dek);
        self.password_hash_cache = Some(sha256_hash(password.as_bytes()));

        Ok(true)
    }

    /// Change password. Requires the old password for verification.
    pub fn change_password(
        &mut self,
        old_password: &str,
        new_password: &str,
    ) -> anyhow::Result<()> {
        // Verify old password (force slow path by temporarily clearing cache)
        let cached_hash = self.password_hash_cache.take();
        let cached_dek = self.dek.take();
        let verified = self.verify_and_unlock(old_password)?;
        if !verified {
            // Restore cache if old password didn't verify
            self.password_hash_cache = cached_hash;
            self.dek = cached_dek;
            anyhow::bail!("Old password is incorrect");
        }

        let dek = self
            .dek
            .ok_or_else(|| anyhow::anyhow!("DEK not available after verification"))?;

        // Generate new salt
        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);

        // Derive new keys from new password
        let master_key = derive_master_key(new_password, &salt)?;
        let auth_key = hkdf_derive(&master_key, b"copywraith-auth")?;
        let kek = hkdf_derive(&master_key, b"copywraith-kek")?;

        // Re-encrypt same DEK with new KEK
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&kek)
            .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let encrypted_dek = cipher
            .encrypt(nonce, dek.as_ref())
            .map_err(|e| anyhow::anyhow!("Failed to encrypt DEK: {}", e))?;

        let config = AuthConfig {
            version: 1,
            argon2_salt: base64_encode(&salt),
            auth_key: base64_encode(&auth_key),
            encrypted_dek: base64_encode(&encrypted_dek),
            dek_nonce: base64_encode(&nonce_bytes),
        };

        // Write updated auth.json atomically (crash-safe)
        let auth_path = self.data_dir.join("auth.json");
        let json = serde_json::to_string_pretty(&config)?;
        atomic_write(&auth_path, json.as_bytes())?;

        // Update cache
        self.auth_config = Some(config);
        self.password_hash_cache = Some(sha256_hash(new_password.as_bytes()));

        Ok(())
    }

    /// Clear DEK and session cache (lock the server).
    pub fn lock(&mut self) {
        self.dek = None;
        self.password_hash_cache = None;
    }
}

// ---------------------------------------------------------------------------
// Standalone encryption functions (used by storage layer)
// ---------------------------------------------------------------------------

/// Encrypt text content. Returns `ENC:1:<base64(nonce||ciphertext||tag)>`.
pub fn encrypt_text(dek: &[u8; DEK_LEN], plaintext: &str) -> anyhow::Result<String> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(dek)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // nonce || ciphertext (includes tag)
    let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(format!("{}{}", ENC_TEXT_PREFIX, base64_encode(&combined)))
}

/// Decrypt text content. Handles both encrypted (`ENC:1:...`) and plaintext.
pub fn decrypt_text(dek: &[u8; DEK_LEN], stored: &str) -> anyhow::Result<String> {
    let payload = match stored.strip_prefix(ENC_TEXT_PREFIX) {
        Some(b64) => b64,
        None => return Ok(stored.to_string()), // plaintext passthrough
    };

    let combined = base64_decode(payload)?;
    if combined.len() < NONCE_LEN + 16 {
        anyhow::bail!("Encrypted text too short");
    }

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(dek)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    String::from_utf8(plaintext).map_err(|e| anyhow::anyhow!("Invalid UTF-8 after decrypt: {}", e))
}

/// Encrypt blob data. Returns `ENCB || nonce[12] || ciphertext || tag[16]`.
pub fn encrypt_blob(dek: &[u8; DEK_LEN], data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(dek)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| anyhow::anyhow!("Blob encryption failed: {}", e))?;

    let mut out = Vec::with_capacity(4 + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(ENC_BLOB_MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt blob data. Handles both encrypted (ENCB header) and raw blobs.
pub fn decrypt_blob(dek: &[u8; DEK_LEN], data: &[u8]) -> anyhow::Result<Vec<u8>> {
    if data.len() < 4 || &data[..4] != ENC_BLOB_MAGIC {
        return Ok(data.to_vec()); // raw passthrough
    }

    let rest = &data[4..];
    if rest.len() < NONCE_LEN + 16 {
        anyhow::bail!("Encrypted blob too short");
    }

    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(dek)
        .map_err(|e| anyhow::anyhow!("Failed to create cipher: {}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Blob decryption failed: {}", e))?;

    Ok(plaintext)
}

/// Returns true if the stored value is encrypted text.
pub fn is_encrypted_text(stored: &str) -> bool {
    stored.starts_with(ENC_TEXT_PREFIX)
}

/// Returns true if the stored blob has the ENCB header.
pub fn is_encrypted_blob(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == ENC_BLOB_MAGIC
}

/// Thread-safe wrapper for use in AppState.
pub type SharedCryptoState = Mutex<CryptoState>;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn derive_master_key(password: &str, salt: &[u8]) -> anyhow::Result<[u8; ARGON2_OUTPUT_LEN]> {
    let params = argon2::Params::new(
        ARGON2_M_COST,
        ARGON2_T_COST,
        ARGON2_P_COST,
        Some(ARGON2_OUTPUT_LEN),
    )
    .map_err(|e| anyhow::anyhow!("Invalid Argon2 params: {}", e))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut output = [0u8; ARGON2_OUTPUT_LEN];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut output)
        .map_err(|e| anyhow::anyhow!("Argon2 hash failed: {}", e))?;

    Ok(output)
}

fn hkdf_derive(ikm: &[u8], info: &[u8]) -> anyhow::Result<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(None, ikm);
    let mut output = [0u8; 32];
    hk.expand(info, &mut output)
        .map_err(|e| anyhow::anyhow!("HKDF expand failed: {}", e))?;
    Ok(output)
}

fn sha256_hash(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn base64_decode(s: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| anyhow::anyhow!("Base64 decode failed: {}", e))
}

/// Write data to `path` atomically by writing to a temporary file first and then
/// renaming it into place. This prevents corruption if the process crashes during
/// the write.
fn atomic_write(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;

    let tmp_path = path.with_extension("json.tmp");
    {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(data)?;
        // Flush to disk before the rename; otherwise a power loss can leave
        // the rename durable but the data truncated.
        file.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_encrypt_decrypt_roundtrip() {
        let mut dek = [0u8; DEK_LEN];
        rand::thread_rng().fill_bytes(&mut dek);

        let plaintext = "Hello, clipboard!";
        let encrypted = encrypt_text(&dek, plaintext).unwrap();
        assert!(encrypted.starts_with(ENC_TEXT_PREFIX));
        assert_ne!(encrypted, plaintext);

        let decrypted = decrypt_text(&dek, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_plaintext_passthrough() {
        let dek = [0u8; DEK_LEN];
        let plaintext = "not encrypted";
        let result = decrypt_text(&dek, plaintext).unwrap();
        assert_eq!(result, plaintext);
    }

    #[test]
    fn test_blob_encrypt_decrypt_roundtrip() {
        let mut dek = [0u8; DEK_LEN];
        rand::thread_rng().fill_bytes(&mut dek);

        let data = b"binary blob data here";
        let encrypted = encrypt_blob(&dek, data).unwrap();
        assert!(encrypted.starts_with(ENC_BLOB_MAGIC));

        let decrypted = decrypt_blob(&dek, &encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_raw_blob_passthrough() {
        let dek = [0u8; DEK_LEN];
        let raw = b"\x89PNG\r\n\x1a\n";
        let result = decrypt_blob(&dek, raw).unwrap();
        assert_eq!(result, raw);
    }

    #[test]
    fn test_wrong_key_fails_decrypt() {
        let mut dek1 = [0u8; DEK_LEN];
        let mut dek2 = [0u8; DEK_LEN];
        rand::thread_rng().fill_bytes(&mut dek1);
        rand::thread_rng().fill_bytes(&mut dek2);

        let encrypted = encrypt_text(&dek1, "secret").unwrap();
        let result = decrypt_text(&dek2, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_setup_and_unlock() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = CryptoState::load(dir.path()).unwrap();

        assert!(!state.is_initialized());
        assert!(!state.is_unlocked());

        state.setup_password("test-password-123").unwrap();
        assert!(state.is_initialized());
        assert!(state.is_unlocked());

        // Lock
        state.lock();
        assert!(state.is_initialized());
        assert!(!state.is_unlocked());

        // Unlock with correct password
        assert!(state.verify_and_unlock("test-password-123").unwrap());
        assert!(state.is_unlocked());

        // Lock and try wrong password
        state.lock();
        assert!(!state.verify_and_unlock("wrong-password").unwrap());
        assert!(!state.is_unlocked());
    }

    #[test]
    fn test_wrong_password_rejected_while_unlocked() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = CryptoState::load(dir.path()).unwrap();

        state.setup_password("test-password-123").unwrap();
        assert!(state.is_unlocked());

        // A wrong password must be rejected on the cached fast path too,
        // and must not lock the server.
        assert!(!state.verify_and_unlock("wrong-password").unwrap());
        assert!(state.is_unlocked());
        assert!(state.verify_and_unlock("test-password-123").unwrap());
    }

    #[test]
    fn test_change_password() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = CryptoState::load(dir.path()).unwrap();

        state.setup_password("old-password").unwrap();
        let dek_before = state.get_dek().unwrap();

        state
            .change_password("old-password", "new-password")
            .unwrap();
        let dek_after = state.get_dek().unwrap();

        // DEK should be the same after password change
        assert_eq!(dek_before, dek_after);

        // Old password should no longer work
        state.lock();
        assert!(!state.verify_and_unlock("old-password").unwrap());

        // New password should work
        assert!(state.verify_and_unlock("new-password").unwrap());
    }
}
