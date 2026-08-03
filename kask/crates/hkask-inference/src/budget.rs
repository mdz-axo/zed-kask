//! Budget governance — cost estimation, reservation, reconciliation,
//! and per-action approval thresholds (OpenMontage pattern).
//!
//! Before `MediaProvider::execute`, the ledger reserves an estimated
//! cost. After, it reconciles actual vs estimate and updates the
//! per-provider cost model. Over-threshold calls return a
//! `BudgetError::OverThreshold` that the skill cascade surfaces as
//! an approval prompt.

use crate::provider::{MediaOp, MediaProvider};

/// Estimated cost for a media operation.
#[derive(Debug, Clone)]
pub struct CostEstimate {
    pub estimated_cost: f64,
    pub currency: String,
    pub confidence: f64,
}

impl CostEstimate {
    pub fn zero() -> Self {
        Self {
            estimated_cost: 0.0,
            currency: "USD".to_string(),
            confidence: 1.0,
        }
    }
}

/// Budget governance errors.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetError {
    /// Estimated cost exceeds the per-action approval threshold.
    /// The skill cascade should surface this as an approval prompt.
    OverThreshold { estimated: f64, threshold: f64 },
    /// Total budget cap exhausted.
    CapExhausted { spent: f64, cap: f64 },
    /// Provider not found in the cost model.
    ProviderNotFound { provider: String },
}

/// Budget ledger — tracks spent, reserved, and enforces caps.
#[derive(Debug, Clone)]
pub struct BudgetLedger {
    /// Total amount spent (reconciled).
    pub spent: f64,
    /// Amount reserved but not yet reconciled.
    pub reserved: f64,
    /// Total budget cap (0.0 = unlimited).
    pub total_cap: f64,
    /// Per-action approval threshold (0.0 = always approve).
    pub per_action_threshold: f64,
    /// Currency (default: USD).
    pub currency: String,
}

impl Default for BudgetLedger {
    fn default() -> Self {
        Self {
            spent: 0.0,
            reserved: 0.0,
            total_cap: 0.0,            // Unlimited by default
            per_action_threshold: 0.0, // Always approve by default
            currency: "USD".to_string(),
        }
    }
}

impl BudgetLedger {
    /// Estimate the cost of an op for a given provider.
    ///
    /// Default cost model (rough estimates based on public pricing):
    /// - DeepInfra: remove_bg $0.018, speech $0.001, transcribe $0.004
    /// - Fal: image $0.003, video $0.05, upscale $0.005
    /// - AtlasCloud: image $0.01, video $0.10
    pub fn estimate(&self, provider: &dyn MediaProvider, op: MediaOp) -> CostEstimate {
        let id = provider.id();
        let cost = match (id, op) {
            ("deepinfra", MediaOp::RemoveBackground) => 0.018,
            ("deepinfra", MediaOp::GenerateSpeech) => 0.001,
            ("deepinfra", MediaOp::Transcribe) => 0.004,
            ("fal.ai", MediaOp::RemoveBackground) => 0.025,
            ("fal.ai", MediaOp::GenerateImage) => 0.003,
            ("fal.ai", MediaOp::ImageToImage) => 0.003,
            ("fal.ai", MediaOp::Upscale) => 0.005,
            ("fal.ai", MediaOp::GenerateVideo) => 0.05,
            ("fal.ai", MediaOp::ImageToVideo) => 0.05,
            ("atlascloud", MediaOp::GenerateImage) => 0.01,
            ("atlascloud", MediaOp::GenerateVideo) => 0.10,
            ("atlascloud", MediaOp::GenerateSpeech) => 0.005,
            ("atlascloud", MediaOp::Transcribe) => 0.008,
            _ => 0.01, // Default estimate
        };
        CostEstimate {
            estimated_cost: cost,
            currency: self.currency.clone(),
            confidence: 0.8,
        }
    }

    /// Reserve an estimated cost. Returns `Err(BudgetError)` if the
    /// estimate exceeds the per-action threshold or the total cap.
    pub fn reserve(&mut self, estimate: &CostEstimate) -> Result<(), BudgetError> {
        if self.per_action_threshold > 0.0 && estimate.estimated_cost > self.per_action_threshold {
            return Err(BudgetError::OverThreshold {
                estimated: estimate.estimated_cost,
                threshold: self.per_action_threshold,
            });
        }
        if self.total_cap > 0.0
            && self.spent + self.reserved + estimate.estimated_cost > self.total_cap
        {
            return Err(BudgetError::CapExhausted {
                spent: self.spent + self.reserved,
                cap: self.total_cap,
            });
        }
        self.reserved += estimate.estimated_cost;
        Ok(())
    }

    /// Reconcile an actual cost against a reservation.
    /// Updates `spent` and adjusts `reserved`.
    pub fn reconcile(&mut self, estimate: &CostEstimate, actual: f64) {
        self.reserved -= estimate.estimated_cost;
        self.spent += actual;
    }

