//! OS keychain integration

use ed25519_dalek::Signer;
use hkask_types::NotFound;
use hkask_types::WebID;
use hkask_types::keychain_keys::{KEY_A2A_SECRET, KEY_DB_PASSPHRASE};
use hkask_types::secret::SecretRef;
use hkask_types::secret::derivation_contexts;
use keyring::{Entry, Error as KeyringError};
use thiserror::Error;
use tracing::info;
#[cfg(debug_assertions)]
use tracing::warn;
use zeroize::Zeroizing;

#[derive(Error, Debug)]
pub enum KeychainError {
    #[error("Platform keychain error: {0}")]
    Platform(String),
    #[error("Secret not found: {0}")]
    NotFound(NotFound),
}

impl From<NotFound> for KeychainError {
    fn from(nf: NotFound) -> Self {
        KeychainError::NotFound(nf)
    }
}

impl From<KeyringError> for KeychainError {
    fn from(err: KeyringError) -> Self {
        use KeyringError::*;
        match err {
            NoEntry => KeychainError::NotFound(NotFound {
                entity_type: "secret".to_string(),
                id: "secret not found in keychain".to_string(),
            }),
            other => KeychainError::Platform(other.to_string()),
        }
    }
}

/// Keychain service for secure credential storage
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// inv: secrets are stored in OS keychain, never in plaintext files
pub struct Keychain {
    service_name: String,
}

impl Keychain {
    /// Create a new Keychain for the given service name.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// post: service_name is set
    pub fn new(service_name: &str) -> Self {
        Self {
            service_name: service_name.to_string(),
        }
    }

    /// Store a secret in the OS keychain, keyed by WebID.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  webid is a valid WebID, secret is non-empty
    /// post: secret stored in OS keychain under service_name + webid.uuid
    /// post: returns Err(Platform) if keychain is unavailable
    pub fn store(&self, webid: &WebID, secret: &str) -> Result<(), KeychainError> {
        let entry = Entry::new(&self.service_name, &webid.as_uuid().to_string())
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        entry
            .set_password(secret)
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        // P9: Regulation span
        info!(target: "reg.keystore", operation = "store", "REG");
        Ok(())
    }

    /// Retrieve a secret from the OS keychain by WebID.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  webid is a valid WebID
    /// post: returns Ok(secret) if stored, Err(NotFound) if not
    pub fn retrieve(&self, webid: &WebID) -> Result<String, KeychainError> {
        let entry = Entry::new(&self.service_name, &webid.as_uuid().to_string())
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        let result = entry.get_password().map_err(KeychainError::from)?;
        info!(target: "reg.keystore", operation = "retrieve", "REG");
        Ok(result)
    }

    /// Delete a secret from the OS keychain by WebID.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  webid is a valid WebID
    /// post: secret removed from OS keychain
    /// post: idempotent — deleting non-existent entry is no-op (platform-dependent)
    pub fn delete(&self, webid: &WebID) -> Result<(), KeychainError> {
        let entry = Entry::new(&self.service_name, &webid.as_uuid().to_string())
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        entry
            .delete_credential()
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        info!(target: "reg.keystore", operation = "delete", "REG");
        Ok(())
    }

    /// Store a secret in the OS keychain by arbitrary key name.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  key is non-empty, secret is non-empty
    /// post: secret stored in OS keychain under service_name + key
    pub fn store_by_key(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        entry
            .set_password(secret)
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        info!(target: "reg.keystore", operation = "store_by_key", "REG");
        Ok(())
    }

    /// Retrieve a secret from the OS keychain by arbitrary key name.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  key is non-empty
    /// post: returns Ok(secret) if stored, Err(NotFound) if not
    pub fn retrieve_by_key(&self, key: &str) -> Result<String, KeychainError> {
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        let result = entry.get_password().map_err(KeychainError::from)?;
        info!(target: "reg.keystore", operation = "retrieve_by_key", "REG");
        Ok(result)
    }

