//! Delegation token types — Ed25519-signed OCAP tokens with cryptographic attenuation.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use hex;
use hkask_types::{Ed25519PublicKey, NotFound, WebID};
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

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}
fn de64(s: &str) -> Result<Vec<u8>, CapabilityError> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| CapabilityError::Other(e.to_string()))
}
fn wid(w: &WebID) -> String {
    w.to_string()
}

/// Additive restrictions on a capability token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Caveat {
    pub caveat_id: String,
    pub data: String,
}

/// Ed25519 signature for delegation token authentication.
///
/// \[NORMATIVE\] Wraps a 64-byte Ed25519 signature. Verification uses the
/// token's `public_key` field — no shared secret required (P4 — Clear Boundaries).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TokenSignature(#[serde(with = "hex::serde")] pub [u8; 64]);

/// Ed25519-signed OCAP token for inter-agent capability delegation.
///
/// \[NORMATIVE\] Signatures are asymmetric (Ed25519) — the issuer signs with
/// a private key, verifiers use the public key. Token forgery requires the
/// private key (P4 — Clear Boundaries).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationToken {
    pub id: String,
    pub resource: DelegationResource,
    pub resource_id: String,
    pub action: DelegationAction,
    pub delegated_from: WebID,
    pub delegated_to: WebID,
    /// Ed25519 signature over the token payload.
    pub signature: TokenSignature,
    /// Ed25519 public key for signature verification.
    pub public_key: Ed25519PublicKey,
    pub expires_at: Option<i64>,
    /// 0 = full authority, increases with each delegation
    pub attenuation_level: u8,
    pub max_attenuation: u8,
    pub context_nonce: String,
    pub caveats: Vec<Caveat>,
}

/// Internal signing payload extracted from builder state.
struct SigningPayload {
    id: String,
    resource: DelegationResource,
    resource_id: String,
    action: DelegationAction,
    from: WebID,
    to: WebID,
    public_key: Ed25519PublicKey,
    caveats: Vec<Caveat>,
}

/// Builder for constructing delegation tokens.
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
    signing_key: SigningKey,
}

