//! Bundle configuration sub-structs — mirror existing manifest YAML fields
//!
//! These config structs are loaded from manifest YAML. Wired into ManifestExecutor
//! for PDCA convergence, gas enforcement, and error handling.

use serde::{Deserialize, Serialize};

/// System constant: 250,000 compute gas cycles = 1 rJoule of inference energy.
/// This reflects the cost differential between local compute and LLM inference.
pub const RJOULE_TO_GAS: u64 = 250_000;

/// Convergence configuration for PDCA loop exit conditions.
///
/// The Improvement Kata model: the agent has a **target condition** (a
/// measurable state it's trying to reach) and a **current condition** (its
/// measured state right now). Convergence is the gap between them closing.
///
/// The gap lives in two orthogonal spaces, forming a right triangle:
///
/// ```text
///         target
///        /|
///       / |
///      /  | process_gap (PKO — procedure progress)
///     /   |
///    /____|
///  current  object_gap (Dublin Core — artifact completeness)
/// ```
///
/// The hypotenuse `sqrt(object_gap² + process_gap²)` is the total distance
/// to the target. Convergence requires both legs to close — you can't reach
/// the target by producing complete artifacts without testing them, or by
/// running experiments without synthesizing them into artifacts.
///
/// Each PDCA cycle produces a **prediction** ("the hypotenuse will decrease
/// by Δ") with a **confidence**. After the experiment, the actual decrease is
/// measured. The **Brier score** `(confidence − actual_outcome)²` tracks
/// whether the agent's predictions are calibrated — whether it's learning to
/// predict the effects of its own interventions. Brier decreasing → the
/// agent's model of itself is improving. Brier stable and low → confidence
/// convergence (the agent is calibrated, even if the gap hasn't fully closed).
///
/// This replaces the old self-grade model where an LLM graded its own plan
/// quality on a [0,1] scale. That was a category error: it measured plan
/// quality (a snapshot) instead of gap closure (a trajectory), and it used
/// the LLM for the deterministic convergence decision (causing the 30s
/// timeouts). The Kata model uses the LLM only for the four Kata steps
/// (grasp current, establish target, predict, experiment); the executor
/// computes the gap and Brier score deterministically.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConvergenceConfig {
    // ── Kata target-condition fields ──
    /// Context field holding the target artifact spec (Dublin Core object
    /// space). The target condition for artifact completeness — which fields
    /// should be populated, which should be grounded. Produced by an early
    /// `select` step or provided as a manifest input.
    #[serde(default)]
    pub target_artifacts_field: Option<String>,
    /// Context field holding the measured current artifact state (Dublin Core
    /// object space). Re-measured after each PDCA cycle because the experiment
    /// changed the system.
    #[serde(default)]
    pub current_artifacts_field: Option<String>,
    /// Context field holding the target procedure spec (PKO process space).
    /// The target condition for procedure progress — which steps must be
    /// complete.
    #[serde(default)]
    pub target_procedure_field: Option<String>,
    /// Context field holding the measured current procedure state (PKO process
    /// space). Re-measured after each PDCA cycle.
    #[serde(default)]
    pub current_procedure_field: Option<String>,
    /// Context field holding the prediction: `{ expected_delta, confidence }`.
    /// The agent predicts the hypotenuse will decrease by `expected_delta`,
    /// with `confidence` in [0,1].
    #[serde(default)]
    pub prediction_field: Option<String>,
    /// Context field holding the actual result after the experiment:
    /// `{ actual_delta }` or `{ occurred: bool }`.
    #[serde(default)]
    pub result_field: Option<String>,

    // ── Convergence thresholds ──
    /// Hypotenuse below this → **gap convergence** (the agent reached the
    /// target condition). This is the limit-of-a-sequence criterion:
    /// `‖xₙ − L‖ < ε` where L is the target and xₙ is the current condition.
    #[serde(default = "default_hypotenuse_epsilon")]
    pub hypotenuse_epsilon: f64,

    /// Epsilon for the **Cauchy convergence** (stall) criterion: the iterates
    /// have stopped moving. A sequence is Cauchy if for all m, n > N,
    /// `‖xₘ − xₙ‖ < ε`. In practice, we check that the maximum pairwise
    /// distance between hypotenuse readings in the last `cauchy_window`
    /// cycles is below this epsilon. This means *all* recent readings are
    /// clustered together — the iterates have genuinely stabilized, not just
    /// locally plateaued.
    ///
    /// This is the canonical mathematical definition of "the process has
    /// stopped producing new information" (learning exhausted, current methods
    /// at their ceiling). It catches oscillation (0.3 → 0.5 → 0.3 → 0.5 has
    /// large pairwise distances → not Cauchy) and plateau (0.3 → 0.31 → 0.3
    /// has small pairwise distances → Cauchy).
    #[serde(default = "default_cauchy_epsilon")]
    pub cauchy_epsilon: f64,
    /// Window size (number of PDCA cycles) for the Cauchy convergence check.
    /// The maximum pairwise distance between hypotenuse readings in the last
    /// `cauchy_window` cycles must be below `cauchy_epsilon`.
    #[serde(default = "default_cauchy_window")]
    pub cauchy_window: u32,

    /// Number of PDCA cycles to compute the rolling Brier average over for
    /// **calibration convergence**: the agent's predictions are calibrated —
    /// it knows what will happen when it acts.
    #[serde(default = "default_brier_window")]
    pub brier_window: u32,
    /// Rolling Brier average below this → calibration converged.
    #[serde(default = "default_brier_threshold")]
    pub brier_threshold: f64,

    /// Convergence mode — selects which stop conditions are active. Any active
    /// condition that fires triggers convergence.
    ///
    /// - `"gap"`: gap convergence only (hypotenuse < epsilon).
    /// - `"cauchy"`: Cauchy convergence only (iterates stabilized).
    /// - `"calibration"`: calibration convergence only (Brier calibrated).
    /// - `"gap_or_cauchy"`: gap or Cauchy (no Brier).
    /// - `"gap_or_cauchy_or_calibration"` (default): any of the three.
    ///
    /// The three signals are orthogonal: gap measures distance to target,
    /// Cauchy measures stability of iterates, Brier measures prediction
    /// quality. They can be combined with OR. The default is all three because
    /// the Kata literature recognizes all three as valid stop conditions.
    #[serde(default = "default_convergence_mode")]
    pub convergence_mode: String,

    // ── Loop control (retained from the old model) ──
    /// Maximum PDCA iterations before forced exit.
    pub max_iterations: u32,
    /// Minimum iterations before exit is allowed. Prevents premature
    /// convergence before the Kata has had time to run at least one full
    /// experiment cycle. Default 2 (need at least 2 readings for Brier).
    #[serde(default = "default_min_iterations")]
    pub min_iterations: u32,
    /// Action when convergence not reached after max_iterations: "abort" | "escalate".
    pub on_not_reached: String,

    // ── Legacy fields (retained for manifests not yet migrated to the Kata model) ──
    //
    // These support the old self-grade convergence model. New skills should use
    // the Kata fields above instead. The executor supports both; if
    // `convergence_mode` is set, the Kata model is used. If not, the legacy
    // model is used (threshold + improvement_gate).
    /// Legacy: absolute quality threshold for self-grade convergence.
    pub threshold: f64,
    /// Legacy: context field to read for self-grade quality measurement.
    pub convergence_field: String,
    /// Legacy: minimum proportional improvement from baseline.
    #[serde(default)]
    pub improvement_ratio: f64,
    /// Legacy: how the threshold and improvement conditions combine.
    #[serde(default = "default_improvement_gate")]
    pub improvement_gate: String,

    // ── Compound aggregation (retained — used by flowdef composition) ──
    /// Aggregation method for compound skills (nested PDCA loops).
    /// - "none" (default): single-field check.
    /// - "min": the worst (highest) quality score across sources.
    /// - "weighted_avg": weighted average of source quality scores.
    /// - "all_converged": every source step must have _convergence.status == "converged".
    #[serde(default = "default_aggregation")]
    pub aggregation: String,
    /// Sources for compound aggregation.
    #[serde(default)]
    pub aggregation_sources: Vec<AggregationSource>,
}

