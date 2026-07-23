//! Capability checker for composition-oriented capability management.
//!
//! \[NORMATIVE\] With Ed25519 tokens, verification uses the token's own public key —
//! no shared secret is required. The checker validates structural properties
//! (expiry, resource match, holder match) against the token (P4 — Clear Boundaries).

use crate::{DelegationAction, DelegationResource, DelegationToken};
use ed25519_dalek::SigningKey;
use hkask_types::Ed25519PublicKey;
use hkask_types::WebID;

/// Capability checker for composition operations.
///
/// \[NORMATIVE\] A token is only valid if its Ed25519 signature verifies AND its
/// embedded public key is one of the checker's `trusted_roots`. Verifying the
/// self-signature alone proves integrity, not authority — without a trusted root,
/// any freshly generated keypair would yield a "valid" token for any resource.
/// The checker therefore anchors trust in a configured root key, and **fails
/// closed**: a checker with no trusted roots rejects every token (P4 — Clear Boundaries).
pub struct CapabilityChecker {
    /// Optional Ed25519 signing key for token creation (grant_* methods).
    /// When absent, grant_* methods panic — token issuance requires a signing key.
    signing_key: Option<SigningKey>,
    /// Trusted issuer public keys. When `enforce_roots` is set, a token is only
    /// accepted if its embedded `public_key` is in this set (empty ⇒ reject all).
    trusted_roots: Vec<Ed25519PublicKey>,
    /// Whether root membership is enforced. `false` for `new()` (integrity-only:
    /// verify the self-signature, used by pod-internal checkers where tokens are
    /// constructed locally and never injected from the wire). `true` for
    /// root-anchored constructors, which reject any token not signed by a
    /// trusted issuer — the bearer-token authority gate (P4 — Clear Boundaries).
    enforce_roots: bool,
}

impl Default for CapabilityChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityChecker {
    /// Create a new integrity-only capability checker (no root enforcement).
    ///
    /// Verifies a token's self-signature but does not require a trusted issuer.
    /// Used by pod-internal checkers, where the capability token is constructed
    /// locally by the trusted factory and never accepted from the wire.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns a [`CapabilityChecker`] that checks signatures only
    pub fn new() -> Self {
        Self {
            signing_key: None,
            trusted_roots: Vec::new(),
            enforce_roots: false,
        }
    }

    /// Create a capability checker with a signing key for token issuance.
    ///
    /// The signing key's own public key is added to the trusted roots and root
    /// enforcement is enabled, so this checker accepts only tokens it issued (or
    /// those from roots added via `trust_root`).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  signing_key is a valid Ed25519 [`SigningKey`]
    /// post: returns a [`CapabilityChecker`] that can issue and verify tokens,
    ///       trusting its own public key as a root
    pub fn with_signing_key(signing_key: SigningKey) -> Self {
        let root = Ed25519PublicKey(signing_key.verifying_key().to_bytes());
        Self {
            signing_key: Some(signing_key),
            trusted_roots: vec![root],
            enforce_roots: true,
        }
    }

    /// Add a trusted issuer public key (chainable). Enables root enforcement.
    ///
    /// Used to trust additional authorities — e.g. the A2A root authority, whose
    /// registration tokens are signed by a key distinct from the system OCAP key.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns self with `root` added to the trusted-root set and root
    ///       enforcement enabled
    pub fn trust_root(mut self, root: Ed25519PublicKey) -> Self {
        if !self.trusted_roots.contains(&root) {
            self.trusted_roots.push(root);
        }
        self.enforce_roots = true;
        self
    }

