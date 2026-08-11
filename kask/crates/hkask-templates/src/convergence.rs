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
use crate::input_mapping::resolve_dot_path;
use crate::step_context::{ContextLookup, ContextMap};
use serde_json::json;

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
/// For skills that use the full Kata gap model (`sequential-inquiry`,
/// `metacognition`), the total distance to the target is the hypotenuse of the
/// right triangle formed by the two gaps: `sqrt(object_gap² + process_gap²)`,
/// produced by the `kata.hypotenuse` compute primitive and pushed into the
/// tracker as the convergence signal. Convergence requires the gap to close.
///
/// For skills that use a custom convergence signal (violation count, finding
/// count, Pareto hypervolume delta, etc.), the signal is whatever scalar the
/// manifest pushes via the loop step's `convergence_signal:` binding. The
/// Cauchy check works on any scalar — it detects when the signal stops moving,
/// regardless of whether the signal is a gap distance.
///
/// Each PDCA cycle, the agent makes a **prediction** ("the signal will
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
    gap_epsilon: f64,
    cauchy_epsilon: f64,
    cauchy_window: u32,
    brier_window: u32,
    brier_threshold: f64,
    convergence_mode: String,

    // ── Trajectory history ──
    /// Convergence signal history, one entry per completed PDCA cycle. For
    /// Kata-gap skills this is the `kata.hypotenuse` value (Euclidean gap
    /// distance, decreasing toward zero). For custom-signal skills this is
    /// whatever scalar the manifest pushes (violation count, finding count,
    /// etc.). Gap convergence is `signal < gap_epsilon`. Cauchy convergence is
    /// `signal stopped moving` (max pairwise delta in window < cauchy_epsilon).
    signal_history: Vec<f64>,
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
            gap_epsilon: config.gap_epsilon,
            cauchy_epsilon: config.cauchy_epsilon,
            cauchy_window: config.cauchy_window,
            brier_window: config.brier_window,
            brier_threshold: config.brier_threshold,
            convergence_mode: config.convergence_mode.clone(),
            signal_history: Vec::new(),
            brier_history: Vec::new(),
            min_iterations: config.min_iterations,
            max_iterations: if config.max_iterations == 0 {
                10
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

    /// Read-only access to the convergence signal history.
    pub fn signal_history(&self) -> &[f64] {
        &self.signal_history
    }

    /// Read-only access to the Brier score history.
    pub fn brier_history(&self) -> &[f64] {
        &self.brier_history
    }

    /// Record a PDCA cycle's convergence signal and Brier score. Called by the
    /// executor after the gap and prediction-vs-result compute steps have run.
    /// For Kata-gap skills, the signal is the `kata.hypotenuse` value
    /// (decreasing toward zero). For custom-signal skills, the signal is
    /// whatever scalar the manifest pushes. The Brier score should be
    /// *decreasing* (the agent's predictions are getting calibrated).
    pub fn push_kata_cycle(&mut self, signal: f64, brier: f64) {
        self.signal_history.push(signal);
        self.brier_history.push(brier);
    }

    /// Record a PDCA cycle's convergence signal only (when Brier is not yet
    /// available — e.g., the first cycle before any prediction has been made).
    pub fn push_signal(&mut self, signal: f64) {
        self.signal_history.push(signal);
        // Push NaN for Brier so the histories stay aligned by cycle count.
        self.brier_history.push(f64::NAN);
    }

    /// Record a PDCA cycle from the executor context. For the Kata model,
    /// reads the convergence signal and Brier score from the context (the
    /// signal is produced by a `compute` step — `kata.hypotenuse` for Kata-gap
    /// skills, or `lisp.eval` / any compute for custom-signal skills — and
    /// bound into context via the loop step's `convergence_signal:` mapping).
    /// For the legacy model, reads the self-grade metric from the convergence
    /// field. Called by the executor after each iteration's compute steps have
    /// run, BEFORE `check_met`.
    pub fn push_cycle_from_context<C: ContextLookup>(&mut self, context: &C) {
        if self.kata_enabled() {
            // Kata model: read convergence signal and Brier from context
            let signal = context
                .get("convergence_signal")
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN);
            let brier = context
                .get("kata_brier")
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN);
            // A missing/non-finite signal silently degrades the Cauchy check
            // (NaN readings are filtered out, so a flat [NaN, NaN, NaN] history
            // never converges — but the operator sees no signal). Warn so an
            // operator reading logs can distinguish "signal is 0" from "signal
            // binding is broken" (the .rules "startup-failure signal" trap).
            if !signal.is_finite() {
                tracing::warn!(
                    target: "hkask.templates.convergence",
                    field = "convergence_signal",
                    "Convergence signal is missing or non-finite — the loop step's \
                     `convergence_signal:` binding did not resolve to a number. \
                     The Cauchy check will not fire until a finite reading is \
                     pushed. Remediation: check the manifest's loop step \
                     input_mapping — the bound expression must produce a finite \
                     f64 (not null, not a string, not an object)."
                );
            }
            // Delegate to `push_kata_cycle` so the test-only method becomes
            // production-load-bearing (the `.rules` "Convention helpers with
            // only test callers are dead code" trap — the prior inline push
            // duplicated `push_kata_cycle`'s logic, leaving it test-only).
            self.push_kata_cycle(signal, brier);
        } else {
            // Legacy model: read self-grade metric via the shared resolver so
            // the trajectory history uses the same value space as
            // `check_legacy_met` and `capture_baseline` (field → composite →
            // _convergence_score).
            let current = self.resolve_quality(context);
            self.quality_history.push(current.unwrap_or(f64::NAN));
        }
    }

    /// Capture the baseline quality on the first full pass. Called once,
    /// after the first pass completes; subsequent calls are no-ops.
    pub fn capture_baseline<C: ContextLookup>(&mut self, context: &C) {
        if self.baseline_quality.is_none() {
            self.baseline_quality = self.resolve_quality(context);
        }
    }

    /// Resolve the current quality reading for the legacy self-grade model.
    ///
    /// Tries the configured `convergence_field` (direct, then dot-path), then
    /// falls back to `composite` (when the field isn't itself `composite`),
    /// then to `_convergence_score`. The SAME chain is used by
    /// `capture_baseline`, `push_cycle_from_context`, and `check_legacy_met`
    /// so the baseline, the trajectory history, and the threshold check all
    /// read the same value space. Previously `capture_baseline` used only the
    /// first step, so when the field was reachable only via the `composite` /
    /// `_convergence_score` fallback the baseline stayed `None` and the
    /// improvement gate (`both`) was silently disabled — convergence could
    /// never fire even though `check_legacy_met` saw a below-threshold value.
    fn resolve_quality<C: ContextLookup>(&self, context: &C) -> Option<f64> {
        let current = context
            .get(&self.field)
            .and_then(|v| v.as_f64())
            .or_else(|| resolve_dot_path(&self.field, context).and_then(|v| v.as_f64()));
        if current.is_some() {
            return current;
        }
        if self.field != "composite" {
            if let Some(v) = context.get("composite").and_then(|v| v.as_f64()) {
                return Some(v);
            }
        }
        context.get("_convergence_score").and_then(|v| v.as_f64())
    }

    /// Check whether convergence has been met.
    ///
    /// If the Kata model is active (`kata_enabled()`), uses the convergence
    /// signal and Brier trajectories:
    /// - "gap": `signal_history.last() < gap_epsilon`.
    /// - "cauchy": max pairwise delta in the last `cauchy_window` readings <
    ///   `cauchy_epsilon` (the signal stopped moving).
    /// - "calibration": rolling Brier average < `brier_threshold` for
    ///   `brier_window` cycles AND signal not decreasing.
    /// - "gap_or_cauchy_or_calibration" (default): any of the three.
    ///
    /// Otherwise, falls back to the legacy self-grade model (threshold +
    /// improvement gate + stability).
    pub fn check_met<C: ContextLookup>(&self, context: &C, iteration: u32) -> bool {
        if iteration <= self.min_iterations {
            return false;
        }

        if self.kata_enabled() {
            return self.check_kata_met();
        }

        // Legacy self-grade model
        self.check_legacy_met(context)
    }

    /// Kata convergence check: gap, Cauchy, and/or calibration.
    ///
    /// Three canonical stop conditions (any active one triggers convergence):
    ///
    /// 1. **Gap convergence** (limit of a sequence): `signal < gap_epsilon`.
    ///    The agent reached the target condition. Only meaningful when the
    ///    signal is a real gap distance (e.g., `kata.hypotenuse` output).
    ///
    /// 2. **Cauchy convergence** (stall): the maximum pairwise distance between
    ///    signal readings in the last `cauchy_window` cycles is below
    ///    `cauchy_epsilon`. The iterates have stopped moving — learning
    ///    exhausted, current methods at their ceiling. Works on any scalar
    ///    signal (gap distance, violation count, finding count, etc.).
    ///
    /// 3. **Calibration convergence**: rolling Brier average below
    ///    `brier_threshold` for `brier_window` cycles. The agent's predictions
    ///    are calibrated — it knows what will happen when it acts.
    ///
    /// The `convergence_mode` field selects which are active. The default
    /// (`gap_or_cauchy_or_calibration`) enables all three.
    fn check_kata_met(&self) -> bool {
        let gap_converged = self.check_gap_converged();
        let cauchy_converged = self.check_cauchy_converged();
        let calibration_converged = self.check_calibration_converged();

        match self.convergence_mode.as_str() {
            "gap" => gap_converged,
            "cauchy" => cauchy_converged,
            "calibration" => calibration_converged,
            "gap_or_cauchy" => gap_converged || cauchy_converged,
            "gap_or_calibration" => gap_converged || calibration_converged,
            "cauchy_or_calibration" => cauchy_converged || calibration_converged,
            "gap_or_cauchy_or_calibration" => {
                gap_converged || cauchy_converged || calibration_converged
            }
            _ => gap_converged, // default to gap
        }
    }

    /// Gap convergence: signal below epsilon (limit of a sequence). Only
    /// meaningful when the signal is a real gap distance (e.g., the
    /// `kata.hypotenuse` output for Kata-gap skills).
    fn check_gap_converged(&self) -> bool {
        self.signal_history
            .last()
            .copied()
            .filter(|h| h.is_finite())
            .map(|h| h < self.gap_epsilon)
            .unwrap_or(false)
    }

    /// Cauchy convergence: the iterates have stopped moving. The maximum
    /// pairwise distance between signal readings in the last `cauchy_window`
    /// cycles is below `cauchy_epsilon`.
    ///
    /// This is the canonical Cauchy criterion: for all m, n > N,
    /// `‖xₘ − xₙ‖ < ε`. It catches both plateau (readings clustered together)
    /// and oscillation (readings bouncing — large pairwise distances → not
    /// Cauchy). Unlike checking just the last two readings, it requires *all*
    /// pairs in the window to be close. Works on any scalar signal — the signal
    /// need not be a gap distance.
    fn check_cauchy_converged(&self) -> bool {
        let window = self.cauchy_window as usize;
        if self.signal_history.len() < window {
            return false;
        }
        let start = self.signal_history.len().saturating_sub(window);
        let window_slice = &self.signal_history[start..];
        let finite: Vec<f64> = window_slice
            .iter()
            .copied()
            .filter(|f| f.is_finite())
            .collect();
        if finite.len() < window {
            return false;
        }
        // Max pairwise distance between all pairs in the window
        let mut max_delta = 0.0_f64;
        for i in 0..finite.len() {
            for j in (i + 1)..finite.len() {
                let delta = (finite[i] - finite[j]).abs();
                if delta > max_delta {
                    max_delta = delta;
                }
            }
        }
        max_delta < self.cauchy_epsilon
    }

    /// Calibration convergence: rolling Brier average below threshold for
    /// brier_window cycles. The agent's predictions are calibrated.
    fn check_calibration_converged(&self) -> bool {
        let window = self.brier_window as usize;
        if self.brier_history.len() < window {
            return false;
        }
        let start = self.brier_history.len().saturating_sub(window);
        let recent: Vec<f64> = self.brier_history[start..]
            .iter()
            .copied()
            .filter(|f| f.is_finite())
            .collect();
        if recent.len() < window {
            return false;
        }
        let rolling_brier: f64 = recent.iter().sum::<f64>() / recent.len() as f64;
        rolling_brier < self.brier_threshold
    }

    /// Legacy self-grade convergence check (threshold + improvement + stability).
    fn check_legacy_met<C: ContextLookup>(&self, context: &C) -> bool {
        let current = self.resolve_quality(context);
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
    pub fn compute_compound_quality<C: ContextLookup>(
        &self,
        context: &C,
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
            other => {
                // Fail-safe: an unknown aggregation method must NOT produce a
                // below-threshold value — `0.0` would falsely satisfy the
                // `quality <= threshold` stop condition (a typo'd `aggregation`
                // silently converged on the first check). Return 1.0 (not
                // converged) and warn so the typo is actionable (the .rules
                // "fails open with no diagnostic" trap).
                tracing::warn!(
                    target: "hkask.templates.convergence",
                    aggregation = other,
                    "Unknown aggregation method — defaulting to 1.0 (not converged). \
                     Remediation: set `aggregation` to all_converged | min | weighted_avg."
                );
                1.0
            }
        }
    }

    /// Finalize the convergence report at cascade exit.
    ///
    /// Writes the 14-field `_convergence` JSON into the context. This is the
    /// single source of truth for the `_convergence` shape — previously
    /// assembled ad-hoc at 11 call sites in the executor.
    pub fn finalize_report<M: ContextMap>(
        &self,
        context: &mut M,
        status: ConvergenceStatus,
        reason: &str,
        iteration: u32,
        gas_used: u64,
        gas_cap: u64,
        rjoule_used: f64,
        rjoule_cap: f64,
    ) {
        let quality = self.resolve_quality(context);

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
                "signal_history": self.signal_history,
                "brier_history": self.brier_history,
                "gap_epsilon": self.gap_epsilon,
                "cauchy_epsilon": self.cauchy_epsilon,
                "cauchy_window": self.cauchy_window,
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
    pub fn inject_running<M: ContextMap>(
        &self,
        context: &mut M,
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
                "signal_history": self.signal_history,
                "brier_history": self.brier_history,
                "gap_epsilon": self.gap_epsilon,
                "cauchy_epsilon": self.cauchy_epsilon,
                "cauchy_window": self.cauchy_window,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(threshold: f64, field: &str, max_iter: u32, min_iter: u32) -> ConvergenceConfig {
        ConvergenceConfig {
            target_artifacts_field: None,
            current_artifacts_field: None,
            target_procedure_field: None,
            current_procedure_field: None,
            prediction_field: None,
            result_field: None,
            gap_epsilon: 0.05,
            cauchy_epsilon: 0.03,
            cauchy_window: 3,
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
            gap_epsilon: 0.05,
            cauchy_epsilon: 0.03,
            cauchy_window: 3,
            brier_window: 3,
            brier_threshold: 0.15,
            convergence_mode: mode.to_string(),
            max_iterations: 10,
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

    // ── Kata convergence: three canonical stop conditions ──

    #[test]
    fn kata_enabled_reports_correctly() {
        let cfg = config(0.15, "composite", 3, 0);
        assert!(!ConvergenceTracker::new(&cfg).kata_enabled());
        let cfg = kata_config("gap_or_cauchy_or_calibration");
        assert!(ConvergenceTracker::new(&cfg).kata_enabled());
    }

    // ── Gap convergence (limit of a sequence) ──

    #[test]
    fn gap_converges_when_signal_below_epsilon() {
        let cfg = kata_config("gap");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_signal(0.3);
        assert!(!tracker.check_met(&ctx, 3));
        tracker.push_signal(0.02); // below epsilon 0.05
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn gap_rejects_when_signal_above_epsilon() {
        let cfg = kata_config("gap");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_signal(0.3);
        tracker.push_signal(0.2); // above 0.05
        assert!(!tracker.check_met(&ctx, 3));
    }

    // ── Cauchy convergence (stall — iterates stopped moving) ──

    #[test]
    fn cauchy_converges_when_readings_clustered() {
        let cfg = kata_config("cauchy");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // 3 readings within 0.03 of each other
        tracker.push_signal(0.30);
        tracker.push_signal(0.31);
        tracker.push_signal(0.30);
        // max pairwise delta = 0.01 < cauchy_epsilon 0.03
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn cauchy_rejects_oscillation() {
        let cfg = kata_config("cauchy");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // Oscillating: 0.3 → 0.5 → 0.3, max pairwise delta = 0.2 >> 0.03
        tracker.push_signal(0.30);
        tracker.push_signal(0.50);
        tracker.push_signal(0.30);
        assert!(!tracker.check_met(&ctx, 3));
    }

    #[test]
    fn cauchy_rejects_when_fewer_than_window_readings() {
        let cfg = kata_config("cauchy");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_signal(0.30);
        tracker.push_signal(0.31); // only 2 readings, window is 3
        assert!(!tracker.check_met(&ctx, 3));
    }

    #[test]
    fn cauchy_rejects_when_still_decreasing() {
        let cfg = kata_config("cauchy");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // Readings are decreasing: 0.5 → 0.3 → 0.1, max pairwise = 0.4 >> 0.03
        tracker.push_signal(0.50);
        tracker.push_signal(0.30);
        tracker.push_signal(0.10);
        assert!(!tracker.check_met(&ctx, 3));
    }

    // ── Calibration convergence (Brier score) ──

    #[test]
    fn calibration_converges_when_brier_low() {
        let cfg = kata_config("calibration");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // 3 cycles with low Brier
        tracker.push_kata_cycle(0.3, 0.05);
        tracker.push_kata_cycle(0.3, 0.05);
        tracker.push_kata_cycle(0.3, 0.05);
        // rolling Brier = 0.05 < 0.15
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn calibration_rejects_when_brier_high() {
        let cfg = kata_config("calibration");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_kata_cycle(0.3, 0.5);
        tracker.push_kata_cycle(0.3, 0.5);
        tracker.push_kata_cycle(0.3, 0.5);
        assert!(!tracker.check_met(&ctx, 3));
    }

    #[test]
    fn calibration_rejects_when_fewer_than_window() {
        let cfg = kata_config("calibration");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_kata_cycle(0.3, 0.05);
        tracker.push_kata_cycle(0.3, 0.05); // only 2, window is 3
        assert!(!tracker.check_met(&ctx, 3));
    }

    // ── Combined modes ──

    #[test]
    fn gap_or_cauchy_accepts_gap() {
        let cfg = kata_config("gap_or_cauchy");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_signal(0.02); // gap converged
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn gap_or_cauchy_accepts_cauchy() {
        let cfg = kata_config("gap_or_cauchy");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // Gap not converged (0.3 > 0.05) but Cauchy converged (clustered)
        tracker.push_signal(0.30);
        tracker.push_signal(0.31);
        tracker.push_signal(0.30);
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn gap_or_cauchy_or_calibration_accepts_any() {
        let cfg = kata_config("gap_or_cauchy_or_calibration");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // Gap not converged, Cauchy not converged, but Brier is low
        tracker.push_kata_cycle(0.5, 0.05); // decreasing → not Cauchy
        tracker.push_kata_cycle(0.3, 0.05);
        tracker.push_kata_cycle(0.1, 0.05);
        // rolling Brier = 0.05 < 0.15 → calibration converged
        assert!(tracker.check_met(&ctx, 3));
    }

    #[test]
    fn gap_or_cauchy_or_calibration_rejects_when_none_met() {
        let cfg = kata_config("gap_or_cauchy_or_calibration");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        // Gap high (0.3), Cauchy not met (decreasing), Brier high
        tracker.push_kata_cycle(0.5, 0.5);
        tracker.push_kata_cycle(0.3, 0.5);
        tracker.push_kata_cycle(0.1, 0.5);
        assert!(!tracker.check_met(&ctx, 3));
    }

    // ── Min iterations ──

    #[test]
    fn kata_min_iterations_prevents_premature_exit() {
        let cfg = kata_config("gap");
        let mut tracker = ConvergenceTracker::new(&cfg);
        let ctx = HashMap::new();
        tracker.push_signal(0.02); // gap already below epsilon
        assert!(!tracker.check_met(&ctx, 2)); // iteration 2 <= min 2
        assert!(tracker.check_met(&ctx, 3)); // iteration 3 > 2
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

    /// C1 regression: an unknown aggregation method must return 1.0 (not
    /// converged), NOT 0.0. The prior `_ => 0.0` arm produced a below-threshold
    /// value, so a typo'd `aggregation` (e.g. "all_converg") silently satisfied
    /// `quality <= threshold` and falsely converged on the first check.
    #[test]
    fn compute_compound_quality_unknown_method_fails_safe_not_converged() {
        let cfg = config(0.5, "composite", 3, 0);
        let tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        // A converged sub-report so sources would resolve if the method were
        // recognized — but the method is a typo, so the result must be 1.0.
        ctx.insert(
            "step_1_result".to_string(),
            json!({"_convergence": {"status": "converged", "quality_at_exit": 0.0}}),
        );
        let sources = vec![AggregationSource {
            step_ordinal: 1,
            field: "_convergence.status".to_string(),
            weight: 1.0,
        }];
        let q = tracker.compute_compound_quality(&ctx, "all_converg", &sources);
        assert_eq!(
            q, 1.0,
            "unknown aggregation method must fail-safe to 1.0 (not converged), got {q}"
        );
    }

    /// C2 regression: `capture_baseline` must use the same field → composite →
    /// _convergence_score fallback chain as `check_legacy_met`. Before the
    /// fix, when the convergence field was reachable only via the composite
    /// fallback, `baseline_quality` stayed `None` and the improvement gate
    /// (`both`) was silently disabled — convergence could never fire even
    /// though `check_legacy_met` saw a below-threshold value with improvement.
    #[test]
    fn capture_baseline_uses_composite_fallback_so_improvement_gate_can_fire() {
        let mut cfg = config(0.15, "score", 3, 0); // field "score" is absent
        cfg.improvement_ratio = 0.25;
        cfg.improvement_gate = "both".to_string();
        let mut tracker = ConvergenceTracker::new(&cfg);
        let mut ctx = HashMap::new();
        // "score" is missing; "composite" is the only reachable quality.
        ctx.insert("composite".to_string(), json!(0.50)); // baseline = 0.50
        tracker.capture_baseline(&ctx);
        assert_eq!(
            tracker.baseline_quality,
            Some(0.50),
            "capture_baseline must fall back to composite when the field is absent"
        );
        // Improve to 0.10 — improvement = (0.50 - 0.10) / 0.50 = 0.80 >= 0.25,
        // and 0.10 <= 0.15 threshold → both gate must fire (previously it
        // could not, because baseline stayed None).
        ctx.insert("composite".to_string(), json!(0.10));
        assert!(
            tracker.check_met(&ctx, 1),
            "improvement gate (both) must fire when baseline is captured via fallback"
        );
    }
}
