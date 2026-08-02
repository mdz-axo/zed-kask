//! Delegation token types — in-process capability tokens.
//!
//! A `DelegationToken` declares "holder X may perform action Y on resource Z".
//! Tokens are minted and consumed in-process (the composition root hands them
//! to `McpRuntime::invoke`); there is no untrusted transport boundary, so the
//! token carries no signature and no public key. The enforced gate is the
//! capability match in `McpRuntime::invoke` (`is_valid_for_at` /
//! `verify_capability_domain`), not cryptography.

use hkask_types::WebID;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::resources::{DelegationAction, DelegationResource};

/// Shared structural bound: cascade depth and subgoal nesting (the matryoshka
/// limit consulted by the manifest executor and the registry bootstrap).
pub const SYSTEM_MAX_RECURSION: u8 = 7;

/// In-process capability token for inter-agent delegation.
///
/// Minted by the composition root (`panel_default_token` / `new_with_expiry`)
/// and checked by `McpRuntime::invoke`'s capability-match gate. Carries no
/// cryptographic material — see the module docs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationToken {
    pub id: String,
    pub resource: DelegationResource,
    pub resource_id: String,
    pub action: DelegationAction,
    pub delegated_from: WebID,
    pub delegated_to: WebID,
    pub expires_at: Option<i64>,
}

impl DelegationToken {
    /// Mint a token with no expiry (ad-hoc call path — never expires).
    #[must_use]
    pub fn new(
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        delegated_from: WebID,
        delegated_to: WebID,
    ) -> Self {
        Self::build(
            resource,
            resource_id,
            action,
            delegated_from,
            delegated_to,
            None,
        )
    }

    /// Mint a token that expires at `expires_at` (cascade path —
    /// `ocap.capability_expiry_seconds` from the manifest).
    #[must_use]
    pub fn new_with_expiry(
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        delegated_from: WebID,
        delegated_to: WebID,
        expires_at: i64,
    ) -> Self {
        Self::build(
            resource,
            resource_id,
            action,
            delegated_from,
            delegated_to,
            Some(expires_at),
        )
    }

    fn build(
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        delegated_from: WebID,
        delegated_to: WebID,
        expires_at: Option<i64>,
    ) -> Self {
        let id = Self::generate_id(
            &resource,
            &resource_id,
            &action,
            &delegated_from,
            &delegated_to,
        );
        Self {
            id,
            resource,
            resource_id,
            action,
            delegated_from,
            delegated_to,
            expires_at,
        }
    }

    fn generate_id(
        resource: &DelegationResource,
        resource_id: &str,
        action: &DelegationAction,
        from: &WebID,
        to: &WebID,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(resource.as_str().as_bytes());
        hasher.update(resource_id.as_bytes());
        hasher.update(action.as_str().as_bytes());
        hasher.update(from.to_string().as_bytes());
        hasher.update(to.to_string().as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Authorization predicate: the token matches `(resource, resource_id, action)`
    /// AND has not expired. This is the gate `McpRuntime::invoke` consults.
    ///
    /// `now` is taken explicitly so the gate and tests are deterministic.
    ///
    /// post: returns true iff self.resource == resource AND self.resource_id == resource_id
    ///       AND self.action == action AND !self.is_expired(now)
    pub fn is_valid_for_at(
        &self,
        resource: DelegationResource,
        resource_id: &str,
        action: DelegationAction,
        now: i64,
    ) -> bool {
        self.resource == resource
            && self.resource_id == resource_id
            && self.action == action
            && !self.is_expired(now)
    }

    /// post: returns true if the token is past its expiry (or has no expiry set and
    ///       `now` is irrelevant)
    pub fn is_expired(&self, now: i64) -> bool {
        self.expires_at.is_some_and(|exp| now > exp)
    }
}
