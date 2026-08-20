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
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Ed25519 signature — 64 bytes.
///
/// Newtype for Ed25519 signatures over manifest content. Used by the
/// skill marketplace signing layer: publishers sign manifests, the
/// collab server and client verify signatures before indexing or
/// installing. Conversion to/from `ed25519_dalek::Signature` lives in
/// `hkask-keystore`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ed25519Signature(pub [u8; 64]);

impl Ed25519Signature {
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}
