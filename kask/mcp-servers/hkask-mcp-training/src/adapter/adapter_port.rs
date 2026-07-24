//! AdapterPort — trait boundary for trained adapter lifecycle operations (P4 Clear Boundaries).
//!
//! Every operation is OCAP-gated via `DelegationToken`. No ambient access.
//! The trait is the seam for provider backends — new providers add without changing the router (P7).

use crate::adapter::TrainedLoRAAdapter;
use crate::adapter::endpoint_lifecycle::{EndpointLifecycle, EndpointPhase};
use crate::adapter::provider_cost::{CostModel, ProviderInfo};
use hkask_capability::DelegationToken;
use hkask_inference::ProviderId;
use hkask_types::InferenceError;
use hkask_types::InferenceResult;
use hkask_types::NotFound;
use hkask_types::template::LLMParameters;
use std::sync::Arc;
use std::sync::Mutex;
use uuid::Uuid;

// ── AdapterPort trait ────────────────────────────────────────────────────────

/// The core trait for trained adapter lifecycle operations.
///
/// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
/// \[P4\] Clear Boundaries — composition is explicit, OCAP-gated, and provider-validated
/// \[P7\] Evolutionary Architecture — the trait is the seam for provider backends
///
/// Every method requires a `DelegationToken` with the appropriate capability:
/// - `adapter:read`  → list_adapters, endpoint_status
/// - `adapter:deploy` → estimate_composition, create_endpoint
/// - `adapter:infer`  → infer
/// - `adapter:teardown` → teardown_endpoint
#[allow(async_fn_in_trait)]
pub trait AdapterPort: Send + Sync {
    /// List adapters owned by the caller, optionally filtered by expertise name.
    fn list_adapters(
        &self,
        expertise: Option<&str>,
        token: &DelegationToken,
    ) -> Result<Vec<TrainedLoRAAdapter>, AdapterError>;

    /// Estimate cost and setup time for composing an adapter with a provider.
    async fn estimate_composition(
        &self,
        adapter_id: Uuid,
        provider: ProviderId,
        token: &DelegationToken,
    ) -> Result<CompositionEstimate, AdapterError>;

    /// Provision an inference endpoint: compose adapter + base model + provider.
    async fn create_endpoint(
        &self,
        adapter_id: Uuid,
        provider: ProviderId,
        token: &DelegationToken,
    ) -> Result<InferenceEndpointHandle, AdapterError>;

    /// Query endpoint status (phase, cost accrued, uptime).
    fn endpoint_status(
        &self,
        endpoint_id: Uuid,
        token: &DelegationToken,
    ) -> Result<EndpointStatus, AdapterError>;

    /// Run inference against a composed endpoint.
    async fn infer(
        &self,
        endpoint_id: Uuid,
        prompt: &str,
        params: LLMParameters,
        token: &DelegationToken,
    ) -> Result<InferenceResult, AdapterError>;

    /// Initiate teardown (transition to Draining → Terminated).
    /// Tear down an endpoint. Authority is the possession of the
    /// `EndpointGuard` itself (unforgeable Rust ownership), so no capability
    /// token is required — the guard IS the capability.
    async fn teardown_endpoint(&self, endpoint_id: Uuid) -> Result<(), AdapterError>;
}

// ── Supporting types ─────────────────────────────────────────────────────────

/// Estimate for adapter composition: cost, setup time, and feasibility.
#[derive(Debug, Clone)]
pub struct CompositionEstimate {
    /// The provider being estimated
    pub provider: ProviderId,
    /// Cost model for this provider
    pub cost_model: CostModel,
    /// Whether this provider can compose the adapter
    pub is_compatible: bool,
    /// Reason for incompatibility (human-readable, empty if compatible)
    pub incompatibility_reason: Option<String>,
    /// Estimated setup cost
    pub estimated_setup_cost: f64,
    /// Estimated hourly cost
    pub estimated_hourly_cost: f64,
}

/// Handle returned after creating an endpoint — owns endpoint identity.
///
/// The handle provides access to the endpoint's URL, provider, lifecycle,
/// and cost information. The actual inference call is routed through the
/// `AdapterPort::infer()` method, not through the handle directly.
#[derive(Debug, Clone)]
pub struct InferenceEndpointHandle {
    /// Unique endpoint identifier
    pub endpoint_id: Uuid,
    /// Provider-assigned endpoint URL for inference
    pub endpoint_url: String,
    /// The model name returned by the provider after adapter upload
    pub model_name: String,
    /// Which provider hosts this endpoint
    pub provider: ProviderId,
    /// The expertise this endpoint serves
    pub expertise_name: String,
    /// Lifecycle state machine (shared, observable)
    pub lifecycle: Arc<Mutex<EndpointLifecycle>>,
    /// Cost model for billing
    pub cost_model: CostModel,
    /// When the endpoint was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl InferenceEndpointHandle {
    /// Current phase of the endpoint lifecycle.
    ///
    /// Returns `Terminated` if the lifecycle lock is poisoned (panic in lifecycle thread).
    /// A tracing error is emitted so Regulation can observe the poisoning.
    pub fn phase(&self) -> EndpointPhase {
        match self.lifecycle.lock() {
            Ok(lc) => lc.phase,
            Err(e) => {
                tracing::error!(
                    target: "reg.adapter",
                    endpoint_id = %self.endpoint_id,
                    "Lifecycle mutex poisoned — returning Terminated"
                );
                e.into_inner().phase
            }
        }
    }

