//! Convergence tracking — PDCA loop exit condition evaluation.
//!
//! Extracted from the executor to collapse the 8-argument `finalize_convergence_report`
//! calls (11 copies in `execute_manifest`) and the 9-argument `check_convergence`
//! into a cohesive state machine. The convergence domain has its own vocabulary
//! (threshold, improvement_ratio, improvement_gate, baseline_quality,
//! min_iterations, aggregation) and its own output contract (the 14-field
//! `_convergence` JSON shape) — both belong together, not threaded through the
//! cascade loop as positional arguments.
//!
//! # Design
//!
//! `ConvergenceTracker` is a pure state machine over `(context, config)` —
//! it has no dependency on `InferencePort`, `ToolPort`, gas, or rJoule. The
//! executor constructs one per cascade, calls `check_met` after each pass,
//! and calls `finalize_report` at exit. The `_convergence` JSON shape is
//! owned by this module, not assembled ad-hoc at 11 call sites.

use crate::bundle::config::{AggregationSource, ConvergenceConfig};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Convergence status at cascade exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvergenceStatus {
    /// Threshold met — cascade converged successfully.
    Converged,
    /// `max_iterations` exhausted without meeting threshold.
    MaxedOut,
    /// `escalate` action or `on_not_reached: escalate` — cascade blocked.
    Escalated,
    /// Still running (used in live context injection).
    Running,
}

impl ConvergenceStatus {
    /// The string representation used in the `_convergence.status` context field.
    pub fn as_str(&self) -> &'static str {
        match self {
            ConvergenceStatus::Converged => "converged",
            ConvergenceStatus::MaxedOut => "maxed_out",
            ConvergenceStatus::Escalated => "escalated",
            ConvergenceStatus::Running => "running",
        }
    }
}

/// Tracks PDCA convergence state using the Improvement Kata model.
///
/// The agent has a **target condition** and a **current condition**, each
/// measured in two orthogonal spaces:
///
/// - **Object space** (Dublin Core): artifact completeness — are the required
///   fields populated and grounded?
/// - **Process space** (PKO): procedure progress — are the required steps
///   executed?
///
/// The total distance to the target is the hypotenuse of the right triangle
/// formed by the two gaps: `sqrt(object_gap² + process_gap²)`. Convergence
/// requires both legs to close.
///
/// Each PDCA cycle, the agent makes a **prediction** ("the hypotenuse will
/// decrease by Δ" with confidence `c`). After the experiment, the actual
/// decrease is measured. The **Brier score** `(c − actual_outcome)²` tracks
/// whether the agent's predictions are calibrated. Brier decreasing → the
/// agent is learning to predict its own progress. Brier stable and low →
/// confidence convergence.
///
/// This replaces the old self-grade model where an LLM graded its own plan
/// quality. The Kata model uses the LLM only for the four Kata steps (grasp,
/// target, predict, experiment); the executor computes the gap and Brier
/// score deterministically.
pub struct ConvergenceTracker {
    // ── Kata target-condition config ──
    #[allow(dead_code)]
    target_artifacts_field: Option<String>,
    #[allow(dead_code)]
    current_artifacts_field: Option<String>,
    #[allow(dead_code)]
    target_procedure_field: Option<String>,
    #[allow(dead_code)]
    current_procedure_field: Option<String>,
    #[allow(dead_code)]
    prediction_field: Option<String>,
    #[allow(dead_code)]
    result_field: Option<String>,
    hypotenuse_epsilon: f64,
    brier_window: u32,
    brier_threshold: f64,
    convergence_mode: String,

    // ── Trajectory history ──
    /// Hypotenuse history, one entry per completed PDCA cycle. Should be
    /// *decreasing* — the agent is getting closer to the target. Convergence
    /// is `h_n < hypotenuse_epsilon`. Confidence convergence is `h` stopped
    /// decreasing AND Brier is calibrated.
    hypotenuse_history: Vec<f64>,
    /// Brier score history, one entry per completed PDCA cycle. Should be
    /// *decreasing* — the agent's predictions are getting calibrated.
    /// Convergence (confidence mode) is rolling average < brier_threshold.
    brier_history: Vec<f64>,

    // ── Loop control ──
    min_iterations: u32,
    max_iterations: u32,

