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

use hkask_types::secret::derivation_contexts;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

/// Salt used for the initial Argon2id master key derivation.
///
/// Fixed so that the same passphrase always produces the same master key.
/// This is not a security weakness — the Argon2id memory-hardness provides
/// the security, and the salt's purpose is domain separation, not secrecy.
const MASTER_KEY_SALT: [u8; 16] = [
    b'h', b'k', b'a', b's', b'k', b'-', b'm', b'a', b's', b't', b'e', b'r', b'-', b'2', b'0', b'2',
];

/// HKDF-Extract salt for sub-key derivation.
/// Uses a fixed application-specific salt for domain separation.
const HKDF_SALT: &[u8; 13] = b"hkask-hkdf-v1";

/// Output length for HKDF expansion (256 bits = 32 bytes = AES-256 / HMAC-SHA256 key size).
const SUB_KEY_LEN: usize = 32;

/// Current key version.
pub const CURRENT_KEY_VERSION: u32 = 1;

/// All internal secrets derived from the master key.
///
/// Each field is a hex-encoded 256-bit key, deterministically derived
/// from the master passphrase via HKDF-SHA256.
pub struct InternalSecrets {
    /// Master key (hex-encoded 256-bit key) — the root from which all sub-keys derive.
    /// Stored in keychain as HKASK_MASTER_KEY for pod OCAP derivation and restart survival.
    pub master_key_hex: String,
    /// A2A HMAC signing secret (hex-encoded 256-bit key)
    pub a2a_secret: String,

    /// OCAP capability token signing secret (hex-encoded 256-bit key)
    pub ocap_secret: String,
}

impl std::fmt::Debug for InternalSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalSecrets")
            .field("a2a_secret", &"[REDACTED]")
            .field("ocap_secret", &"[REDACTED]")
            .finish()
    }
}

/// Derive all internal secrets from a master passphrase.
///
/// Uses Argon2id (slow, memory-hard) once to stretch the passphrase into a
/// 32-byte master key, then HKDF-SHA256 (fast, deterministic) to derive each
/// sub-key with domain separation.
///
/// # Security
///
/// - Argon2id with OWASP-recommended parameters (64 MiB, 3 iterations, 4 lanes)
/// - HKDF-SHA256 with per-context info strings for domain separation
/// - All intermediate key material is zeroized on drop
///
/// # Panics
///
/// Cannot panic — Argon2id and HKDF are infallible with valid parameters.
pub fn derive_all_internal_secrets(master_passphrase: &str) -> InternalSecrets {
    derive_all_internal_secrets_with_version(master_passphrase, CURRENT_KEY_VERSION)
}

/// Derive all internal secrets with a specific key version.
///
/// The `key_version` is embedded in the HKDF info string as
/// `"hkask-v{version}:{context}"`. This ensures that rotating the
/// passphrase (incrementing the version) produces cryptographically
/// independent secrets while old-version secrets remain derivable.
///
/// Use this when:
/// - Rotating the master passphrase (increment version)
/// - Accessing old-version data after rotation (use old version)
/// - Initial setup (use version 1)
pub fn derive_all_internal_secrets_with_version(
    master_passphrase: &str,
    key_version: u32,
) -> InternalSecrets {
    let start = std::time::Instant::now();

    // P9: Regulation span
    tracing::info!(target: "reg.keystore", operation = "derive_internal_secrets", key_version = key_version, "REG");

    // Step 1: Argon2id stretch (slow, ~100ms)
    let master_key = crate::encryption::derive_key(master_passphrase, &MASTER_KEY_SALT)
        .expect("Argon2id derivation cannot fail with valid parameters");

    // Step 2: HKDF-SHA256 expand with versioned contexts (fast, ~1μs each)
    let master_key_bytes: &[u8] = &*master_key;
    let a2a_secret = derive_sub_key_hex_versioned(
        master_key_bytes,
        derivation_contexts::A2A_SECRET,
        key_version,
    );

    let ocap_secret = derive_sub_key_hex_versioned(
        master_key_bytes,
        derivation_contexts::OCAP_SECRET,
        key_version,
    );

    // P9: Regulation span
    tracing::info!(target: "reg.keystore", operation = "internal_secrets_derived", latency_ms = start.elapsed().as_millis(), "REG");

    InternalSecrets {
        master_key_hex: hex::encode(*master_key),
        a2a_secret,
        ocap_secret,
    }
}

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

/// Derive a sub-key with a key version embedded in the HKDF info string.
///
/// The info string becomes `"hkask-v{version}:{context}"` instead of
/// just `context`. This provides cryptographic domain separation between
/// different key versions — version N and version N+1 produce completely
/// independent sub-keys from the same master key.
pub fn derive_sub_key_with_version(
    master_key: &[u8],
    context: &str,
    key_version: u32,
) -> Zeroizing<Vec<u8>> {
    let versioned_context = format!("hkask-v{key_version}:{context}");
    derive_sub_key(master_key, &versioned_context)
}

/// Derive a versioned sub-key and return it as a hex-encoded string.
fn derive_sub_key_hex_versioned(master_key: &[u8], context: &str, key_version: u32) -> String {
    let sub_key = derive_sub_key_with_version(master_key, context, key_version);
    hex::encode(&*sub_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    //
    // Version N and version N+1 must produce cryptographically independent
    // sub-keys from the same master key and context.
    #[test]
    fn different_versions_produce_different_keys() {
        let master_key = [0u8; 32];
        let context = "test-context";

        let v1 = derive_sub_key_with_version(&master_key, context, 1);
        let v2 = derive_sub_key_with_version(&master_key, context, 2);

        assert_ne!(&*v1, &*v2, "Different versions must produce different keys");
    }

    //
    // The same master key, context, and version must always produce
    // the same sub-key (deterministic derivation).
    #[test]
    fn same_version_produces_same_key() {
        let master_key = [0u8; 32];
        let context = "test-context";

        let v1_a = derive_sub_key_with_version(&master_key, context, 1);
        let v1_b = derive_sub_key_with_version(&master_key, context, 1);

        assert_eq!(&*v1_a, &*v1_b, "Same version must produce same key");
    }

    #[test]
    fn derive_all_secrets_with_version_is_deterministic() {
        let passphrase = "test-passphrase-for-versioning";

        let secrets_a = derive_all_internal_secrets_with_version(passphrase, 1);
        let secrets_b = derive_all_internal_secrets_with_version(passphrase, 1);

        assert_eq!(secrets_a.a2a_secret, secrets_b.a2a_secret);
        assert_eq!(secrets_a.ocap_secret, secrets_b.ocap_secret);
    }

    #[test]
    fn derive_all_secrets_different_versions_differ() {
        let passphrase = "test-passphrase-for-versioning";

        let secrets_v1 = derive_all_internal_secrets_with_version(passphrase, 1);
        let secrets_v2 = derive_all_internal_secrets_with_version(passphrase, 2);

        assert_ne!(secrets_v1.a2a_secret, secrets_v2.a2a_secret);
        assert_ne!(secrets_v1.ocap_secret, secrets_v2.ocap_secret);
    }
}
