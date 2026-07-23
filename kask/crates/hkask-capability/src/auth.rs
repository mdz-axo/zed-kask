//! Authentication context and signing key derivation.

use ed25519_dalek::SigningKey;
use hkask_types::WebID;
use sha2::{Digest, Sha256};

use super::token_types::DelegationToken;

/// Verified authentication context — caller's identity and capability token.
/// Both API (middleware verification) and CLI (keystore resolution) produce this type.
///
/// `token` is `Some` for capability-token auth; `None` for session-cookie auth (DEP-020).
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub token: Option<DelegationToken>,
    pub webid: WebID,
}

impl AuthContext {
    /// Create an AuthContext from a session (no DelegationToken).
    /// expect: "System types preserve semantic identity and are provenance-aware"
    pub fn from_session(webid: WebID) -> Self {
        Self { token: None, webid }
    }

    /// Create an AuthContext from a verified DelegationToken.
    pub fn from_token(token: DelegationToken, webid: WebID) -> Self {
        Self {
            token: Some(token),
            webid,
        }
    }
}

/// Derive an Ed25519 signing key from arbitrary secret bytes.
///
/// \[NORMATIVE\] Hashes the input with SHA-256 to produce a 32-byte seed,
/// then constructs a `SigningKey`. This allows existing HMAC-secret-based
/// callers to migrate to Ed25519 without changing their secret management (P4 — Clear Boundaries).
pub fn derive_signing_key(secret: &[u8]) -> SigningKey {
    let seed: [u8; 32] = Sha256::digest(secret).into();
    SigningKey::from_bytes(&seed)
}