    // ── Legacy self-grade fields (for manifests not yet migrated) ──
    threshold: f64,
    field: String,
    improvement_ratio: f64,
    improvement_gate: String,
    baseline_quality: Option<f64>,
    /// Self-grade quality history — used by the legacy stability gates.
    /// Retained for manifests that haven't migrated to the Kata model.
    quality_history: Vec<f64>,
}

impl ConvergenceTracker {
    /// Construct from a manifest's convergence config.
    pub fn new(config: &ConvergenceConfig) -> Self {
        Self {
            target_artifacts_field: config.target_artifacts_field.clone(),
            current_artifacts_field: config.current_artifacts_field.clone(),
            target_procedure_field: config.target_procedure_field.clone(),
            current_procedure_field: config.current_procedure_field.clone(),
            prediction_field: config.prediction_field.clone(),
            result_field: config.result_field.clone(),
            hypotenuse_epsilon: config.hypotenuse_epsilon,
            brier_window: config.brier_window,
            brier_threshold: config.brier_threshold,
            convergence_mode: config.convergence_mode.clone(),
            hypotenuse_history: Vec::new(),
            brier_history: Vec::new(),
            min_iterations: config.min_iterations,
            max_iterations: if config.max_iterations == 0 {
                1
            } else {
                config.max_iterations
            },
            // Legacy
            threshold: config.threshold,
            field: config.convergence_field.clone(),
            improvement_ratio: config.improvement_ratio,
            improvement_gate: config.improvement_gate.clone(),
            baseline_quality: None,
            quality_history: Vec::new(),
        }
    }

    /// The configured threshold (legacy self-grade model).
    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    /// The configured convergence field (legacy self-grade model).
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The max iterations (1 for single-pass manifests).
    pub fn max_iterations(&self) -> u32 {
        self.max_iterations
    }

    /// Whether improvement tracking is enabled (legacy self-grade model).
    pub fn improvement_enabled(&self) -> bool {
        self.improvement_ratio > 0.0
    }

    /// Whether the Kata convergence model is active (convergence_mode is set
    /// and at least one target-condition field is configured).
    pub fn kata_enabled(&self) -> bool {
        !self.convergence_mode.is_empty()
            && (self.target_artifacts_field.is_some() || self.target_procedure_field.is_some())
    }

    /// Read-only access to the hypotenuse history.
    pub fn hypotenuse_history(&self) -> &[f64] {
        &self.hypotenuse_history
    }

    /// Read-only access to the Brier score history.
    pub fn brier_history(&self) -> &[f64] {
        &self.brier_history
    }

    /// Record a PDCA cycle's hypotenuse and Brier score. Called by the executor
    /// after the gap and prediction-vs-result compute steps have run. The
    /// hypotenuse should be *decreasing* (the agent is closing the gap); the
    /// Brier score should be *decreasing* (the agent's predictions are getting
    /// calibrated).
    pub fn push_kata_cycle(&mut self, hypotenuse: f64, brier: f64) {
        self.hypotenuse_history.push(hypotenuse);
        self.brier_history.push(brier);
    }

    /// Record a PDCA cycle's hypotenuse only (when Brier is not yet available —
    /// e.g., the first cycle before any prediction has been made).
    pub fn push_hypotenuse(&mut self, hypotenuse: f64) {
        self.hypotenuse_history.push(hypotenuse);
        // Push NaN for Brier so the histories stay aligned by cycle count.
        self.brier_history.push(f64::NAN);
    }

