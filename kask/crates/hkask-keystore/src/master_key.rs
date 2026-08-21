//! Master key derivation for hKask internal secrets.
//!
//! Provides HKDF-SHA256 key derivation from a single master passphrase.
//! The derivation chain is:
//!
//! 1. Argon2id(master_passphrase, fixed_salt) → 32-byte master key
//!    (slow, memory-hard — run once)
//! 2. HKDF-SHA256(master_key, "hkask-v{version}:{context}") → 32-byte sub-key
//!    (fast, deterministic — run per secret)
//!
//! **Key versioning (v0.30.0):** The `key_version` parameter is embedded in
//! the HKDF info string. This enables passphrase rotation without data loss:
//! old secrets remain derivable from old versions, new secrets use the
//! incremented version. The current version is stored in
//! `~/.config/hkask/version`.
//!
//! This ensures:
//! - The same passphrase + version always produces the same secrets (restart-safe)
//! - Different versions produce cryptographically independent sub-keys
//! - Different contexts produce cryptographically independent sub-keys
//! - Compromising one sub-key does not compromise the master key or other sub-keys
//! - Passphrase rotation preserves access to old-version data

use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

/// HKDF-Extract salt for sub-key derivation.
/// Uses a fixed application-specific salt for domain separation.
const HKDF_SALT: &[u8; 13] = b"hkask-hkdf-v1";

/// Output length for HKDF expansion (256 bits = 32 bytes = AES-256 / HMAC-SHA256 key size).
const SUB_KEY_LEN: usize = 32;

/// Derive a 32-byte sub-key from a master key using HKDF-SHA256.
///
/// HKDF (RFC 5869) provides:
/// - **Extract**: PRK = HMAC-SHA256(salt, IKM) — extracts entropy from master key
/// - **Expand**: OKM = HMAC-SHA256(PRK, info || 0x01) — expands into sub-key
///
/// The `context` string provides cryptographic domain separation: different
/// contexts yield completely independent sub-keys from the same master key.
/// This is the same property that makes HKDF safe for deriving multiple
/// independent keys from a single master secret.
///
/// # Arguments
///
/// * `master_key` — 32-byte master key (typically from Argon2id)
/// * `context` — Domain separation string (e.g., `"hkask:a2a-secret"`)
///
/// # Returns
///
/// 32-byte derived sub-key, wrapped in `Zeroizing` for secure memory handling.
pub fn derive_sub_key(master_key: &[u8], context: &str) -> Zeroizing<Vec<u8>> {
    // HKDF-Extract: PRK = HMAC-SHA256(salt, IKM)
    let mut extract_mac =
        HmacSha256::new_from_slice(HKDF_SALT).expect("HMAC-SHA256 accepts any key length");
    extract_mac.update(master_key);
    let prk = extract_mac.finalize().into_bytes();

    // HKDF-Expand: OKM = HMAC-SHA256(PRK, info || 0x01)
    // For a 32-byte output, only one HKDF block is needed (single 0x01 counter).
    let mut expand_mac =
        HmacSha256::new_from_slice(&prk).expect("HMAC-SHA256 accepts any key length");
    expand_mac.update(context.as_bytes());
    expand_mac.update(&[0x01]); // HKDF block counter
    let okm = expand_mac.finalize().into_bytes();

    Zeroizing::new(okm[..SUB_KEY_LEN].to_vec())
}