    /// Create a verify-only checker anchored to a set of trusted roots (fail-closed).
    ///
    /// Root enforcement is enabled: a token is accepted only if its embedded
    /// public key is one of `roots`. An empty set rejects every token — used by
    /// the API bearer-token gate so that forged tokens (and the no-root
    /// misconfiguration) are denied (P4 — Clear Boundaries).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  roots are the Ed25519 public keys of trusted issuers
    /// post: returns a [`CapabilityChecker`] that accepts only tokens signed by a
    ///       trusted root; cannot issue tokens
    pub fn with_trusted_roots(roots: Vec<Ed25519PublicKey>) -> Self {
        Self {
            signing_key: None,
            trusted_roots: roots,
            enforce_roots: true,
        }
    }

    /// Verify a capability token's signature, and — when root enforcement is on —
    /// that its issuer is trusted.
    ///
    /// Integrity-only checkers (`new()`) verify the self-signature alone.
    /// Root-anchored checkers (`with_signing_key` / `with_trusted_roots`)
    /// additionally require the embedded public key to be a trusted root, which
    /// is what makes a bearer token unforgeable: a self-signed token from an
    /// unknown keypair is rejected. A root-anchored checker with an empty
    /// trusted-root set rejects everything (fail closed).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`CapabilityChecker`]; token is any [`DelegationToken`]
    /// post: returns true iff the signature is valid AND (root enforcement is off
    ///       OR the public key is a trusted root)
    pub fn verify(&self, token: &DelegationToken) -> bool {
        if !token.verify() {
            return false;
        }
        if self.enforce_roots {
            return self.trusted_roots.contains(&token.public_key);
        }
        true
    }

