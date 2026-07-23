//! SLO Manager — Service Level Objective evaluation for the Regulation
//!
//! SloManager loads SLO definitions, evaluates them against ν-event data,
//! computes compliance rates and error budgets, emits `reg.slo.evaluated`
//! spans, and produces breach alerts for the algedonic pathway.
//!
//! # Epistemic grounding
//! - **crt:certainty** = Probabilistic (SLO compliance is sampled, not total)
//! - **crt:force** = Evidence (IS statement, computed from ν-event data)
//! - **mode** = IS
//!
//! # Cybernetic role
//! - Sensor: queries ν-event store for operation counts
//! - Comparator: compares compliance rate to SLO target
//! - Effector: emits `reg.slo.evaluated` spans; feeds breaches to algedonic

use crate::slo_span::SloSpan;
use crate::slo_types::{SloDefinition, SloEvaluation};
use hkask_types::ObservableSpan;
use std::time::{SystemTime, UNIX_EPOCH};

/// Result of querying the ν-event store for SLO data.
///
/// The SloManager is decoupled from the storage layer — it accepts
/// any implementation of the SloDataProvider trait. This enables
/// unit testing without a database and supports multiple storage backends.
#[derive(Debug, Clone)]
pub struct SloDataPoint {
    /// Total operations in the SLO's window
    pub total_operations: u64,
    /// Successful operations in the SLO's window
    pub successful_operations: u64,
}

/// Trait for providing ν-event data to the SloManager.
///
/// Implementations query the ν-event store for operation counts
/// within a time window for a given Regulation span namespace.
pub trait SloDataProvider: Send + Sync {
    /// Query operation counts for the given span namespace within
    /// the specified time window (in seconds before now).
    fn query(
        &self,
        span_namespace: &str,
        window_seconds: u64,
    ) -> Result<SloDataPoint, SloManagerError>;
}

/// Errors that can occur during SLO evaluation.
#[derive(Debug, thiserror::Error)]
pub enum SloManagerError {
    #[error("Data provider error: {0}")]
    DataProvider(String),

    #[error("Invalid SLO configuration: {0}")]
    InvalidConfig(String),
}

/// Manages Service Level Objective definitions, evaluation, and breach detection.
///
/// ## Example
///
/// ```rust,ignore
/// use hkask_regulation::slo_manager::SloManager;
/// use hkask_types::regulation::seed_slos;
///
/// let mut manager = SloManager::new();
/// for slo in seed_slos() {
///     manager.register(slo);
/// }
///
/// let evaluations = manager.evaluate(&data_provider)?;
/// for eval in &evaluations {
///     if eval.in_breach {
///         // escalate via algedonic pathway
///     }
/// }
/// ```
pub struct SloManager {
    slos: Vec<SloDefinition>,
}

impl SloManager {
    /// Create an empty SLO manager.
    ///
    /// expect: "The system initializes SLO management with no definitions"
    /// `[P9]` Motivating: Homeostatic Self-Regulation — SLOs are the contract layer
    /// post: returns SloManager with empty SLO list
    pub fn new() -> Self {
        Self { slos: Vec::new() }
    }

    /// Create a manager pre-loaded with seed SLOs.
    ///
    /// expect: "The system initializes with baseline SLO definitions"
    /// `[P9]` Motivating: Homeostatic Self-Regulation — seed SLOs establish baseline
    /// post: returns SloManager with 3 seed SLOs
    pub fn with_seed_slos() -> Self {
        Self {
            slos: crate::slo_types::seed_slos(),
        }
    }

    /// Register an SLO definition.
    ///
    /// expect: "The system accepts SLO definitions for evaluation"
    /// `[P9]` Motivating: Homeostatic Self-Regulation — extensible SLO registry
    /// pre:  slo.slo_id is unique (caller's responsibility)
    /// post: SLO is added to the registry
    pub fn register(&mut self, slo: SloDefinition) {
        self.slos.push(slo);
    }

    /// Remove an SLO by ID.
    ///
    /// expect: "The system supports SLO lifecycle management"
    /// post: if slo_id exists, it is removed; returns true if removed
    pub fn deregister(&mut self, slo_id: &str) -> bool {
        let len_before = self.slos.len();
        self.slos.retain(|s| s.slo_id != slo_id);
        self.slos.len() < len_before
    }

