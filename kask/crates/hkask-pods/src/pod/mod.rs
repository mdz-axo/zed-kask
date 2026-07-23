//! Agent Pod Lifecycle Management
//!
//! Agent pods are minimal runtime containers that host A2A agents (bots or userpods)
//! within the hKask ecosystem. Each pod provides:
//!
//! - **Isolation**: Independent capability tokens, no shared state
//! - **Identity**: WebID-based A2A registration
//! - **Access**: Capability-gated MCP tool invocation
//! - **Observability**: Regulation span emission for all lifecycle events
//! - **Persistence**: Memory artifact generation (episodic/semantic h_mems)
//!
//! # Lifecycle States
//!
//! ```text
//! Active ↔ Sleeping
//! ```rust,no_run
//!
//! # Security Model
//!
//! Implements OCAP (Object-Capability) security with attenuation on delegation.
//! Each pod holds capability tokens that grant access to specific resources
//! (tools, templates, memory) with cryptographic verification.
//!
//! # Example
//!
//! ```rust,no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use hkask_pods::pod::{AgentPod, PodLifecycleState};
//! use hkask_templates::TemplateCrateLoader;
//! use hkask_types::WebID;
//! use hkask_capability::CapabilityChecker;
//! use hkask_pods::{DenyAllConsent, SovereigntyConsent};
//! use std::sync::Arc;
//!
//! // Create adapters
//! let loader = TemplateCrateLoader::from_path(std::path::PathBuf::from("/tmp/hkask-templates"));
//! let checker = Arc::new(CapabilityChecker::new());
//! let webid = "did:hkask:test".parse::<hkask_types::WebID>()?;
//!
//! // Pod starts Active. Capabilities are granted at creation.
//! let pod = AgentPod::new(
//!     "my-userpod",
//!     &webid,
//!     &["tool:inference:call".to_string()],
//!     &loader,
//!     Arc::new(DenyAllConsent) as Arc<dyn SovereigntyConsent>,
//! )?;
//! assert_eq!(pod.state(), PodLifecycleState::Active);
//! # Ok(())
//! # }
//! ```

mod active_pods;
mod context;
mod deployment;
mod types;

use hkask_capability::{
    CapabilitySpec, DelegationAction, DelegationResource, DelegationToken, SYSTEM_MAX_ATTENUATION,
    derive_signing_key,
};
use hkask_types::DataCategory;
use hkask_types::WebID;
use std::sync::Arc;
use thiserror::Error;
use tracing::info;
use zeroize::Zeroizing;

use crate::SovereigntyChecker;
use hkask_templates::TemplateCrateLoader;

pub use active_pods::{ActivePods, PodStatusInfo};
pub use context::{MemoryContext, PodContext};
pub use deployment::{
    PerPodLedger, PerPodStorage, PodDeployError, PodDeployment, PodFactory, PodRegistry,
};
pub use hkask_types::template::{TemplateCrate, TemplateFile};
pub use types::{CommunicationPosture, PodID, PodKind, PodLifecycleState};

/// Agent Pod — Runtime container for A2A agents
pub struct AgentPod {
    /// Unique pod identifier
    pub id: PodID,
    /// Agent's WebID
    pub webid: WebID,
    /// Pod name (1:1 per user; curator = "curator")
    pub name: String,
    /// Capabilities granted to this pod
    pub capabilities: Vec<String>,
    /// Template crate reference
    pub template_crate: TemplateCrate,
    /// Primary capability token
    pub capability_token: DelegationToken,
    /// Current lifecycle state
    pub state: PodLifecycleState,
    /// Pod creation timestamp (Unix epoch)
    pub created_at: i64,
    /// Sovereignty checker for this pod
    pub(crate) sovereignty_checker: SovereigntyChecker,
}

/// Agent pod error types
#[derive(Debug, Error)]
pub enum AgentPodError {
    #[error("CuratorPod already exists — only one CuratorPod per system")]
    DuplicateCurator,
    #[error("Deploy error: {0}")]
    DeployError(String),
    #[error("SemanticIndex missing after CuratorPod creation")]
    SemanticIndexMissing,
    #[error("Pod not found by name: {0}")]
    PodNotFoundByName(String),
    #[error("Mode error: {0}")]
    ModeError(String),

