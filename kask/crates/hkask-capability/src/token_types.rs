//! Delegation token types — in-process capability tokens.
//!
//! A `DelegationToken` declares "holder X may perform action Y on resource Z".
//! Tokens are minted and consumed in-process (the composition root hands them
//! to `McpRuntime::invoke`); there is no untrusted transport boundary, so the
//! token carries no signature and no public key. The enforced gate is the
//! capability match in `McpRuntime::invoke` (`is_valid_for` /
//! `verify_capability_domain`), not cryptography.

use hkask_types::{NotFound, WebID};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::resources::{DelegationAction, DelegationResource};

/// Capability-domain errors.
#[derive(Debug, Error)]
pub enum CapabilityError {
    #[error("{0}")]
    Other(String),
}

/// Shared structural bound: capability attenuation, cascade depth, subgoal nesting.
pub const SYSTEM_MAX_RECURSION: u8 = 7;

/// Capability-domain alias for SYSTEM_MAX_RECURSION.
pub const SYSTEM_MAX_ATTENUATION: u8 = SYSTEM_MAX_RECURSION;

/// Additive restrictions on a capability token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Caveat {
    pub caveat_id: String,
    pub data: String,
}

/// In-process capability token for inter-agent delegation.
///
/// Minted by the composition root (`panel_default_token`) and checked by
/// `McpRuntime::invoke`'s capability-match gate. Carries no cryptographic
/// material — see the module docs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationToken {
    pub id: String,
    pub resource: DelegationResource,
    pub resource_id: String,
    pub action: DelegationAction,
    pub delegated_from: WebID,
    pub delegated_to: WebID,
    pub expires_at: Option<i64>,
    /// 0 = full authority, increases with each delegation
    pub attenuation_level: u8,
    pub max_attenuation: u8,
    pub context_nonce: String,
    pub caveats: Vec<Caveat>,
}

/// Builder for constructing delegation tokens.
#[derive(Debug, Clone)]
pub struct DelegationTokenBuilder {
    resource: DelegationResource,
    resource_id: String,
    action: DelegationAction,
    delegated_from: WebID,
    delegated_to: WebID,
    expires_at: Option<i64>,
    attenuation_level: u8,
    max_attenuation: u8,
    context_nonce: Option<String>,
    caveats: Vec<Caveat>,
}