    /// Record a PDCA cycle from the executor context. For the Kata model,
    /// reads the hypotenuse and Brier score from the context (produced by
    /// `compute` steps with `compute_ref: kata.hypotenuse` and
    /// `kata.prediction_vs_result`). For the legacy model, reads the self-grade
    /// metric from the convergence field. Called by the executor after each
    /// iteration's compute steps have run, BEFORE `check_met`.
    pub fn push_cycle_from_context(&mut self, context: &HashMap<String, Value>) {
        if self.kata_enabled() {
            // Kata model: read hypotenuse and Brier from context
            let hypotenuse = context
                .get("kata_hypotenuse")
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN);
            let brier = context
                .get("kata_brier")
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN);
            self.hypotenuse_history.push(hypotenuse);
            self.brier_history.push(brier);
        } else {
            // Legacy model: read self-grade metric from convergence field
            let current = context
                .get(&self.field)
                .and_then(|v| v.as_f64())
                .or_else(|| resolve_dot_path(&self.field, context).and_then(|v| v.as_f64()));
            let current = if current.is_none() && self.field != "composite" {
                context.get("composite").and_then(|v| v.as_f64())
            } else {
                current
            };
            let current = if current.is_none() {
                context.get("_convergence_score").and_then(|v| v.as_f64())
            } else {
                current
            };
            self.quality_history.push(current.unwrap_or(f64::NAN));
        }
    }

    /// Capture the baseline quality on the first full pass. Called once,
    /// after the first pass completes; subsequent calls are no-ops.
    pub fn capture_baseline(&mut self, context: &HashMap<String, Value>) {
        if self.baseline_quality.is_none() {
            self.baseline_quality = context
                .get(&self.field)
                .and_then(|v| v.as_f64())
                .or_else(|| resolve_dot_path(&self.field, context).and_then(|v| v.as_f64()));
        }
    }

    /// Check whether convergence has been met.
    ///
    /// If the Kata model is active (`kata_enabled()`), uses the hypotenuse and
    /// Brier trajectories:
    /// - "hypotenuse": `hypotenuse_history.last() < hypotenuse_epsilon`.
    /// - "confidence": rolling Brier average < `brier_threshold` for
    ///   `brier_window` cycles AND hypotenuse not decreasing.
    /// - "hypotenuse_or_confidence": either condition.
    ///
    /// Otherwise, falls back to the legacy self-grade model (threshold +
    /// improvement gate + stability).
    pub fn check_met(&self, context: &HashMap<String, Value>, iteration: u32) -> bool {
        if iteration <= self.min_iterations {
            return false;
        }

        if self.kata_enabled() {
            return self.check_kata_met();
        }

        // Legacy self-grade model
        self.check_legacy_met(context)
    }

    /// Kata convergence check: hypotenuse and/or Brier trajectory.
    fn check_kata_met(&self) -> bool {
        let last_hypotenuse = self.hypotenuse_history.last().copied();
        let gap_converged = last_hypotenuse
            .filter(|h| h.is_finite())
            .map(|h| h < self.hypotenuse_epsilon)
            .unwrap_or(false);

        let confidence_converged = self.check_confidence_converged();

        match self.convergence_mode.as_str() {
            "hypotenuse" => gap_converged,
            "confidence" => confidence_converged,
            "hypotenuse_or_confidence" => gap_converged || confidence_converged,
            _ => gap_converged, // default to gap
        }
    }

    /// Confidence convergence: rolling Brier average < threshold for brier_window
    /// cycles AND hypotenuse not decreasing (the agent is calibrated but stuck).
    fn check_confidence_converged(&self) -> bool {
        if (self.brier_history.len() as u32) < self.brier_window {
            return false;
        }

        // Rolling Brier average over the last brier_window cycles
        let start = self
            .brier_history
            .len()
            .saturating_sub(self.brier_window as usize);
        let recent: Vec<f64> = self.brier_history[start..]
            .iter()
            .copied()
            .filter(|f| f.is_finite())
            .collect();
        if (recent.len() as u32) < self.brier_window {
            return false;
        }
        let rolling_brier: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
        if rolling_brier >= self.brier_threshold {
            return false;
        }

        // Hypotenuse must not be decreasing (agent is stuck, not still progressing)
        !self.is_hypotenuse_decreasing()
    }

    /// Is the hypotenuse still decreasing? True if the last two readings show
    /// a decrease larger than the epsilon (the agent is still making progress).
    fn is_hypotenuse_decreasing(&self) -> bool {
        if self.hypotenuse_history.len() < 2 {
            return false;
        }
        let n = self.hypotenuse_history.len();
        let prev = self.hypotenuse_history[n - 2];
        let curr = self.hypotenuse_history[n - 1];
        prev.is_finite()
            && curr.is_finite()
            && prev > curr
            && (prev - curr) > self.hypotenuse_epsilon
    }

    /// Legacy self-grade convergence check (threshold + improvement + stability).
    fn check_legacy_met(&self, context: &HashMap<String, Value>) -> bool {
        let current = context
            .get(&self.field)
            .and_then(|v| v.as_f64())
            .or_else(|| resolve_dot_path(&self.field, context).and_then(|v| v.as_f64()));
        let current = if current.is_none() && self.field != "composite" {
            context.get("composite").and_then(|v| v.as_f64())
        } else {
            current
        };
        let current = if current.is_none() {
            context.get("_convergence_score").and_then(|v| v.as_f64())
        } else {
            current
        };

        let threshold_met = current.map(|q| q <= self.threshold).unwrap_or(false);

        let improvement_met = if self.improvement_ratio > 0.0 {
            match (self.baseline_quality, current) {
                (Some(b), Some(c)) if b > 0.0 => ((b - c) / b) >= self.improvement_ratio,
                _ => false,
            }
        } else {
            false
        };

        match self.improvement_gate.as_str() {
            "both" => threshold_met && improvement_met,
            "either" => threshold_met || improvement_met,
            _ => threshold_met, // "threshold_only"
        }
    }

    /// Compute compound quality from nested inner skill convergence reports.
    /// Used when `aggregation != "none"` and `aggregation_sources` is non-empty.
    pub fn compute_compound_quality(
        &self,
        context: &HashMap<String, Value>,
        method: &str,
        sources: &[AggregationSource],
    ) -> f64 {
        match method {
            "all_converged" => {
                let all_ok = sources.iter().all(|src| {
                    let key = format!("step_{}_result", src.step_ordinal);
                    context
                        .get(&key)
                        .and_then(|v| v.get("_convergence"))
                        .and_then(|c| c.get("status"))
                        .and_then(|s| s.as_str())
                        .map(|s| s == "converged")
                        .unwrap_or(false)
                });
                if all_ok { 0.0 } else { 1.0 }
            }
            "min" => sources
                .iter()
                .filter_map(|src| {
                    let key = format!("step_{}_result", src.step_ordinal);
                    context
                        .get(&key)
                        .and_then(|v| v.get("_convergence"))
                        .and_then(|c| c.get("quality_at_exit"))
                        .and_then(|v| v.as_f64())
                })
                .fold(1.0_f64, f64::min),
            "weighted_avg" => {
                let mut sum = 0.0_f64;
                let mut total = 0.0_f64;
                for src in sources {
                    let key = format!("step_{}_result", src.step_ordinal);
                    if let Some(v) = context
                        .get(&key)
                        .and_then(|v| v.get("_convergence"))
                        .and_then(|c| c.get("quality_at_exit"))
                        .and_then(|v| v.as_f64())
                    {
                        sum += v * src.weight;
                        total += src.weight;
                    }
                }
                if total > 0.0 { sum / total } else { 1.0 }
            }
            _ => 0.0,
        }
    }

    /// Finalize the convergence report at cascade exit.
    ///
    /// Writes the 14-field `_convergence` JSON into the context. This is the
    /// single source of truth for the `_convergence` shape — previously
    /// assembled ad-hoc at 11 call sites in the executor.
    #[allow(clippy::too_many_arguments)]
    pub fn finalize_report(
        &self,
        context: &mut HashMap<String, Value>,
        status: ConvergenceStatus,
        reason: &str,
        iteration: u32,
        gas_used: u64,
        gas_cap: u64,
        rjoule_used: f64,
        rjoule_cap: f64,
    ) {
        let quality = context
            .get(&self.field)
            .and_then(|v| v.as_f64())
            .or_else(|| resolve_dot_path(&self.field, context).and_then(|v| v.as_f64()));

        let improvement_achieved = self
            .baseline_quality
            .and_then(|b| quality.map(|q| if b > 0.0 { (b - q) / b } else { 0.0 }));
        let improvement_pct = self
            .baseline_quality
            .and_then(|b| quality.map(|q| if b > 0.0 { ((b - q) / b) * 100.0 } else { 0.0 }));

        context.insert(
            "_convergence".to_string(),
            json!({
                "status": status.as_str(),
                "reason": reason,
                "iterations_completed": iteration,
                "quality_at_exit": quality,
                "threshold": self.threshold,
                "field": self.field,
                "improvement_achieved": improvement_achieved,
                "improvement_pct": improvement_pct,
                "improvement_target": self.improvement_ratio,
                "baseline_quality": self.baseline_quality,
                "quality_history": self.quality_history,
                // Kata model fields
                "hypotenuse_history": self.hypotenuse_history,
                "brier_history": self.brier_history,
                "hypotenuse_epsilon": self.hypotenuse_epsilon,
                "brier_threshold": self.brier_threshold,
                "brier_window": self.brier_window,
                "convergence_mode": self.convergence_mode,
                "kata_enabled": self.kata_enabled(),
                "gas_used": gas_used as f64,
                "gas_cap": gas_cap as f64,
                "gas_remaining": (gas_cap as f64 - gas_used as f64).max(0.0),
                "gas_pct": if gas_cap > 0 { (gas_used as f64 / gas_cap as f64) * 100.0 } else { 0.0 },
                "rjoule_used": rjoule_used,
                "rjoule_cap": rjoule_cap,
            }),
        );
    }

    /// Inject the live (running) convergence context for template awareness.
    /// Called at the start of each iteration so templates can reference
    /// `{{ _convergence.iterations_completed }}` etc.
    pub fn inject_running(
        &self,
        context: &mut HashMap<String, Value>,
        iteration: u32,
        gas_used: u64,
        gas_cap: u64,
        rjoule_used: f64,
        rjoule_cap: f64,
    ) {
        context.insert(
            "_convergence".to_string(),
            json!({
                "threshold": self.threshold,
                "max_iterations": self.max_iterations,
                "field": self.field,
                "status": ConvergenceStatus::Running.as_str(),
                "iterations_completed": iteration,
                "exit_reason": null,
                "improvement_target": self.improvement_ratio,
                "baseline_quality": self.baseline_quality,
                "quality_history": self.quality_history,
                // Kata model fields
                "hypotenuse_history": self.hypotenuse_history,
                "brier_history": self.brier_history,
                "hypotenuse_epsilon": self.hypotenuse_epsilon,
                "brier_threshold": self.brier_threshold,
                "brier_window": self.brier_window,
                "convergence_mode": self.convergence_mode,
                "kata_enabled": self.kata_enabled(),
                "gas_cap": gas_cap,
                "gas_used": gas_used,
                "gas_remaining": gas_cap.saturating_sub(gas_used),
                "rjoule_cap": rjoule_cap,
                "rjoule_used": rjoule_used,
                "rjoule_remaining": (rjoule_cap - rjoule_used).max(0.0),
            }),
        );
    }
}