impl Default for ConvergenceConfig {
    fn default() -> Self {
        Self {
            target_artifacts_field: None,
            current_artifacts_field: None,
            target_procedure_field: None,
            current_procedure_field: None,
            prediction_field: None,
            result_field: None,
            hypotenuse_epsilon: 0.05,
            cauchy_epsilon: 0.03,
            cauchy_window: 3,
            brier_window: 3,
            brier_threshold: 0.15,
            convergence_mode: "gap_or_cauchy_or_calibration".to_string(),
            max_iterations: 10,
            min_iterations: 2,
            on_not_reached: "abort".to_string(),
            // Legacy defaults — used when convergence_mode is empty/unset
            threshold: 0.1,
            convergence_field: "composite".to_string(),
            improvement_ratio: 0.0,
            improvement_gate: "threshold_only".to_string(),
            aggregation: "none".to_string(),
            aggregation_sources: vec![],
        }
    }
}

fn default_aggregation() -> String {
    "none".to_string()
}

fn default_improvement_gate() -> String {
    "threshold_only".to_string()
}

fn default_hypotenuse_epsilon() -> f64 {
    0.05
}

fn default_cauchy_epsilon() -> f64 {
    0.03
}

fn default_cauchy_window() -> u32 {
    3
}

fn default_brier_window() -> u32 {
    3
}

