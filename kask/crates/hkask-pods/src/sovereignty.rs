//! Sovereignty checking for agent pods
//!
//! Ensures agent operations respect user sovereignty boundaries. Per the
//! Magna Carta, episodic memory / personal context / capability tokens /
//! OCAP boundaries are sovereign and require explicit consent; semantic
//! memory and template invocations are shared and require consent; the
//! lexicon and template registry are public.
//!
//! Consent is resolved through a `SovereigntyConsent` port, decoupling the
//! checker from the concrete `ConsentManager`. Production wiring uses a
//! `ConsentManager`-backed port so that grants via `kask sovereignty grant`
//! or `POST /consent/grant` are observed on the next sovereignty check.

use hkask_types::{DataCategory, UserSovereigntyState, WebID};
use std::sync::Arc;

/// Port for resolving explicit user consent for a (webid, category) pair.
///
/// Implementations are responsible for whatever backing store is appropriate
/// (a SQLite-backed `ConsentManager` in production, an in-memory map in
/// tests, an `AllowAll` policy in scaffolding).
pub trait SovereigntyConsent: Send + Sync {
    /// Returns `true` iff the given WebID has active, non-revoked consent
    /// for the given data category.
    fn has_consent(&self, webid: &str, category: &DataCategory) -> bool;
}

/// Default `SovereigntyConsent` implementation: deny everything.
///
/// \[NORMATIVE\] Sovereignty must fail closed. New `PodManager`s use this until (P1 — User Sovereignty).
/// `with_consent_port` is called with a real backend. This guarantees
/// that a misconfigured or partially-initialized agent cannot access
/// sovereign data without an explicit grant.
pub struct DenyAllConsent;

impl SovereigntyConsent for DenyAllConsent {
    fn has_consent(&self, _webid: &str, _category: &DataCategory) -> bool {
        false
    }
}

/// Test/scaffolding `SovereigntyConsent` implementation: grant everything.
///
/// Used in unit tests that don't care about the consent semantics, only
/// \[NORMATIVE\] about the access path. Production must never use this — it bypasses (P1 — User Sovereignty).
/// the Magna Carta's explicit-consent requirement.
pub struct AllowAllConsent;

impl SovereigntyConsent for AllowAllConsent {
    fn has_consent(&self, _webid: &str, _category: &DataCategory) -> bool {
        true
    }
}

/// Sovereignty checker for agent pods.
///
/// Reads explicit consent from the supplied `SovereigntyConsent` port.
/// This is the live wiring of the Magna Carta's "explicit consent tracking"
/// requirement.
pub struct SovereigntyChecker {
    state: UserSovereigntyState,
    owner_webid: WebID,
    consent: Arc<dyn SovereigntyConsent>,
}

impl Clone for SovereigntyChecker {
    fn clone(&self) -> Self {
        // The Arc<dyn SovereigntyConsent> shares state by reference, so a
        // shallow clone is safe and preserves the consent-port wiring.
        // `UserSovereigntyState` is `Copy`-able (it holds primitive enums).
        Self {
            state: self.state.clone(),
            owner_webid: self.owner_webid,
            consent: Arc::clone(&self.consent),
        }
    }
}

impl SovereigntyChecker {
    /// expect: "My agents operate within my sovereignty boundaries"
    /// \[P1\] Motivating: User Sovereignty — checker enforces the user-data boundary
    /// \[P2\] Constraining: Affirmative Consent — delegates to consent port
    /// pre:  `owner_webid` is a valid `WebID`; `consent` is a valid
    ///       `Arc<dyn SovereigntyConsent>`.
    /// post: Returns a `SovereigntyChecker` with a fresh
    ///       `UserSovereigntyState` and the given owner and consent port.
    pub fn new(owner_webid: WebID, consent: Arc<dyn SovereigntyConsent>) -> Self {
        Self {
            state: UserSovereigntyState::new(),
            owner_webid,
            consent,
        }
    }

    /// Live per-category consent lookup.
    fn has_consent(&self, webid: &WebID, category: &DataCategory) -> bool {
        self.consent.has_consent(&webid.to_string(), category)
    }

    /// expect: "My agents operate within my sovereignty boundaries"
    /// \[P1\] Motivating: User Sovereignty — access decision combines consent + ownership
    /// pre:  `data_category` is a valid `DataCategory`; `requester` is a
    ///       valid `WebID`.
    /// post: Returns `true` iff the requester is permitted to access the
    ///       category: sovereign data requires consent AND requester==owner;
    ///       shared data requires consent; public data is always accessible.
    pub fn can_access(&self, data_category: &DataCategory, requester: &WebID) -> bool {
        if self.state.boundary.is_sovereign(data_category) {
            // Sovereign data: requires explicit consent AND requester == owner.
            return self.has_consent(requester, data_category) && requester == &self.owner_webid;
        }
        if self.state.boundary.is_category_shared(data_category) {
            // Shared data: requires explicit consent for the requesting WebID.
            return self.has_consent(requester, data_category);
        }
        self.state.boundary.is_category_public(data_category)
    }

    /// expect: "My agents operate within my sovereignty boundaries"
    /// \[P1\] Motivating: User Sovereignty — action decision combines consent + operation
    /// pre:  `operation` is a non-empty string; `data_category` is a
    ///       valid `DataCategory`.
    /// post: For "acquisition", returns `true` iff affirmative consent is
    ///       NOT required. For all other operations, delegates to
    ///       `can_access` with the owner WebID as requester.
    pub fn check_operation(&self, operation: &str, data_category: &DataCategory) -> bool {
        if operation == "acquisition" {
            return !self.state.boundary.requires_affirmative_consent();
        }
        self.can_access(data_category, &self.owner_webid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::DataCategory;
    use std::sync::Arc;

    fn test_webid() -> WebID {
        WebID::new()
    }

    /// expect: "My agents operate within my sovereignty boundaries"
    #[test]
    fn deny_all_consent_always_denies() {
        let consent = DenyAllConsent;
        assert!(!consent.has_consent("user:alice", &DataCategory::EpisodicMemory));
        assert!(!consent.has_consent("user:alice", &DataCategory::SemanticMemory));
        assert!(!consent.has_consent("user:bob", &DataCategory::EpisodicMemory));
    }

    /// expect: "My agents operate within my sovereignty boundaries"
    #[test]
    fn sovereignty_checker_sovereign_data_requires_consent_and_owner() {
        let owner = test_webid();
        let consent = Arc::new(DenyAllConsent);
        let checker = SovereigntyChecker::new(owner, consent);

        // Sovereign data with DenyAllConsent: denied even for owner
        assert!(!checker.can_access(&DataCategory::EpisodicMemory, &owner));

        // With AllowAllConsent: owner can access sovereign data
        let consent = Arc::new(AllowAllConsent);
        let checker = SovereigntyChecker::new(owner, consent);
        assert!(checker.can_access(&DataCategory::EpisodicMemory, &owner));

        // But a different requester is still denied (not the owner)
        let other = test_webid();
        assert!(!checker.can_access(&DataCategory::EpisodicMemory, &other));
    }
}