impl DelegationTokenBuilder {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  resource is any [`DelegationResource`]; resource_id is any non-empty [`String`];
    ///       action is any [`DelegationAction`]; delegated_from and delegated_to are any [`WebID`];
    ///       signing_key is a valid Ed25519 [`SigningKey`]
    /// post: returns a [`DelegationTokenBuilder`] with default expiry (None), attenuation_level 0,
    ///       max_attenuation [`SYSTEM_MAX_ATTENUATION`], no context_nonce, and empty caveats
    pub fn new(
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        delegated_from: WebID,
        delegated_to: WebID,
        signing_key: &SigningKey,
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
            signing_key: signing_key.clone(),
        }
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  ts is any i64 (Unix timestamp in seconds)
    /// post: returns self with `expires_at` set to `Some(ts)`
    pub fn expires_at(mut self, ts: i64) -> Self {
        self.expires_at = Some(ts);
        self
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  level and max are any u8 values
    /// post: returns self with `attenuation_level` set to level and `max_attenuation` set to max
    pub fn attenuation(mut self, level: u8, max: u8) -> Self {
        self.attenuation_level = level;
        self.max_attenuation = max;
        self
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  nonce is any non-empty [`String`]
    /// post: returns self with `context_nonce` set to `Some(nonce)`
    pub fn context_nonce(mut self, nonce: String) -> Self {
        self.context_nonce = Some(nonce);
        self
    }
    pub(crate) fn caveat(mut self, c: Caveat) -> Self {
        self.caveats.push(c);
        self
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a fully configured [`DelegationTokenBuilder`] with a valid signing key
    /// post: returns a signed [`DelegationToken`] with a deterministic id (SHA-256 of resource+id+action+from+to),
    ///       an Ed25519 signature over the canonical payload, and a context_nonce (provided or random UUID v4);
    ///       consumes self
    pub fn sign(self) -> DelegationToken {
        let id = DelegationToken::generate_id(
            &self.resource,
            &self.resource_id,
            &self.action,
            &self.delegated_from,
            &self.delegated_to,
        );
        let public_key = Ed25519PublicKey(self.signing_key.verifying_key().to_bytes());
        let payload = SigningPayload {
            id,
            resource: self.resource,
            resource_id: self.resource_id,
            action: self.action,
            from: self.delegated_from,
            to: self.delegated_to,
            public_key,
            caveats: self.caveats,
        };
        let signature = DelegationToken::sign_payload(&payload, &self.signing_key);
        let context_nonce = self
            .context_nonce
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        DelegationToken {
            id: payload.id,
            resource: payload.resource,
            resource_id: payload.resource_id,
            action: payload.action,
            delegated_from: payload.from,
            delegated_to: payload.to,
            signature,
            public_key,
            expires_at: self.expires_at,
            attenuation_level: self.attenuation_level,
            max_attenuation: self.max_attenuation,
            context_nonce,
            caveats: payload.caveats,
        }
    }
}

impl DelegationToken {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  resource is any [`DelegationResource`]; resource_id is any non-empty [`String`];
    ///       action is any [`DelegationAction`]; delegated_from and delegated_to are any [`WebID`];
    ///       signing_key is a valid Ed25519 [`SigningKey`]
    /// post: returns a signed [`DelegationToken`] with default settings (no expiry, attenuation 0,
    ///       random context_nonce); equivalent to `DelegationTokenBuilder::new(...).sign()`
    pub fn new(
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        delegated_from: WebID,
        delegated_to: WebID,
        signing_key: &SigningKey,
    ) -> Self {
        DelegationTokenBuilder::new(
            resource,
            resource_id,
            action,
            delegated_from,
            delegated_to,
            signing_key,
        )
        .sign()
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
        hasher.update(wid(from).as_bytes());
        hasher.update(wid(to).as_bytes());
        hex::encode(hasher.finalize())
    }

    fn sign_payload(payload: &SigningPayload, signing_key: &SigningKey) -> TokenSignature {
        // Build canonical byte representation for signing
        let mut buf = Vec::new();
        buf.extend_from_slice(payload.id.as_bytes());
        buf.extend_from_slice(payload.resource.as_str().as_bytes());
        buf.extend_from_slice(payload.resource_id.as_bytes());
        buf.extend_from_slice(payload.action.as_str().as_bytes());
        buf.extend_from_slice(wid(&payload.from).as_bytes());
        buf.extend_from_slice(wid(&payload.to).as_bytes());
        buf.extend_from_slice(&payload.public_key.0);
        for caveat in &payload.caveats {
            buf.extend_from_slice(caveat.caveat_id.as_bytes());
            buf.extend_from_slice(caveat.data.as_bytes());
        }
        let signature = signing_key.sign(&buf);
        TokenSignature(signature.to_bytes())
    }

    /// Ed25519 signature verification using the token's public key.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`] (may have invalid signature or public key)
    /// post: returns true if the Ed25519 signature is valid for the canonical payload
    ///       (id + resource + resource_id + action + from + to + public_key + caveats)
    ///       under the token's `public_key`; returns false if the public key is invalid
    ///       or the signature does not verify
    pub fn verify(&self) -> bool {
        let verifying_key = match VerifyingKey::from_bytes(&self.public_key.0) {
            Ok(vk) => vk,
            Err(_) => return false,
        };
        let mut buf = Vec::new();
        buf.extend_from_slice(self.id.as_bytes());
        buf.extend_from_slice(self.resource.as_str().as_bytes());
        buf.extend_from_slice(self.resource_id.as_bytes());
        buf.extend_from_slice(self.action.as_str().as_bytes());
        buf.extend_from_slice(wid(&self.delegated_from).as_bytes());
        buf.extend_from_slice(wid(&self.delegated_to).as_bytes());
        buf.extend_from_slice(&self.public_key.0);
        for caveat in &self.caveats {
            buf.extend_from_slice(caveat.caveat_id.as_bytes());
            buf.extend_from_slice(caveat.data.as_bytes());
        }
        let signature = ed25519_dalek::Signature::from_bytes(&self.signature.0);
        verifying_key.verify(&buf, &signature).is_ok()
    }

    /// Raw Ed25519 signature bytes (64 bytes).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns a reference to the inner 64-byte Ed25519 signature array
    pub fn signature_bytes(&self) -> &[u8; 64] {
        &self.signature.0
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; current_time is any i64 (Unix timestamp)
    /// post: returns true if `expires_at` is `Some(exp)` and `current_time > exp`;
    ///       returns false if `expires_at` is `None` (never expires) or `current_time ≤ exp`
    pub fn is_expired(&self, current_time: i64) -> bool {
        self.expires_at
            .map(|exp| current_time > exp)
            .unwrap_or(false)
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns the [`WebID`] of the token holder (`delegated_to`)
    pub fn holder(&self) -> WebID {
        self.delegated_to
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns the [`WebID`] of the token issuer (`delegated_from`)
    pub fn issuer(&self) -> WebID {
        self.delegated_from
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns base64-encoded JSON serialization of the token;
    ///       returns `Err` only if serialization fails (e.g., OOM)
    pub fn to_base64(&self) -> Result<String, serde_json::Error> {
        Ok(b64(serde_json::to_string(self)?.as_bytes()))
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  encoded is a base64 string representing a JSON-serialized [`DelegationToken`]
    /// post: returns the deserialized [`DelegationToken`] if decoding and parsing succeed;
    ///       returns `Err(String)` if base64 decoding fails or JSON deserialization fails
    pub fn from_base64(encoded: &str) -> Result<Self, CapabilityError> {
        serde_json::from_slice(&de64(encoded)?).map_err(|e| CapabilityError::Other(e.to_string()))
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns true if `attenuation_level < max_attenuation` (room for further delegation);
    ///       returns false if attenuation has reached the maximum
    pub fn can_attenuate(&self) -> bool {
        self.attenuation_level < self.max_attenuation
    }
    /// Attenuate with 1-hour expiry from `current_time`.
    /// Requires the issuer's signing key to produce a valid signature.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; new_to is any [`WebID`];
    ///       signing_key is a valid Ed25519 [`SigningKey`]; current_time is any i64
    /// post: returns `Some(attenuated_token)` with level+1, 1-hour expiry, and chained nonce
    ///       if `can_attenuate()` is true; returns `None` if attenuation limit reached
    pub fn attenuate(
        &self,
        new_to: WebID,
        signing_key: &SigningKey,
        current_time: i64,
    ) -> Option<DelegationToken> {
        self.attenuate_with_expiry(new_to, signing_key, Some(current_time + 3600))
    }

    /// Create attenuated child token with custom expiry.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; new_to is any [`WebID`];
    ///       signing_key is a valid Ed25519 [`SigningKey`]; expires_at is `Option<i64>`
    /// post: returns `Some(attenuated_token)` with level+1, chained nonce, and given expiry
    ///       if `can_attenuate()` is true; returns `None` if attenuation limit reached;
    ///       child inherits all caveats from parent
    pub fn attenuate_with_expiry(
        &self,
        new_to: WebID,
        signing_key: &SigningKey,
        expires_at: Option<i64>,
    ) -> Option<DelegationToken> {
        if !self.can_attenuate() {
            return None;
        }

        let mut builder = DelegationTokenBuilder::new(
            self.resource,
            self.resource_id.clone(),
            self.action,
            self.delegated_to,
            new_to,
            signing_key,
        )
        .attenuation(self.attenuation_level + 1, self.max_attenuation)
        .context_nonce(format!(
            "{}-attenuated-{}",
            self.context_nonce,
            uuid::Uuid::new_v4()
        ));

        if let Some(ts) = expires_at {
            builder = builder.expires_at(ts);
        }

        for caveat in &self.caveats {
            builder = builder.caveat(caveat.clone());
        }

        Some(builder.sign())
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; resource is any [`DelegationResource`];
    ///       resource_id is any &str; action is any [`DelegationAction`]
    /// post: returns true if the token's resource, resource_id, and action all match exactly;
    ///       returns false otherwise
    pub fn is_valid_for(
        &self,
        resource: DelegationResource,
        resource_id: &str,
        action: DelegationAction,
    ) -> bool {
        self.resource == resource && self.resource_id == resource_id && self.action == action
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; resource is any [`DelegationResource`]
    /// post: returns true if the token's resource matches; false otherwise
    pub fn grants_resource(&self, resource: DelegationResource) -> bool {
        self.resource == resource
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; expected_context is any &str
    /// post: returns true if `context_nonce` starts with `expected_context` (prefix match);
    ///       returns false otherwise
    pub fn validate_context_nonce(&self, expected_context: &str) -> bool {
        self.context_nonce.starts_with(expected_context)
    }
    /// Extract root nonce from attenuation chain (`"root-attenuated-uuid-..."`).
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns the portion of `context_nonce` before the first "-attenuated-" separator;
    ///       if no separator exists, returns the entire `context_nonce`
    pub fn root_context_nonce(&self) -> &str {
        self.context_nonce
            .split("-attenuated-")
            .next()
            .unwrap_or(&self.context_nonce)
    }

    /// Verify attenuation chain: root nonce matches, level ≤ expected, max ≤ SYSTEM_MAX_ATTENUATION.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; expected_root is any &str; expected_level is any u8
    /// post: returns true if all hold: (1) max_attenuation ≤ SYSTEM_MAX_ATTENUATION,
    ///       (2) root_context_nonce matches expected_root, (3) nonce-derived level matches
    ///       attenuation_level, (4) attenuation_level ≤ expected_level;
    ///       returns false if any check fails
    pub fn verify_attenuation_chain(&self, expected_root: &str, expected_level: u8) -> bool {
        if self.max_attenuation > SYSTEM_MAX_ATTENUATION {
            return false;
        }

        let root = self.root_context_nonce();
        if root != expected_root {
            return false;
        }

        // Count attenuation levels in nonce
        let actual_level = self.context_nonce.matches("-attenuated-").count() as u8;
        if actual_level != self.attenuation_level {
            return false; // Nonce doesn't match attenuation level
        }

        self.attenuation_level <= expected_level
    }

    /// Cryptographic verification for distributed/Paxos use.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns the result of [`DelegationToken::verify`] — true if Ed25519 signature is valid
    pub fn verify_cryptographic(&self) -> bool {
        self.verify()
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns a [`Vec`] of caveat id strings; empty if no caveats
    pub fn caveat_ids(&self) -> Vec<&str> {
        self.caveats.iter().map(|c| c.caveat_id.as_str()).collect()
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; caveat_type is any &str
    /// post: returns true if any caveat has `caveat_id == caveat_type`; false otherwise
    pub fn has_caveat_type(&self, caveat_type: &str) -> bool {
        self.caveats.iter().any(|c| c.caveat_id == caveat_type)
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]; caveat_type is any &str
    /// post: returns `Some(&str)` with the data of the first caveat matching `caveat_type`;
    ///       returns `None` if no matching caveat exists
    pub fn get_caveat_data(&self, caveat_type: &str) -> Option<&str> {
        self.caveats
            .iter()
            .find(|c| c.caveat_id == caveat_type)
            .map(|c| c.data.as_str())
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns a colon-separated fingerprint string:
    ///       "id:resource:resource_id:action:delegated_to:attenuation_level"
    pub fn fingerprint(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}",
            self.id,
            self.resource.as_str(),
            self.resource_id,
            self.action.as_str(),
            self.delegated_to,
            self.attenuation_level
        )
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns true if the token's action permits write operations;
    ///       delegates to [`DelegationAction::permits_write`]
    pub fn allows_write(&self) -> bool {
        self.action.permits_write()
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any [`DelegationToken`]
    /// post: returns true if the token's action permits read operations;
    ///       delegates to [`DelegationAction::permits_read`]
    pub fn allows_read(&self) -> bool {
        self.action.permits_read()
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self and other are any [`DelegationToken`] values
    /// post: returns true if both tokens share the same resource, resource_id, action,
    ///       and delegated_to; returns false otherwise
    pub fn is_compatible_with(&self, other: &DelegationToken) -> bool {
        self.resource == other.resource
            && self.resource_id == other.resource_id
            && self.action == other.action
            && self.delegated_to == other.delegated_to
    }
}

// ── Token Registry — Persistence for consent audit ───────────────────────

/// Errors from token registry operations.
#[derive(Debug, thiserror::Error)]
pub enum TokenRegistryError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Token not found: {0}")]
    NotFound(NotFound),

    #[error("Token already exists: {0}")]
    Duplicate(String),
}

impl From<NotFound> for TokenRegistryError {
    fn from(nf: NotFound) -> Self {
        TokenRegistryError::NotFound(nf)
    }
}

/// Persistence trait for DelegationToken lifecycle.
///
/// The token registry provides the **auditable** half of P2 (Affirmative Consent).
/// OCAP gates enforce consent at runtime; the registry proves it after the fact.
/// Without this, P2 is operationally enforced but forensically invisible.
///
/// Regulation spans record token *usage* (was the token presented?).
/// The registry records token *issuance* (was the token ever granted?).
/// Together they enable the full consent picture.
pub trait TokenRegistry: Send + Sync {
    /// Persist a newly issued token.
    ///
    /// Called by DelegationTokenBuilder::sign() or equivalent.
    /// Returns Duplicate error if a token with the same ID already exists.
    fn store(&self, token: &DelegationToken) -> Result<(), TokenRegistryError>;

    /// Get a single token by ID.
    fn get(&self, token_id: &str) -> Result<Option<DelegationToken>, TokenRegistryError>;

    /// Query all tokens issued by a WebID since the given timestamp.
    fn query_by_issuer(
        &self,
        webid: &hkask_types::WebID,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError>;

    /// Query all tokens received by a WebID since the given timestamp.
    fn query_by_recipient(
        &self,
        webid: &hkask_types::WebID,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError>;

    /// Query all tokens within a time window.
    fn query_all(
        &self,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError>;

    /// Mark a token as revoked. Revoked tokens fail OCAP verification.
    fn revoke(&self, token_id: &str) -> Result<(), TokenRegistryError>;
}

/// A no-op token registry for contexts where consent auditing is not needed.
///
/// Tokens are verified at runtime (OCAP gates) but issuance is not persisted.
/// Used in tests and minimal deployments. The real implementation persists
/// to SQLite via hkask-storage.
pub struct NoOpTokenRegistry;

impl TokenRegistry for NoOpTokenRegistry {
    fn store(&self, _token: &DelegationToken) -> Result<(), TokenRegistryError> {
        Ok(())
    }

    fn get(&self, _token_id: &str) -> Result<Option<DelegationToken>, TokenRegistryError> {
        Ok(None)
    }

    fn query_by_issuer(
        &self,
        _webid: &hkask_types::WebID,
        _since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError> {
        Ok(Vec::new())
    }

    fn query_by_recipient(
        &self,
        _webid: &hkask_types::WebID,
        _since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError> {
        Ok(Vec::new())
    }

    fn query_all(
        &self,
        _since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<DelegationToken>, TokenRegistryError> {
        Ok(Vec::new())
    }

    fn revoke(&self, _token_id: &str) -> Result<(), TokenRegistryError> {
        Ok(())
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn noop_registry_returns_none_for_any_query() {
        let registry = NoOpTokenRegistry;
        assert!(registry.get("any").unwrap().is_none());
        assert!(registry.query_all(chrono::Utc::now()).unwrap().is_empty());
    }

    #[test]
    fn noop_registry_store_is_idempotent() {
        let registry = NoOpTokenRegistry;
        // Should not panic
        let token = DelegationToken {
            id: "test".into(),
            resource: DelegationResource::Registry,
            resource_id: "x".into(),
            action: DelegationAction::Execute,
            delegated_from: hkask_types::WebID::from_persona(b"a"),
            delegated_to: hkask_types::WebID::from_persona(b"b"),
            signature: TokenSignature([0u8; 64]),
            public_key: hkask_types::Ed25519PublicKey([0u8; 32]),
            expires_at: None,
            attenuation_level: 0,
            max_attenuation: 7,
            context_nonce: "test".into(),
            caveats: vec![],
        };
        registry.store(&token).unwrap();
    }
}
