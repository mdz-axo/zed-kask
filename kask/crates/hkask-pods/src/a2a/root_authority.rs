//! Root Authority for OCAP capability delegation
//!
//! All capability tokens trace back to a root authority. The root authority
//! is the ultimate source of all capabilities in the system.
//!
//! # OCAP Discipline
//!
//! - No ambient authority: capabilities must be explicitly granted
//! - Attenuation chain: each delegation reduces authority

use ed25519_dalek::SigningKey;
use hkask_capability::{
    DelegationAction, DelegationResource, DelegationToken, DelegationTokenBuilder,
    SYSTEM_MAX_ATTENUATION,
};
use hkask_types::WebID;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::A2AError;

/// Root authority for OCAP capability delegation
///
/// All capability tokens trace back to a root authority. The root authority
/// is the ultimate source of all capabilities in the system.
///
/// # OCAP Discipline
///
/// \[NORMATIVE\] - No ambient authority: capabilities must be explicitly granted (P4 — Clear Boundaries).
/// - Attenuation chain: each delegation reduces authority
pub(crate) struct RootAuthority {
    /// Root authority WebID (system identity)
    root_webid: WebID,
    /// Ed25519 signing key for token issuance
    signing_key: SigningKey,
    /// Next token ID counter
    token_counter: Arc<RwLock<u64>>,
}

impl RootAuthority {
    /// Create new root authority.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — root authority is the capability issuer
    /// pre:  `root_webid` is a valid `WebID`; `signing_key` is a valid
    ///       Ed25519 `SigningKey`.
    /// post: Returns a `RootAuthority` with token counter initialized to 0.
    pub fn new(root_webid: WebID, signing_key: &SigningKey) -> Self {
        Self {
            root_webid,
            signing_key: signing_key.clone(),
            token_counter: Arc::new(RwLock::new(0)),
        }
    }

    /// Create root capability token.
    ///
    /// This is the starting point of an attenuation chain.
    /// Root tokens have attenuation_level=0 and max_attenuation=7.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — root tokens start the delegation chain
    /// \[P7\] Constraining: Evolutionary Architecture — attenuation limits emerged from usage
    /// pre:  `resource` is a valid `DelegationResource`; `resource_id` is
    ///       a non-empty string; `action` is a valid `DelegationAction`;
    ///       `delegated_to` is a valid `WebID`.
    /// post: Returns `Ok(DelegationToken)` — a signed root token with
    ///       attenuation_level=0, max_attenuation=SYSTEM_MAX_ATTENUATION,
    ///       and a unique context nonce.
    pub async fn create_root_token(
        &self,
        resource: DelegationResource,
        resource_id: String,
        action: DelegationAction,
        delegated_to: WebID,
    ) -> Result<DelegationToken, A2AError> {
        let token_id = {
            let mut counter = self.token_counter.write().await;
            *counter += 1;
            *counter
        };

        let context_nonce = format!("root-{}-{}", self.root_webid, token_id);

        let token = DelegationTokenBuilder::new(
            resource,
            resource_id,
            action,
            self.root_webid,
            delegated_to,
            &self.signing_key,
        )
        .attenuation(0, SYSTEM_MAX_ATTENUATION)
        .context_nonce(context_nonce)
        .sign();

        Ok(token)
    }
}