impl DelegationTokenBuilder {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  resource is any [`DelegationResource`]; resource_id is any non-empty [`String`];
    ///       action is any [`DelegationAction`]; delegated_from and delegated_to are any [`WebID`]
    /// post: returns a [`DelegationTokenBuilder`] with default expiry (None), attenuation_level 0,
    ///       max_attenuation [`SYSTEM_MAX_ATTENUATION`], no context_nonce, and empty caveats
    pub fn new(
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        delegated_from: WebID,
        delegated_to: WebID,
    ) -> Self {
        Self {
            resource,
            resource_id,
            action,
            delegated_from,
            delegated_to,
            expires_at: None,
            attenuation_level: 0,
            max_attenuation: SYSTEM_MAX_ATTENUATION,
            context_nonce: None,
            caveats: Vec::new(),
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    pub fn expires_at(mut self, timestamp: i64) -> Self {
        self.expires_at = Some(timestamp);
        self
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    pub fn attenuation_level(mut self, level: u8) -> Self {
        self.attenuation_level = level;
        self
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    pub fn context_nonce(mut self, nonce: String) -> Self {
        self.context_nonce = Some(nonce);
        self
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    pub fn caveat(mut self, caveat: Caveat) -> Self {
        self.caveats.push(caveat);
        self
    }

    /// Finalize the token. The id is a deterministic content hash.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns a [`DelegationToken`] whose id deterministically encodes
    ///       resource + resource_id + action + from + to
    pub fn build(self) -> DelegationToken {
        let id = DelegationToken::generate_id(
            &self.resource,
            &self.resource_id,
            &self.action,
            &self.delegated_from,
            &self.delegated_to,
        );
        DelegationToken {
            id,
            resource: self.resource,
            resource_id: self.resource_id,
            action: self.action,
            delegated_from: self.delegated_from,
            delegated_to: self.delegated_to,
            expires_at: self.expires_at,
            attenuation_level: self.attenuation_level,
            max_attenuation: self.max_attenuation,
            context_nonce: self.context_nonce.unwrap_or_default(),
            caveats: self.caveats,
        }
    }
}

impl DelegationToken {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  resource is any [`DelegationResource`]; resource_id is any non-empty [`String`];
    ///       action is any [`DelegationAction`]; delegated_from and delegated_to are any [`WebID`]
    /// post: returns a [`DelegationToken`] with default settings (no expiry, attenuation 0,
    ///       empty context_nonce); equivalent to `DelegationTokenBuilder::new(...).build()`
    pub fn new(
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        delegated_from: WebID,
        delegated_to: WebID,
    ) -> Self {
        DelegationTokenBuilder::new(resource, resource_id, action, delegated_from, delegated_to)
            .build()
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

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; resource is any [`DelegationResource`];
    ///       resource_id is any &str; action is any [`DelegationAction`]
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

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns true if the token's resource matches; false otherwise
    pub fn grants_resource(&self, resource: DelegationResource) -> bool {
        self.resource == resource
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns true if `context_nonce` starts with `expected_context` (prefix match);
    ///       returns false otherwise
    pub fn validate_context_nonce(&self, expected_context: &str) -> bool {
        self.context_nonce.starts_with(expected_context)
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns true if the token grants write-level action
    pub fn allows_write(&self) -> bool {
        matches!(
            self.action,
            DelegationAction::Write | DelegationAction::Execute
        )
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns true if the token grants any read-capable action
    pub fn allows_read(&self) -> bool {
        matches!(
            self.action,
            DelegationAction::Read | DelegationAction::Write | DelegationAction::Execute
        )
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns true if the token is past its expiry (or has no expiry set and
    ///       `now` is irrelevant)
    pub fn is_expired(&self, now: i64) -> bool {
        self.expires_at.is_some_and(|exp| now > exp)
    }

    /// Caveat id strings; empty if no caveats.
    pub fn caveat_ids(&self) -> Vec<&str> {
        self.caveats.iter().map(|c| c.caveat_id.as_str()).collect()
    }
}

/// Token registry errors.
#[derive(Debug, Error)]
pub enum TokenRegistryError {
    #[error("Token not found: {0}")]
    NotFound(NotFound),
    #[error("Duplicate token: {0}")]
    Duplicate(String),
    #[error("Storage error: {0}")]
    Storage(String),
}

/// Consent-audit trail for the DelegationToken lifecycle.
///
/// The production implementation is `TokenRegistryStore` in `hkask-storage`,
/// consumed by the curator MCP server's `list_tokens` tool (consent auditing
/// and anomaly detection). Note: revocation is a *recording* surface — the
/// in-process capability-match gate in `McpRuntime::invoke` does not consult
/// the registry. Do not describe this as an authorization gate.
pub trait TokenRegistry: Send + Sync {
    /// Record an issued token.
    fn store(&self, token: &DelegationToken) -> Result<(), TokenRegistryError>;

    /// Fetch a token by id.
    fn get(&self, token_id: &str) -> Result<Option<DelegationToken>, TokenRegistryError>;

    /// Query tokens issued by a WebID since a timestamp.
    fn query_by_issuer(
        &self,
        webid: &WebID,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError>;

    /// Query tokens issued to a WebID since a timestamp.
    fn query_by_recipient(
        &self,
        webid: &WebID,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError>;

    /// Query all tokens since a timestamp.
    fn query_all(
        &self,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError>;

    /// Mark a token as revoked (audit record only — see trait docs).
    fn revoke(&self, token_id: &str) -> Result<(), TokenRegistryError>;
}
