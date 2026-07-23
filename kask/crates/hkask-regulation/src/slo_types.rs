//! SLO (Service Level Objective) types.
//!
//! Extracted from hkask-types/src/reg.rs during Regulation refactoring.

use serde::{Deserialize, Serialize};

/// Severity of an SLO — determines algedonic escalation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SloSeverity {
    /// Critical — pages the Curator, blocks energy-intensive operations on breach.
    Critical,
    /// High — alerts the Curator, logged prominently.
    High,
    /// Medium — logged, surfaced in health checks.
    Medium,
}

/// A Service Level Objective — a reliability contract attached to a Regulation span.
///
/// Each SLO defines a target success rate over a time window for a specific
/// Regulation span namespace. The Regulation evaluates SLOs on a configurable cadence and
/// escalates breaches through the algedonic pathway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SloDefinition {
    /// Unique SLO identifier (e.g., "SLO-INF-001")
    pub slo_id: String,
    /// Human-readable name
    pub name: String,
    /// Regulation span namespace this SLO monitors
    pub span_namespace: String,
    /// Target success rate (0.0–1.0)
    pub target: f64,
    /// Evaluation window in seconds
    pub window_seconds: u64,
    /// Minimum operations required before the SLO can be evaluated.
    /// Below this threshold, compliance is unknown (not 100%).
    /// Default: 1 — at least one operation must exist for a valid evaluation.
    pub minimum_operations: u64,
    /// Severity for algedonic escalation on breach
    pub severity: SloSeverity,
    /// Whether this SLO is currently active
    pub active: bool,
}

/// Result of evaluating a single SLO at a point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SloEvaluation {
    /// The SLO being evaluated
    pub slo_id: String,
    /// Current compliance rate (0.0–1.0). Meaningful only when data_available is true.
    pub current_compliance: f64,
    /// Error budget remaining as fraction of total budget (1.0 = full budget)
    pub error_budget_remaining: f64,
    /// Current burn rate — fraction of error budget consumed per hour
    pub burn_rate: f64,
    /// Whether sufficient data was available to evaluate this SLO.
    /// False when total_operations < minimum_operations — compliance is unknown.
    pub data_available: bool,
    /// Whether this SLO is currently in breach of its target.
    /// Only meaningful when data_available is true.
    pub in_breach: bool,
    /// Unix timestamp of this evaluation
    pub evaluated_at: u64,
}

impl SloDefinition {
    /// Create a new active SLO with default minimum_operations of 1.
    ///
    /// Use `with_minimum_operations()` to set a higher threshold for
    /// high-traffic SLOs where small sample sizes would produce misleading
    /// compliance rates.
    pub fn new(
        slo_id: impl Into<String>,
        name: impl Into<String>,
        span_namespace: impl Into<String>,
        target: f64,
        window_seconds: u64,
        severity: SloSeverity,
    ) -> Self {
        SloDefinition {
            slo_id: slo_id.into(),
            name: name.into(),
            span_namespace: span_namespace.into(),
            target: target.clamp(0.0, 1.0),
            window_seconds,
            minimum_operations: 1,
            severity,
            active: true,
        }
    }

    /// Set the minimum operations threshold for this SLO.
    ///
    /// Below this threshold, evaluation will report data_available = false
    /// and compliance as 0.0 (unknown). This prevents "no data = perfect"
    /// fallacies — P8 Semantic Grounding.
    pub fn with_minimum_operations(mut self, min_ops: u64) -> Self {
        self.minimum_operations = min_ops;
        self
    }

    /// Compute the total error budget for this SLO.
    ///
    /// Error Budget = (1 - target) × total_operations
    pub fn error_budget(&self, total_operations: u64) -> f64 {
        (1.0 - self.target) * total_operations as f64
    }

    /// Check whether a given compliance rate breaches this SLO.
    pub fn is_breached(&self, compliance_rate: f64) -> bool {
        compliance_rate < self.target
    }