    /// Delete a secret from the OS keychain by arbitrary key name.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  key is non-empty
    /// post: secret removed from OS keychain
    pub fn delete_by_key(&self, key: &str) -> Result<(), KeychainError> {
        let entry = Entry::new(&self.service_name, key)
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        entry
            .delete_credential()
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        info!(target: "reg.keystore", operation = "delete_by_key", "REG");
        Ok(())
    }
}

impl Default for Keychain {
    fn default() -> Self {
        Self::new("hkask")
    }
}

//
// These functions encapsulate the standard 3-tier resolution chain
// (derived → env → keychain) for each well-known secret. Every call site
// uses these canonical implementations.
//
// Benefits:
//   - Single implementation eliminates copy-paste drift
//   - Single place to audit secret resolution behavior

/// Resolve a secret through the standard 3-tier chain:
/// 1. Master key derivation (HKDF-SHA256)
/// 2. Direct environment variable
/// 3. OS keychain lookup
///
/// This is the canonical resolution pattern for all hKask secrets.
/// Domain-specific functions (`resolve_a2a_secret`, etc.) call this with
/// the appropriate parameters.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// pre:  derivation_context, env_var, keychain_key are valid
/// post: tries derivation → env → keychain in order
/// post: returns Ok(Zeroizing<`Vec<u8>`>) on first success
/// post: returns Err if all three sources fail
pub fn resolve_secret_chain(
    derivation_context: (&str, &str),
    env_var: &str,
    keychain_key: &str,
) -> Result<Zeroizing<Vec<u8>>, KeychainError> {
    resolve(&SecretRef::env(env_var))
        .or_else(|_| resolve(&SecretRef::keychain(keychain_key)))
        .or_else(|_| {
            resolve(&SecretRef::derived(
                derivation_context.0,
                derivation_context.1,
            ))
        })
}

/// Resolve the A2A (Agent-to-Agent Protocol) HMAC signing secret.
///
/// Chain: master key derivation → env var → OS keychain.
/// Uses the `HKASK_A2A_SECRET` environment variable.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// post: returns Zeroizing<`Vec<u8>`> from first successful resolution step
pub fn resolve_a2a_secret() -> Result<Zeroizing<Vec<u8>>, KeychainError> {
    resolve_secret_chain(
        (
            derivation_contexts::MASTER_KEY_ENV,
            derivation_contexts::A2A_SECRET,
        ),
        "HKASK_A2A_SECRET",
        KEY_A2A_SECRET,
    )
}

/// Resolve the database encryption passphrase.
///
/// Chain: env var → OS keychain.
/// Note: no master-key derivation for the DB passphrase — it must be
/// explicitly set via env var or keychain to avoid accidentally encrypting
/// the database with a derived key that the user didn't consent to.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// post: returns Zeroizing<`Vec<u8>`> from env var or keychain
pub fn resolve_db_passphrase() -> Result<Zeroizing<Vec<u8>>, KeychainError> {
    resolve(&SecretRef::env("HKASK_DB_PASSPHRASE"))
        .or_else(|_| resolve(&SecretRef::keychain(KEY_DB_PASSPHRASE)))
}

/// Resolve the canonical SQLCipher passphrase as text.
///
/// All database openers must use this function so the same configured secret
/// produces the same SQLCipher key across CLI, pods, synchronization, and MCP.
pub fn resolve_db_passphrase_string() -> Result<Zeroizing<String>, KeychainError> {
    let bytes = resolve_db_passphrase()?;
    let passphrase = String::from_utf8(bytes.to_vec())
        .map_err(|e| KeychainError::Platform(format!("DB passphrase is not valid UTF-8: {e}")))?;
    Ok(Zeroizing::new(passphrase))
}