    #[error("Failed to load template crate: {0}")]
    CrateLoadError(#[from] hkask_types::InfrastructureError),

    #[error("A2A registration failed: {0}")]
    A2ARegistrationError(String),

    #[error("Capability attenuation limit exceeded")]
    AttenuationLimitExceeded,

    #[error("Invalid lifecycle transition: {0} -> {1}")]
    InvalidStateTransition(PodLifecycleState, PodLifecycleState),

    #[error("Clock error: {0}")]
    ClockError(String),

    #[error("Capability denied: token does not grant {resource:?} {action:?}")]
    CapabilityDenied {
        resource: DelegationResource,
        action: DelegationAction,
    },

    #[error(
        "Sovereignty denied: data category {category:?} requires explicit consent for WebID {requester}"
    )]
    SovereigntyDenied {
        category: DataCategory,
        requester: WebID,
    },

    #[error("Inference port unavailable: {0}")]
    InferenceUnavailable(String),

    #[error("Memory operation failed: {0}")]
    MemoryError(#[from] crate::error::MemoryError),

    #[error("Tool invocation failed: {0}")]
    ToolError(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Key derivation failed: {0}")]
    KeyDerivation(#[from] hkask_keystore::KeystoreError),

    #[error("Pod not found: {0}")]
    PodNotFound(PodID),

    #[error("Pod must be activated before creating context")]
    PodNotActive,
}

use crate::error::MemoryError;

impl From<hkask_capability::ToolPortError> for AgentPodError {
    fn from(e: hkask_capability::ToolPortError) -> Self {
        AgentPodError::ToolError(Box::new(e))
    }
}

/// Result type for agent pod operations
pub type AgentPodResult<T> = Result<T, AgentPodError>;

impl AgentPod {
    /// Create a new AgentPod.
    ///
    /// expect: "My agents operate within my sovereignty boundaries"
    /// \[P1\] Motivating: User Sovereignty — AgentPod is the user's agent container
    /// \[P4\] Constraining: Clear Boundaries — OCAP secret + capability token on creation
    /// pre:  `crate_name` is a non-empty string; `name` is a non-empty string;
    ///       `capabilities` is a valid capability list; `loader` is a valid `TemplateCrateLoader`; `consent`
    ///       is a valid `Arc<dyn SovereigntyConsent>`.
    /// post: Returns `Ok(AgentPod)` in `Populated` state with a derived
    ///       OCAP secret, capability token, and sovereignty checker.
    ///       Returns `Err` if template loading or key derivation fails.
    pub fn new(
        crate_name: &str,
        name: &str,
        webid: WebID,
        capabilities: Vec<String>,
        loader: &TemplateCrateLoader,
        consent: Arc<dyn crate::SovereigntyConsent>,
    ) -> AgentPodResult<Self> {
        let template_crate = loader.load_template_crate_or_synthesize(crate_name)?;

        // Sign with the system OCAP authority key (derived from the master key),
        // so the pod's CapabilityChecker — anchored to the matching public key —
        // accepts this token while rejecting forgeries (P4). The pod boundary,
        // not the token's resource field, is the OCAP perimeter (P4.1).
        let signing_key = system_ocap_signing_key()?;

        let default_capability = "tool:execute".to_string();
        let capability_str = capabilities.first().unwrap_or(&default_capability);
        let spec = CapabilitySpec::parse(capability_str).unwrap_or_else(|_| {
            tracing::warn!(
                target: "hkask.capability",
                capability = %capability_str,
                "Malformed capability — falling back to 'tool:execute'"
            );
            CapabilitySpec::parse(&default_capability)
                .expect("Default capability 'tool:execute' must always parse")
        });

        let capability_token = DelegationToken::new(
            spec.resource,
            spec.resource_id,
            spec.action,
            WebID::from_persona(b"system-pod-creator"),
            webid,
            &signing_key,
        );

        // Initialize sovereignty checker for this pod, wired to the live
        // consent port. Grants via the API or CLI are observed here.
        let sovereignty_checker = SovereigntyChecker::new(webid, consent);

        Ok(Self {
            id: PodID::new(),
            webid,
            name: name.to_string(),
            capabilities: capabilities.clone(),
            template_crate,
            capability_token,
            state: PodLifecycleState::Active,
            created_at: current_timestamp()?,
            sovereignty_checker,
        })
    }

    /// Activate the pod for A2A communication.
    ///
    /// Verifies OCAP capability (pod is already Active) after verifying that the
    /// pod's capability token is rooted in the configured OCAP authority.
    pub fn activate(
        &mut self,
        checker: &hkask_capability::CapabilityChecker,
    ) -> AgentPodResult<()> {
        // Verify the capability token is rooted in the OCAP authority.
        // Pod starts Active; this is a capability check, not a state transition.
        if !checker.verify(&self.capability_token)
            || self.capability_token.delegated_to != self.webid
        {
            return Err(AgentPodError::CapabilityDenied {
                resource: hkask_capability::DelegationResource::Tool,
                action: hkask_capability::DelegationAction::Execute,
            });
        }

        tracing::debug!(
            target: "hkask.pod",
            span = "reg.pod.activated",
            verb = "activated",
            pod_id = %self.id,
            webid = %self.webid,
            mcp_access = true,
            confidence = 1.0,
            "Regulation event"
        );

        info!("Agent pod {} activated for A2A communication", self.id);
        Ok(())
    }

    /// Deactivate the pod and revoke capabilities
    ///
    /// Transitions state: `Active` → `Sleeping`
    ///
    /// # Returns
    /// * `Ok(())` — Deactivation successful
    ///
    /// expect: "My agents operate within my sovereignty boundaries"
    /// \[P1\] Motivating: User Sovereignty — deactivate terminates MCP access
    /// pre:  `self.state` must be `Activated` (or `Deactivated` for
    ///       idempotent re-deactivation).
    /// post: `self.state` is `Deactivated`.
    pub fn sleep(&mut self) -> AgentPodResult<()> {
        if !self.state.can_transition_to(PodLifecycleState::Sleeping) {
            return Err(AgentPodError::InvalidStateTransition(
                self.state,
                PodLifecycleState::Sleeping,
            ));
        }

        self.state = PodLifecycleState::Sleeping;

        tracing::debug!(
            target: "hkask.pod",
            span = "reg.pod.deactivated",
            verb = "deactivated",
            pod_id = %self.id,
            webid = %self.webid,
            capabilities_revoked = true,
            confidence = 1.0,
            "Regulation event"
        );

        info!("Agent pod {} deactivated", self.id);
        Ok(())
    }

    /// Create an attenuated capability token for delegation
    ///
    /// # Arguments
    /// * `new_holder` — WebID of the delegate
    /// * `current_time` — Current Unix timestamp
    ///
    /// # Returns
    /// * `Ok(DelegationToken)` — Attenuated child token
    /// * `Err(AgentPodError)` — Attenuation limit exceeded or keystore error
    ///
    /// expect: "My agents operate within my sovereignty boundaries"
    /// \[P4\] Motivating: Clear Boundaries — delegate capability to another holder with attenuation
    /// \[P7\] Constraining: Evolutionary Architecture — attenuation limit emerged from usage
    /// pre:  `new_holder` is a valid `WebID`; `current_time` is a valid
    ///       Unix timestamp; `self.capability_token.attenuation_level`
    ///       must be < `SYSTEM_MAX_ATTENUATION`.
    /// post: Returns `Ok(DelegationToken)` — an attenuated child token —
    ///       or `Err(AttenuationLimitExceeded)`.
    pub fn delegate(
        &self,
        new_holder: WebID,
        current_time: i64,
    ) -> AgentPodResult<DelegationToken> {
        // Check attenuation limit
        if self.capability_token.attenuation_level >= SYSTEM_MAX_ATTENUATION {
            return Err(AgentPodError::AttenuationLimitExceeded);
        }

        // Attenuate with the system OCAP authority key so the child token is
        // signed by the trusted root and verifies against pod checkers (P4).
        let signing_key = system_ocap_signing_key()?;

        self.capability_token
            .attenuate(new_holder, &signing_key, current_time)
            .ok_or(AgentPodError::AttenuationLimitExceeded)
    }

    /// Check if the pod can perform A2A operations.
    ///
    /// expect: "My agents operate within my sovereignty boundaries"
    /// \[P8\] Motivating: Semantic Grounding — state accessor for Active
    /// pre:  (none).
    /// post: Returns `true` iff `self.state == PodLifecycleState::Active`.
    pub fn is_active(&self) -> bool {
        self.state == PodLifecycleState::Active
    }

    /// Get the current lifecycle state.
    ///
    /// expect: "My agents operate within my sovereignty boundaries"
    /// \[P8\] Motivating: Semantic Grounding — lifecycle state accessor
    /// pre:  (none — accessor).
    /// post: Returns the current `PodLifecycleState`.
    pub fn state(&self) -> PodLifecycleState {
        self.state
    }

    // ── Agent Mode Transitions ──
    // AgentMode/role system was removed — pods are always in chat mode.
    // The is_in_server_mode/is_in_chat_mode accessors were deleted with the
    // mode field. If mode-based behavior is needed in the future, it should
    // be re-introduced as a typed field on AgentPod, not a free-floating enum.

    // ── Voice ──

    /// Execute action with sovereignty check
    ///
    /// This method performs an OCAP sovereignty check before executing
    /// any action that accesses data categories.
    ///
    /// # Arguments
    /// * `action` — The action to execute
    /// * `data_category` — The data category being accessed
    /// * `requester` — The WebID requesting the action
    ///
    /// # Returns
    /// * `Ok(true)` — Action is permitted
    /// * `Ok(false)` — Action denied by sovereignty check
    /// * `Err(AgentPodError)` — Sovereignty check error
    ///
    /// expect: "My agents operate within my sovereignty boundaries"
    /// \[P1\] Motivating: User Sovereignty — verify action against sovereignty/consent
    /// \[P2\] Constraining: Affirmative Consent — delegates to consent boundary
    /// pre:  `action` is a non-empty string; `data_category` is a valid
    ///       `DataCategory`; `requester` is a valid `WebID`.
    /// post: Returns `Ok(true)` if both `check_operation` and `can_access`
    ///       pass; `Ok(false)` if either fails; `Err` on sovereignty
    ///       checker error.
    pub fn check_sovereignty(
        &self,
        action: &str,
        data_category: &DataCategory,
        requester: &WebID,
    ) -> Result<bool, AgentPodError> {
        let checker = &self.sovereignty_checker;

        // Check if operation is permitted
        if !checker.check_operation(action, data_category) {
            return Ok(false);
        }

        // Check if requester can access the data category
        if !checker.can_access(data_category, requester) {
            return Ok(false);
        }

        Ok(true)
    }
}

fn current_timestamp() -> Result<i64, AgentPodError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| AgentPodError::ClockError(e.to_string()))
}