    /// Describe this SLO for display purposes.
    pub fn describe(&self) -> String {
        format!(
            "{} [{}]: {:.2}% over {}s ({:?})",
            self.slo_id,
            self.name,
            self.target * 100.0,
            self.window_seconds,
            self.severity,
        )
    }
}

/// Seed SLO definitions — the three initial SLOs deployed in Phase 1.
pub fn seed_slos() -> Vec<SloDefinition> {
    vec![
        SloDefinition::new(
            "SLO-INF-001",
            "Inference availability",
            "reg.inference",
            0.999,     // 99.9%
            2_592_000, // 30 days
            SloSeverity::Critical,
        ),
        SloDefinition::new(
            "SLO-SKL-001",
            "Skill dispatch success rate",
            "reg.tool",
            0.995,     // 99.5%
            2_592_000, // 30 days
            SloSeverity::Critical,
        ),
        SloDefinition::new(
            "SLO-API-001",
            "API endpoint availability",
            "reg.deploy",
            0.999,     // 99.9%
            2_592_000, // 30 days
            SloSeverity::Critical,
        ),
    ]
}

#[cfg(test)]
mod slo_tests {
    use super::*;

    #[test]
    fn slo_definition_clamps_target_to_valid_range() {
        let slo = SloDefinition::new("test", "test", "reg.test", 1.5, 3600, SloSeverity::Medium);
        assert_eq!(slo.target, 1.0);
        let slo = SloDefinition::new("test", "test", "reg.test", -0.5, 3600, SloSeverity::Medium);
        assert_eq!(slo.target, 0.0);
    }

    #[test]
    fn slo_error_budget_calculation() {
        let slo = SloDefinition::new("test", "test", "reg.test", 0.999, 3600, SloSeverity::High);
        // 0.001 × 1,000,000 = 1,000
        assert!((slo.error_budget(1_000_000) - 1_000.0).abs() < 1.0);
    }

    #[test]
    fn slo_breach_detection() {
        let slo = SloDefinition::new(
            "test",
            "test",
            "reg.test",
            0.999,
            3600,
            SloSeverity::Critical,
        );
        assert!(slo.is_breached(0.998)); // below target
        assert!(!slo.is_breached(0.9995)); // above target
        assert!(!slo.is_breached(1.0)); // perfect
    }

    #[test]
    fn seed_slos_are_valid() {
        let slos = seed_slos();
        assert_eq!(slos.len(), 3);
        for slo in &slos {
            assert!(
                slo.target > 0.0 && slo.target <= 1.0,
                "{} target out of range",
                slo.slo_id
            );
            assert!(
                slo.window_seconds > 0,
                "{} window must be positive",
                slo.slo_id
            );
            assert!(slo.active, "{} must be active", slo.slo_id);
            assert_eq!(
                slo.minimum_operations, 1,
                "{} default min ops must be 1",
                slo.slo_id
            );
        }
    }

    #[test]
    fn slo_with_minimum_operations_builder() {
        let slo = SloDefinition::new("test", "test", "reg.test", 0.99, 3600, SloSeverity::High)
            .with_minimum_operations(1000);
        assert_eq!(slo.minimum_operations, 1000);
        // Default is 1
        let slo2 = SloDefinition::new("test2", "test", "reg.test", 0.99, 3600, SloSeverity::High);
        assert_eq!(slo2.minimum_operations, 1);
    }

    #[test]
    fn slo_describe_is_human_readable() {
        let slo = SloDefinition::new(
            "SLO-TEST",
            "Test SLO",
            "reg.test",
            0.95,
            3600,
            SloSeverity::High,
        );
        let desc = slo.describe();
        assert!(desc.contains("SLO-TEST"));
        assert!(desc.contains("95"));
        assert!(desc.contains("High"));
    }

    #[test]
    fn slo_target_boundary_exactly_on_target_not_breached() {
        let slo = SloDefinition::new(
            "test",
            "test",
            "reg.test",
            0.999,
            3600,
            SloSeverity::Critical,
        );
        assert!(!slo.is_breached(0.999)); // exactly at target is NOT breached (< operator)
    }
}
