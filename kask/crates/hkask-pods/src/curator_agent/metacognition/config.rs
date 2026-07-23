//! Metacognition configuration types and constants.

use std::collections::HashMap;

use hkask_types::event::SpanNamespace;

pub(crate) const MC_TARGET: &str = "curator.metacognition";

/// Default expected variety per domain for deficit calculation.
pub(crate) const DEFAULT_EXPECTED_VARIETY_PER_DOMAIN: u64 = 50;

/// Default maximum concurrent escalations (VSM algedonic paradox — fewer signals = higher fidelity).
pub(crate) const DEFAULT_MAX_CONCURRENT_ESCALATIONS: usize = 3;

/// Health snapshot — unified system health state.
#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub reg_health: String,
    pub variety_counters: HashMap<SpanNamespace, u64>,
    pub variety_deficit: u64,
    pub critical_alerts: usize,
    pub total_alerts: usize,
    /// Ratio of regulatory actions that were accepted (0.0–1.0).
    /// 1.0 = all actions effective, 0.0 = all actions blocked/staged.
    /// Read from `RegulationLedger::regulation_health()`.
    pub regulation_effectiveness: f64,
}

/// Metacognition loop configuration.
///
/// The loop's tick cadence is governed by `LoopScheduler` (Curation = 10s),
/// not by this config — so there is no `interval` field here.
#[derive(Debug, Clone)]
pub struct MetacognitionConfig {
    /// Escalation thresholds
    pub(crate) thresholds: super::escalation::EscalationThresholds,
    /// Expected variety per domain (for deficit calculation)
    pub expected_variety_per_domain: u64,
    /// Max concurrent escalations before batching (VSM algedonic paradox). Default: 3.
    pub max_concurrent_escalations: usize,
}

impl Default for MetacognitionConfig {
    fn default() -> Self {
        Self {
            thresholds: super::escalation::EscalationThresholds::default(),
            expected_variety_per_domain: DEFAULT_EXPECTED_VARIETY_PER_DOMAIN,
            max_concurrent_escalations: DEFAULT_MAX_CONCURRENT_ESCALATIONS,
        }
    }
}