    /// Whether the ledger allows spending `amount`.
    pub fn can_spend(&self, amount: f64) -> bool {
        if self.total_cap <= 0.0 {
            return true;
        }
        self.spent + self.reserved + amount <= self.total_cap
    }

    /// Remaining budget (f64::INFINITY = unlimited or exhausted).
    pub fn remaining(&self) -> f64 {
        if self.total_cap <= 0.0 {
            return f64::INFINITY;
        }
        (self.total_cap - self.spent - self.reserved).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{MediaOp, MediaProvider};
    use hkask_types::MediaGenerateParams;
    use std::future::Future;
    use std::pin::Pin;

    struct MockProvider {
        id: &'static str,
        supported: Vec<MediaOp>,
    }

    impl MediaProvider for MockProvider {
        fn id(&self) -> &'static str {
            self.id
        }
        fn supports(&self, op: MediaOp) -> bool {
            self.supported.contains(&op)
        }
        fn execute<'a>(
            &'a self,
            _op: MediaOp,
            _params: &'a MediaGenerateParams,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<serde_json::Value, hkask_types::InferenceError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(serde_json::json!({"provider": self.id})) })
        }
    }

    #[test]
    fn estimate_returns_nonzero_cost() {
        let ledger = BudgetLedger::default();
        let provider = MockProvider {
            id: "fal.ai",
            supported: vec![MediaOp::GenerateImage],
        };
        let estimate = ledger.estimate(&provider, MediaOp::GenerateImage);
        assert!(estimate.estimated_cost > 0.0);
        assert_eq!(estimate.currency, "USD");
    }

    #[test]
    fn estimate_deepinfra_cheaper_than_fal_for_bg_removal() {
        let ledger = BudgetLedger::default();
        let di = MockProvider {
            id: "deepinfra",
            supported: vec![MediaOp::RemoveBackground],
        };
        let fal = MockProvider {
            id: "fal.ai",
            supported: vec![MediaOp::RemoveBackground],
        };
        let di_cost = ledger
            .estimate(&di, MediaOp::RemoveBackground)
            .estimated_cost;
        let fal_cost = ledger
            .estimate(&fal, MediaOp::RemoveBackground)
            .estimated_cost;
        assert!(
            di_cost < fal_cost,
            "DeepInfra should be cheaper for bg removal"
        );
    }

    #[test]
    fn reserve_and_reconcile_updates_spent() {
        let mut ledger = BudgetLedger::default();
        let estimate = CostEstimate {
            estimated_cost: 0.05,
            currency: "USD".into(),
            confidence: 0.8,
        };
        assert!(ledger.reserve(&estimate).is_ok());
        assert_eq!(ledger.reserved, 0.05);
        assert_eq!(ledger.spent, 0.0);
        ledger.reconcile(&estimate, 0.04);
        assert_eq!(ledger.reserved, 0.0);
        assert!((ledger.spent - 0.04).abs() < 0.001);
    }

    #[test]
    fn over_threshold_pauses_for_approval() {
        let mut ledger = BudgetLedger {
            per_action_threshold: 0.02,
            ..Default::default()
        };
        let estimate = CostEstimate {
            estimated_cost: 0.05,
            currency: "USD".into(),
            confidence: 0.8,
        };
        let err = ledger.reserve(&estimate).unwrap_err();
        match err {
            BudgetError::OverThreshold {
                estimated,
                threshold,
            } => {
                assert_eq!(estimated, 0.05);
                assert_eq!(threshold, 0.02);
            }
            _ => panic!("expected OverThreshold"),
        }
    }

    #[test]
    fn total_cap_blocks_when_exhausted() {
        let mut ledger = BudgetLedger {
            total_cap: 0.10,
            ..Default::default()
        };
        let estimate = CostEstimate {
            estimated_cost: 0.06,
            currency: "USD".into(),
            confidence: 0.8,
        };
        assert!(ledger.reserve(&estimate).is_ok());
        ledger.reconcile(&estimate, 0.06);
        let estimate2 = CostEstimate {
            estimated_cost: 0.06,
            currency: "USD".into(),
            confidence: 0.8,
        };
        let err = ledger.reserve(&estimate2).unwrap_err();
        match err {
            BudgetError::CapExhausted { spent, cap } => {
                assert!((spent - 0.06).abs() < 0.001);
                assert_eq!(cap, 0.10);
            }
            _ => panic!("expected CapExhausted"),
        }
    }

    #[test]
    fn can_spend_unlimited_by_default() {
        let ledger = BudgetLedger::default();
        assert!(ledger.can_spend(1000.0));
    }

    #[test]
    fn remaining_unlimited_by_default() {
        let ledger = BudgetLedger::default();
        assert!(ledger.remaining().is_infinite());
    }
}
