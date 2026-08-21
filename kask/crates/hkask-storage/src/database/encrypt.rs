//! Driver-level encryption — transparent AES-256-GCM for DbValue::Text.
//!
//! When a passphrase is configured, text values are encrypted before
//! storage and decrypted on retrieval. Format: `ENCv1:<base64(nonce || tag || ct)>`.
//! The ENCv1: prefix enables automatic detection — plaintext passes through.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use blake3::Hasher;
use rand::RngCore;

const PREFIX: &str = "ENCv1:";
const NONCE_LEN: usize = 12;

pub struct Encryptor {
    key: Key<Aes256Gcm>,
}

impl Encryptor {
    pub fn from_passphrase(passphrase: &str) -> Self {
        let mut hasher = Hasher::new();
        hasher.update(b"hkask-db-encrypt-v1");
        hasher.update(passphrase.as_bytes());
        let hash = hasher.finalize();
        let key = *Key::<Aes256Gcm>::from_slice(hash.as_bytes());
        Self { key }
    }

    /// Encrypt a plaintext string → `ENCv1:<base64>`.
    ///
    /// If AES-GCM encryption fails (extremely unlikely for h_mem-sized values —
    /// the only failure mode is plaintext exceeding the AES-GCM max message
    /// size of ~64GB), the plaintext is returned as-is with a `log::error!`.
    /// This is degraded (unencrypted) but keeps the process alive instead of
    /// panicking.
    pub fn encrypt(&self, plaintext: &str) -> String {
        let cipher = Aes256Gcm::new(&self.key);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        match cipher.encrypt(nonce, plaintext.as_bytes()) {
            Ok(ct) => {
                let mut combined = Vec::with_capacity(NONCE_LEN + ct.len());
                combined.extend_from_slice(&nonce_bytes);
                combined.extend_from_slice(&ct);
                format!(
                    "{PREFIX}{}",
                    base64::engine::general_purpose::STANDARD.encode(&combined)
                )
            }
            Err(e) => {
                tracing::error!(
                    target: "reg.storage",
                    error = %e,
                    plaintext_len = plaintext.len(),
                    "AES-GCM encryption failed — returning plaintext unencrypted (degraded). \
                     This should not happen for h_mem-sized values; investigate the \
                     plaintext size."
                );
                plaintext.to_string()
            }
        }
    }

    /// Decrypt if prefixed, else return as-is.
    pub fn decrypt(&self, value: &str) -> String {
        let rest = match value.strip_prefix(PREFIX) {
            Some(r) => r,
            None => return value.to_string(),
        };
        let Ok(combined) = base64::engine::general_purpose::STANDARD.decode(rest) else {
            return value.to_string();
        };
        if combined.len() < NONCE_LEN + 16 {
            return value.to_string();
        }
        let cipher = Aes256Gcm::new(&self.key);
        let nonce = Nonce::from_slice(&combined[..NONCE_LEN]);
        cipher
            .decrypt(nonce, &combined[NONCE_LEN..])
            .map(|pt| String::from_utf8_lossy(&pt).into_owned())
            .unwrap_or_else(|_| value.to_string())
    }
}