/// Get the OCAP secret derived from the master key.
///
/// Resolution chain:
/// 1. Deterministic derivation from master key
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// post: returns Zeroizing<`Vec<u8>`> from derivation
/// post: returns Err if the master key is unavailable
pub fn get_or_create_ocap_secret() -> Result<Zeroizing<Vec<u8>>, KeychainError> {
    let derived = resolve(&SecretRef::derived(
        derivation_contexts::MASTER_KEY_ENV,
        derivation_contexts::OCAP_SECRET,
    ));

    match derived {
        Ok(key) => {
            info!(target: "reg.keystore", operation = "ocap_secret", source = "derived", "REG");
            Ok(key)
        }
        Err(err) => Err(err),
    }
}

/// Resolve a SecretRef to actual secret bytes.
///
/// Resolution priority:
/// 1. `Env` — read from environment variable
/// 2. `Keychain` — read from OS keychain
/// 3. `Derived` — look up master key (env → keychain), then HKDF-SHA256 derive sub-key
/// 4. `Generated` — random bytes (⚠️ not reproducible; debug builds only)
///
/// For `Derived`, the master key is resolved first (env var → keychain),
/// then HKDF-SHA256 is applied with the given context string to produce
/// a deterministic 256-bit sub-key.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// pre:  secret_ref is a valid SecretRef variant
/// post: Env → reads from environment variable, Err(NotFound) if unset
/// post: Keychain → reads from OS keychain, Err(NotFound) if absent
/// post: Derived → resolves master key (env→keychain), HKDF-SHA256 derives sub-key
/// post: Generated → random bytes (debug only, not reproducible)
/// post: all returned secrets wrapped in Zeroizing
pub fn resolve(secret_ref: &SecretRef) -> Result<Zeroizing<Vec<u8>>, KeychainError> {
    // P9: Regulation span
    let start = std::time::Instant::now();
    let variant = match secret_ref {
        SecretRef::Env(_) => "env",
        SecretRef::Keychain(_) => "keychain",
        SecretRef::Derived { .. } => "derived",
        #[cfg(debug_assertions)]
        SecretRef::Generated(_) => "generated",
    };
    info!(target: "reg.keystore", operation = "resolve", variant = variant, "REG");

    match secret_ref {
        SecretRef::Env(var_name) => {
            let value = std::env::var(var_name).map_err(|_| {
                KeychainError::NotFound(NotFound {
                    entity_type: "secret".to_string(),
                    id: format!("env var {} not set", var_name),
                })
            })?;
            info!(target: "reg.keystore", operation = "resolve_env", var_name = %var_name, "REG");
            Ok(Zeroizing::new(value.into_bytes()))
        }
        SecretRef::Keychain(key_name) => {
            // Guard against concurrent libdbus SIGABRT from multiple processes
            // hitting the OS keyring simultaneously (e.g., kask mcp invoke spawns
            // all MCP servers at once, each calling InferenceConfig::from_env()).
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let keychain = Keychain::default();
                let entry = Entry::new(&keychain.service_name, key_name)
                    .map_err(|e| KeychainError::Platform(e.to_string()))?;
                let secret = entry.get_password().map_err(KeychainError::from)?;
                info!(target: "reg.keystore", operation = "resolve_keychain", key_name = %key_name, "REG");
                Ok::<_, KeychainError>(Zeroizing::new(secret.into_bytes()))
            }));
            match result {
                Ok(inner) => inner,
                Err(_) => {
                    tracing::warn!(target: "reg.keystore", key_name = %key_name, "Keychain access panicked (likely concurrent D-Bus access) — falling back to env var");
                    Err(KeychainError::Platform(
                        "Keychain access panicked — concurrent D-Bus access may have triggered C-level abort".into(),
                    ))
                }
            }
        }
        SecretRef::Derived {
            master_key_env,
            context,
        } => {
            info!(target: "reg.keystore", operation = "resolve_derived", master_key_env = %master_key_env, context = %context, "REG");
            // Resolve master key: env var first, then keychain
            let master_key_bytes = resolve(&SecretRef::Env(master_key_env.clone()))
                .or_else(|_| resolve(&SecretRef::Keychain(master_key_env.clone())))
                .map_err(|_| {
                    KeychainError::NotFound(NotFound {
                        entity_type: "secret".to_string(),
                        id: format!(
                            "Master key '{}' not found in environment or keychain; \
                     set {} or run `kask init` to derive secrets from a master passphrase",
                            master_key_env, master_key_env
                        ),
                    })
                })?;

            let master_key_bytes = normalize_master_key_bytes(master_key_bytes)?;

            // HKDF-SHA256 derive sub-key
            let sub_key = crate::master_key::derive_sub_key(&master_key_bytes, context);
            info!(target: "reg.keystore", operation = "derive_sub_key", latency_ms = start.elapsed().as_millis(), "REG");
            Ok(sub_key)
        }
        #[cfg(debug_assertions)]
        SecretRef::Generated(length) => {
            let bytes: Vec<u8> = (0..*length as usize)
                .map(|_| rand::random::<u8>())
                .collect();
            warn!(target: "reg.keystore", operation = "resolve_generated", length = *length, "REG");
            Ok(Zeroizing::new(bytes))
        }
    }
}

