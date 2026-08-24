//! OS keychain integration

use crate::keychain_keys::{KEY_DB_PASSPHRASE, KEY_SWARM_MEMORY_PASSPHRASE};
use hkask_types::NotFound;
use hkask_types::WebID;
use hkask_types::secret::SecretRef;
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
    /// Returns `Zeroizing<String>` so the secret is wiped when the caller drops
    /// it. A bare `String` here left retrieved secrets in freed heap memory while
    /// the sibling `resolve()` correctly zeroized — two accessor families with
    /// different hygiene invited callers onto the unprotected one (RR-0063).
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  webid is a valid WebID
    /// post: returns Ok(secret) if stored, Err(NotFound) if not
    pub fn retrieve(&self, webid: &WebID) -> Result<Zeroizing<String>, KeychainError> {
        let entry = Entry::new(&self.service_name, &webid.as_uuid().to_string())
            .map_err(|e| KeychainError::Platform(e.to_string()))?;

        let result = entry.get_password().map_err(KeychainError::from)?;
        info!(target: "reg.keystore", operation = "retrieve", "REG");
        Ok(Zeroizing::new(result))
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
    /// Returns `Zeroizing<String>` — see [`Self::retrieve`] (RR-0063).
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  key is non-empty
    /// post: returns Ok(secret) if stored, Err(NotFound) if not
    ///
    /// Wraps the keychain access in `catch_unwind` to guard against
    /// concurrent D-Bus panics (libdbus can SIGABRT when multiple processes
    /// hit the OS keyring simultaneously). A panic here would kill the MCP
    /// server process; the guard converts it to an `Err` so the caller can
    /// fall back to env var resolution.
    pub fn retrieve_by_key(&self, key: &str) -> Result<Zeroizing<String>, KeychainError> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let entry = Entry::new(&self.service_name, key)
                .map_err(|e| KeychainError::Platform(e.to_string()))?;
            let secret = entry.get_password().map_err(KeychainError::from)?;
            info!(target: "reg.keystore", operation = "retrieve_by_key", "REG");
            Ok::<_, KeychainError>(Zeroizing::new(secret))
        }));
        match result {
            Ok(inner) => inner,
            Err(_) => {
                tracing::warn!(
                    target: "reg.keystore",
                    key = %key,
                    "Keychain access panicked (likely concurrent D-Bus access) — returning NotFound"
                );
                Err(KeychainError::Platform(
                    "Keychain access panicked — concurrent D-Bus access may have triggered C-level abort".into(),
                ))
            }
        }
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

/// Resolve the database encryption passphrase.
///
/// Chain: env var → OS keychain. No master-key derivation — the passphrase
/// must be explicitly set via env var or keychain to avoid accidentally
/// encrypting the database with a derived key the user didn't consent to.
///
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
    // Validate in place and copy only on success. `String::from_utf8(bytes.to_vec())`
    // moved the passphrase into a plain `Vec` that escaped `Zeroizing`; on the error
    // path the resulting `FromUtf8Error` then OWNED those bytes and dropped them
    // unwiped (RR-0063). `from_utf8` on a borrowed slice cannot take ownership, so
    // the failure path leaves nothing behind.
    let passphrase = std::str::from_utf8(&bytes)
        .map_err(|e| KeychainError::Platform(format!("DB passphrase is not valid UTF-8: {e}")))?;
    Ok(Zeroizing::new(passphrase.to_string()))
}

/// Resolve the swarm memory SQLCipher passphrase as text.
///
/// Chain: env var `HKASK_SWARM_MEMORY_PASSPHRASE` → OS keychain
/// `hkask-swarm-memory-passphrase`. Mirrors [`resolve_db_passphrase_string`]
/// but for the swarm memory store (a separate SQLCipher DB). Used by the
/// provisioning path (`provision_swarm_memory_passphrase`) to read an existing
/// passphrase before deciding to generate one.
pub fn resolve_swarm_memory_passphrase_string() -> Result<Zeroizing<String>, KeychainError> {
    let bytes = resolve(&SecretRef::env("HKASK_SWARM_MEMORY_PASSPHRASE"))
        .or_else(|_| resolve(&SecretRef::keychain(KEY_SWARM_MEMORY_PASSPHRASE)))?;
    let passphrase = std::str::from_utf8(&bytes).map_err(|e| {
        KeychainError::Platform(format!("swarm memory passphrase is not valid UTF-8: {e}"))
    })?;
    Ok(Zeroizing::new(passphrase.to_string()))
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
                     set {} or ensure the zed-kask composition root has provisioned the keystore",
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

// ── Tests ──────────────────────────────────────────────────────────────────────
