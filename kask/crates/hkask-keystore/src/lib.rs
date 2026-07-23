#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Keystore — OS keychain, encryption, and master key derivation

pub mod encryption;
pub mod error;
pub mod keychain;
pub mod master_key;
pub mod version_file;

pub use encryption::derive_key;
pub use error::KeystoreError;
pub use keychain::{Keychain, KeychainError, resolve};
pub use master_key::derive_all_internal_secrets_with_version;
