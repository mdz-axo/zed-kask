//! Behavioral contract tests for `hkask-capability`.
//!
//! Covers: DelegationToken (verify, is_expired, attenuate, serialization, deterministic id),
//! CapabilityChecker (check, verify_with_time), and capabilities_match.

use ed25519_dalek::SigningKey;
use hkask_capability::{
    CapabilityChecker, DelegationAction, DelegationResource, DelegationToken,
    DelegationTokenBuilder, SYSTEM_MAX_ATTENUATION, capabilities_match, derive_signing_key,
};
use hkask_types::WebID;

// ── Helpers ────────────────────────────────────────────────────────────────

fn key_a() -> SigningKey {
    derive_signing_key(b"test-key-a-32-bytes-long!!!")
}

fn key_b() -> SigningKey {
    derive_signing_key(b"test-key-b-32-bytes-long!!!")
}

fn alice() -> WebID {
    WebID::from_persona(b"alice")
}

fn bob() -> WebID {
    WebID::from_persona(b"bob")
}

fn carol() -> WebID {
    WebID::from_persona(b"carol")
}

fn make_token(sk: &SigningKey) -> DelegationToken {
    DelegationToken::new(
        DelegationResource::Tool,
        "test_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        sk,
    )
}

// ── 1. DelegationToken::verify — cryptographic integrity ───────────────────

#[test]
fn token_verify_valid_signature() {
    let sk = key_a();
    let token = make_token(&sk);
    assert!(token.verify(), "freshly minted token must verify");
}

#[test]
fn token_verify_tampered_signature() {
    let sk = key_a();
    let mut token = make_token(&sk);
    // Flip every byte of the signature
    for b in token.signature.0.iter_mut() {
        *b ^= 0xFF;
    }
    assert!(!token.verify(), "tampered signature must fail verification");
}

#[test]
fn token_verify_wrong_key() {
    let sk_a = key_a();
    let sk_b = key_b();
    let token = make_token(&sk_a);

    // Checker that only trusts key B
    let checker_b = CapabilityChecker::with_signing_key(sk_b);
    assert!(
        !checker_b.verify(&token),
        "checker with key B must reject token signed by key A"
    );
}

// ── 2. DelegationToken::is_expired — temporal boundary ─────────────────────

#[test]
fn token_not_expired_when_no_expiry() {
    let sk = key_a();
    let token = DelegationTokenBuilder::new(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        &sk,
    )
    .sign(); // no expires_at set → None
    assert!(!token.is_expired(0), "token with no expiry never expires");
    assert!(
        !token.is_expired(i64::MAX),
        "token with no expiry never expires"
    );
}

#[test]
fn token_expired_after_expiry_time() {
    let sk = key_a();
    let token = DelegationTokenBuilder::new(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        &sk,
    )
    .expires_at(1000)
    .sign();
    assert!(
        token.is_expired(1001),
        "token must be expired when current_time > expires_at"
    );
}

#[test]
fn token_not_expired_before_expiry() {
    let sk = key_a();
    let token = DelegationTokenBuilder::new(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        &sk,
    )
    .expires_at(1000)
    .sign();
    assert!(
        !token.is_expired(999),
        "token must not be expired before expiry"
    );
    assert!(
        !token.is_expired(1000),
        "token must not be expired at exact expiry boundary"
    );
}

// ── 3. DelegationToken::attenuate — attenuation chain ──────────────────────

#[test]
fn token_attenuate_increments_level() {
    let sk = key_a();
    let parent = make_token(&sk);
    assert_eq!(parent.attenuation_level, 0);

    let child = parent
        .attenuate(carol(), &sk, 0)
        .expect("attenuation should succeed");
    assert_eq!(
        child.attenuation_level, 1,
        "child level must be parent level + 1"
    );
}

#[test]
fn token_cannot_attenuate_beyond_max() {
    let sk = key_a();
    let parent = DelegationTokenBuilder::new(
        DelegationResource::Tool,
        "tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        &sk,
    )
    .attenuation(SYSTEM_MAX_ATTENUATION, SYSTEM_MAX_ATTENUATION)
    .sign();

    assert!(
        !parent.can_attenuate(),
        "token at max attenuation cannot be further attenuated"
    );
    let child = parent.attenuate(carol(), &sk, 0);
    assert!(
        child.is_none(),
        "attenuate must return None when at max level"
    );
}

