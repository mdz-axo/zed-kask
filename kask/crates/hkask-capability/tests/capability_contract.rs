//! Behavioral contract tests for `hkask-capability`.
//!
//! Covers: DelegationToken (is_expired, is_valid_for_at, deterministic id)
//! and capabilities_match.

use hkask_capability::{DelegationAction, DelegationResource, DelegationToken, capabilities_match};
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
    let token = DelegationToken::new(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    );
    assert!(!token.is_expired(0), "token with no expiry never expires");
    assert!(
        !token.is_expired(i64::MAX),
        "token with no expiry never expires"
    );
}

#[test]
fn token_expired_after_expiry_time() {
    let token = DelegationToken::new_with_expiry(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        1000,
    );
    assert!(
        token.is_expired(1001),
        "token must be expired when current_time > expires_at"
    );
}

#[test]
fn token_not_expired_before_expiry() {
    let token = DelegationToken::new_with_expiry(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        1000,
    );
    assert!(
        !token.is_expired(999),
        "token must not be expired before expiry"
    );
    assert!(
        !token.is_expired(1000),
        "token must not be expired at exact expiry boundary"
    );
}

// ── DelegationToken::is_valid_for_at — the capability-match gate ─────────

#[test]
fn is_valid_for_at_matches_exact_triple() {
    let token = make_token();
    assert!(token.is_valid_for_at(
        DelegationResource::Tool,
        "test_tool",
        DelegationAction::Execute,
        0
    ));
    assert!(!token.is_valid_for_at(
        DelegationResource::Tool,
        "other_tool",
        DelegationAction::Execute,
        0
    ));
    assert!(!token.is_valid_for_at(
        DelegationResource::Tool,
        "test_tool",
        DelegationAction::Read,
        0
    ));
    assert!(!token.is_valid_for_at(
        DelegationResource::Registry,
        "test_tool",
        DelegationAction::Execute,
        0
    ));
}

// ── DelegationToken::is_valid_for_at — the expiry-aware gate ────────────
// Pins the enforcement point for `ocap.capability_expiry_seconds`: the gate
// (`McpRuntime::invoke`) rejects tokens whose `expires_at` has passed, even
// when the (resource, resource_id, action) triple matches exactly.

#[test]
fn is_valid_for_at_rejects_expired_token_even_on_match() {
    let token = DelegationToken::new_with_expiry(
        DelegationResource::Tool,
        "test_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        1000,
    );
    assert!(
        !token.is_valid_for_at(
            DelegationResource::Tool,
            "test_tool",
            DelegationAction::Execute,
            1001
        ),
        "expired token must be denied even when the capability triple matches"
    );
    assert!(
        token.is_valid_for_at(
            DelegationResource::Tool,
            "test_tool",
            DelegationAction::Execute,
            1000
        ),
        "token at exact expiry boundary must still be valid"
    );
}

#[test]
fn is_valid_for_at_admits_no_expiry_token() {
    let token = make_token();
    assert!(
        token.is_valid_for_at(
            DelegationResource::Tool,
            "test_tool",
            DelegationAction::Execute,
            i64::MAX
        ),
        "token with no expiry must remain valid at any time"
    );
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
