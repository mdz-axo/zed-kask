#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask Keystore — OS keychain, encryption, and master key derivation.
//!
//! All keychain reads/writes go through the `keyring` crate directly
//! (synchronous OS keychain I/O). In zed-kask, API keys for inference
//! providers are handled by zed's own `CredentialsProvider` through the
//! `LanguageModelRegistry` — the kask keystore only handles sovereignty
//! keys (db_passphrase).
//!
//! Inference keys (`OPENROUTER_API_KEY`, etc.) are
//! **never** read from this keystore. They are injected into MCP server
//! child processes as environment variables by the parent zed process
//! (via `kask_bridge::build_mcp_server_env`, which reads from zed's
//! `CredentialsProvider` keychain under `kask://credentials/<key>`).
//! The MCP servers' `resolve_credential` reads API keys from env vars only.
//! This crate's `service=hkask` keychain namespace is reserved for internal
//! keys (DB passphrase, swarm memory passphrase, master key, signing keys)
//! that predate the zed integration.

pub mod encryption;
pub mod error;
pub mod keychain;
pub mod keychain_keys;
pub mod master_key;
pub mod passphrase;

pub use encryption::derive_key;
pub use error::KeystoreError;
pub use keychain::{
    Keychain, KeychainError, resolve, resolve_db_passphrase_string,
    resolve_swarm_memory_passphrase_string,
};
pub use passphrase::generate_random_passphrase;