/// Resolve a dot-path like "step_1_result.field" from the context.
/// (Duplicated from executor.rs to keep this module self-contained —
/// the function is a pure leaf with no dependency on executor state.)
fn resolve_dot_path(path: &str, context: &HashMap<String, Value>) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return None;
    }
    let first = context.get(parts[0])?.clone();
    let mut current = first;
    for part in &parts[1..] {
        match current {
            Value::Object(map) => {
                current = map.get(*part)?.clone();
            }
            _ => return None,
        }
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(threshold: f64, field: &str, max_iter: u32, min_iter: u32) -> ConvergenceConfig {
        ConvergenceConfig {
            target_artifacts_field: None,
            current_artifacts_field: None,
            target_procedure_field: None,
            current_procedure_field: None,
            prediction_field: None,
            result_field: None,
            hypotenuse_epsilon: 0.05,
            brier_window: 3,
            brier_threshold: 0.15,
            convergence_mode: String::new(), // empty = legacy mode
            max_iterations: max_iter,
            min_iterations: min_iter,
            on_not_reached: "abort".to_string(),
            threshold,
            convergence_field: field.to_string(),
            improvement_ratio: 0.0,
            improvement_gate: "threshold_only".to_string(),
            aggregation: "none".to_string(),
            aggregation_sources: vec![],
        }
    }

    fn kata_config(mode: &str) -> ConvergenceConfig {
        ConvergenceConfig {
            target_artifacts_field: Some("current_artifacts".to_string()),
            current_artifacts_field: Some("current_artifacts".to_string()),
            target_procedure_field: Some("current_procedure".to_string()),
            current_procedure_field: Some("current_procedure".to_string()),
            prediction_field: Some("prediction".to_string()),
            result_field: Some("result".to_string()),
            hypotenuse_epsilon: 0.05,
            brier_window: 3,
            brier_threshold: 0.15,
            convergence_mode: mode.to_string(),
            max_iterations: 5,
            min_iterations: 2,
            on_not_reached: "abort".to_string(),
            threshold: 0.1,
            convergence_field: "composite".to_string(),
            improvement_ratio: 0.0,
            improvement_gate: "threshold_only".to_string(),
            aggregation: "none".to_string(),
            aggregation_sources: vec![],
        }
    }

    #[test]
    fn check_met_returns_false_when_below_min_iterations() {
        let cfg = config(0.15, "composite", 3, 2);
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert("composite".to_string(), json!(0.05)); // below threshold
        // iteration 1 <= min_iterations 2 → false even though threshold met
        assert!(!tracker.check_met(&ctx, 1));
        assert!(tracker.check_met(&ctx, 3)); // iteration 3 > 2 → true
    }

    #[test]
    fn check_met_threshold_only() {
        let cfg = config(0.15, "composite", 3, 0);
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert("composite".to_string(), json!(0.10)); // 0.10 <= 0.15 → met
        assert!(tracker.check_met(&ctx, 1));
        ctx.insert("composite".to_string(), json!(0.20)); // 0.20 > 0.15 → not met
        assert!(!tracker.check_met(&ctx, 1));
    }

    #[test]
    fn check_met_falls_back_to_composite_when_field_missing() {
        let cfg = config(0.15, "score", 3, 0); // field is "score"
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        // "score" is missing, but "composite" is present and below threshold
        ctx.insert("composite".to_string(), json!(0.10));
        assert!(tracker.check_met(&ctx, 1));
    }

    #[test]
    fn check_met_falls_back_to_convergence_score() {
        let cfg = config(0.15, "score", 3, 0);
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        // Neither "score" nor "composite" — fall back to "_convergence_score"
        ctx.insert("_convergence_score".to_string(), json!(0.05));
        assert!(tracker.check_met(&ctx, 1));
    }

    #[test]
    fn check_met_returns_false_when_quality_missing() {
        let cfg = config(0.15, "score", 3, 0);
        let tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new(); // no quality fields at all
        assert!(!tracker.check_met(&ctx, 1));
    }

    #[test]
    fn check_met_both_gate_requires_threshold_and_improvement() {
        let mut cfg = config(0.15, "composite", 3, 0);
        cfg.improvement_ratio = 0.25;
        cfg.improvement_gate = "both".to_string();
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert("composite".to_string(), json!(0.10)); // threshold met
        // No baseline captured → improvement_met = false → both gate fails
        assert!(!tracker.check_met(&ctx, 1));
    }

    #[test]
    fn check_met_either_gate_accepts_either() {
        let mut cfg = config(0.15, "composite", 3, 0);
        cfg.improvement_ratio = 0.25;
        cfg.improvement_gate = "either".to_string();
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert("composite".to_string(), json!(0.10)); // threshold met
        // improvement not met (no baseline), but either gate → true
        assert!(tracker.check_met(&ctx, 1));
    }

    #[test]
    fn capture_baseline_records_first_pass_quality() {
        let mut cfg = config(0.15, "composite", 3, 0);
        cfg.improvement_ratio = 0.25;
        cfg.improvement_gate = "both".to_string();
        let mut tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert("composite".to_string(), json!(0.50)); // baseline = 0.50
        tracker.capture_baseline(&ctx);
        // Now improve to 0.10 — improvement = (0.50 - 0.10) / 0.50 = 0.80 >= 0.25
        ctx.insert("composite".to_string(), json!(0.10));
        assert!(tracker.check_met(&ctx, 1)); // both threshold and improvement met
    }

    #[test]
    fn capture_baseline_is_idempotent() {
        let cfg = config(0.15, "composite", 3, 0);
        let mut tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert("composite".to_string(), json!(0.50));
        tracker.capture_baseline(&ctx);
        let first_baseline = tracker.baseline_quality;
        ctx.insert("composite".to_string(), json!(0.20));
        tracker.capture_baseline(&ctx); // should not overwrite
        assert_eq!(tracker.baseline_quality, first_baseline);
    }

    // ── Kata hypotenuse + Brier convergence model ──

    #[test]
    fn kata_enabled_reports_correctly() {
        let cfg = config(0.15, "composite", 3, 0);
        assert!(!ConvergenceTracker::new(&cfg).kata_enabled());
        let cfg = kata_config("hypotenuse_or_confidence");
        assert!(ConvergenceTracker::new(&cfg).kata_enabled());
    }

    #[test]
    fn kata_hypotenuse_converges_when_gap_below_epsilon() {
        let cfg = kata_config("hypotenuse");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // Cycle 1: gap = 0.3 (not converged)
        tracker.push_hypotenuse(0.3);
        assert!(!tracker.check_met(&ctx, 3));
        // Cycle 2: gap = 0.02 (below epsilon 0.05)
        tracker.push_hypotenuse(0.02);
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn kata_hypotenuse_rejects_when_gap_above_epsilon() {
        let cfg = kata_config("hypotenuse");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_hypotenuse(0.3);
        tracker.push_hypotenuse(0.2); // still above 0.05
        assert!(!tracker.check_met(&ctx, 3));
    }

    #[test]
    fn kata_confidence_converges_when_brier_low_and_gap_stuck() {
        let cfg = kata_config("confidence");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // 3 cycles with low Brier and gap not decreasing (stuck at 0.3)
        tracker.push_kata_cycle(0.3, 0.05);
        tracker.push_kata_cycle(0.3, 0.05);
        tracker.push_kata_cycle(0.3, 0.05);
        // Brier rolling avg = 0.05 < 0.15, gap not decreasing → confidence converged
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn kata_confidence_rejects_when_brier_high() {
        let cfg = kata_config("confidence");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_kata_cycle(0.3, 0.5); // high Brier
        tracker.push_kata_cycle(0.3, 0.5);
        tracker.push_kata_cycle(0.3, 0.5);
        assert!(!tracker.check_met(&ctx, 3));
    }

    #[test]
    fn kata_confidence_rejects_when_gap_still_decreasing() {
        let cfg = kata_config("confidence");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // Brier is low but gap is still decreasing → not confidence converged
        tracker.push_kata_cycle(0.5, 0.05);
        tracker.push_kata_cycle(0.4, 0.05);
        tracker.push_kata_cycle(0.3, 0.05); // gap decreased 0.4→0.3
        assert!(!tracker.check_met(&ctx, 3));
    }

    #[test]
    fn kata_hypotenuse_or_confidence_accepts_either() {
        let cfg = kata_config("hypotenuse_or_confidence");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // Gap converged (0.02 < 0.05) even though Brier is high
        tracker.push_kata_cycle(0.02, 0.5);
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn kata_hypotenuse_or_confidence_accepts_confidence() {
        let cfg = kata_config("hypotenuse_or_confidence");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // Gap not converged (0.3 > 0.05) but Brier is low and gap stuck
        tracker.push_kata_cycle(0.3, 0.05);
        tracker.push_kata_cycle(0.3, 0.05);
        tracker.push_kata_cycle(0.3, 0.05);
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn kata_min_iterations_prevents_premature_exit() {
        let cfg = kata_config("hypotenuse");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_hypotenuse(0.02); // gap already below epsilon
        // iteration 2 <= min_iterations 2 → false even though gap is tiny
        assert!(!tracker.check_met(&ctx, 2));
        // iteration 3 > 2 → true
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn kata_is_hypotenuse_decreasing_detects_progress() {
        let cfg = kata_config("hypotenuse");
        let mut tracker = ConvergenceTracker::new(&cfg);
        assert!(!tracker.is_hypotenuse_decreasing()); // < 2 readings
        tracker.push_hypotenuse(0.5);
        tracker.push_hypotenuse(0.3); // decreased by 0.2 > epsilon
        assert!(tracker.is_hypotenuse_decreasing());
        tracker.push_hypotenuse(0.29); // decreased by 0.01 < epsilon
        assert!(!tracker.is_hypotenuse_decreasing());
    }

    #[test]
    fn finalize_report_writes_14_field_json() {
        let cfg = config(0.15, "composite", 3, 0);
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert("composite".to_string(), json!(0.10));
        tracker.finalize_report(
            &mut ctx,
            ConvergenceStatus::Converged,
            "quality_met",
            2,
            500,
            1000,
            1.5,
            5.0,
        );
        let conv = ctx.get("_convergence").unwrap();
        assert_eq!(conv["status"], "converged");
        assert_eq!(conv["reason"], "quality_met");
        assert_eq!(conv["iterations_completed"], 2);
        assert_eq!(conv["quality_at_exit"], 0.10);
        assert_eq!(conv["threshold"], 0.15);
        assert_eq!(conv["field"], "composite");
        assert_eq!(conv["gas_used"], 500.0);
        assert_eq!(conv["gas_cap"], 1000.0);
        assert_eq!(conv["gas_remaining"], 500.0);
        assert_eq!(conv["gas_pct"], 50.0);
        assert_eq!(conv["rjoule_used"], 1.5);
        assert_eq!(conv["rjoule_cap"], 5.0);
        // 14 fields total (status, reason, iterations_completed, quality_at_exit,
        // threshold, field, improvement_achieved, improvement_pct,
        // improvement_target, baseline_quality, gas_used, gas_cap, gas_remaining,
        // gas_pct, rjoule_used, rjoule_cap) — note: improvement_achieved and
        // improvement_pct are null when baseline is None, but still present.
        let obj = conv.as_object().unwrap();
        assert!(obj.len() >= 14, "expected >=14 fields, got {}", obj.len());
    }

    #[test]
    fn inject_running_writes_running_status() {
        let cfg = config(0.15, "composite", 3, 0);
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        tracker.inject_running(&mut ctx, 2, 300, 1000, 1.0, 5.0);
        let conv = ctx.get("_convergence").unwrap();
        assert_eq!(conv["status"], "running");
        assert_eq!(conv["iterations_completed"], 2);
        assert_eq!(conv["gas_used"], 300);
        assert_eq!(conv["gas_remaining"], 700);
    }

    #[test]
    fn compute_compound_quality_all_converged() {
        let cfg = config(0.15, "composite", 3, 0);
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert(
            "step_1_result".to_string(),
            json!({"_convergence": {"status": "converged", "quality_at_exit": 0.05}}),
        );
        ctx.insert(
            "step_2_result".to_string(),
            json!({"_convergence": {"status": "converged", "quality_at_exit": 0.10}}),
        );
        let sources = vec![
            AggregationSource {
                step_ordinal: 1,
                field: "_convergence.quality_at_exit".to_string(),
                weight: 1.0,
            },
            AggregationSource {
                step_ordinal: 2,
                field: "_convergence.quality_at_exit".to_string(),
                weight: 1.0,
            },
        ];
        let q = tracker.compute_compound_quality(&ctx, "all_converged", &sources);
        assert_eq!(q, 0.0); // all converged → 0.0
    }

    #[test]
    fn compute_compound_quality_min_takes_lowest_score() {
        // The `min` aggregation returns the lowest quality score (best quality,
        // since lower = closer to threshold = better). This mirrors the original
        // `fold(1.0, f64::min)` semantics.
        let cfg = config(0.15, "composite", 3, 0);
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert(
            "step_1_result".to_string(),
            json!({"_convergence": {"quality_at_exit": 0.05}}),
        );
        ctx.insert(
            "step_2_result".to_string(),
            json!({"_convergence": {"quality_at_exit": 0.20}}),
        );
        let sources = vec![
            AggregationSource {
                step_ordinal: 1,
                field: "_convergence.quality_at_exit".to_string(),
                weight: 1.0,
            },
            AggregationSource {
                step_ordinal: 2,
                field: "_convergence.quality_at_exit".to_string(),
                weight: 1.0,
            },
        ];
        let q = tracker.compute_compound_quality(&ctx, "min", &sources);
        assert_eq!(q, 0.05); // min(0.05, 0.20) = 0.05 (best quality)
    }

    #[test]
    fn compute_compound_quality_weighted_avg() {
        let cfg = config(0.15, "composite", 3, 0);
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        ctx.insert(
            "step_1_result".to_string(),
            json!({"_convergence": {"quality_at_exit": 0.10}}),
        );
        ctx.insert(
            "step_2_result".to_string(),
            json!({"_convergence": {"quality_at_exit": 0.20}}),
        );
        let sources = vec![
            AggregationSource {
                step_ordinal: 1,
                field: "_convergence.quality_at_exit".to_string(),
                weight: 3.0,
            },
            AggregationSource {
                step_ordinal: 2,
                field: "_convergence.quality_at_exit".to_string(),
                weight: 1.0,
            },
        ];
        let q = tracker.compute_compound_quality(&ctx, "weighted_avg", &sources);
        // (0.10*3 + 0.20*1) / (3+1) = 0.50/4 = 0.125
        assert!((q - 0.125).abs() < 1e-9);
    }
}
