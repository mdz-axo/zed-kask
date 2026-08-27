//! OS keychain integration — unified `kask://credentials/*` namespace.
//!
//! All keychain entries live at `kask://credentials/<key>` with attributes
//! `url=kask://credentials/<key>`, `username=kask`, label `zed-github-account`.
//! This is the same schema zed's `LinuxPlatform::write_credentials` uses,
//! so `hkask-keystore` and zed's `CredentialsProvider` read and write the
//! same entries. One keychain, one namespace, one attribute schema.
//!
//! Previous architecture had two namespaces: `service=hkask` (via the
//! `keyring` crate) and `kask://credentials/*` (via zed's `oo7`-backed
//! `CredentialsProvider`). The `service=hkask` namespace was never updated
//! after passphrase rotation (the settings UI wrote to `kask://credentials/*`
//! but the resolve path read from `service=hkask`), causing stale-passphrase
//! failures on direct-launched MCP servers. Unifying eliminates this class
//! of bug — there is only one copy of each secret.

use crate::keychain_keys::{KEY_DB_PASSPHRASE, KEY_SWARM_MEMORY_PASSPHRASE};
use hkask_types::NotFound;
use hkask_types::secret::SecretRef;
use thiserror::Error;
use tracing::info;
use zeroize::Zeroizing;

/// The label zed's `LinuxPlatform::write_credentials` uses for all keychain
/// items. We match it so our entries are indistinguishable from zed's.
const KEYRING_LABEL: &str = "zed-github-account";

/// The URL prefix for kask-namespaced credentials. Matches
/// `kask_bridge::credentials::KASK_CREDENTIAL_NAMESPACE`.
const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";

/// Build the credential URL for a key.
fn credential_url(key: &str) -> String {
    format!("{KASK_CREDENTIAL_NAMESPACE}/{key}")
}

/// Block on an async future from sync context.
///
/// `oo7` uses `async-io` (epoll-based) for I/O, which works with any
/// executor. `futures::executor::block_on` drives the future to completion
/// on the current thread. This is safe because:
/// - MCP servers are separate processes with no async runtime.
/// - `provision_agent` runs on background threads via `cx.background_spawn`.
/// - Settings UI rotation runs in spawned tasks.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    futures::executor::block_on(future)
}

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

impl From<oo7::Error> for KeychainError {
    fn from(err: oo7::Error) -> Self {
        // oo7 does not have a dedicated NotFound variant — empty search
        // results return an empty Vec, not an error. Any error here is a
        // platform-level failure (D-Bus, I/O, etc.).
        KeychainError::Platform(err.to_string())
    }
}

/// Report from a legacy-namespace migration pass.
#[derive(Debug, Default)]
pub struct MigrationReport {
    /// Keys that were copied from the old `service=hkask` namespace to
    /// `kask://credentials/*`.
    pub migrated: Vec<String>,
    /// Keys that already existed in the new namespace (skipped).
    pub skipped: Vec<String>,
}

/// Keychain — unified `kask://credentials/*` namespace.
///
/// All reads and writes go through `oo7::Keyring` with `url`-attributed
/// entries matching zed's `LinuxPlatform::write_credentials` schema.
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// inv: secrets are stored in OS keychain, never in plaintext files
pub struct Keychain;

impl Keychain {
    /// Store a secret in the OS keychain at `kask://credentials/<key>`.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  key is non-empty, secret is non-empty
    /// post: secret stored at `kask://credentials/<key>` with label `zed-github-account`
    pub fn store_by_key(&self, key: &str, secret: &str) -> Result<(), KeychainError> {
        let url = credential_url(key);
        block_on(async {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            keyring
                .create_item(
                    KEYRING_LABEL,
                    &[("url", url.as_str()), ("username", "kask")],
                    secret.as_bytes(),
                    true,
                )
                .await?;
            info!(target: "reg.keystore", operation = "store_by_key", key = %key, "REG");
            Ok::<_, KeychainError>(())
        })
    }

    /// Retrieve a secret from the OS keychain at `kask://credentials/<key>`.
    ///
    /// Returns `Zeroizing<String>` so the secret is wiped when the caller drops
    /// it.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  key is non-empty
    /// post: returns Ok(secret) if stored, Err(NotFound) if not
    pub fn retrieve_by_key(&self, key: &str) -> Result<Zeroizing<String>, KeychainError> {
        let url = credential_url(key);
        block_on(async {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            let items = keyring.search_items(&[("url", url.as_str())]).await?;
            for item in items {
                if item.label().await.is_ok_and(|label| label == KEYRING_LABEL) {
                    item.unlock().await?;
                    let secret = item.secret().await?;
                    let secret_str = String::from_utf8_lossy(&secret).into_owned();
                    info!(
                        target: "reg.keystore",
                        operation = "retrieve_by_key",
                        key = %key,
                        "REG"
                    );
                    return Ok(Zeroizing::new(secret_str));
                }
            }
            Err(KeychainError::NotFound(NotFound {
                entity_type: "secret".to_string(),
                id: format!("keychain entry not found at url={url}"),
            }))
        })
    }

