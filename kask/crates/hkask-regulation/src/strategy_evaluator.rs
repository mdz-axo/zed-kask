//! StrategyEvaluator — Multi-model regulation strategy selection (Fermi improvement-loop pattern).
//!
//! Where `try_substitute` walks a fixed action ladder within a single strategy,
//! `StrategyEvaluator` selects between *different strategies* for the same metric.
//! This is Fermi's `improvement_loop` applied to cybernetic regulation:
//! try multiple model variants, score each by held-out effectiveness, promote the winner.
//!
//! ## Architecture
//!
//! - **Strategy**: a named action ladder (e.g., "default", "aggressive").
//! - **Evaluator**: tracks per-metric strategy effectiveness from pipeline records.
//! - **Promotion**: when the active strategy's effectiveness drops below threshold,
//!   the evaluator promotes the next-best strategy and emits a Regulation span.
//!
//! ## Current strategies (per metric)
//!
//! | Strategy   | Ladder                                        | Philosophy |
//! |------------|-----------------------------------------------|------------|
//! | default    | Standard substitution ladder                  | Balanced — tries alternatives before escalating |
//! | aggressive | Escalate-first, shorter ladder, higher urgency | Escalates early, tolerates more variation |

use std::collections::HashMap;

use crate::types::loops::SignalMetric;

/// Maximum history entries considered for strategy scoring.
const STRATEGY_SCORE_WINDOW: usize = 20;

/// Minimum cycles before a strategy is considered for promotion/demotion.
const MIN_CYCLES_FOR_EVALUATION: u64 = 5;

/// Effectiveness threshold below which the active strategy is considered failing.
const PROMOTION_THRESHOLD: f64 = 0.50;

/// Consecutive cycles below threshold before promotion triggers.
const PROMOTION_PATIENCE: u32 = 3;

/// A named regulation strategy variant.
#[derive(Debug, Clone)]
pub(crate) struct RegulationStrategy {
    pub name: String,
}

/// Per-metric strategy tracking state.
#[derive(Debug, Clone, Default)]
struct MetricStrategyState {
    /// Name of the currently active strategy.
    active: String,
    /// Consecutive cycles with effectiveness below PROMOTION_THRESHOLD.
    below_threshold_count: u32,
    /// Accumulated pipeline records for scoring.
    history: Vec<StrategyCycleResult>,
}

/// A single cycle's outcome for a specific metric+strategy.
#[derive(Debug, Clone)]
struct StrategyCycleResult {
    accepted: u64,
    total: u64,
}

/// Multi-model strategy evaluator.
///
/// Tracks strategy effectiveness per metric and promotes alternatives
/// when the active strategy consistently underperforms.
pub(crate) struct StrategyEvaluator {
    strategies: HashMap<SignalMetric, Vec<RegulationStrategy>>,
    state: HashMap<SignalMetric, MetricStrategyState>,
}

impl StrategyEvaluator {
    /// Create an evaluator with default strategies for all regulated metrics.
    pub fn new() -> Self {
        let mut strategies: HashMap<SignalMetric, Vec<RegulationStrategy>> = HashMap::new();

        let regulated_metrics = [
            SignalMetric::EnergyRemaining,
            SignalMetric::VarietyDeficit,
            SignalMetric::ErrorRate,
            SignalMetric::ConnectorLatency,
            SignalMetric::CommunicationQueueDepth,
            SignalMetric::WalletBalanceRatio,
            SignalMetric::WalletKeyHealth,
            SignalMetric::ToolReliability,
        ];

        for &metric in &regulated_metrics {
            strategies.insert(
                metric,
                vec![
                    RegulationStrategy {
                        name: "default".into(),
                    },
                    RegulationStrategy {
                        name: "aggressive".into(),
                    },
                ],
            );
        }

        Self {
            strategies,
            state: HashMap::new(),
        }
    }

    /// Record a cycle's outcome for strategy scoring.
    pub fn record_cycle(&mut self, metric: SignalMetric, accepted: u64, staged: u64, blocked: u64) {
        let total = accepted + staged + blocked;
        if total == 0 {
            return;
        }
        let state = self.state.entry(metric).or_default();
        state.history.push(StrategyCycleResult { accepted, total });
        if state.history.len() > STRATEGY_SCORE_WINDOW {
            state.history.remove(0);
        }
    }

    /// Evaluate strategy effectiveness and check for promotion.
    /// Returns true if a promotion occurred this cycle.
    pub fn active_policy(&mut self, metric: SignalMetric) -> bool {
        let state = self.state.entry(metric).or_default();
        if state.active.is_empty() {
            state.active = "default".into();
        }

        // Score the active strategy.
        let score = Self::effectiveness_from_history(&state.history);

        if state.history.len() as u64 >= MIN_CYCLES_FOR_EVALUATION && score < PROMOTION_THRESHOLD {
            state.below_threshold_count += 1;
        } else {
            state.below_threshold_count = 0;
        }

        if state.below_threshold_count >= PROMOTION_PATIENCE {
            // Promote to next strategy.
            let strategies = self.strategies.get(&metric);
            if let Some(list) = strategies {
                let current_idx = list.iter().position(|s| s.name == state.active);
                if let Some(idx) = current_idx {
                    let next_idx = (idx + 1) % list.len();
                    if next_idx != idx {
                        let old = state.active.clone();
                        state.active = list[next_idx].name.clone();
                        state.below_threshold_count = 0;
                        tracing::info!(
                            target: "hkask.strategy",
                            metric = metric.as_str(),
                            from = %old,
                            to = %state.active,
                            effectiveness = score,
                            "Strategy promoted due to sustained ineffectiveness"
                        );
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Current effectiveness of the active strategy (0.0–1.0).
    fn effectiveness_from_history(history: &[StrategyCycleResult]) -> f64 {
        if history.is_empty() {
            return 1.0;
        }
        let accepted: u64 = history.iter().map(|r| r.accepted).sum();
        let total: u64 = history.iter().map(|r| r.total).sum();
        if total == 0 {
            1.0
        } else {
            accepted as f64 / total as f64
        }
    }
}

impl Default for StrategyEvaluator {
    fn default() -> Self {
        Self::new()
    }
}
