//! AES-256-GCM encryption with Argon2 key derivation

use argon2::{Algorithm, Argon2, Params, Version};
use std::time::Instant;
use thiserror::Error;
use zeroize::Zeroizing;

/// Argon2id memory cost: 64 MiB (OWASP recommendation for high-security)
/// This is the amount of memory used in KiB.
pub(crate) const ARGON2_MEMORY_COST: u32 = 65536;

/// Argon2id iteration count: 3 (balanced for interactive use)
/// Higher values increase security but also latency.
pub(crate) const ARGON2_TIME_COST: u32 = 3;

/// Argon2id parallelism: 4 lanes
/// Should match the number of CPU cores available.
pub(crate) const ARGON2_PARALLELISM: u32 = 4;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum EncryptionError {
    #[error("Key derivation failed: {0}")]
    KeyDerivation(String),
    #[error("Encryption failed: {0}")]
    Encryption(String),
    #[error("Decryption failed: {0}")]
    Decryption(String),
    #[error("Invalid passphrase")]
    InvalidPassphrase,
}

/// Derive a 32-byte key from a passphrase using Argon2id with secure parameters
///
/// **Security Parameters:**
/// - Algorithm: Argon2id (hybrid, resistant to side-channel and GPU attacks)
/// - Memory: 64 MiB (65536 KiB)
/// - Iterations: 3
/// - Parallelism: 4 lanes
/// - Output: 32 bytes (256 bits for AES-256)
///
/// These parameters follow OWASP recommendations for high-security applications.
pub fn derive_key(passphrase: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, EncryptionError> {
    if passphrase.is_empty() {
        return Err(EncryptionError::InvalidPassphrase);
    }
    let start = Instant::now();
    let mut key = Zeroizing::new([0u8; 32]);
    let params = Params::new(
        ARGON2_MEMORY_COST,
        ARGON2_TIME_COST,
        ARGON2_PARALLELISM,
        Some(32),
    )
    .map_err(|e| EncryptionError::KeyDerivation(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut *key)
        .map_err(|e| EncryptionError::KeyDerivation(e.to_string()))?;
    // P9: Regulation span
    tracing::info!(target: "reg.keystore", operation = "derive_key", latency_ms = start.elapsed().as_millis(), "REG");
    Ok(key)
}
