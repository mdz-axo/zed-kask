//! Secrets port — abstraction over credential storage.
//!
//! hKask crates that need API keys or sovereignty secrets depend on this trait,
//! not on any concrete keystore. The zed-kask bridge crate implements it over
//! zed's `CredentialsProvider` (kask namespace: `kask://credentials/<service>`).
//! This keeps the dependency direction hKask → (port) ← zed-kask, never hKask → zed-kask.

use std::future::Future;
use std::pin::Pin;

/// Error reading or writing secrets.
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("secret not found: {0}")]
    NotFound(String),
    #[error("secret store error: {0}")]
    Store(String),
}

/// Pinned boxed future for dyn-compatibility.
pub type SecretsFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Port for reading and writing secrets (API keys, sovereignty keys).
///
/// Implementations:
/// - zed-kask bridge: over `CredentialsProvider` with `kask://credentials/<key>` URLs.
/// - hKask standalone (if ever needed): over the OS keychain directly.
pub trait SecretsPort: Send + Sync {
    /// Read a secret by key (e.g., "fmp", "eodhd", "db_passphrase").
    fn read<'a>(&'a self, key: &'a str) -> SecretsFuture<'a, Result<Option<String>, SecretsError>>;

    /// Write a secret by key.
    fn write<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
    ) -> SecretsFuture<'a, Result<(), SecretsError>>;

    /// Delete a secret by key.
    fn delete<'a>(&'a self, key: &'a str) -> SecretsFuture<'a, Result<(), SecretsError>>;
}
