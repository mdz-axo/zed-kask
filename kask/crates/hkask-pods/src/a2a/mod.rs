//! A2A (Agent-to-Agent Protocol) Runtime Integration
//!
//! This module provides A2A runtime adapters for agent registration,
//! A2A message handling, and capability-gated communication.
//!
//! # Security Model
//!
//! - **No hardcoded secrets**: Secret loaded from environment or keystore
//! - **Explicit capabilities**: No wildcard tokens; each capability explicitly granted
//! - **Audit logging**: All A2A messages logged for forensic analysis
//!
//! # A2A Message Flow
//!
//! ```text
//! Agent Pod A → A2A Message (template:dispatch) → hKask Router → Agent Pod B
//!                                                              ↓
//!                                                   Capability Verification
//!                                                              ↓
//!                                                   Audit Log Entry
//!                                                              ↓
//!                                                   Template Execution
//!                                                              ↓
//!                                                   Response to Agent A
//! ```

mod audit;

mod root_authority;

pub use audit::AuditEntry;
pub(crate) use audit::AuditLog;
pub(crate) use root_authority::RootAuthority;

use crate::types::audit::AuditOutcome;
use hkask_capability::{
    CapabilitySpec, DelegationAction, DelegationResource, DelegationToken, derive_signing_key,
};
use hkask_types::WebID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;
use zeroize::Zeroizing;

/// Parse a capability string using the canonical [`CapabilitySpec`] parser.
///
/// Converts `CapabilityParseError` into `A2AError::MalformedCapability`.
fn parse_capability(
    capability: &str,
) -> Result<(DelegationResource, String, DelegationAction), A2AError> {
    let spec = CapabilitySpec::parse(capability)
        .map_err(|e| A2AError::MalformedCapability(e.to_string()))?;
    Ok((spec.resource, spec.resource_id, spec.action))
}

/// A2A error types for security and validation
#[derive(Debug, Error)]
pub enum A2AError {
    #[error("Agent {0:?} already registered")]
    AgentAlreadyRegistered(WebID),

    #[error("Agent {0:?} not found")]
    AgentNotFound(WebID),

    #[error("Capability denied: agent {0:?} lacks permission for {1}")]
    CapabilityDenied(WebID, String),

    #[error("Invalid capability: wildcards not allowed")]
    WildcardCapabilityNotAllowed,

    #[error("Malformed capability: {0}")]
    MalformedCapability(String),

    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("Clock error: {0}")]
    ClockError(String),

    #[error("Key derivation failed: {0}")]
    KeyDerivation(#[from] hkask_keystore::KeystoreError),

    #[error(transparent)]
    Infra(#[from] hkask_types::InfrastructureError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AAgent {
    pub webid: WebID,
    /// Explicit capabilities — no wildcards
    pub capabilities: Vec<String>,
    pub registered_at: i64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "message_type")]
pub enum A2AMessage {
    /// Template dispatch
    TemplateDispatch {
        from: WebID,
        /// Recipient (or broadcast)
        to: Option<WebID>,
        template_id: String,
        input: serde_json::Value,
        correlation_id: String,
    },
    /// Template response
    TemplateResponse {
        correlation_id: String,
        result: serde_json::Value,
        error: Option<String>,
    },
    /// Memory artifact notification
    MemoryArtifact {
        producer: WebID,
        artifact_type: String,
        artifact_id: String,
        visibility: String,
    },
}

/// Variant payloads as named structs so visitors and external callers can
/// match on a type name rather than reach into enum-tuple field positions.
///
/// Each struct mirrors the corresponding `A2AMessage` arm. Constructed only by
/// `A2AMessage::visit`; not `pub` outside the module.
pub struct TemplateDispatch<'a> {
    pub from: &'a WebID,
    pub to: Option<WebID>,
    pub template_id: &'a str,
    pub input: &'a serde_json::Value,
    pub correlation_id: &'a str,
}