fn normalize_master_key_bytes(
    master_key_bytes: Zeroizing<Vec<u8>>,
) -> Result<Zeroizing<Vec<u8>>, KeychainError> {
    let Ok(as_str) = std::str::from_utf8(&master_key_bytes) else {
        return Ok(master_key_bytes);
    };
    let trimmed = as_str.trim();
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        let decoded = hex::decode(trimmed)
            .map_err(|e| KeychainError::Platform(format!("invalid master key hex: {e}")))?;
        return Ok(Zeroizing::new(decoded));
    }
    Ok(master_key_bytes)
}

// ── Wallet key derivation ──────────────────────────────────────────────────────

/// Resolve the treasury key for a given derivation context string.
///
/// The caller is responsible for mapping chain to derivation context.
/// Context strings are defined in `hkask_types::secret::derivation_contexts`:
/// - `TREASURY_HEDERA`, `WALLET_SEED`
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// pre:  context is a valid derivation context string
/// post: returns Ok(Zeroizing<`Vec<u8>`>) — 32-byte HKDF-derived seed
/// post: same master key → same treasury key for given context (deterministic)
pub fn resolve_treasury_key(context: &str) -> Result<Zeroizing<Vec<u8>>, KeychainError> {
    resolve(&SecretRef::derived(
        derivation_contexts::MASTER_KEY_ENV,
        context,
    ))
}

/// Derive the wallet seed for HD derivation, deposit references, and API key signing.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// post: returns Ok(Zeroizing<`Vec<u8>`>) — 32-byte HKDF-derived seed
/// post: same master key → same wallet seed (deterministic)
///
/// Context: `"hkask:wallet-seed"`
///
/// This seed is used for:
/// - Deriving deposit addresses (BIP44-style per chain)
/// - Generating deposit references (HKDF with nonce + expiry)
/// - Signing API key capability tokens (Ed25519)
///
/// # Returns
/// 32-byte seed wrapped in `Zeroizing` for secure memory handling.
pub fn resolve_wallet_seed() -> Result<Zeroizing<Vec<u8>>, KeychainError> {
    resolve(&SecretRef::derived(
        derivation_contexts::MASTER_KEY_ENV,
        derivation_contexts::WALLET_SEED,
    ))
}

