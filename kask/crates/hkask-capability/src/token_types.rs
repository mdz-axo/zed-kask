//! Delegation token types — in-process capability tokens.
//!
//! A `DelegationToken` declares "holder X may perform action Y on resource Z".
//! Tokens are minted and consumed in-process (the composition root hands them
//! to `McpRuntime::invoke`); there is no untrusted transport boundary, so the
//! token carries no signature and no public key. The enforced gate is the
//! capability match in `McpRuntime::invoke` (`is_valid_for` /
//! `verify_capability_domain`), not cryptography.

use hkask_types::WebID;
use sha2::{Digest, Sha256};

use super::resources::{DelegationAction, DelegationResource};

/// Shared structural bound: cascade depth and subgoal nesting (the matryoshka
/// limit consulted by the manifest executor and the registry bootstrap).
pub const SYSTEM_MAX_RECURSION: u8 = 7;

/// In-process capability token for inter-agent delegation.
///
/// Minted by the composition root (`panel_default_token`) and checked by
/// `McpRuntime::invoke`'s capability-match gate. Carries no cryptographic
/// material — see the module docs.
#[derive(Debug, Clone)]
pub struct DelegationToken {
    pub id: String,
    pub resource: DelegationResource,
    pub resource_id: String,
    pub action: DelegationAction,
    pub delegated_from: WebID,
    pub delegated_to: WebID,
}

impl DelegationToken {
    /// Mint a token for in-process tool invocation.
    #[must_use]
    pub fn new(
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        delegated_from: WebID,
        delegated_to: WebID,
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

    /// Authorization predicate: the token matches `(resource, resource_id, action)`.
    /// This is the gate `McpRuntime::invoke` consults.
    ///
    /// post: returns true iff self.resource == resource AND self.resource_id == resource_id
    ///       AND self.action == action
    pub fn is_valid_for(
        &self,
        resource: DelegationResource,
        resource_id: &str,
        action: DelegationAction,
    ) -> bool {
        self.resource == resource && self.resource_id == resource_id && self.action == action
    }
}