pub struct A2ATemplateResponse<'a> {
    pub correlation_id: &'a str,
    pub result: &'a serde_json::Value,
    pub error: Option<&'a str>,
}

pub struct MemoryArtifact<'a> {
    pub producer: &'a WebID,
    pub artifact_type: &'a str,
    pub artifact_id: &'a str,
    pub visibility: &'a str,
}

/// Visitor for `A2AMessage`.
///
/// Replaces match-on-variant for "ask the message a question" — adding a new
/// variant means adding one `on_*` method (default no-op) here and one arm to
/// `A2AMessage::visit`; the existing extraction sites do not change.
///
/// Every method is defaulted to a no-op so concrete visitors only override
/// the variants they care about (Visitor pattern, Gamma et al.).
pub trait A2AMessageVisitor {
    fn on_template_dispatch(&mut self, _msg: TemplateDispatch<'_>) {}
    fn on_template_response(&mut self, _msg: A2ATemplateResponse<'_>) {}
    fn on_memory_artifact(&mut self, _msg: MemoryArtifact<'_>) {}
}

// Compile-time guard: the visitor trait must remain object-safe because
// `A2AMessage::visit` takes `&mut dyn A2AMessageVisitor`. Wrapped in
// `#[allow(dead_code)]` because the body never runs at runtime.
#[allow(dead_code)]
const _: fn() = || {
    fn assert_obj_safe(_: &dyn A2AMessageVisitor) {}
};

impl A2AMessage {
    /// Dispatch a visitor over the variant. Single match site in the codebase.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — single dispatch site for A2A message variants
    /// pre:  `visitor` is a valid `&mut dyn A2AMessageVisitor`.
    /// post: Calls the appropriate visitor method based on the message
    ///       variant (`on_template_dispatch`, `on_template_response`,
    ///       or `on_memory_artifact`).
    pub fn visit(&self, visitor: &mut dyn A2AMessageVisitor) {
        match self {
            A2AMessage::TemplateDispatch {
                from,
                to,
                template_id,
                input,
                correlation_id,
            } => visitor.on_template_dispatch(TemplateDispatch {
                from,
                to: *to,
                template_id,
                input,
                correlation_id,
            }),
            A2AMessage::TemplateResponse {
                correlation_id,
                result,
                error,
            } => visitor.on_template_response(A2ATemplateResponse {
                correlation_id,
                result,
                error: error.as_deref(),
            }),
            A2AMessage::MemoryArtifact {
                producer,
                artifact_type,
                artifact_id,
                visibility,
            } => visitor.on_memory_artifact(MemoryArtifact {
                producer,
                artifact_type,
                artifact_id,
                visibility,
            }),
        }
    }

    /// Get the sender's WebID regardless of message type.
    ///
    /// Returns `Some` for `TemplateDispatch` (from) and `MemoryArtifact` (producer),
    /// `None` for `TemplateResponse` (no sender).
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — sender identity is explicit per variant
    /// \[P1\] Constraining: User Sovereignty — identity belongs to the agent/user
    /// pre:  (none).
    /// post: Returns `Some(&WebID)` for variants with a sender field;
    ///       `None` for `TemplateResponse`.
    pub fn from_webid(&self) -> Option<&WebID> {
        match self {
            A2AMessage::TemplateDispatch { from, .. } => Some(from),
            A2AMessage::TemplateResponse { .. } => None,
            A2AMessage::MemoryArtifact { producer, .. } => Some(producer),
        }
    }

    /// Get the correlation/artifact ID for this message.
    ///
    /// All variants carry an identifier that serves as a correlation key:
    /// `TemplateDispatch` and `TemplateResponse` use `correlation_id`,
    /// `MemoryArtifact` uses `artifact_id`.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — correlation/artifact IDs enable traceability
    /// pre:  (none).
    /// post: Returns the correlation/artifact ID string for the message
    ///       variant.
    pub fn correlation_id(&self) -> &str {
        match self {
            A2AMessage::TemplateDispatch { correlation_id, .. } => correlation_id,
            A2AMessage::TemplateResponse { correlation_id, .. } => correlation_id,
            A2AMessage::MemoryArtifact { artifact_id, .. } => artifact_id,
        }
    }

    /// Get a human-readable message type name.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P8\] Motivating: Semantic Grounding — stable message type labels
    /// pre:  (none).
    /// post: Returns a `&'static str`: `"template_dispatch"`,
    ///       `"template_response"`, or `"memory_artifact"`.
    pub fn message_type(&self) -> &'static str {
        match self {
            A2AMessage::TemplateDispatch { .. } => "template_dispatch",
            A2AMessage::TemplateResponse { .. } => "template_response",
            A2AMessage::MemoryArtifact { .. } => "memory_artifact",
        }
    }
}

// MDS P8 invariant: the visitor dispatch must visit exactly one variant,
// and `RouteFields` must produce the routing fields `send_message` consumes.
// The first test pins the bijection between enum variants and visitor
// methods; the rest pin the per-variant routing invariants.

/// Consolidated mutable A2A state behind a single lock (P2.1).
///
/// Replaces 5 independent `Arc<RwLock<....>` fields with one
/// lock, eliminating dead-lock potential from multi-lock acquisitions
/// and guaranteeing consistent snapshots across read-modify-write ops.
#[derive(Default)]
struct A2AState {
    agents: HashMap<WebID, A2AAgent>,
    pending_messages: HashMap<String, A2AMessage>,
    capability_tokens: HashMap<WebID, Vec<DelegationToken>>,
    revoked_tokens: std::collections::HashSet<String>,
}

pub struct A2ARuntime {
    state: Arc<RwLock<A2AState>>,
    secret: Arc<Zeroizing<Vec<u8>>>,
    audit_log: Arc<AuditLog>,
    root_authority: Arc<RootAuthority>,
}

impl A2ARuntime {
    /// `secret` is the master key for HKDF agent-secret derivation.
    /// The Ed25519 signing key for token issuance is derived from it.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — A2A runtime derives root authority from master secret
    /// \[P1\] Constraining: User Sovereignty — root WebID is user-derived
    /// pre:  `secret` is a non-empty byte slice (master key material).
    /// post: Returns an `A2ARuntime` with a derived root WebID, signing
    ///       key, empty agent state, and a fresh audit log.
    pub fn new(secret: &[u8]) -> Self {
        // Derive root WebID deterministically from a fixed "root" persona
        let root_persona = b"hkask-root-authority";
        let root_webid = WebID::from_persona(root_persona);
        let signing_key = derive_signing_key(secret);
        let root_authority = Arc::new(RootAuthority::new(root_webid, &signing_key));
        let secret_arc = Arc::new(Zeroizing::new(secret.to_vec()));

        Self {
            state: Arc::new(RwLock::new(A2AState::default())),
            secret: secret_arc,
            audit_log: Arc::new(AuditLog::new()),
            root_authority,
        }
    }

    /// The public key of this runtime's root authority.
    ///
    /// Registration tokens minted by `register_agent` are signed by the A2A
    /// root key (derived from the A2A secret), distinct from the system OCAP key.
    /// A `CapabilityChecker` must trust this key to accept A2A-issued tokens.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// post: returns the Ed25519 public key of the A2A root authority
    pub fn root_public_key(&self) -> hkask_types::Ed25519PublicKey {
        let signing_key = derive_signing_key(self.secret.as_ref());
        hkask_types::Ed25519PublicKey(signing_key.verifying_key().to_bytes())
    }

    /// Returns primary DelegationToken for the agent.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — DelegationToken attenuates capabilities
    /// \[P1\] Constraining: User Sovereignty — tokens are issued to named agents
    /// pre:  `webid` is a valid `WebID`; `capabilities` is a list of
    ///       capability strings (no wildcards allowed).
    /// post: On success, returns `Ok(DelegationToken)` — the primary token
    ///       for the agent. On failure, returns `Err(A2AError)`:
    ///       `WildcardCapabilityNotAllowed` if any capability is `"*"`;
    ///       `AgentAlreadyRegistered` if the WebID is already registered.
    pub async fn register_agent(
        &self,
        webid: WebID,
        capabilities: Vec<String>,
    ) -> Result<DelegationToken, A2AError> {
        // Validate capabilities - reject wildcards
        for cap in &capabilities {
            if cap == "*" {
                return Err(A2AError::WildcardCapabilityNotAllowed);
            }
        }

        let agent = A2AAgent {
            webid,
            capabilities: capabilities.clone(),
            registered_at: current_timestamp()?,
            active: true,
        };

        // Create delegation tokens for ALL capabilities via root authority
        let mut tokens_vec: Vec<DelegationToken> = Vec::with_capacity(capabilities.len());
        for cap in &capabilities {
            let (resource, resource_id, action) = parse_capability(cap)?;
            let token = self
                .root_authority
                .create_root_token(resource, resource_id, action, webid)
                .await?;
            tokens_vec.push(token);
        }

        // If no capabilities were provided, mint a default token
        let primary_token = if let Some(first) = tokens_vec.first() {
            first.clone()
        } else {
            let (resource, resource_id, action) = parse_capability("tool:execute")?;
            let token = self
                .root_authority
                .create_root_token(resource, resource_id, action, webid)
                .await?;
            tokens_vec.push(token.clone());
            token
        };

        // Store agent and capabilities under single lock
        {
            let mut state = self.state.write().await;
            if state.agents.contains_key(&webid) {
                return Err(A2AError::AgentAlreadyRegistered(webid));
            }
            state.agents.insert(webid, agent);
            state.capability_tokens.insert(webid, tokens_vec);
        }

        info!(
            target: "hkask.a2a",
            webid = %webid,
            capabilities = ?capabilities,
            "Agent registered with A2A runtime"
        );

        Ok(primary_token)
    }

    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — unregister revokes all agent capabilities
    /// pre:  `webid` is a valid `WebID`.
    /// post: If the agent exists, removes it and its capability tokens
    ///       and derived key, returns `Ok(())`. If not found, returns
    ///       `Err(A2AError::AgentNotFound)`.
    pub async fn unregister_agent(&self, webid: &WebID) -> Result<(), A2AError> {
        let mut state = self.state.write().await;

        if state.agents.remove(webid).is_none() {
            return Err(A2AError::AgentNotFound(*webid));
        }

        state.capability_tokens.remove(webid);

        info!(
            target: "hkask.a2a",
            webid = %webid,
            "Agent unregistered from A2A runtime"
        );

        Ok(())
    }

    /// R2: Persist Agent State. Returns count of agents restored.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — restore preserves capability graph
    /// pre:  `agents` is a list of `A2AAgent` records; `tokens` is a map
    ///       of WebID → `Vec<DelegationToken>`.
    /// post: All agents and tokens are inserted into the runtime state;
    ///       returns `Ok(usize)` with the count of agents restored.
    pub async fn restore_from_storage(
        &self,
        agents: Vec<A2AAgent>,
        tokens: std::collections::HashMap<WebID, Vec<DelegationToken>>,
    ) -> Result<usize, A2AError> {
        let mut state = self.state.write().await;

        let count = agents.len();

        for agent in agents {
            state.agents.insert(agent.webid, agent);
        }

        for (webid, token_list) in tokens {
            state.capability_tokens.insert(webid, token_list);
        }

        info!(
            target: "hkask.a2a",
            agent_count = count,
            "Agent state restored from storage"
        );

        Ok(count)
    }

    pub(crate) async fn send_message(&self, message: A2AMessage) -> Result<String, A2AError> {
        let from = message.from_webid().copied();
        let to = match &message {
            A2AMessage::TemplateDispatch { to, .. } => *to,
            _ => None,
        };
        let correlation_id = match &message {
            A2AMessage::MemoryArtifact { .. } => format!("artifact:{}", message.correlation_id()),
            _ => message.correlation_id().to_string(),
        };
        let message_type = message.message_type().to_string();

        let mut state = self.state.write().await;
        state
            .pending_messages
            .insert(correlation_id.clone(), message);

        // Log audit entry
        let mut audit_entry = AuditEntry::new(
            from.unwrap_or(WebID::from_persona(b"unknown")),
            "sent".to_string(),
            message_type.clone(),
            AuditOutcome::Success,
        )
        .with_correlation_id(correlation_id.clone())
        .with_metadata(serde_json::json!({
            "event_type": "sent",
            "message_type": &message_type
        }));
        if let Some(recipient) = to {
            audit_entry = audit_entry.with_recipient(recipient);
        }
        self.audit_log.log(audit_entry).await;

        info!(
            target: "hkask.a2a",
            correlation_id = %correlation_id,
            message_type = %message_type,
            "A2A message sent"
        );

        Ok(correlation_id)
    }

    /// Revoke a capability token by ID
    pub async fn revoke_capability(&self, token_id: &str) {
        let mut state = self.state.write().await;
        state.revoked_tokens.insert(token_id.to_string());
    }

    /// List all registered agents.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — enumerate registered agents
    /// pre:  (none).
    /// post: Returns a `Vec<A2AAgent>` containing clones of all currently
    ///       registered agents.
    pub async fn list_agents(&self) -> Vec<A2AAgent> {
        let state = self.state.read().await;
        state.agents.values().cloned().collect()
    }
}

fn current_timestamp() -> Result<i64, A2AError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| A2AError::ClockError(e.to_string()))
}

