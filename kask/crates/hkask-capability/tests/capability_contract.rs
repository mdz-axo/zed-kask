//! Behavioral contract tests for `hkask-capability`.
//!
//! Covers: DelegationToken (is_valid_for, deterministic id) and capabilities_match.

use hkask_capability::{DelegationAction, DelegationResource, DelegationToken, capabilities_match};
use hkask_test_harness::{arb_action, arb_resource};
use hkask_types::WebID;
use proptest::prelude::*;

// ── Helpers ───────────────────────────────────────────────────────────────

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
        WebID::from_persona(b"alice"),
        WebID::from_persona(b"bob"),
    )
}

// ── Property-based tests ─────────────────────────────────────────────────

proptest! {
    // is_valid_for returns true iff the triple matches exactly.
    #[test]
    fn is_valid_for_exact_match_property(
        resource in arb_resource(),
        resource_id in proptest::arbitrary::any::<String>(),
        action in arb_action(),
        alt_resource_id in proptest::arbitrary::any::<String>(),
    ) {
        let token = DelegationToken::new(
            resource,
            resource_id.clone(),
            action,
            WebID::from_persona(b"alice"),
            WebID::from_persona(b"bob"),
        );
        // Exact match
        prop_assert!(
            token.is_valid_for(resource, &resource_id, action),
            "exact triple match must be valid"
        );
        // Different resource_id → invalid
        prop_assume!(resource_id != alt_resource_id);
        prop_assert!(
            !token.is_valid_for(resource, &alt_resource_id, action),
            "different resource_id must be invalid"
        );
    }

    // Same params produce same id; different resource_id produces different id.
    #[test]
    fn token_id_deterministic_property(
        resource_id in proptest::arbitrary::any::<String>(),
        alt_resource_id in proptest::arbitrary::any::<String>(),
    ) {
        let t1 = DelegationToken::new(
            DelegationResource::Tool,
            resource_id.clone(),
            DelegationAction::Execute,
            WebID::from_persona(b"alice"),
            WebID::from_persona(b"bob"),
        );
        let t2 = DelegationToken::new(
            DelegationResource::Tool,
            resource_id.clone(),
            DelegationAction::Execute,
            WebID::from_persona(b"alice"),
            WebID::from_persona(b"bob"),
        );
        prop_assert_eq!(&t1.id, &t2.id, "same params must produce same id");

        prop_assume!(resource_id != alt_resource_id);
        let t3 = DelegationToken::new(
            DelegationResource::Tool,
            alt_resource_id,
            DelegationAction::Execute,
            WebID::from_persona(b"alice"),
            WebID::from_persona(b"bob"),
        );
        prop_assert_ne!(&t1.id, &t3.id, "different resource_id must produce different id");
    }

    // Action hierarchy: execute ≥ write ≥ read. Domain mismatch always fails.
    #[test]
    fn capabilities_match_action_hierarchy(
        tool_name in "[a-z][a-z0-9_]*",
        other_tool in "[a-z][a-z0-9_]*",
    ) {
        let exec = format!("tool:{}:execute", tool_name);
        let write = format!("tool:{}:write", tool_name);
        let read = format!("tool:{}:read", tool_name);

        prop_assert!(capabilities_match(&exec, &write), "execute permits write");
        prop_assert!(capabilities_match(&exec, &read), "execute permits read");
        prop_assert!(capabilities_match(&write, &read), "write permits read");
        prop_assert!(!capabilities_match(&read, &write), "read does not permit write");
        prop_assert!(!capabilities_match(&read, &exec), "read does not permit execute");
        prop_assert!(!capabilities_match(&write, &exec), "write does not permit execute");

        prop_assume!(tool_name != other_tool);
        let other_exec = format!("tool:{}:execute", other_tool);
        prop_assert!(!capabilities_match(&exec, &other_exec), "domain mismatch must fail");
    }
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
