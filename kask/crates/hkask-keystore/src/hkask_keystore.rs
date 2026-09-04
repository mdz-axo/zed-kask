#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask Keystore — OS keychain access and passphrase defaults.
//!
//! All keychain reads/writes go through `oo7::Keyring` directly
//! (synchronous wrappers around async OS keychain I/O). Every entry lives
//! at `kask://credentials/<key>` with the same attribute schema zed's
//! `LinuxPlatform::write_credentials` uses — one keychain, one namespace.
//!
//! In zed-kask, API keys for inference providers are handled by zed's own
//! `CredentialsProvider` through the `LanguageModelRegistry` — both paths
//! hit the same `kask://credentials/*` entries.
//!
//! The MCP servers' `resolve_credential` reads API keys from env vars only
//! (injected by `build_mcp_server_env`, which reads from the same namespace).
//! The DB passphrase is read by this crate via `resolve_db_passphrase_string`,
//! which also hits `kask://credentials/*`. There is ONE passphrase for all
//! SQLCipher databases — the swarm memory DB uses the same one.

pub mod error;
pub mod keychain;
pub mod keychain_keys;
pub mod passphrase;

pub use error::KeystoreError;
pub use keychain::{
    Keychain, KeychainError, provision_db_passphrase_string, purge_legacy_hkask_entries, resolve,
    resolve_db_passphrase_string,
};
pub use passphrase::DEFAULT_PASSPHRASE;