    /// Check if token is valid and not expired
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`CapabilityChecker`]; token is any [`DelegationToken`];
    ///       current_time is any i64 (Unix timestamp)
    /// post: returns true if both signature is valid and token is not expired at current_time;
    ///       returns false otherwise
    pub fn verify_with_time(&self, token: &DelegationToken, current_time: i64) -> bool {
        self.verify(token) && !token.is_expired(current_time)
    }

    /// Check if a holder has capability for a resource/action
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`CapabilityChecker`]; token is any [`DelegationToken`];
    ///       holder is any [`WebID`]; resource, resource_id, action describe the requested access
    /// post: returns true if signature is valid, token.delegated_to matches holder,
    ///       and token.is_valid_for(resource, resource_id, action) is true;
    ///       returns false otherwise
    pub fn check(
        &self,
        token: &DelegationToken,
        holder: &WebID,
        resource: DelegationResource,
        resource_id: &str,
        action: DelegationAction,
    ) -> bool {
        self.verify(token)
            && token.delegated_to == *holder
            && token.is_valid_for(resource, resource_id, action)
    }

    /// Check if holder has any capability for a resource type
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`CapabilityChecker`]; token is any [`DelegationToken`];
    ///       holder is any [`WebID`]; resource is any [`DelegationResource`]
    /// post: returns true if signature is valid, token.delegated_to matches holder,
    ///       and token.grants_resource(resource) is true; returns false otherwise
    pub fn check_resource(
        &self,
        token: &DelegationToken,
        holder: &WebID,
        resource: DelegationResource,
    ) -> bool {
        self.verify(token) && token.delegated_to == *holder && token.grants_resource(resource)
    }

    /// Create a capability token for the given resource, domain, and action.
    ///
    /// Requires a signing key — panics if constructed via `new()` instead of `with_signing_key()`.
    /// This single method replaces 6 domain-specific `grant_*` methods (DRY consolidation).
    /// Domain convenience wrappers (`grant_tool`, `grant_registry`) delegate to this method.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self was constructed via `with_signing_key`; resource_id is any non-empty [`String`];
    ///       from and to are any [`WebID`]
    /// post: returns a [`DelegationToken`] signed with the checker's Ed25519 key;
    ///       panics if no signing key is available
    pub fn grant(
        &self,
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        from: WebID,
        to: WebID,
    ) -> DelegationToken {
        let sk = self.signing_key.as_ref().expect(
            "CapabilityChecker::grant requires a signing key. Use with_signing_key() to construct.",
        );
        DelegationToken::new(resource, resource_id, action, from, to, sk)
    }

    /// Convenience: grant a tool capability with Execute action.
    pub fn grant_tool(&self, tool_name: String, from: WebID, to: WebID) -> DelegationToken {
        self.grant(
            DelegationResource::Tool,
            tool_name,
            DelegationAction::Execute,
            from,
            to,
        )
    }

    /// Convenience: grant a wildcard registry capability.
    pub fn grant_registry(
        &self,
        action: DelegationAction,
        from: WebID,
        to: WebID,
    ) -> DelegationToken {
        self.grant(
            DelegationResource::Registry,
            "*".to_string(),
            action,
            from,
            to,
        )
    }

    /// Create an attenuated token for delegation
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`CapabilityChecker`]; token is any [`DelegationToken`];
    ///       new_to is any [`WebID`]; current_time is any i64
    /// post: returns `Some(attenuated_token)` if self has a signing key and token.can_attenuate();
    ///       returns `None` if no signing key is available or attenuation limit reached
    pub fn attenuate(
        &self,
        token: &DelegationToken,
        new_to: WebID,
        current_time: i64,
    ) -> Option<DelegationToken> {
        let sk = self.signing_key.as_ref()?;
        token.attenuate(new_to, sk, current_time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive_signing_key;
    use hkask_types::WebID;

    #[test]
    fn capability_checker_new_creates_checker() {
        let secret = b"test-secret-32-bytes-long!!";
        let sk = derive_signing_key(secret);
        let checker = CapabilityChecker::with_signing_key(sk);
        // Verify it can verify a token it created
        let from = WebID::from_persona(b"issuer");
        let to = WebID::from_persona(b"holder");
        let token = checker.grant_tool("test_tool".into(), from, to);
        assert!(checker.verify(&token));
    }

    /// \[C1 regression\] A self-signed token from an untrusted keypair must be
    /// rejected. Before the trust anchor, `verify()` only checked the token's
    /// self-signature, so any attacker-minted token was accepted.
    #[test]
    fn verify_rejects_token_from_untrusted_issuer() {
        // Trusted authority.
        let system_sk = derive_signing_key(b"system-ocap-root-secret");
        let checker = CapabilityChecker::with_signing_key(system_sk);

        // Attacker mints a token with their OWN keypair for any resource/holder.
        let attacker_sk = derive_signing_key(b"attacker-controlled-secret");
        let attacker = WebID::from_persona(b"attacker");
        let forged = DelegationToken::new(
            DelegationResource::Tool,
            "any_tool".into(),
            DelegationAction::Execute,
            attacker,
            attacker,
            &attacker_sk,
        );

        // The forged token's self-signature is valid (integrity) ...
        assert!(forged.verify(), "self-signature is structurally valid");
        // ... but the issuer is not trusted, so the checker rejects it (authority).
        assert!(
            !checker.verify(&forged),
            "C1: token from untrusted issuer must be rejected"
        );
        assert!(
            !checker.check(
                &forged,
                &attacker,
                DelegationResource::Tool,
                "any_tool",
                DelegationAction::Execute
            ),
            "C1: check() must also reject untrusted issuer"
        );
    }

    /// \[C1 regression\] A root-anchored checker with no trusted roots must reject
    /// every token (fail closed). This is the API bearer-token misconfiguration
    /// posture. (`new()` is integrity-only and intentionally does not enforce.)
    #[test]
    fn verify_with_no_roots_fails_closed() {
        let checker = CapabilityChecker::with_trusted_roots(vec![]);
        let sk = derive_signing_key(b"some-secret");
        let to = WebID::from_persona(b"holder");
        let token = DelegationToken::new(
            DelegationResource::Tool,
            "t".into(),
            DelegationAction::Execute,
            to,
            to,
            &sk,
        );
        assert!(
            !checker.verify(&token),
            "checker with empty trusted roots must reject all tokens"
        );
    }
}