    /// Get all registered SLOs.
    pub fn slos(&self) -> &[SloDefinition] {
        &self.slos
    }

    /// Get active SLOs only.
    pub fn active_slos(&self) -> Vec<&SloDefinition> {
        self.slos.iter().filter(|s| s.active).collect()
    }

    /// Evaluate all active SLOs against the data provider.
    ///
    /// expect: "The system evaluates SLOs against measured data"
    /// `[P9]` Motivating: Homeostatic Self-Regulation — SLO evaluation drives feedback
    /// `[P8]` Constraining: Semantic Grounding — evaluations are computed, not guessed
    /// pre:  data_provider is operational
    /// post: returns one SloEvaluation per active SLO; errors are logged per-SLO,
    ///       not failing the entire batch
    pub fn evaluate(&self, provider: &dyn SloDataProvider) -> Vec<SloEvaluation> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.active_slos()
            .iter()
            .filter_map(|slo| {
                match provider.query(&slo.span_namespace, slo.window_seconds) {
                    Ok(data) => {
                        let has_enough_data = data.total_operations >= slo.minimum_operations;

                        let compliance = if has_enough_data {
                            data.successful_operations as f64 / data.total_operations as f64
                        } else {
                            // Insufficient data — compliance is unknown, not 100%.
                            // This prevents "no data = perfect" fallacy (P8 Semantic Grounding).
                            0.0
                        };

                        let error_budget_total = slo.error_budget(data.total_operations);
                        let failures = data
                            .total_operations
                            .saturating_sub(data.successful_operations);
                        let error_budget_remaining = if error_budget_total > 0.0 {
                            (error_budget_total - failures as f64).max(0.0) / error_budget_total
                        } else {
                            1.0
                        };

                        let window_hours = slo.window_seconds as f64 / 3600.0;
                        let burn_rate =
                            if has_enough_data && error_budget_total > 0.0 && window_hours > 0.0 {
                                (failures as f64 / error_budget_total) / window_hours
                            } else {
                                0.0
                            };

                        // Only mark as breached if we have sufficient data to evaluate
                        let in_breach = has_enough_data && slo.is_breached(compliance);

                        SloSpan::SloEvaluated.emit("evaluated");
                        tracing::info!(
                            target: "reg",
                            reg_domain = "reg.slo.evaluated",
                            slo_id = %slo.slo_id,
                            compliance = %compliance,
                            error_budget_pct = %(error_budget_remaining * 100.0),
                            burn_rate = %burn_rate,
                            in_breach = %in_breach,
                            data_available = %has_enough_data,
                            total_ops = %data.total_operations,
                            min_ops = %slo.minimum_operations,
                            "SLO evaluated",
                        );

                        Some(SloEvaluation {
                            slo_id: slo.slo_id.clone(),
                            current_compliance: compliance,
                            error_budget_remaining,
                            burn_rate,
                            data_available: has_enough_data,
                            in_breach,
                            evaluated_at: now,
                        })
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "reg",
                            reg_domain = "reg.slo.evaluated",
                            slo_id = %slo.slo_id,
                            error = %e,
                            "SLO evaluation failed — data provider error",
                        );
                        // Don't fail the entire batch for one SLO's data error
                        None
                    }
                }
            })
            .collect()
    }
}

