//! Secret derivation — Loop 6 (Cybernetics): key management
//!
//! Secret derivation contexts are used by the Cybernetics Access Guard (6.1)
//! and the keystore for capability token signing and verification.

use serde::{Deserialize, Serialize};

/// Loop: Cybernetics
/// Declarative reference to a secret's source.
///
/// Each variant specifies how to resolve a secret value at runtime.
/// The resolution priority (in `hkask_keystore::resolve`) is:
///
/// 1. `Env` — read from an environment variable
/// 2. `Keychain` — read from the OS keychain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecretRef {
    /// Read secret from an environment variable.
    Env(String),

    /// Read secret from the OS keychain by service name.
    Keychain(String),
}

impl SecretRef {
    /// Reference a secret stored in an environment variable.
    pub fn env(name: &str) -> Self {
        Self::Env(name.to_string())
    }

    /// Reference a secret stored in the OS keychain.
    pub fn keychain(service: &str) -> Self {
        Self::Keychain(service.to_string())
    }
}
