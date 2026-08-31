//! OS keychain integration — unified `kask://credentials/*` namespace.
//!
//! All keychain entries live at `kask://credentials/<key>` with attributes
//! `url=kask://credentials/<key>`, `username=kask`, label `zed-github-account`.
//! This is the same schema zed's `LinuxPlatform::write_credentials` uses,
//! so `hkask-keystore` and zed's `CredentialsProvider` read and write the
//! same entries. One keychain, one namespace, one attribute schema.
//!
//! The legacy `service=hkask` namespace was fully removed. All entries in
//! that namespace are purged at startup via `purge_legacy_hkask_entries`.
//! No code reads from or writes to the old namespace — it is dead surface.
//! There is exactly one copy of each secret, in `kask://credentials/*`.

use crate::keychain_keys::KEY_DB_PASSPHRASE;
use crate::passphrase::DEFAULT_PASSPHRASE;
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
/// `oo7` uses `async-std` for I/O (`zbus/async-io`). Its reactor must be
/// driven by an `async-std` task executor — `futures::executor::block_on`
/// does NOT drive the `async-std` reactor and will deadlock on the first
/// I/O operation. We spawn a dedicated OS thread with an `async-std`
/// runtime, run the future to completion there, and return the result.
/// This is safe from any calling context (sync, tokio, GPUI background)
/// because the `async-std` reactor runs on the dedicated thread, not the
/// caller's thread.
fn block_on<F>(future: F) -> F::Output
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("hkask-keystore-keychain".to_string())
        .spawn(move || {
            let result = async_std::task::block_on(future);
            let _ = tx.send(result);
        })
        .expect("Failed to spawn keychain thread");
    rx.recv().expect("Keychain thread panicked")
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
        let key = key.to_string();
        let secret = secret.to_string();
        block_on(async move {
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
        let key = key.to_string();
        block_on(async move {
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
        let key = key.to_string();
        block_on(async move {
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

    // ── Generic URL-based access ─────────────────────────────────────
    //
    // These accept any URL string (e.g. `kask://credentials/exa` or
    // `https://openrouter.ai/api/v1`) and use the same oo7 pattern as the
    // key-based methods above. Used by `KeychainCredentialsProvider` so ALL
    // credential URLs route through the keystore's dedicated async-std
    // thread, not just `kask://credentials/*`.

    /// Store a secret at an arbitrary URL in the OS keychain.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  url is non-empty, secret is non-empty
    /// post: secret stored with label `zed-github-account`, attribute `url=<url>`
    pub fn store_by_url(
        &self,
        url: &str,
        username: &str,
        secret: &str,
    ) -> Result<(), KeychainError> {
        let url = url.to_string();
        let username = username.to_string();
        let secret = secret.to_string();
        block_on(async move {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            keyring
                .create_item(
                    KEYRING_LABEL,
                    &[("url", url.as_str()), ("username", username.as_str())],
                    secret.as_bytes(),
                    true,
                )
                .await?;
            Ok::<_, KeychainError>(())
        })
    }

    /// Retrieve a secret from an arbitrary URL in the OS keychain.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  url is non-empty
    /// post: returns Ok(secret) if stored, Err(NotFound) if not
    pub fn retrieve_by_url(&self, url: &str) -> Result<Zeroizing<String>, KeychainError> {
        let url = url.to_string();
        block_on(async move {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            let items = keyring.search_items(&[("url", url.as_str())]).await?;
            for item in items {
                if item.label().await.is_ok_and(|label| label == KEYRING_LABEL) {
                    item.unlock().await?;
                    let secret = item.secret().await?;
                    return Ok(Zeroizing::new(
                        String::from_utf8_lossy(&secret).into_owned(),
                    ));
                }
            }
            Err(KeychainError::NotFound(NotFound {
                entity_type: "secret".to_string(),
                id: format!("keychain entry not found at url={url}"),
            }))
        })
    }

    /// Delete a secret at an arbitrary URL from the OS keychain.
    ///
    /// expect: "My keys are generated, stored, and rotated under my sovereignty"
    /// pre:  url is non-empty
    /// post: secret removed (idempotent — no-op if absent)
    pub fn delete_by_url(&self, url: &str) -> Result<(), KeychainError> {
        let url = url.to_string();
        block_on(async move {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            let items = keyring.search_items(&[("url", url.as_str())]).await?;
            for item in items {
                if item.label().await.is_ok_and(|label| label == KEYRING_LABEL) {
                    item.delete().await?;
                    return Ok(());
                }
            }
            Ok::<_, KeychainError>(())
        })
    }

    /// Purge ALL entries from the old `service=hkask` namespace.
    ///
    /// The legacy namespace was replaced by the unified `kask://credentials/*`
    /// namespace. The old entries were copied (not moved) during the migration,
    /// leaving duplicate secrets in the keychain — a security liability.
    /// This function deletes every entry with `service=hkask` attribute,
    /// regardless of its key name.
    ///
    /// Idempotent — if no legacy entries exist, returns 0.
    ///
    /// Returns the count of deleted entries.
    pub fn purge_legacy_entries(&self) -> Result<usize, KeychainError> {
        block_on(async move {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            let items = keyring.search_items(&[("service", "hkask")]).await?;
            let mut deleted = 0;
            for item in items {
                let label = item.label().await.unwrap_or_default();
                match item.delete().await {
                    Ok(()) => {
                        tracing::info!(
                            target: "hkask.identity",
                            label = %label,
                            "Deleted legacy service=hkask keychain entry"
                        );
                        deleted += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "hkask.identity",
                            label = %label,
                            error = %e,
                            "Failed to delete legacy service=hkask keychain entry"
                        );
                    }
                }
            }
            Ok(deleted)
        })
    }
}

/// Resolve the database encryption passphrase.
///
/// Chain: env var → OS keychain (`kask://credentials/hkask_db_passphrase`).
/// An empty or whitespace-only env value is treated as unset — a
/// misconfiguration is not a credential, so the keychain tier runs.
/// No master-key derivation — the passphrase must be explicitly set via
/// env var or keychain to avoid accidentally encrypting the database with
/// a derived key the user didn't consent to.
///
/// post: returns Zeroizing<`Vec<u8>`> from env var or keychain
pub fn resolve_db_passphrase() -> Result<Zeroizing<Vec<u8>>, KeychainError> {
    // An empty or whitespace-only env value is a misconfiguration, not a
    // credential — treat it as unset so the keychain tier runs. (This guard
    // lived inline in the bridge's old provisioning chain; it moved here
    // when the chain was unified.)
    match resolve(&SecretRef::env("HKASK_DB_PASSPHRASE")) {
        Ok(value) if !value.iter().all(|byte| byte.is_ascii_whitespace()) => Ok(value),
        _ => resolve(&SecretRef::keychain(KEY_DB_PASSPHRASE)),
    }
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

/// Provision the DB passphrase — the one canonical chain (env → keychain
/// → first-run default), so no consumer re-implements resolution tiers.
///
/// `resolve_db_passphrase_string` covers env → keychain; this adds the
/// first-run tier: when no entry exists, store the fixed default
/// (`DEFAULT_PASSPHRASE`) and return it. The bridge's agent provisioning
/// and the MCP launch path both route through here — one chain keeps the
/// rotation-ordering invariant (rotate every DB before the keychain
/// write) anchored to a single resolution path.
pub fn provision_db_passphrase_string() -> Result<Zeroizing<String>, KeychainError> {
    match resolve_db_passphrase_string() {
        Ok(passphrase) => Ok(passphrase),
        Err(KeychainError::NotFound(_)) => {
            let word = DEFAULT_PASSPHRASE.to_string();
            Keychain.store_by_key(KEY_DB_PASSPHRASE, &word)?;
            info!(
                "Provisioned the DB passphrase with the first-run default and stored it \
                 in the keychain. Change it via the settings UI (Security page) or the \
                 HKASK_DB_PASSPHRASE env var."
            );
            Ok(Zeroizing::new(word))
        }
        Err(e) => Err(e),
    }
}

/// Migrate legacy `service=hkask` keychain entries to the unified
/// `kask://credentials/*` namespace.
///
/// Purge ALL legacy `service=hkask` keychain entries.
///
/// Convenience wrapper around `Keychain::purge_legacy_entries`.
/// Returns the count of deleted entries.
pub fn purge_legacy_hkask_entries() -> Result<usize, KeychainError> {
    Keychain.purge_legacy_entries()
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

// ── Integration tests (live keychain; run with -- --ignored) ──────────────────
//
// These tests exercise the real OS keychain via `oo7`, not a mock. They
// verify that:
//   1. `store_by_key` → `retrieve_by_key` round-trips a value
//   2. `delete_by_key` removes it (subsequent `retrieve_by_key` → NotFound)
//   3. `resolve_db_passphrase_string` finds an entry written by `store_by_key`
//      (proves the resolve path and the store path hit the same namespace)
//   4. `purge_legacy_entries` deletes old `service=hkask` entries (security cleanup)
//      unified `kask://credentials/*` namespace
//
// They use a sentinel key (`__hkask_test_round_trip__`) to avoid touching
// real credentials. `#[ignore]` keeps them out of `cargo test` by default;
// run with `cargo test -p hkask-keystore -- --ignored`.

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Sentinel key for round-trip tests. Never used by production code.
    const TEST_KEY: &str = "__hkask_test_round_trip__";
    const TEST_VALUE: &str = "test-value-not-a-real-secret-1234567890";

    fn cleanup() {
        let _ = Keychain.delete_by_key(TEST_KEY);
    }

    #[test]
    #[ignore]
    fn store_retrieve_delete_round_trips() {
        cleanup();
        let kc = Keychain;

        // Store
        kc.store_by_key(TEST_KEY, TEST_VALUE)
            .expect("store_by_key should succeed");

        // Retrieve — should return the same value
        let retrieved = kc
            .retrieve_by_key(TEST_KEY)
            .expect("retrieve_by_key should find the stored value");
        assert_eq!(
            retrieved.as_str(),
            TEST_VALUE,
            "retrieve_by_key must return the same value that store_by_key wrote"
        );

        // Delete
        kc.delete_by_key(TEST_KEY)
            .expect("delete_by_key should succeed");

        // Retrieve after delete — should be NotFound
        let result = kc.retrieve_by_key(TEST_KEY);
        assert!(
            matches!(result, Err(KeychainError::NotFound(_))),
            "retrieve_by_key after delete must return NotFound, got: {:?}",
            result
        );
    }

    #[test]
    #[ignore]
    fn resolve_finds_entry_written_by_store_by_key() {
        // Delete first — the migration test may have already written the
        // real passphrase to kask://credentials/hkask_db_passphrase.
        let _ = Keychain.delete_by_key(KEY_DB_PASSPHRASE);
        let kc = Keychain;

        // Write via the store path
        kc.store_by_key(KEY_DB_PASSPHRASE, TEST_VALUE)
            .expect("store_by_key for DB passphrase should succeed");

        // Read via the resolve path — this proves both paths hit the
        // same namespace and the same entry.
        let resolved = resolve_db_passphrase_string()
            .expect("resolve_db_passphrase_string should find the entry written by store_by_key");
        assert_eq!(
            resolved.as_str(),
            TEST_VALUE,
            "resolve_db_passphrase_string must return the same value that store_by_key wrote"
        );

        // Cleanup: delete the test value. The next zed-kask startup will
        // re-provision via the migration or the default "allostery".
        let _ = kc.delete_by_key(KEY_DB_PASSPHRASE);
    }

    /// The provisioning chain: absent entry → default stored + returned;
    /// existing entry → returned unchanged. Snapshots and restores the
    /// operator's real entry — a rotated passphrase must survive the test —
    /// and restores BEFORE asserting so a failed assert cannot leave the
    /// keychain without the real entry (every DB would be unopenable at
    /// next startup).
    #[test]
    #[ignore]
    fn provision_stores_default_when_absent_and_is_stable_after() {
        if std::env::var_os("HKASK_DB_PASSPHRASE").is_some() {
            eprintln!(
                "skipping: HKASK_DB_PASSPHRASE is set — the env tier would mask the keychain tiers"
            );
            return;
        }
        let original = Keychain.retrieve_by_key(KEY_DB_PASSPHRASE).ok();
        let _ = Keychain.delete_by_key(KEY_DB_PASSPHRASE);

        let first = provision_db_passphrase_string();
        let second = provision_db_passphrase_string();

        match original.as_ref() {
            Some(value) => {
                Keychain
                    .store_by_key(KEY_DB_PASSPHRASE, value.as_str())
                    .expect("restore the operator's real passphrase entry");
            }
            None => {
                let _ = Keychain.delete_by_key(KEY_DB_PASSPHRASE);
            }
        }

        let first = first.expect("provision with no entry must store the default and return it");
        assert_eq!(
            first.as_str(),
            DEFAULT_PASSPHRASE,
            "first-run provisioning must use the fixed default"
        );
        let second = second.expect("second provision must resolve the stored entry");
        assert_eq!(
            second.as_str(),
            DEFAULT_PASSPHRASE,
            "provisioning must be idempotent — the stored entry wins on the second call"
        );
    }

    /// The empty-env guard: `HKASK_DB_PASSPHRASE=""` must fall through
    /// to the keychain tier, never resolve to an empty passphrase
    /// (SQLCipher would key every DB on an empty string). Runs by default
    /// — no keychain writes; a machine with no entry resolves Err, which
    /// the assert tolerates (the pinned property is "never Ok-and-empty").
    #[test]
    fn empty_env_passphrase_falls_through_to_keychain() {
        let prev = std::env::var("HKASK_DB_PASSPHRASE").ok();
        unsafe { std::env::set_var("HKASK_DB_PASSPHRASE", "") };
        let resolved = resolve_db_passphrase();
        match prev {
            Some(value) => unsafe { std::env::set_var("HKASK_DB_PASSPHRASE", value) },
            None => unsafe { std::env::remove_var("HKASK_DB_PASSPHRASE") },
        }
        assert!(
            !matches!(&resolved, Ok(value) if value.iter().all(|byte| byte.is_ascii_whitespace())),
            "an empty env value must never resolve to an empty passphrase — it must \
             fall through to the keychain tier (entry found, or Err when none exists)"
        );
    }

    #[test]
    #[ignore]
    fn purge_legacy_entries_deletes_old_namespace() {
        // This test verifies that `purge_legacy_entries` deletes ALL entries
        // with the `service=hkask` attribute, regardless of key name.
        // Run manually after confirming legacy entries exist:
        //   secret-tool search --all service hkask
        let deleted =
            purge_legacy_hkask_entries().expect("purge_legacy_hkask_entries should not error");
        assert!(
            deleted > 0,
            "Expected at least one legacy entry to be deleted"
        );
        // Verify no legacy entries remain
        let second_pass = purge_legacy_hkask_entries().expect("second purge should succeed");
        assert_eq!(
            second_pass, 0,
            "No legacy entries should remain after purge"
        );
    }

    #[test]
    #[ignore]
    fn retrieve_missing_key_returns_not_found() {
        let result = Keychain.retrieve_by_key("__hkask_definitely_does_not_exist__");
        assert!(
            matches!(result, Err(KeychainError::NotFound(_))),
            "retrieve_by_key for a missing key must return NotFound, got: {:?}",
            result
        );
    }
}
