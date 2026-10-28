use crate::error::AppError;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2,
};
use base64::{engine::general_purpose, Engine as _};
use rand::{rngs::OsRng, RngCore};
use std::fs;
use std::path::{Path, PathBuf};

/// Static salt used by installs created before Sprint 7.5 (T-05). Legacy
/// encrypted databases were keyed with this value; it must remain the
/// fallback so those vaults stay unlockable.
pub const STATIC_SALT: &str = "arnold_static_salt";

/// Resolve (and on first use, create) the per-install salt sidecar.
///
/// Salt file: `<db_path>.salt` containing 16 random bytes, base64-encoded.
/// The salt is NOT secret and is NOT the key — it is a per-install value that
/// makes the Argon2 output unique across machines (guards against cross-install
/// rainbow-table reuse) without weakening the password-derived key. The salt is
/// shared across the AES key (journal content) and the SQLCipher key (whole DB)
/// because both derive from the same password + salt pair.
///
/// Precedence:
/// 1. Existing sidecar → read and return it (stable across restarts).
/// 2. No sidecar but the DB is already encrypted (a pre-T-05 install keyed
///    with `STATIC_SALT`) → persist and return `STATIC_SALT`. Generating a
///    random salt here would change the derived key and LOCK THE VAULT.
/// 3. No sidecar and no encrypted DB (fresh install) → generate 16 random
///    bytes, persist the sidecar, return it.
pub fn get_or_create_salt(db_path: &Path) -> Result<String, AppError> {
    let salt_path = salt_sidecar_path(db_path);
    if let Ok(existing) = fs::read_to_string(&salt_path) {
        let existing = existing.trim().to_string();
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    if crate::db::vault_migrate::is_encrypted(db_path) {
        // Legacy vault — the DB is keyed with the static salt. Persist it so
        // the fallback becomes explicit and durable, then return it.
        fs::write(&salt_path, STATIC_SALT)
            .map_err(|e| AppError::Io(format!("failed to persist salt sidecar: {e}")))?;
        return Ok(STATIC_SALT.to_string());
    }
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let salt = general_purpose::STANDARD.encode(bytes);
    fs::write(&salt_path, &salt)
        .map_err(|e| AppError::Io(format!("failed to write salt sidecar: {e}")))?;
    Ok(salt)
}

fn salt_sidecar_path(db_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.salt", db_path.display()))
}

pub fn derive_key(password: &str, salt: &str) -> [u8; 32] {
    let mut key = [0u8; 32];
    // In a production app, we would use a more robust derivation setup.
    // For now, we'll use a simple Argon2 pass.
    let argon2 = Argon2::default();
    // Salt is deterministic per install (T-05: `arnold.db.salt` sidecar,
    // static-salt fallback for pre-T-05 vaults) so the derived key is identical
    // on every launch — the encrypted DB must stay unlockable across restarts,
    // backups, and portable copies. `encode_b64` (unlike `from_b64`) never
    // rejects our salt string, so there is no panic path here. The salt is NOT
    // secret: it only guards against rainbow-table reuse of Argon2 outputs, and
    // the key is further protected by the user's password.
    let salt_obj =
        SaltString::encode_b64(salt.as_bytes()).expect("salt is well under the 512-byte B64 limit");

    // Argon2 hasher returns a hash, we can use the first 32 bytes as a key
    if let Ok(hash) = argon2.hash_password(password.as_bytes(), &salt_obj) {
        if let Some(hash_bytes) = hash.hash {
            key.copy_from_slice(&hash_bytes.as_bytes()[..32]);
        }
    }
    key
}

pub fn encrypt(data: &str, key: &[u8; 32]) -> Result<String, AppError> {
    let cipher = Aes256Gcm::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data.as_bytes())
        .map_err(|e| AppError::AiError(format!("Encryption failed: {}", e)))?;

    // Combine nonce + ciphertext
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);

    Ok(general_purpose::STANDARD.encode(combined))
}

pub fn decrypt(encoded_data: &str, key: &[u8; 32]) -> Result<String, AppError> {
    let combined = general_purpose::STANDARD
        .decode(encoded_data)
        .map_err(|e| AppError::InvalidInput(format!("Invalid base64: {}", e)))?;

    if combined.len() < 12 {
        return Err(AppError::InvalidInput("Ciphertext too short".into()));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(key.into());

    let decrypted = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::VaultInvalidPassword)?;

    String::from_utf8(decrypted)
        .map_err(|e| AppError::AiError(format!("UTF-8 conversion failed: {}", e)))
}