    /// Delete a secret from the OS keychain at `kask://credentials/<key>`.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  key is non-empty
    /// post: secret removed from OS keychain (idempotent — no-op if absent)
    pub fn delete_by_key(&self, key: &str) -> Result<(), KeychainError> {
        let url = credential_url(key);
        block_on(async {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            let items = keyring.search_items(&[("url", url.as_str())]).await?;
            for item in items {
                if item.label().await.is_ok_and(|label| label == KEYRING_LABEL) {
                    item.delete().await?;
                    info!(
                        target: "reg.keystore",
                        operation = "delete_by_key",
                        key = %key,
                        "REG"
                    );
                    return Ok(());
                }
            }
            Ok::<_, KeychainError>(())
        })
    }

    /// Migrate entries from the old `service=hkask` namespace to
    /// `kask://credentials/*`.
    ///
    /// For each key in the mapping, if the new entry doesn't exist but the
    /// old one does, copies the value. Idempotent — skips keys that already
    /// exist in the new namespace.
    ///
    /// This is a one-time migration path for existing installations. New
    /// installations never have old-namespace entries.
    pub fn migrate_legacy_entries(&self) -> Result<MigrationReport, KeychainError> {
        // Old hyphenated key → new underscored credential key.
        let mapping = [
            ("hkask-db-passphrase", KEY_DB_PASSPHRASE),
            ("hkask-swarm-memory-passphrase", KEY_SWARM_MEMORY_PASSPHRASE),
        ];

        let mut report = MigrationReport::default();

        for (old_key, new_key) in mapping {
            // Skip if the new entry already exists.
            if self.retrieve_by_key(new_key).is_ok() {
                report.skipped.push(new_key.to_string());
                continue;
            }

            // Search for the old entry by its `service=hkask` + `username` attributes.
            let old_secret = block_on(async {
                let keyring = oo7::Keyring::new().await?;
                keyring.unlock().await?;
                let items = keyring
                    .search_items(&[("service", "hkask"), ("username", old_key)])
                    .await?;
                if let Some(item) = items.into_iter().next() {
                    let secret = item.secret().await?;
                    return Ok(Some(secret));
                }
                Ok::<_, KeychainError>(None)
            })?;

            if let Some(secret_bytes) = old_secret {
                let secret_str = String::from_utf8_lossy(&secret_bytes);
                self.store_by_key(new_key, &secret_str)?;
                report.migrated.push(new_key.to_string());
                tracing::info!(
                    target: "hkask.identity",
                    old_key = %old_key,
                    new_key = %new_key,
                    "Migrated legacy keychain entry to unified kask://credentials/* namespace"
                );
            }
        }

        Ok(report)
    }
}

/// Resolve the database encryption passphrase.
///
/// Chain: env var → OS keychain (`kask://credentials/hkask_db_passphrase`).
/// No master-key derivation — the passphrase must be explicitly set via
/// env var or keychain to avoid accidentally encrypting the database with
/// a derived key the user didn't consent to.
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
/// `kask://credentials/hkask_swarm_memory_passphrase`. Mirrors
/// [`resolve_db_passphrase_string`] but for the swarm memory store (a separate
/// SQLCipher DB). Used by the provisioning path (`provision_swarm_memory_passphrase`)
/// to read an existing passphrase before deciding to generate one.
pub fn resolve_swarm_memory_passphrase_string() -> Result<Zeroizing<String>, KeychainError> {
    let bytes = resolve(&SecretRef::env("HKASK_SWARM_MEMORY_PASSPHRASE"))
        .or_else(|_| resolve(&SecretRef::keychain(KEY_SWARM_MEMORY_PASSPHRASE)))?;
    let passphrase = std::str::from_utf8(&bytes).map_err(|e| {
        KeychainError::Platform(format!("swarm memory passphrase is not valid UTF-8: {e}"))
    })?;
    Ok(Zeroizing::new(passphrase.to_string()))
}

/// Migrate legacy `service=hkask` keychain entries to the unified
/// `kask://credentials/*` namespace.
///
/// Convenience wrapper around `Keychain::migrate_legacy_entries`.
pub fn migrate_legacy_hkask_entries() -> Result<MigrationReport, KeychainError> {
    Keychain.migrate_legacy_entries()
}

/// Resolve a SecretRef to actual secret bytes.
///
/// Resolution priority:
/// 1. `Env` — read from environment variable
/// 2. `Keychain` — read from OS keychain at `kask://credentials/<key>`
/// 3. `Derived` — look up master key (env → keychain), then HKDF-SHA256 derive sub-key
/// 4. `Generated` — random bytes (⚠️ not reproducible; debug builds only)
///
/// expect: "My keys are generated, stored, and rotated under my sovereignty"
/// pre:  secret_ref is a valid SecretRef variant
/// post: all returned secrets wrapped in Zeroizing
pub fn resolve(secret_ref: &SecretRef) -> Result<Zeroizing<Vec<u8>>, KeychainError> {
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
            let keychain = Keychain;
            let secret = keychain.retrieve_by_key(key_name)?;
            info!(target: "reg.keystore", operation = "resolve_keychain", key_name = %key_name, "REG");
            Ok(Zeroizing::new(secret.as_bytes().to_vec()))
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
            tracing::warn!(target: "reg.keystore", operation = "resolve_generated", length = *length, "REG");
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
