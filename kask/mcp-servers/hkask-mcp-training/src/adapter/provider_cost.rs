//! Provider cost model — transparent pricing for adapter deployment (P9 Homeostatic Self-Regulation).
//!
//! Every inference provider has a `CostModel` that exposes honest estimates for
//! GPU hourly rate, setup time, and teardown grace period. This enables
//! budget-aware deployment decisions.

use hkask_inference::ProviderId;
use serde::{Deserialize, Serialize};

/// Cost model for a specific inference provider.
///
/// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
/// \[P9\] Homeostatic Self-Regulation — cost transparency enables budget-aware decisions
/// pre:  provider is a recognized ProviderId variant
/// post: CostModel returns honest estimates: gpu_hourly_rate, setup_minutes, teardown_grace
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostModel {
    /// Which provider this cost model applies to
    pub provider: ProviderId,
    /// Hourly GPU rate in the configured currency
    pub gpu_hourly_rate: f64,
    /// Estimated minutes to provision and cold-start the endpoint
    pub estimated_setup_minutes: u32,
    /// Grace period in seconds after teardown signal before billing stops
    pub estimated_teardown_grace_seconds: u32,
    /// Currency code (e.g. "USD")
    pub currency: String,
}

impl CostModel {
    /// Create a new cost model with validation.
    ///
    /// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
    /// pre:  gpu_hourly_rate > 0.0, estimated_setup_minutes > 0
    /// post: returns CostModel for the given provider
    pub fn new(
        provider: ProviderId,
        gpu_hourly_rate: f64,
        estimated_setup_minutes: u32,
        estimated_teardown_grace_seconds: u32,
        currency: impl Into<String>,
    ) -> Result<Self, CostModelError> {
        if gpu_hourly_rate <= 0.0 {
            return Err(CostModelError::InvalidHourlyRate(gpu_hourly_rate));
        }
        if estimated_setup_minutes == 0 {
            return Err(CostModelError::InvalidSetupTime);
        }
        Ok(Self {
            provider,
            gpu_hourly_rate,
            estimated_setup_minutes,
            estimated_teardown_grace_seconds,
            currency: currency.into(),
        })
    }

    /// Estimated cost for a given duration in hours.
    pub fn estimated_cost_for_hours(&self, hours: f64) -> f64 {
        self.gpu_hourly_rate * hours
    }

    /// Estimated setup cost (fraction of hourly rate for setup minutes).
    pub fn estimated_setup_cost(&self) -> f64 {
        self.gpu_hourly_rate * (self.estimated_setup_minutes as f64 / 60.0)
    }
}

/// Errors for CostModel construction.
#[derive(Debug, thiserror::Error)]
pub enum CostModelError {
    #[error("Hourly rate must be positive, got {0}")]
    InvalidHourlyRate(f64),

    #[error("Setup time must be at least 1 minute")]
    InvalidSetupTime,
}

/// Provider capabilities — whether a provider supports LoRA composition.
///
/// expect: "The adapter manages LoRA adapter lifecycle and inference composition"
/// pre:  provider is a recognized ProviderId variant
/// post: ProviderCapability indicates whether LoRA composition is supported
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapability {
    /// Whether this provider can compose a LoRA adapter with a base model
    pub supports_lora_composition: bool,
    /// Maximum adapter size in MB (None = no documented limit)
    pub max_adapter_size_mb: Option<u64>,
    /// Base model families this provider can compose adapters with
    pub supported_base_model_families: Vec<String>,
}

impl ProviderCapability {
    /// Check if this provider can compose the given adapter + base model combo.
    pub fn can_compose(&self, base_model_family: &str) -> bool {
        self.supports_lora_composition
            && (self.supported_base_model_families.is_empty()
                || self
                    .supported_base_model_families
                    .iter()
                    .any(|f| f == base_model_family))
    }
}

/// Packaged provider info: cost model + capability for user-facing selection.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub provider: ProviderId,
    pub cost_model: CostModel,
    pub capability: ProviderCapability,
}

/// Static cost models for known providers.
///
/// These are honest estimates — not hardcoded truths. They can be overridden
/// by per-user configuration. The system never silently selects a provider;
/// costs are always presented to the user (P2 — Affirmative Consent).
impl CostModel {
    /// Runpod — ~$0.79/hr for comparable GPU, ~5 min setup
    pub fn runpod() -> Self {
        Self {
            provider: ProviderId::Runpod,
            gpu_hourly_rate: 0.79,
            estimated_setup_minutes: 5,
            estimated_teardown_grace_seconds: 30,
            currency: "USD".into(),
        }
    }
}

/// Static provider capabilities.
impl ProviderCapability {
    /// Runpod supports LoRA composition with broader model support.
    pub fn runpod() -> Self {
        Self {
            supports_lora_composition: true,
            max_adapter_size_mb: Some(500),
            supported_base_model_families: vec![
                "llama-3.3-70b".into(),
                "llama-3.1-70b".into(),
                "qwen2.5-72b".into(),
                "mixtral-8x7b".into(),
                "qwen3.6-27b".into(),
            ],
        }
    }

    /// DeepInfra — cloud provider, no LoRA composition.
    pub fn deepinfra() -> Self {
        Self {
            supports_lora_composition: false,
            max_adapter_size_mb: None,
            supported_base_model_families: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_model_new_valid() {
        let cm = CostModel::new(ProviderId::Runpod, 0.79, 5, 30, "USD").expect("valid cost model");
        assert_eq!(cm.gpu_hourly_rate, 0.79);
        assert_eq!(cm.estimated_setup_minutes, 5);
        assert_eq!(cm.currency, "USD");
    }

    #[test]
    fn cost_model_new_invalid_rate() {
        let result = CostModel::new(ProviderId::Runpod, 0.0, 5, 30, "USD");
        assert!(result.is_err());
    }

    #[test]
    fn cost_model_new_zero_setup() {
        let result = CostModel::new(ProviderId::Runpod, 1.0, 0, 30, "USD");
        assert!(result.is_err());
    }

    #[test]
    fn estimated_cost_for_hours() {
        let cm = CostModel::runpod();
        assert_eq!(cm.estimated_cost_for_hours(1.0), 0.79);
        assert_eq!(cm.estimated_cost_for_hours(2.5), 1.975);
        assert_eq!(cm.estimated_cost_for_hours(0.0), 0.0);
    }

    #[test]
    fn estimated_setup_cost() {
        let cm = CostModel::runpod(); // 5 min setup at $0.79/hr
        let expected = 0.79 * (5.0 / 60.0);
        assert!((cm.estimated_setup_cost() - expected).abs() < 0.001);
    }

    #[test]
    fn provider_capability_can_compose() {
        let rp = ProviderCapability::runpod();
        assert!(rp.can_compose("llama-3.3-70b"));
        assert!(rp.can_compose("qwen2.5-72b"));
        assert!(rp.can_compose("mixtral-8x7b"));
    }

    #[test]
    fn no_compose_providers_reject_all() {
        let deepinfra = ProviderCapability::deepinfra();
        assert!(!deepinfra.supports_lora_composition);
        assert!(!deepinfra.can_compose("llama-3.3-70b"));
    }

    #[test]
    fn static_models_integrity() {
        let rp = CostModel::runpod();
        assert_eq!(rp.provider, ProviderId::Runpod);
        assert!(rp.gpu_hourly_rate > 0.0);
    }
}