fn default_brier_threshold() -> f64 {
    0.15
}

fn default_convergence_mode() -> String {
    "gap_or_cauchy_or_calibration".to_string()
}

fn default_min_iterations() -> u32 {
    2
}

/// A source for compound quality aggregation — specifies which inner skill's
/// convergence report to read and at what weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationSource {
    pub step_ordinal: u32,
    /// Dot-path within the step result, e.g. "_convergence.quality_at_exit"
    pub field: String,
    #[serde(default = "default_weight")]
    pub weight: f64,
}

fn default_weight() -> f64 {
    1.0
}

/// Gas (compute cycle budget) configuration — caps local loop iterations.
/// Gas is cheap compute. 250,000 gas cycles ≈ 1 rJoule of inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BundleGasConfig {
    /// Total compute gas budget for the cascade.
    pub cap: u32,
    /// Compute gas cost per cascade iteration (loop pass).
    pub cost_per_iteration: u32,
    pub alert_threshold: f64,
    pub hard_limit: bool,
}
impl Default for BundleGasConfig {
    fn default() -> Self {
        Self {
            cap: 100000,
            cost_per_iteration: 100,
            alert_threshold: 0.8,
            hard_limit: true,
        }
    }
}

/// rJoule (inference energy budget) configuration — caps LLM inference cost.
/// Cost per token is set by the inference provider/model, not the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RjouleConfig {
    /// Total rJoule budget for inference in this cascade.
    pub cap: u32,
    pub alert_threshold: f64,
    pub hard_limit: bool,
}
impl Default for RjouleConfig {
    fn default() -> Self {
        Self {
            cap: 0, // 0 = no rJoule budget (backward compat)
            alert_threshold: 0.8,
            hard_limit: true,
        }
    }
}

/// Error handling configuration. Loaded from manifest YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ErrorHandlingConfig {
    pub on_gas_exceeded: String,
    pub on_timeout: String,
    pub max_retries: u32,
    pub retry_backoff_seconds: u32,
    pub on_validation_failure: String,
    /// Policy when an OCAP capability check denies a tool invocation.
    /// Parsed from the manifest but not yet wired into the executor — the
    /// executor currently propagates `TemplateError::CapabilityDenied` via `?`
    /// without consulting this field. 10 manifests declare `escalate`.
    #[serde(default)]
    pub on_capability_denied: String,
}
impl Default for ErrorHandlingConfig {
    fn default() -> Self {
        Self {
            on_gas_exceeded: "abort".into(),
            on_timeout: "retry".into(),
            max_retries: 2,
            retry_backoff_seconds: 1,
            on_validation_failure: "abort".into(),
            on_capability_denied: "escalate".into(),
        }
    }
}

/// Regulation monitoring configuration. Loaded from manifest YAML, spans handled by `McpRuntime::invoke` / `ToolGovernance` (in `hkask-mcp`) at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BundleLedgerConfig {
    pub emit_spans: bool,
    pub span_namespace: String,
    /// Performative telemetry namespace (hkask.template.<skill-id>). Optional —
    /// used for fine-grained execution telemetry that is NOT regulated. Distinct
    /// from span_namespace (reg.skill.<skill-id>) which IS regulated.
    #[serde(default)]
    pub telemetry_namespace: Option<String>,
    pub variety_monitoring: bool,
    pub algedonic_threshold: u32,
    pub escalation_target: String,
}
impl Default for BundleLedgerConfig {
    fn default() -> Self {
        Self {
            emit_spans: true,
            span_namespace: String::new(),
            telemetry_namespace: None,
            variety_monitoring: true,
            algedonic_threshold: 100,
            escalation_target: "Curator".into(),
        }
    }
}

/// Audit trail configuration. Loaded from manifest YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BundleAuditConfig {
    pub enabled: bool,
    pub log_level: String,
    pub include_input: bool,
    pub include_output: bool,
    pub include_gas_cost: bool,
    pub include_reg_events: bool,
}
impl Default for BundleAuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_level: "info".into(),
            include_input: true,
            include_output: true,
            include_gas_cost: true,
            include_reg_events: true,
        }
    }
}