    /// Total cost accrued so far.
    ///
    /// Returns 0.0 if the lifecycle lock is poisoned.
    pub fn cost_accrued(&self) -> f64 {
        match self.lifecycle.lock() {
            Ok(lc) => lc.cost_accrued,
            Err(e) => {
                tracing::error!(
                    target: "reg.adapter",
                    endpoint_id = %self.endpoint_id,
                    "Lifecycle mutex poisoned in cost_accrued — returning 0.0"
                );
                e.into_inner().cost_accrued
            }
        }
    }

    /// Whether the endpoint is in a billable phase.
    ///
    /// Returns false if the lifecycle lock is poisoned.
    pub fn is_billable(&self) -> bool {
        match self.lifecycle.lock() {
            Ok(lc) => lc.is_billable(),
            Err(e) => {
                tracing::error!(
                    target: "reg.adapter",
                    endpoint_id = %self.endpoint_id,
                    "Lifecycle mutex poisoned in is_billable — returning false"
                );
                e.into_inner().is_billable()
            }
        }
    }
}

/// Lightweight endpoint status for querying without holding the lifecycle lock.
#[derive(Debug, Clone)]
pub struct EndpointStatus {
    pub endpoint_id: Uuid,
    pub phase: EndpointPhase,
    pub cost_accrued: f64,
    pub provider: ProviderId,
    pub expertise_name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub elapsed_seconds: f64,
}

/// Result of provider selection — returned by `AdapterRouter::select_provider()`.
///
/// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
/// \[P2\] Affirmative Consent — the caller must present this to the user and obtain explicit consent
#[derive(Debug, Clone)]
pub struct ProviderSelection {
    pub adapter_id: Uuid,
    pub expertise_name: String,
    pub base_model_family: String,
    /// All compatible providers, sorted cheapest first
    pub providers: Vec<ProviderInfo>,
    /// How many providers fall within the budget limit (if specified)
    pub within_budget_count: usize,
    /// If exactly one provider is compatible, it's returned here
    /// but the caller MUST still confirm (P2 — never silent selection)
    pub single_candidate: Option<SingleCandidate>,
}

/// Exactly one compatible provider — convenience for the caller.
///
/// The `requires_confirmation` flag is always `true` (P2).
#[derive(Debug, Clone)]
pub struct SingleCandidate {
    pub provider: ProviderInfo,
    /// Always `true` — the caller must confirm with the user before provisioning
    pub requires_confirmation: bool,
}

// ── Error types ──────────────────────────────────────────────────────────────

/// Errors for adapter port operations.
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("Adapter not found: {0}")]
    NotFound(NotFound),

    #[error("Endpoint not found: {0}")]
    EndpointNotFound(Uuid),

    #[error("Provider {0} not available for adapter composition")]
    ProviderUnavailable(String),

    #[error("Incompatible: {reason}")]
    Incompatible { reason: String },

    #[error("Invalid phase transition: attempted to {attempted} while {current}")]
    InvalidTransition {
        current: EndpointPhase,
        attempted: EndpointPhase,
    },

    #[error("Adapter store error: {0}")]
    Store(#[from] crate::adapter::AdapterStoreError),

    #[error("Inference error: {0}")]
    Inference(#[from] InferenceError),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<NotFound> for AdapterError {
    fn from(nf: NotFound) -> Self {
        AdapterError::NotFound(nf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::endpoint_lifecycle::EndpointLifecycle;
    use crate::adapter::provider_cost::CostModel;

    #[test]
    fn handle_phase_reflects_lifecycle() {
        let cost = CostModel::runpod();
        let lc = EndpointLifecycle::new(cost.gpu_hourly_rate).expect("lifecycle creation");
        let mut lc_mut = lc.clone();
        lc_mut.transition(EndpointPhase::Ready).expect("transition");

        let handle = InferenceEndpointHandle {
            endpoint_id: Uuid::new_v4(),
            endpoint_url: "https://example.com/v1".into(),
            model_name: "test-model".into(),
            provider: ProviderId::Runpod,
            expertise_name: "solidity-audit".into(),
            lifecycle: Arc::new(Mutex::new(lc_mut)),
            cost_model: cost,
            created_at: chrono::Utc::now(),
        };

        assert_eq!(handle.phase(), EndpointPhase::Ready);
    }

    #[test]
    fn handle_is_billable_delegates() {
        let cost = CostModel::runpod();
        let lc = EndpointLifecycle::new(cost.gpu_hourly_rate).expect("lifecycle creation");

        let handle = InferenceEndpointHandle {
            endpoint_id: Uuid::new_v4(),
            endpoint_url: "https://example.com/v1".into(),
            model_name: "test-model".into(),
            provider: ProviderId::Runpod,
            expertise_name: "solidity-audit".into(),
            lifecycle: Arc::new(Mutex::new(lc)),
            cost_model: cost,
            created_at: chrono::Utc::now(),
        };

        assert!(handle.is_billable()); // Provisioning is billable
    }

    #[test]
    fn adapter_error_display() {
        let err = AdapterError::NotFound(NotFound {
            entity_type: "adapter".to_string(),
            id: Uuid::nil().to_string(),
        });
        let s = err.to_string();
        assert!(s.contains("00000000"), "error should contain nil UUID");
    }
}