/// Derive OCAP secret per WebID from master key via HKDF-SHA256.
///
/// Uses the per-agent context `"hkask:ocap-secret:<webid>"`
/// to produce cryptographically independent signing keys for each
/// agent, while remaining deterministic (same passphrase + same
/// WebID → same secret) for restart safety.
///
/// # Security
///
/// - Derives from the master passphrase via Argon2id → HKDF-SHA256
/// - Different WebIDs produce independent sub-keys (HKDF domain separation)
/// - \[DECLARATIVE\] Same WebID always produces the same key (UUID v5 from persona)
/// - No random generation — ADR-027 compliant
/// - No keystore dependency per pod — only the master key needs storage
///   Derive the system OCAP signing key from the master key.
///
/// \[NORMATIVE\] One of the two roots of trust for pod capability tokens (the
/// other is the A2A root). A pod's pre-registration token is signed with this
/// key, and pod `CapabilityChecker`s are anchored to its public key, so a token
/// from any other keypair is rejected (P4 — Clear Boundaries).
///
/// Derived ONCE per process and cached: issuance and verification both call this
/// and must agree. When the master key is unavailable the underlying secret
/// resolution falls back to a random secret; deriving twice would then produce
/// two different keys and break verification, so the first derived seed is
/// cached to guarantee a single consistent authority.
pub fn system_ocap_signing_key() -> AgentPodResult<ed25519_dalek::SigningKey> {
    use std::sync::OnceLock;
    static SYSTEM_OCAP_SEED: OnceLock<[u8; 32]> = OnceLock::new();

    if let Some(seed) = SYSTEM_OCAP_SEED.get() {
        return Ok(ed25519_dalek::SigningKey::from_bytes(seed));
    }
    let secret = hkask_keystore::keychain::get_or_create_ocap_secret().map_err(|e| {
        AgentPodError::KeyDerivation(hkask_keystore::KeystoreError::KeyDerivation(e.to_string()))
    })?;
    let derived = derive_signing_key(secret.as_slice()).to_bytes();
    let seed = *SYSTEM_OCAP_SEED.get_or_init(|| derived);
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}

/// Build a `CapabilityChecker` anchored to the system OCAP authority.
///
/// Callers that also accept A2A-issued tokens (e.g. registered pods) should
/// chain `.trust_root(a2a.root_public_key())`.
pub fn system_capability_checker() -> AgentPodResult<hkask_capability::CapabilityChecker> {
    Ok(hkask_capability::CapabilityChecker::with_signing_key(
        system_ocap_signing_key()?,
    ))
}

/// Resolve the canonical SQLCipher passphrase configured for this installation.
///
/// Database encryption and OCAP signing are separate concerns: pod databases use
/// `HKASK_DB_PASSPHRASE`, while capability tokens retain per-authority key derivation.
pub(crate) fn resolve_db_passphrase() -> AgentPodResult<Zeroizing<String>> {
    hkask_keystore::keychain::resolve_db_passphrase_string().map_err(|e| {
        AgentPodError::KeyDerivation(hkask_keystore::KeystoreError::KeyDerivation(e.to_string()))
    })
}

impl From<hkask_memory::MemoryPortError> for AgentPodError {
    fn from(e: hkask_memory::MemoryPortError) -> Self {
        AgentPodError::from(MemoryError::from(e))
    }
}
