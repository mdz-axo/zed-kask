//! Behavioral contract tests for `hkask-capability`.
//!
//! Covers: DelegationToken (is_expired, is_valid_for, allows_*, deterministic id)
//! and capabilities_match.

use hkask_capability::{
    DelegationAction, DelegationResource, DelegationToken, DelegationTokenBuilder,
    SYSTEM_MAX_ATTENUATION, capabilities_match,
};
use hkask_types::WebID;

// ── Helpers ────────────────────────────────────────────────────────────────

fn alice() -> WebID {
    WebID::from_persona(b"alice")
}

fn bob() -> WebID {
    WebID::from_persona(b"bob")
}

fn make_token() -> DelegationToken {
    DelegationToken::new(
        DelegationResource::Tool,
        "test_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    )
}

// ── DelegationToken::is_expired — temporal boundary ─────────────────────

#[test]
fn token_not_expired_when_no_expiry() {
    let token = DelegationTokenBuilder::new(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    )
    .build(); // no expires_at set → None
    assert!(!token.is_expired(0), "token with no expiry never expires");
    assert!(
        !token.is_expired(i64::MAX),
        "token with no expiry never expires"
    );
}

#[test]
fn token_expired_after_expiry_time() {
    let token = DelegationTokenBuilder::new(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    )
    .expires_at(1000)
    .build();
    assert!(
        token.is_expired(1001),
        "token must be expired when current_time > expires_at"
    );
}

#[test]
fn token_not_expired_before_expiry() {
    let token = DelegationTokenBuilder::new(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    )
    .expires_at(1000)
    .build();
    assert!(
        !token.is_expired(999),
        "token must not be expired before expiry"
    );
    assert!(
        !token.is_expired(1000),
        "token must not be expired at exact expiry boundary"
    );
}

// ── DelegationToken::is_valid_for — the capability-match gate ───────────

#[test]
fn is_valid_for_matches_exact_triple() {
    let token = make_token();
    assert!(token.is_valid_for(
        DelegationResource::Tool,
        "test_tool",
        DelegationAction::Execute
    ));
    assert!(!token.is_valid_for(
        DelegationResource::Tool,
        "other_tool",
        DelegationAction::Execute
    ));
    assert!(!token.is_valid_for(
        DelegationResource::Tool,
        "test_tool",
        DelegationAction::Read
    ));
    assert!(!token.is_valid_for(
        DelegationResource::Registry,
        "test_tool",
        DelegationAction::Execute
    ));
}

// ── DelegationToken::allows_* — action hierarchy ────────────────────────

#[test]
fn execute_allows_read_and_write() {
    let token = make_token(); // Execute
    assert!(token.allows_read());
    assert!(token.allows_write());
}

#[test]
fn read_allows_only_read() {
    let token = DelegationToken::new(
        DelegationResource::Tool,
        "t".into(),
        DelegationAction::Read,
        alice(),
        bob(),
    );
    assert!(token.allows_read());
    assert!(!token.allows_write());
}

// ── capabilities_match — action hierarchy ───────────────────────────────

#[test]
fn execute_permits_write() {
    assert!(capabilities_match(
        "tool:mytool:execute",
        "tool:mytool:write"
    ));
}

#[test]
fn write_permits_read() {
    assert!(capabilities_match("tool:mytool:write", "tool:mytool:read"));
}

#[test]
fn read_does_not_permit_write() {
    assert!(!capabilities_match("tool:mytool:read", "tool:mytool:write"));
}

#[test]
fn domain_mismatch_fails() {
    assert!(!capabilities_match(
        "tool:domain_a:execute",
        "tool:domain_b:execute"
    ));
}

// ── Deterministic id — same params → same id ────────────────────────────

#[test]
fn token_id_deterministic() {
    let t1 = DelegationToken::new(
        DelegationResource::Tool,
        "my_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    );
    let t2 = DelegationToken::new(
        DelegationResource::Tool,
        "my_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    );
    assert_eq!(t1.id, t2.id, "same params must produce same token id");
}

#[test]
fn token_id_varies_with_params() {
    let t1 = make_token();
    let t2 = DelegationToken::new(
        DelegationResource::Tool,
        "different_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    );
    assert_ne!(
        t1.id, t2.id,
        "different resource_id must produce different id"
    );
}

// ── Builder defaults ────────────────────────────────────────────────────

#[test]
fn builder_defaults() {
    let token = DelegationTokenBuilder::new(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    )
    .build();
    assert_eq!(token.attenuation_level, 0);
    assert_eq!(token.max_attenuation, SYSTEM_MAX_ATTENUATION);
    assert!(token.context_nonce.is_empty());
    assert!(token.caveats.is_empty());
    assert!(token.expires_at.is_none());
}
