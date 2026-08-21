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
//! The MCP servers' `InferenceConfig::from_env()` reads only the env var.
//! This closes the two-namespace split that previously caused silent "API
//! key not configured" errors when the `hkask` keychain fallback read a
//! namespace that was always empty in zed-kask.

pub mod encryption;
pub mod error;
pub mod keychain;
pub mod keychain_keys;
pub mod master_key;

pub use encryption::derive_key;
pub use error::KeystoreError;
pub use keychain::{Keychain, KeychainError, resolve};