impl Default for A2ARuntime {
    /// Construct a default A2A runtime using the resolved A2A secret.
    ///
    /// P4.1: The `Default` trait cannot return `Result`, so this is a
    /// documented panic if the secret is unavailable. The panic message
    /// is the actionable onboarding instruction ("run `kask chat` to
    /// complete onboarding, or set HKASK_MASTER_KEY or HKASK_A2A_SECRET").
    /// \[NORMATIVE\] Callers that need graceful failure should call (P4 — Clear Boundaries).
    /// `hkask_keystore::resolve_a2a_secret()` directly and handle the
    /// `Result` instead of using `Default::default()`.
    fn default() -> Self {
        let secret = hkask_keystore::keychain::resolve_a2a_secret().expect(
            "A2A secret not available. Run `kask chat` to complete onboarding, \
                 or set HKASK_MASTER_KEY or HKASK_A2A_SECRET.",
        );
        Self::new(&secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_capability::DelegationResource;
    use hkask_types::WebID;

    const TEST_SECRET: &[u8] = b"test-a2a-secret-32-bytes-min!";

    fn test_webid(label: &str) -> WebID {
        WebID::from_persona(label.as_bytes())
    }

    // ── A2A Wildcard Rejection ──────────────────────────────────────────────

    /// expect: "Agent interactions are gated by OCAP boundaries"
    #[tokio::test]
    async fn a2a_rejects_wildcard_capability() {
        let a2a = A2ARuntime::new(TEST_SECRET);
        let webid = test_webid("test-agent");

        let result = a2a.register_agent(webid, vec!["*".to_string()]).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            A2AError::WildcardCapabilityNotAllowed => {} // expected
            other => panic!("Expected WildcardCapabilityNotAllowed, got: {:?}", other),
        }
    }

    /// expect: "Agent interactions are gated by OCAP boundaries"
    #[tokio::test]
    async fn a2a_rejects_wildcard_mixed_with_valid_capabilities() {
        let a2a = A2ARuntime::new(TEST_SECRET);
        let webid = test_webid("test-agent");

        let result = a2a
            .register_agent(webid, vec!["tool:execute".to_string(), "*".to_string()])
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            A2AError::WildcardCapabilityNotAllowed => {}
            other => panic!("Expected WildcardCapabilityNotAllowed, got: {:?}", other),
        }
    }

