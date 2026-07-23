//! Verification functions for delegation tokens.
//!
//! Unified verification entry points that produce structured [`VerificationOutcome`]
//! instead of bare booleans. Also includes read/write access guard functions.

use super::checker::CapabilityChecker;
use super::types::VerificationOutcome;
use crate::{CapabilityError, DelegationAction, DelegationResource, DelegationToken};
use hkask_types::WebID;

/// Verify a delegation token using the current system time.
///
/// Equivalent to calling [`verify_delegation_token`] with `current_time` set to
/// the current UNIX epoch timestamp (seconds). Uses `std::time::SystemTime` so
/// no external time dependency is required.
///
/// expect: "System types preserve semantic identity and are provenance-aware"
/// pre:  checker is `Option<&CapabilityChecker>`; token is any [`DelegationToken`];
///       holder is any [`WebID`]; resource, resource_id, action describe the requested access
/// post: returns a [`VerificationOutcome`] using the current system time as the expiry reference;
///       delegates to [`verify_delegation_token`]
pub fn verify_delegation_token_now(
    checker: Option<&CapabilityChecker>,
    token: &DelegationToken,
    holder: &WebID,
    resource: DelegationResource,
    resource_id: &str,
    action: DelegationAction,
) -> VerificationOutcome {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    verify_delegation_token(
        checker,
        token,
        holder,
        resource,
        resource_id,
        action,
        current_time,
    )
}

/// Verify a delegation token against an optional capability checker.
///
/// Unified verification entry point that produces a structured
/// [`VerificationOutcome`] instead of a bare boolean. Call sites
/// in MCP servers and adapters use this to map each failure mode to
/// a specific error response.
///
/// When `checker` is `None`, returns `VerificationOutcome::NoChecker`.
///
/// expect: "System types preserve semantic identity and are provenance-aware"
/// pre:  checker is `Option<&CapabilityChecker>`; token is any [`DelegationToken`];
///       holder is any [`WebID`]; resource, resource_id, action describe the requested access;
///       current_time is any i64 (Unix timestamp)
/// post: returns [`VerificationOutcome::NoChecker`] if checker is None;
///       [`VerificationOutcome::InvalidSignature`] if Ed25519 signature fails;
///       [`VerificationOutcome::Expired`] if token is expired at current_time;
///       [`VerificationOutcome::InsufficientAccess`] if holder/resource/action mismatch;
///       [`VerificationOutcome::Valid`] if all checks pass
pub fn verify_delegation_token(
    checker: Option<&CapabilityChecker>,
    token: &DelegationToken,
    holder: &WebID,
    resource: DelegationResource,
    resource_id: &str,
    action: DelegationAction,
    current_time: i64,
) -> VerificationOutcome {
    let checker = match checker {
        Some(c) => c,
        None => return VerificationOutcome::NoChecker,
    };

    if !checker.verify(token) {
        return VerificationOutcome::InvalidSignature;
    }

    if token.is_expired(current_time) {
        return VerificationOutcome::Expired;
    }

    if !checker.check(token, holder, resource, resource_id, action) {
        return VerificationOutcome::InsufficientAccess {
            resource_id: resource_id.to_string(),
            action: action.as_str().to_string(),
        };
    }

    VerificationOutcome::Valid
}

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
    fn verify_delegation_token_returns_no_checker_when_none() {
        let from = WebID::from_persona(b"issuer");
        let to = WebID::from_persona(b"holder");
        let sk = SigningKey::from_bytes(&[0x42u8; 32]);
        let token = DelegationToken::new(
            DelegationResource::Tool,
            "test_tool".into(),
            DelegationAction::Execute,
            from,
            to,
            &sk,
        );
        let outcome = verify_delegation_token(
            None,
            &token,
            &to,
            DelegationResource::Tool,
            "test_tool",
            DelegationAction::Execute,
            i64::MAX, // far future — not expired
        );
        assert_eq!(outcome, VerificationOutcome::NoChecker);
    }

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
