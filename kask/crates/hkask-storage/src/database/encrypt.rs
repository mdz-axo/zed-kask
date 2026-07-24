//! Driver-level encryption — transparent AES-256-GCM for DbValue::Text.
//!
//! When a passphrase is configured, text values are encrypted before
//! storage and decrypted on retrieval. Format: `ENCv1:<base64(nonce || tag || ct)>`.
//! The ENCv1: prefix enables automatic detection — plaintext passes through.

use super::value::{DbRow, DbValue};
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
    pub fn encrypt(&self, plaintext: &str) -> String {
        let cipher = Aes256Gcm::new(&self.key);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .expect("AES-GCM encrypt");
        let mut combined = Vec::with_capacity(NONCE_LEN + ct.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ct);
        format!(
            "{PREFIX}{}",
            base64::engine::general_purpose::STANDARD.encode(&combined)
        )
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

    /// Encrypt text params in-place.
    pub fn encrypt_params(&self, params: &mut [DbValue]) {
        for p in params.iter_mut() {
            if let DbValue::Text(s) = p {
                *p = DbValue::Text(self.encrypt(s));
            }
        }
    }

    /// Decrypt text values in query results, returning new rows.
    pub fn decrypt_rows(&self, rows: Vec<DbRow>) -> Vec<DbRow> {
        rows.into_iter().map(|row| self.decrypt_row(row)).collect()
    }

    fn decrypt_row(&self, row: DbRow) -> DbRow {
        let columns = row.column_names().to_vec();
        let values: Vec<DbValue> = (0..row.len())
            .map(|i| {
                row.get(i)
                    .map(|v| match v {
                        DbValue::Text(s) => DbValue::Text(self.decrypt(s)),
                        other => other.clone(),
                    })
                    .unwrap_or(DbValue::Null)
            })
            .collect();
        DbRow::new(columns, values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let enc = Encryptor::from_passphrase("test-passphrase");
        let plaintext = "hello world";
        let encrypted = enc.encrypt(plaintext);
        assert!(encrypted.starts_with("ENCv1:"));
        let decrypted = enc.decrypt(&encrypted);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn plaintext_passes_through() {
        let enc = Encryptor::from_passphrase("test");
        assert_eq!(enc.decrypt("plain"), "plain");
    }

    #[test]
    fn different_passphrases_produce_different_ciphertexts() {
        let e1 = Encryptor::from_passphrase("a");
        let e2 = Encryptor::from_passphrase("b");
        let ct1 = e1.encrypt("hello");
        let ct2 = e2.encrypt("hello");
        assert_ne!(ct1, ct2);
        // Cross-decrypt should fail — return original ciphertext (graceful degradation)
        assert_ne!(e1.decrypt(&ct2), "hello");
    }

    #[test]
    fn same_passphrase_produces_deterministic_keys() {
        let e1 = Encryptor::from_passphrase("same");
        let e2 = Encryptor::from_passphrase("same");
        let ct = e1.encrypt("test");
        assert_eq!(e2.decrypt(&ct), "test");
    }
}
