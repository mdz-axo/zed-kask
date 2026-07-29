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

/// Tracks PDCA convergence state: threshold, improvement gate, baseline quality,
/// iteration count, and trajectory history. Owns the `_convergence` JSON shape
/// contract.
///
/// # Trajectory memory
///
/// The tracker records the quality metric after each completed iteration in
/// `quality_history`. This enables trajectory-based convergence detection
/// (the "stability" and "threshold_and_stability" gates): convergence is a
/// property of a *trajectory* (the metric stopped changing), not of a
/// *snapshot* (the metric is low on one reading). Snapshot convergence is a
/// category error — it exits on a single optimistic self-grade. Trajectory
/// convergence requires at least 2 readings and enforces that the metric is
/// stable, not just low.
pub struct ConvergenceTracker {
    threshold: f64,
    field: String,
    improvement_ratio: f64,
    improvement_gate: String,
    stability_epsilon: f64,
    min_iterations: u32,
    max_iterations: u32,
    baseline_quality: Option<f64>,
    /// Quality metric history, one entry per completed iteration. Populated by
    /// `push_quality` (called from the executor after each iteration's metric
    /// is computed). Used by the "stability" and "threshold_and_stability"
    /// gates to detect trajectory convergence.
    quality_history: Vec<f64>,
}

impl ConvergenceTracker {
    /// Construct from a manifest's convergence config.
    pub fn new(config: &ConvergenceConfig) -> Self {
        Self {
            threshold: config.threshold,
            field: config.convergence_field.clone(),
            improvement_ratio: config.improvement_ratio,
            improvement_gate: config.improvement_gate.clone(),
            stability_epsilon: config.stability_epsilon,
            min_iterations: config.min_iterations,
            max_iterations: if config.max_iterations == 0 {
                1
            } else {
                config.max_iterations
            },
            baseline_quality: None,
            quality_history: Vec::new(),
        }
    }

    /// The configured threshold.
    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    /// The configured convergence field (e.g. "composite").
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The max iterations (1 for single-pass manifests).
    pub fn max_iterations(&self) -> u32 {
        self.max_iterations
    }

    /// Whether improvement tracking is enabled (improvement_ratio > 0).
    pub fn improvement_enabled(&self) -> bool {
        self.improvement_ratio > 0.0
    }

    /// Whether the convergence gate uses trajectory stability (requires >= 2
    /// quality readings). Returns true for "stability" and
    /// "threshold_and_stability" gates.
    pub fn stability_enabled(&self) -> bool {
        matches!(
            self.improvement_gate.as_str(),
            "stability" | "threshold_and_stability"
        )
    }

    /// Record the quality metric for the just-completed iteration. Called by
    /// the executor after each pass's convergence metric is computed (whether
    /// by an LLM `select` step or a deterministic `compute` step). The history
    /// is read by the "stability" and "threshold_and_stability" gates in
    /// `check_met`.
    ///
    /// If the metric is missing from the context, pushes `f64::NAN` so the
    /// history length stays aligned with the iteration count (a missing
    /// reading is not a stable reading).
    pub fn push_quality(&mut self, context: &HashMap<String, Value>) {
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

    /// Read-only access to the quality history (for `finalize_report` and
    /// `inject_running` context injection, and for tests).
    pub fn quality_history(&self) -> &[f64] {
        &self.quality_history
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

    /// Check whether the convergence threshold has been met.
    ///
    /// Enforces `min_iterations` (returns false if iteration <= min_iterations),
    /// then evaluates the threshold and improvement gate.
    ///
    /// For the "stability" and "threshold_and_stability" gates, also enforces
    /// that at least 2 quality readings exist in `quality_history` (trajectory
    /// convergence is undefined for a single reading) and that the last two
    /// readings differ by less than `stability_epsilon`.
    pub fn check_met(&self, context: &HashMap<String, Value>, iteration: u32) -> bool {
        // Enforce minimum iterations before exit is allowed
        if iteration <= self.min_iterations {
            return false;
        }

        // Compute current quality via the 3-level fallback chain:
        // 1. The configured field (direct or dot-path)
        // 2. "composite" (if the configured field isn't "composite")
        // 3. "_convergence_score" (last-resort metadata fallback)
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

        // Compute improvement from baseline as proportional ratio
        let improvement_met = if self.improvement_ratio > 0.0 {
            match (self.baseline_quality, current) {
                (Some(b), Some(c)) if b > 0.0 => ((b - c) / b) >= self.improvement_ratio,
                _ => false,
            }
        } else {
            false
        };

        // Compute trajectory stability: |q_n - q_{n-1}| < epsilon.
        // Requires at least 2 readings. A missing reading (NaN) is never stable.
        let stability_met = if self.quality_history.len() >= 2 {
            let n = self.quality_history.len();
            let prev = self.quality_history[n - 2];
            let curr = self.quality_history[n - 1];
            prev.is_finite() && curr.is_finite() && (curr - prev).abs() < self.stability_epsilon
        } else {
            false
        };

        match self.improvement_gate.as_str() {
            "both" => threshold_met && improvement_met,
            "either" => threshold_met || improvement_met,
            "stability" => stability_met,
            "threshold_and_stability" => threshold_met && stability_met,
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
            threshold,
            improvement_ratio: 0.0,
            improvement_gate: "threshold_only".to_string(),
            max_iterations: max_iter,
            min_iterations: min_iter,
            convergence_field: field.to_string(),
            on_not_reached: "abort".to_string(),
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
