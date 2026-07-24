#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Keystore — OS keychain, encryption, and master key derivation
//!
//! D5: The keychain storage backend is injectable via [`set_secrets_port`].
//! When a [`SecretsPort`] is injected (by the zed-kask composition root),
//! keychain reads/writes go through it (backed by zed's `CredentialsProvider`
//! in the `kask://credentials/<key>` namespace). When no port is injected
//! (standalone MCP server child processes), the `keyring` crate is used
//! directly as a fallback.

pub mod encryption;
pub mod error;
pub mod keychain;
pub mod master_key;
pub mod version_file;

use std::sync::{Arc, OnceLock};

use hkask_types::SecretsPort;

pub use encryption::derive_key;
pub use error::KeystoreError;
pub use keychain::{Keychain, KeychainError, resolve};
pub use master_key::derive_all_internal_secrets_with_version;

/// Global secrets port — injected by the zed-kask composition root (D5).
///
/// When set, keychain reads/writes go through this port (backed by zed's
/// `CredentialsProvider` in the `kask://credentials/<key>` namespace).
/// When unset (standalone MCP server child processes), the `keyring` crate
/// is used directly as a fallback.
static SECRETS_PORT: OnceLock<Option<Arc<dyn SecretsPort>>> = OnceLock::new();

/// Inject the global [`SecretsPort`] for keychain access (D5).
///
/// Called by the zed-kask composition root (`crates/zed/src/main.rs`) after
/// constructing the `CredentialsSecretsPort` (from `kask_bridge`). Must be
/// called before any `resolve_a2a_secret()` or `resolve()` call.
///
/// Passing `None` resets to the `keyring` fallback (used by MCP server child
/// processes that don't have access to the GPUI `CredentialsProvider`).
pub fn set_secrets_port(port: Option<Arc<dyn SecretsPort>>) {
    let _ = SECRETS_PORT.set(port);
}

/// Get the injected global [`SecretsPort`], if any (D5).
///
/// Returns `None` when no port has been injected (standalone MCP server use).
pub fn secrets_port() -> Option<&'static Arc<dyn SecretsPort>> {
    SECRETS_PORT.get().and_then(|opt| opt.as_ref())
}