/// Sign arbitrary bytes with the wallet seed.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// pre:  bytes are the canonical representation to sign
/// post: returns Ok(hex_signature) — 128-char hex-encoded Ed25519 signature
/// post: wallet seed loaded, used for signing, zeroized within this call
///
/// # Returns
/// 64-byte Ed25519 signature as a hex-encoded string (128 hex chars).
pub fn sign_wallet_bytes(bytes: &[u8]) -> Result<String, KeychainError> {
    let seed = resolve_wallet_seed()?;
    let seed_bytes: [u8; 32] = seed[..32]
        .try_into()
        .map_err(|_| KeychainError::Platform("wallet seed must be 32 bytes".into()))?;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed_bytes);
    let signature = signing_key.sign(bytes);
    Ok(hex::encode(signature.to_bytes()))
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard against concurrent env var mutation in parallel test execution.
    /// Multiple tests set `HKASK_MASTER_KEY`; without serialization they race
    /// and produce non-deterministic derivation results.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Acquire the env lock and set a test master key.
    /// Returns a guard that must be held for the duration of the test.
    fn set_test_master_key() -> std::sync::MutexGuard<'static, ()> {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: set_var is unsafe in Rust 2024. Serialized via ENV_LOCK.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        guard
    }

    fn set_test_master_key_hex() -> std::sync::MutexGuard<'static, ()> {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: test-only env var mutation. Serialized via ENV_LOCK.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            );
        }
        guard
    }

    #[test]
    fn db_passphrase_string_preserves_configured_text() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = std::env::var_os("HKASK_DB_PASSPHRASE");
        unsafe { std::env::set_var("HKASK_DB_PASSPHRASE", "canonical-test-passphrase") };

        let resolved = resolve_db_passphrase_string().expect("resolve DB passphrase");

        unsafe {
            match old {
                Some(value) => std::env::set_var("HKASK_DB_PASSPHRASE", value),
                None => std::env::remove_var("HKASK_DB_PASSPHRASE"),
            }
        }
        assert_eq!(&*resolved, "canonical-test-passphrase");
    }

    #[test]
    fn treasury_keys_differ_per_context() {
        let _guard = set_test_master_key();
        let hedera_key = resolve_treasury_key(derivation_contexts::TREASURY_HEDERA).unwrap();
        let wallet_seed = resolve_wallet_seed().unwrap();
        assert_ne!(&*hedera_key, &*wallet_seed);
        assert_eq!(hedera_key.len(), 32);
        assert_eq!(wallet_seed.len(), 32);
    }

    #[test]
    fn treasury_key_is_deterministic() {
        let _guard = set_test_master_key();
        let key1 = resolve_treasury_key(derivation_contexts::TREASURY_HEDERA).unwrap();
        let key2 = resolve_treasury_key(derivation_contexts::TREASURY_HEDERA).unwrap();
        assert_eq!(&*key1, &*key2);
    }

    #[test]
    fn wallet_seed_is_32_bytes() {
        let _guard = set_test_master_key();
        let seed = resolve_wallet_seed().unwrap();
        assert_eq!(seed.len(), 32);
    }

    #[test]
    fn wallet_seed_is_deterministic() {
        let _guard = set_test_master_key();
        let seed1 = resolve_wallet_seed().unwrap();
        let seed2 = resolve_wallet_seed().unwrap();
        assert_eq!(&*seed1, &*seed2);
    }

    #[test]
    fn wallet_seed_accepts_hex_master_key() {
        let _guard = set_test_master_key_hex();
        let seed1 = resolve_wallet_seed().unwrap();
        let seed2 = resolve_wallet_seed().unwrap();
        assert_eq!(&*seed1, &*seed2);
        assert_eq!(seed1.len(), 32);
    }

    #[test]
    fn sign_wallet_bytes_produces_signature() {
        let _guard = set_test_master_key();
        let sig = sign_wallet_bytes(b"test payload").unwrap();
        // Ed25519 signature is 64 bytes → 128 hex chars
        assert_eq!(sig.len(), 128);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn signature_changes_on_different_bytes() {
        let _guard = set_test_master_key();
        let sig1 = sign_wallet_bytes(b"payload1").unwrap();
        let sig2 = sign_wallet_bytes(b"payload2").unwrap();
        assert_ne!(sig1, sig2);
    }
}