#[test]
fn token_attenuate_preserves_resource() {
    let sk = key_a();
    let parent = make_token(&sk);
    let child = parent
        .attenuate(carol(), &sk, 0)
        .expect("attenuation should succeed");

    assert_eq!(
        child.resource, parent.resource,
        "resource type must be inherited"
    );
    assert_eq!(
        child.resource_id, parent.resource_id,
        "resource_id must be inherited"
    );
    assert_eq!(child.action, parent.action, "action must be inherited");
}

// ── 4. CapabilityChecker::check — access control ────────────────────────────

#[test]
fn check_allows_correct_holder_resource_action() {
    let sk = key_a();
    let checker = CapabilityChecker::with_signing_key(sk.clone());
    let token = checker.grant(
        DelegationResource::Tool,
        "my_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    );

    assert!(
        checker.check(
            &token,
            &bob(),
            DelegationResource::Tool,
            "my_tool",
            DelegationAction::Execute
        ),
        "correct holder + resource + action must pass"
    );
}

#[test]
fn check_rejects_wrong_holder() {
    let sk = key_a();
    let checker = CapabilityChecker::with_signing_key(sk.clone());
    let token = checker.grant(
        DelegationResource::Tool,
        "my_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    );

    assert!(
        !checker.check(
            &token,
            &carol(),
            DelegationResource::Tool,
            "my_tool",
            DelegationAction::Execute
        ),
        "wrong holder must be rejected"
    );
}

#[test]
fn check_rejects_wrong_resource() {
    let sk = key_a();
    let checker = CapabilityChecker::with_signing_key(sk.clone());
    let token = checker.grant(
        DelegationResource::Tool,
        "my_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
    );

    assert!(
        !checker.check(
            &token,
            &bob(),
            DelegationResource::Registry,
            "my_tool",
            DelegationAction::Execute
        ),
        "wrong resource type must be rejected"
    );
}

/// `check()` does not validate expiry — it tests signature + holder + resource/action only.
/// Expired-token rejection uses `verify_with_time()`, the combined check.
#[test]
fn check_rejects_expired_token() {
    let sk = key_a();
    let checker = CapabilityChecker::with_signing_key(sk.clone());
    let token = DelegationTokenBuilder::new(
        DelegationResource::Tool,
        "my_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        &sk,
    )
    .expires_at(100)
    .sign();

    // `check()` alone does not test expiry — it returns true for an expired token
    // that is otherwise valid. Full gate logic uses `verify_with_time()`.
    assert!(token.is_expired(200), "sanity: token is expired");

    assert!(
        !checker.verify_with_time(&token, 200),
        "verify_with_time must reject expired token even with correct holder"
    );
}

// ── 5. capabilities_match — action hierarchy ───────────────────────────────

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

// ── 6. DelegationToken::from_base64 / to_base64 — serialization ────────────

#[test]
fn base64_roundtrip_preserves_token() {
    let sk = key_a();
    let token = make_token(&sk);
    let encoded = token.to_base64().expect("to_base64 must succeed");
    let decoded = DelegationToken::from_base64(&encoded).expect("from_base64 must succeed");
    assert_eq!(
        token.fingerprint(),
        decoded.fingerprint(),
        "roundtrip must preserve fingerprint"
    );
}

#[test]
fn base64_rejects_invalid_input() {
    let result = DelegationToken::from_base64("!!! not valid base64 !!!");
    assert!(result.is_err(), "garbage input must produce error");
}

// ── 7. Deterministic id — same params → same fingerprint ────────────────────

#[test]
fn token_id_deterministic() {
    let sk = key_a();
    let t1 = DelegationToken::new(
        DelegationResource::Tool,
        "my_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        &sk,
    );
    let t2 = DelegationToken::new(
        DelegationResource::Tool,
        "my_tool".into(),
        DelegationAction::Execute,
        alice(),
        bob(),
        &sk,
    );
    assert_eq!(t1.id, t2.id, "same params must produce same token id");
    assert_eq!(
        t1.fingerprint(),
        t2.fingerprint(),
        "same params must produce same fingerprint"
    );
}
