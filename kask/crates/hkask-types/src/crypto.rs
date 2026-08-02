//! Minimal crypto types — shared across capability, wallet, and skill-signing domains.
//!
//! Only contains value types with zero crypto library dependencies.
//! Conversion to/from `ed25519_dalek` types lives in downstream crates.

use serde::{Deserialize, Serialize};

/// Ed25519 public key — 32 bytes.
///
/// Newtype to prevent accidental mixing with other 32-byte values
/// (hashes, secrets, UUIDs). Conversion to/from `ed25519_dalek::VerifyingKey`
/// lives in `hkask-keystore` where the crypto dependency exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ed25519PublicKey(pub [u8; 32]);

impl Ed25519PublicKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Ed25519PublicKey(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Display for Ed25519PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl std::str::FromStr for Ed25519PublicKey {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Ed25519PublicKey(arr))
    }
}

/// Ed25519 signature — 64 bytes.
///
/// Newtype for Ed25519 signatures over manifest content. Used by the
/// skill marketplace signing layer: publishers sign manifests, the
/// collab server and client verify signatures before indexing or
/// installing. Conversion to/from `ed25519_dalek::Signature` lives in
/// `hkask-keystore`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ed25519Signature(pub [u8; 64]);

impl Ed25519Signature {
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Ed25519Signature(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl std::fmt::Display for Ed25519Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl std::str::FromStr for Ed25519Signature {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 64 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&bytes);
        Ok(Ed25519Signature(arr))
    }
}
