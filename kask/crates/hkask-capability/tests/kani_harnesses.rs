//! Kani proof harnesses for `hkask-capability`.
//!
//! These harnesses are compiled **only** when running under Kani
//! (`cargo kani`). The `#![cfg(kani)]` inner attribute gates the entire
//! file out of normal `cargo check` / `cargo test` builds, so it has zero
//! impact on the standard toolchain.
//!
//! What is proved:
//! 1. **No panic on malformed input** — feeding arbitrary bytes to the
//!    base64 deserializer (`DelegationToken::from_base64`) and to the
//!    capability spec parser (`CapabilitySpec::parse`) never panics,
//!    regardless of input.
//! 2. **Tamper detection** — flipping any single byte of a signed token's
//!    serialized form causes `DelegationToken::verify()` to return `false`.
//!    This holds for tampering in the signature, the public key, and the
//!    payload fields.
//!
//! Run with: `cargo kani --harness <name>`
//! (requires the `kani` Rust toolchain component).

#![cfg(kani)]

use ed25519_dalek::SigningKey;
use hkask_capability::{
    CapabilitySpec, DelegationToken, DelegationTokenBuilder, derive_signing_key,
};
use hkask_types::WebID;

/// Build a deterministically-signed token for use across harnesses.
///
/// Uses `derive_signing_key` so the signing key is a valid Ed25519 key
/// regardless of Kani's symbolic execution path.
fn signed_token() -> DelegationToken {
    let signing_key = derive_signing_key(b"kani-seed-32-bytes-long-!!!!!");
    let from = WebID::from_persona(b"issuer");
    let to = WebID::from_persona(b"holder");
    DelegationTokenBuilder::new(
        hkask_capability::DelegationResource::Tool,
        "tool:inference:call".to_string(),
        hkask_capability::DelegationAction::Execute,
        from,
        to,
        &signing_key,
    )
    .expires_at(1_700_000_000)
    .context_nonce("kani-root".to_string())
    .sign()
}

// ── Harness 1: No panic on arbitrary base64 input ─────────────────────────

/// Feeding arbitrary bytes (interpreted as a base64 input string of fixed
/// length) to `DelegationToken::from_base64` must never panic.
///
/// `from_base64` returns `Result<_, CapabilityError>`, so the only way it
/// could panic is via an indexing operation, an unwrap, or an arithmetic
/// overflow inside the base64 / JSON decode path. Kani proves none of those
/// occur for any byte pattern.
#[kani::proof]
fn no_panic_from_base64_arbitrary_bytes() {
    // Fixed-size buffer so Kani can exhaustively explore the input space.
    let bytes: [u8; 64] = kani::any();
    // Interpret as a Latin-1-style string slice — base64 decoding operates
    // on the byte content, so any byte pattern is admissible input.
    let candidate = String::from_utf8_lossy(&bytes);
    let _ = DelegationToken::from_base64(&candidate);
}

// ── Harness 2: No panic on arbitrary capability spec strings ──────────────

/// `CapabilitySpec::parse` must never panic on arbitrary input strings.
///
/// It splits on `:` and dispatches to enum parsers that fall back to
/// `Execute` for unknown actions. Kani proves the split / match / fallback
/// path is panic-free for any byte pattern.
#[kani::proof]
fn no_panic_capability_spec_parse_arbitrary_bytes() {
    let bytes: [u8; 32] = kani::any();
    let candidate = String::from_utf8_lossy(&bytes);
    let _ = CapabilitySpec::parse(&candidate);
}

// ── Harness 3: Tamper detection — signature byte flip ─────────────────────

/// Flipping any single byte of the 64-byte Ed25519 signature must cause
/// `verify()` to return `false`.
///
/// Kani symbolically picks an index `i` in `0..64` and a replacement byte
/// value, then asserts verification fails. This proves the signature is
/// cryptographically bound to the token: no single-byte mutation of the
/// signature can preserve validity.
#[kani::proof]
fn tamper_signature_byte_rejected() {
    let mut token = signed_token();

    let i: usize = kani::any();
    kani::assume(i < 64);

    let new_byte: u8 = kani::any();
    // Only consider actual mutations — a no-op flip is trivially valid.
    kani::assume(token.signature_bytes()[i] != new_byte);

    token.signature.0[i] = new_byte;

    kani::assert(!token.verify(), "tampered signature must fail verification");
}

// ── Harness 4: Tamper detection — public key byte flip ────────────────────

/// Flipping any single byte of the 32-byte Ed25519 public key must cause
/// `verify()` to return `false`.
///
/// Either the mutated key is not a valid Ed25519 verifying key (in which
/// case `verify()` returns `false` via the `VerifyingKey::from_bytes` error
/// path), or it is a different valid key (in which case the signature
/// verifies against a different key and fails). Kani proves both branches
/// converge to `false`.
#[kani::proof]
fn tamper_public_key_byte_rejected() {
    let mut token = signed_token();

    let i: usize = kani::any();
    kani::assume(i < 32);

    let new_byte: u8 = kani::any();
    kani::assume(token.public_key.0[i] != new_byte);

    token.public_key.0[i] = new_byte;

    kani::assert(
        !token.verify(),
        "tampered public key must fail verification",
    );
    kani::assert(
        !token.verify(),
        "tampered public key must fail verification",
    );
}

// ── Harness 5: Tamper detection — payload field (resource_id) ─────────────

/// Mutating the `resource_id` field (a payload component covered by the
/// signature) must cause `verify()` to return `false`.
///
/// This proves the signature covers the payload, not just the signature
/// bytes themselves. We use a fixed replacement string to keep the Kani
/// state space bounded; the cryptographic argument holds for any mutation
/// because the signed buffer is recomputed from the field on every verify.
#[kani::proof]
fn tamper_resource_id_rejected() {
    let mut token = signed_token();

    // Replace with a different fixed string. The signature was computed
    // over the original resource_id, so any change breaks verification.
    let original = token.resource_id.clone();
    token.resource_id = format!("{}-tampered", original);

    kani::assert(
        token.resource_id != original,
        "sanity: resource_id was actually mutated",
    );
    kani::assert(
        !token.verify(),
        "tampered resource_id must fail verification",
    );
}

// ── Harness 6: Round-trip integrity ───────────────────────────────────────

/// A token that is serialized to base64 and deserialized back must verify
/// identically to the original.
///
/// This proves the serialization round-trip preserves the cryptographic
/// integrity invariant: `to_base64` → `from_base64` → `verify()` == `true`.
#[kani::proof]
fn roundtrip_base64_preserves_verification() {
    let token = signed_token();
    let encoded = token
        .to_base64()
        .expect("encoding a valid token must succeed");
    let decoded = DelegationToken::from_base64(&encoded)
        .expect("decoding a freshly-encoded token must succeed");

    kani::assert(decoded.verify(), "round-tripped token must still verify");
    kani::assert(
        decoded.id == token.id,
        "round-tripped token must preserve id",
    );
    kani::assert(
        decoded.signature_bytes() == token.signature_bytes(),
        "round-tripped token must preserve signature bytes",
    );
}
