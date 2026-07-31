//! Verification functions for delegation tokens.
//!
//! Read/write access guard functions for delegation tokens.

use crate::{CapabilityError, DelegationToken};

/// Require write-level access from a delegation token.
///
/// Returns an error string if the token only grants read access.
/// Consolidates the repeated `if token.action == DelegationAction::Read` guard
/// that appeared in `memory_loop_adapter.rs` (4 occurrences) and `pod/context.rs`.
///
/// # Arguments
/// * `token` — The delegation token to check.
/// * `store_type` — Human-readable name of the store being accessed ("episodic" or "semantic").
///   Used in the error message for traceability.
///
/// # Returns
/// * `Ok(())` — Token grants write access.
/// * `Err(String)` — Token is read-only; the error message explains which store was denied.
///
/// expect: "System types preserve semantic identity and are provenance-aware"
/// pre:  token is any [`DelegationToken`]; store_type is any non-empty &str
/// post: returns `Ok(())` if `token.allows_write()` is true;
///       returns `Err("read-only token cannot write to {store_type} storage")` otherwise
pub fn require_write_access(
    token: &DelegationToken,
    store_type: &str,
) -> Result<(), CapabilityError> {
    if token.allows_write() {
        Ok(())
    } else {
        Err(CapabilityError::Other(format!(
            "read-only token cannot write to {} storage",
            store_type
        )))
    }
}

/// Require read-level access from a delegation token.
///
/// Returns an error string if the token doesn't grant any read-capable action.
///
/// # Arguments
/// * `token` — The delegation token to check.
/// * `store_type` — Human-readable name of the store being accessed.
///
/// expect: "System types preserve semantic identity and are provenance-aware"
/// pre:  token is any [`DelegationToken`]; store_type is any non-empty &str
/// post: returns `Ok(())` if `token.allows_read()` is true;
///       returns `Err("token does not grant read access for {store_type} recall")` otherwise
pub fn require_read_access(
    token: &DelegationToken,
    store_type: &str,
) -> Result<(), CapabilityError> {
    if token.allows_read() {
        Ok(())
    } else {
        Err(CapabilityError::Other(format!(
            "token does not grant read access for {} recall",
            store_type
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use hkask_types::WebID;

    #[test]
    fn require_write_access_accepts_write_token() {
        let from = WebID::from_persona(b"issuer");
        let to = WebID::from_persona(b"holder");
        let sk = SigningKey::from_bytes(&[0x42u8; 32]);
        let token = DelegationToken::new(
            DelegationResource::Tool,
            "episodic".into(),
            DelegationAction::Write,
            from,
            to,
            &sk,
        );
        assert!(require_write_access(&token, "episodic").is_ok());
    }

    #[test]
    fn require_write_access_rejects_read_only_token() {
        let from = WebID::from_persona(b"issuer");
        let to = WebID::from_persona(b"holder");
        let sk = SigningKey::from_bytes(&[0x42u8; 32]);
        let token = DelegationToken::new(
            DelegationResource::Tool,
            "episodic".into(),
            DelegationAction::Read,
            from,
            to,
            &sk,
        );
        let result = require_write_access(&token, "episodic");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("read-only"));
    }
}