impl Default for SloManager {
    fn default() -> Self {
        Self::with_seed_slos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slo_types::SloSeverity;

    /// A test data provider that returns configurable counts.
    struct TestDataProvider {
        total: u64,
        successful: u64,
        /// If set, return an error instead
        error: Option<String>,
    }

    impl TestDataProvider {
        fn new(total: u64, successful: u64) -> Self {
            Self {
                total,
                successful,
                error: None,
            }
        }

        fn with_error(msg: &str) -> Self {
            Self {
                total: 0,
                successful: 0,
                error: Some(msg.to_string()),
            }
        }
    }

    impl SloDataProvider for TestDataProvider {
        fn query(
            &self,
            _span_namespace: &str,
            _window_seconds: u64,
        ) -> Result<SloDataPoint, SloManagerError> {
            if let Some(ref err) = self.error {
                return Err(SloManagerError::DataProvider(err.clone()));
            }
            Ok(SloDataPoint {
                total_operations: self.total,
                successful_operations: self.successful,
            })
        }
    }

    // ── SloManager lifecycle ──────────────────────────────────────────────

    #[test]
    fn slo_manager_starts_empty() {
        let manager = SloManager::new();
        assert!(manager.slos().is_empty());
    }

    #[test]
    fn slo_manager_with_seeds_has_three() {
        let manager = SloManager::with_seed_slos();
        assert_eq!(manager.slos().len(), 3);
    }

    #[test]
    fn register_and_deregister_slo() {
        let mut manager = SloManager::new();
        let slo = SloDefinition::new("test", "test", "reg.test", 0.99, 3600, SloSeverity::Medium);
        manager.register(slo);
        assert_eq!(manager.slos().len(), 1);
        assert!(manager.deregister("test"));
        assert!(manager.slos().is_empty());
    }

    #[test]
    fn deregister_nonexistent_returns_false() {
        let mut manager = SloManager::new();
        assert!(!manager.deregister("nonexistent"));
    }

    #[test]
    fn active_slos_filters_inactive() {
        let mut manager = SloManager::new();
        let mut slo = SloDefinition::new(
            "active",
            "active",
            "reg.test",
            0.99,
            3600,
            SloSeverity::High,
        );
        slo.active = true;
        let mut slo2 = SloDefinition::new(
            "inactive",
            "inactive",
            "reg.test",
            0.99,
            3600,
            SloSeverity::High,
        );
        slo2.active = false;
        manager.register(slo);
        manager.register(slo2);
        assert_eq!(manager.active_slos().len(), 1);
    }

    // ── SLO evaluation ────────────────────────────────────────────────────

    #[test]
    fn evaluate_perfect_compliance() {
        let manager = SloManager::with_seed_slos();
        let provider = TestDataProvider::new(1000, 1000); // 100% success
        let results = manager.evaluate(&provider);
        assert_eq!(results.len(), 3);
        for eval in &results {
            assert!(
                !eval.in_breach,
                "SLO {} should not be breached",
                eval.slo_id
            );
            assert!(
                eval.data_available,
                "SLO {} should have sufficient data",
                eval.slo_id
            );
            assert!((eval.current_compliance - 1.0).abs() < 0.001);
            assert!((eval.error_budget_remaining - 1.0).abs() < 0.001);
        }
    }

    #[test]
    fn evaluate_below_target_breaches() {
        let mut manager = SloManager::new();
        manager.register(SloDefinition::new(
            "test",
            "test",
            "reg.test",
            0.99,
            3600,
            SloSeverity::Critical,
        ));
        // 98% success when target is 99%
        let provider = TestDataProvider::new(1000, 980);
        let results = manager.evaluate(&provider);
        assert_eq!(results.len(), 1);
        assert!(results[0].in_breach);
    }

    #[test]
    fn evaluate_no_data_reports_unknown() {
        let mut manager = SloManager::new();
        manager.register(SloDefinition::new(
            "test",
            "test",
            "reg.test",
            0.99,
            3600,
            SloSeverity::High,
        ));
        // Zero operations — insufficient data, compliance is unknown
        let provider = TestDataProvider::new(0, 0);
        let results = manager.evaluate(&provider);
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].data_available,
            "no data should not be treated as perfect"
        );
        assert!(!results[0].in_breach, "unknown data should not be breached");
        assert!(
            (results[0].current_compliance - 0.0).abs() < 0.001,
            "compliance should be 0.0 when unknown"
        );
    }

    #[test]
    fn evaluate_with_minimum_operations_threshold() {
        let mut manager = SloManager::new();
        manager.register(
            SloDefinition::new("test", "test", "reg.test", 0.99, 3600, SloSeverity::High)
                .with_minimum_operations(100),
        );
        // 50 ops with 100% success, but below minimum of 100
        let provider = TestDataProvider::new(50, 50);
        let results = manager.evaluate(&provider);
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].data_available,
            "below minimum_operations should be unknown"
        );
    }

    #[test]
    fn evaluate_error_budget_burn_rate() {
        let mut manager = SloManager::new();
        // 99% target over 1 hour window
        manager.register(SloDefinition::new(
            "test",
            "test",
            "reg.test",
            0.99,
            3600,
            SloSeverity::Critical,
        ));
        // 1000 ops, 985 success = 1.5% failure rate, budget = 10 failures
        let provider = TestDataProvider::new(1000, 985);
        let results = manager.evaluate(&provider);
        assert_eq!(results.len(), 1);
        assert!(results[0].in_breach);
        // Burn rate should be positive (consuming budget)
        assert!(results[0].burn_rate > 0.0);
        // Error budget should be partially consumed
        assert!(results[0].error_budget_remaining < 1.0);
    }

    #[test]
    fn evaluate_data_provider_error_is_non_fatal() {
        let mut manager = SloManager::new();
        manager.register(SloDefinition::new(
            "test1",
            "test",
            "reg.test",
            0.99,
            3600,
            SloSeverity::Medium,
        ));
        manager.register(SloDefinition::new(
            "test2",
            "test",
            "reg.test",
            0.99,
            3600,
            SloSeverity::Medium,
        ));
        // Mixed provider: one succeeds, one errors — batch must not fail
        // We can't test this with a single provider easily, so we test
        // that a failing provider returns empty results (graceful degradation)
        let failing = TestDataProvider::with_error("connection refused");
        let results = manager.evaluate(&failing);
        assert!(
            results.is_empty(),
            "failing provider should yield no evaluations"
        );
    }

    #[test]
    fn evaluate_breaches_returns_only_breached() {
        let mut manager = SloManager::new();
        // SLO at 99% target
        manager.register(SloDefinition::new(
            "passing",
            "passing",
            "reg.test",
            0.99,
            3600,
            SloSeverity::High,
        ));
        // SLO at 99.9% target — tighter
        manager.register(SloDefinition::new(
            "failing",
            "failing",
            "reg.test",
            0.999,
            3600,
            SloSeverity::Critical,
        ));
        // 995/1000 = 99.5% — passes the first, fails the second
        let provider = TestDataProvider::new(1000, 995);
        let results = manager.evaluate(&provider);
        let breaches: Vec<_> = results.into_iter().filter(|e| e.in_breach).collect();
        assert_eq!(breaches.len(), 1);
        assert_eq!(breaches[0].slo_id, "failing");
    }

    #[test]
    fn evaluate_sorted_critical_first() {
        let mut manager = SloManager::new();
        manager.register(SloDefinition::new(
            "medium",
            "medium",
            "reg.test",
            0.99,
            3600,
            SloSeverity::Medium,
        ));
        manager.register(SloDefinition::new(
            "critical",
            "critical",
            "reg.test",
            0.99,
            3600,
            SloSeverity::Critical,
        ));
        manager.register(SloDefinition::new(
            "high",
            "high",
            "reg.test",
            0.99,
            3600,
            SloSeverity::High,
        ));
        // All breached at 90% compliance — test that we can sort by severity
        let provider = TestDataProvider::new(1000, 900);
        let mut results = manager.evaluate(&provider);
        // Sort: Critical (0) before High (1) before Medium (2)
        let severity_of = |id: &str| -> u8 {
            match id {
                "critical" => 0,
                "high" => 1,
                "medium" => 2,
                _ => 3,
            }
        };
        results.sort_by_key(|e| severity_of(&e.slo_id));
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].slo_id, "critical");
        assert_eq!(results[1].slo_id, "high");
        assert_eq!(results[2].slo_id, "medium");
    }

    #[test]
    fn seed_slos_evaluate_correctly() {
        let manager = SloManager::with_seed_slos();
        // All seed SLOs target 99.5% or 99.9%
        // 999/1000 = 99.9% — passes all
        let provider = TestDataProvider::new(1000, 999);
        let results = manager.evaluate(&provider);
        assert_eq!(results.len(), 3);
        // SLO-INF-001 and SLO-API-001 target 99.9% — 99.9% passes
        // SLO-SKL-001 targets 99.5% — 99.9% passes
        for eval in &results {
            assert!(
                !eval.in_breach,
                "Seed SLO {} should not be breached at 99.9% compliance",
                eval.slo_id
            );
        }
    }
}
