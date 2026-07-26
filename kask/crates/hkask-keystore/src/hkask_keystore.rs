#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Keystore — OS keychain, encryption, and master key derivation.
//!
//! All keychain reads/writes go through the `keyring` crate directly
//! (synchronous OS keychain I/O). In zed-kask, API keys for inference
//! providers are handled by zed's own `CredentialsProvider` through the
//! `LanguageModelRegistry` — the kask keystore only handles sovereignty
//! keys (a2a_secret, db_passphrase, ocap_secret).

pub mod encryption;
pub mod error;
pub mod keychain;
pub mod master_key;
pub mod version_file;

pub use encryption::derive_key;
pub use error::KeystoreError;
pub use keychain::{Keychain, KeychainError, resolve};
pub use master_key::derive_all_internal_secrets_with_version;
