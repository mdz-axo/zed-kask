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
/// Supports two exit rails: absolute quality threshold AND/OR improvement from baseline.
/// The improvement kata measures progress from the starting condition toward the target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConvergenceConfig {
    /// Absolute quality threshold. If quality_at_exit <= threshold, the condition is met.
    pub threshold: f64,
    /// Minimum proportional improvement from baseline. E.g., 0.25 means
    /// (baseline - current) / baseline >= 0.25. Set to 0.0 to disable.
    #[serde(default)]
    pub improvement_ratio: f64,
    /// How the threshold and improvement conditions combine:
    /// - "threshold_only" (default): only check quality <= threshold.
    /// - "both": must satisfy quality <= threshold AND improvement >= improvement_ratio.
    /// - "either": must satisfy quality <= threshold OR improvement >= improvement_ratio.
    #[serde(default = "default_improvement_gate")]
    pub improvement_gate: String,
    /// Maximum PDCA iterations before forced exit.
    pub max_iterations: u32,
    /// Minimum iterations before exit is allowed. Prevents premature convergence
    /// before the improvement kata has had time to work. Default 0 (no minimum).
    #[serde(default)]
    pub min_iterations: u32,
    /// Context field to read for quality measurement (e.g., "composite").
    pub convergence_field: String,
    /// Action when convergence not reached after max_iterations: "abort" | "escalate".
    pub on_not_reached: String,
    /// Aggregation method for compound skills (nested PDCA loops).
    /// - "none" (default): single-field check against convergence_field.
    /// - "min": the worst (highest) quality score across sources.
    /// - "weighted_avg": weighted average of source quality scores.
    /// - "all_converged": every source step must have _convergence.status == "converged".
    #[serde(default = "default_aggregation")]
    pub aggregation: String,
    /// Sources for compound aggregation. Each source specifies a step ordinal and
    /// a dot-path field within that step's result (e.g. "_convergence.quality_at_exit").
    #[serde(default)]
    pub aggregation_sources: Vec<AggregationSource>,
}

impl Default for ConvergenceConfig {
    fn default() -> Self {
        Self {
            threshold: 0.1,
            improvement_ratio: 0.0,
            improvement_gate: "threshold_only".to_string(),
            max_iterations: 3,
            min_iterations: 0,
            convergence_field: "composite".to_string(),
            on_not_reached: "abort".to_string(),
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

/// Error handling configuration. Loaded from manifest YAML, future wiring target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorHandlingConfig {
    pub on_gas_exceeded: String,
    pub on_timeout: String,
    pub max_retries: u32,
    pub retry_backoff_seconds: u32,
    pub on_validation_failure: String,
}
impl Default for ErrorHandlingConfig {
    fn default() -> Self {
        Self {
            on_gas_exceeded: "abort".into(),
            on_timeout: "retry".into(),
            max_retries: 2,
            retry_backoff_seconds: 1,
            on_validation_failure: "abort".into(),
        }
    }
}

/// OCAP configuration. Loaded from manifest YAML, future wiring target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OcapConfig {
    pub delegation_chain_required: bool,
    pub signature_algorithm: String,
    pub capability_expiry_seconds: u32,
    pub template_scoped: bool,
}
impl Default for OcapConfig {
    fn default() -> Self {
        Self {
            delegation_chain_required: true,
            signature_algorithm: "ed25519".into(),
            capability_expiry_seconds: 3600,
            template_scoped: true,
        }
    }
}

/// Regulation monitoring configuration. Loaded from manifest YAML, spans handled by GovernedTool at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BundleLedgerConfig {
    pub emit_spans: bool,
    pub span_namespace: String,
    pub variety_monitoring: bool,
    pub algedonic_threshold: u32,
    pub escalation_target: String,
}
impl Default for BundleLedgerConfig {
    fn default() -> Self {
        Self {
            emit_spans: true,
            span_namespace: String::new(),
            variety_monitoring: true,
            algedonic_threshold: 100,
            escalation_target: "Curator".into(),
        }
    }
}

/// Audit trail configuration. Loaded from manifest YAML, future wiring target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
