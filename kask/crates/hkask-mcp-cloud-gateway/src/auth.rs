//! Token verification middleware for cloud gateway requests.
//!
//! Verifies Ed25519-signed `DelegationToken`s against the hKask capability
//! system. Every request must carry a valid, non-expired token whose
//! `delegated_to` field matches the mTLS client certificate Common Name.
//!
//! # Identity Binding
//!
//! mTLS certificates carry human-readable Common Names (e.g., "alice").
//! hKask WebIDs are UUIDs derived from persona bytes via v5 UUID.
//! The gateway derives the expected WebID from the cert CN using
//! `WebID::from_persona(cn.as_bytes())` and compares it against
//! the token's `delegated_to` field.

use hkask_capability::DelegationToken;
use hkask_types::WebID;
use thiserror::Error;

/// Errors that can occur during token verification.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Missing DelegationToken in request")]
    MissingToken,

    #[error("Token delegated_to '{token_to}' does not match mTLS CN '{cert_cn}'")]
    IdentityMismatch { token_to: String, cert_cn: String },

    #[error("Token resource_id '{token_resource}' does not match requested tool '{tool}'")]
    ToolMismatch {
        token_resource: String,
        tool: String,
    },

    #[error("Token signature verification failed")]
    InvalidSignature,

    #[error("Token expired at {0}")]
    Expired(String),
}

/// Result of token verification — the verified token identity.
pub struct VerifiedIdentity {
    pub webid: String,
    pub token_id: String,
}

/// Verify a DelegationToken for a tool call.
///
/// Identity binding: derives the expected WebID from the cert CN via
/// `WebID::from_persona(cert_cn.as_bytes())` and compares it to
/// `token.delegated_to`.
pub fn verify_cloud_request(
    token: &DelegationToken,
    cert_cn: &str,
    tool: &str,
) -> Result<VerifiedIdentity, AuthError> {
    // Gate 1: Identity binding — derive WebID from CN, compare to token holder
    let expected_webid = WebID::from_persona(cert_cn.as_bytes());
    if token.delegated_to != expected_webid {
        return Err(AuthError::IdentityMismatch {
            token_to: token.delegated_to.to_string(),
            cert_cn: cert_cn.to_string(),
        });
    }

    // Gate 2: Tool binding — the token must authorize this specific tool
    if token.resource_id != tool {
        return Err(AuthError::ToolMismatch {
            token_resource: token.resource_id.clone(),
            tool: tool.to_string(),
        });
    }

    // Gate 3: Ed25519 signature verification
    if !token.verify() {
        return Err(AuthError::InvalidSignature);
    }

    // Gate 4: Expiry check
    if let Some(expires_at) = token.expires_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        if now > expires_at {
            return Err(AuthError::Expired(expires_at.to_string()));
        }
    }

    Ok(VerifiedIdentity {
        webid: token.delegated_to.to_string(),
        token_id: token.id.clone(),
    })
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_capability::auth::derive_signing_key;
    use hkask_capability::{DelegationAction, DelegationResource};

    /// Create a test token where delegated_to matches the persona-derived WebID from `cn`.
    fn test_token(cn: &str, tool: &str) -> DelegationToken {
        let sk = derive_signing_key(b"test-gateway-secret");
        let webid = WebID::from_persona(cn.as_bytes());
        DelegationToken::new(
            DelegationResource::Tool,
            tool.to_string(),
            DelegationAction::Read,
            WebID::from_persona(b"issuer"),
            webid,
            &sk,
        )
    }

    #[test]
    fn verify_matching_identity_and_tool_succeeds() {
        let token = test_token("alice", "curator_health");
        let result = verify_cloud_request(&token, "alice", "curator_health");
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn verify_identity_mismatch_fails() {
        let token = test_token("alice", "curator_health");
        let result = verify_cloud_request(&token, "bob", "curator_health");
        assert!(
            matches!(result, Err(AuthError::IdentityMismatch { .. })),
            "expected IdentityMismatch, got {:?}",
            result.err()
        );
    }

    #[test]
    fn verify_tool_mismatch_fails() {
        let token = test_token("alice", "curator_health");
        let result = verify_cloud_request(&token, "alice", "curator_escalations");
        assert!(
            matches!(result, Err(AuthError::ToolMismatch { .. })),
            "expected ToolMismatch, got {:?}",
            result.err()
        );
    }

    #[test]
    fn verify_wrong_tool_denied_even_with_matching_identity() {
        let token = test_token("alice", "curator:health");
        let result = verify_cloud_request(&token, "alice", "curator_escalations");
        assert!(result.is_err());
    }
}
