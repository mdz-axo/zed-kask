#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask Keystore — OS keychain access and passphrase defaults.
//!
//! All keychain reads/writes go through `oo7::Keyring` directly.
//! URL operations offer sync wrappers and async-std-spawned async methods;
//! key-based operations remain synchronous. Data-service and passphrase
//! entries use `kask://credentials/<key>`; inference-provider entries use
//! their provider API URL. Both use zed's `LinuxPlatform::write_credentials`
//! attribute schema in the same OS keychain.
//!
//! The MCP servers' `resolve_credential` reads API keys from env vars only
//! (injected by `build_mcp_server_env`, which reads the canonical keychain URLs).
//! The DB passphrase is read by this crate via `resolve_db_passphrase_string`,
//! which also hits `kask://credentials/*`. There is ONE passphrase for all
//! SQLCipher databases — the swarm memory DB uses the same one.

pub mod error;
pub mod keychain;
pub mod keychain_keys;
pub mod passphrase;

pub use error::KeystoreError;
pub use keychain::{
    Keychain, KeychainError, provision_db_passphrase_string, purge_legacy_hkask_entries,
    purge_obsolete_runpod_s3_credentials, resolve, resolve_db_passphrase_string,
};
pub use passphrase::DEFAULT_PASSPHRASE;