    // ── ACP Registration ────────────────────────────────────────────────────

    /// expect: "Agent interactions are gated by OCAP boundaries"
    #[tokio::test]
    async fn a2a_registers_agent_and_returns_token() {
        let a2a = A2ARuntime::new(TEST_SECRET);
        let webid = test_webid("test-agent");

        let token = a2a
            .register_agent(webid, vec!["tool:execute".to_string()])
            .await
            .expect("Registration should succeed");

        assert_eq!(token.delegated_to, webid);
        assert_eq!(token.resource, DelegationResource::Tool);
        assert!(token.verify());
        assert!(a2a.state.read().await.agents.contains_key(&webid));
    }

    /// expect: "Agent interactions are gated by OCAP boundaries"
    #[tokio::test]
    async fn a2a_rejects_duplicate_registration() {
        let a2a = A2ARuntime::new(TEST_SECRET);
        let webid = test_webid("test-agent");

        a2a.register_agent(webid, vec!["tool:execute".to_string()])
            .await
            .expect("First registration should succeed");

        let result = a2a
            .register_agent(webid, vec!["tool:execute".to_string()])
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            A2AError::AgentAlreadyRegistered(w) => assert_eq!(w, webid),
            other => panic!("Expected AgentAlreadyRegistered, got: {:?}", other),
        }
    }

    /// expect: "Agent interactions are gated by OCAP boundaries"
    #[tokio::test]
    async fn a2a_unregister_unknown_agent_returns_error() {
        let a2a = A2ARuntime::new(TEST_SECRET);
        let webid = test_webid("nonexistent");

        let result = a2a.unregister_agent(&webid).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            A2AError::AgentNotFound(w) => assert_eq!(w, webid),
            other => panic!("Expected AgentNotFound, got: {:?}", other),
        }
    }

    // ── ACP Token Revocation ────────────────────────────────────────────────

    /// expect: "Agent interactions are gated by OCAP boundaries"
    #[tokio::test]
    async fn a2a_revokes_token() {
        let a2a = A2ARuntime::new(TEST_SECRET);
        let webid = test_webid("test-agent");

        let token = a2a
            .register_agent(webid, vec!["tool:execute".to_string()])
            .await
            .expect("Registration should succeed");

        let token_id = token.id.clone();
        a2a.revoke_capability(&token_id).await;

        let state = a2a.state.read().await;
        assert!(state.revoked_tokens.contains(&token_id));
    }

    // ── ACP List Agents ─────────────────────────────────────────────────────

    /// expect: "Agent interactions are gated by OCAP boundaries"
    #[tokio::test]
    async fn a2a_lists_registered_agents() {
        let a2a = A2ARuntime::new(TEST_SECRET);
        let alice = test_webid("alice");
        let bob = test_webid("bob");

        a2a.register_agent(alice, vec!["tool:execute".to_string()])
            .await
            .expect("Alice registration should succeed");

        a2a.register_agent(bob, vec!["memory:read".to_string()])
            .await
            .expect("Bob registration should succeed");

        let agents = a2a.list_agents().await;
        assert_eq!(agents.len(), 2);

        let webids: Vec<WebID> = agents.iter().map(|a| a.webid).collect();
        assert!(webids.contains(&alice));
        assert!(webids.contains(&bob));
    }

    /// expect: "Agent interactions are gated by OCAP boundaries"
    #[tokio::test]
    async fn a2a_list_empty_when_no_agents() {
        let a2a = A2ARuntime::new(TEST_SECRET);
        let agents = a2a.list_agents().await;
        assert!(agents.is_empty());
    }
}
