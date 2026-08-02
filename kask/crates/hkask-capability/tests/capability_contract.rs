//! Behavioral contract tests for `hkask-capability`.
//!
//! Covers: DelegationToken (is_valid_for, deterministic id) and capabilities_match.

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
