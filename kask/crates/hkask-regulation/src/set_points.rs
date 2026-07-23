//! Set-points and configuration for the Cybernetics Loop.
//!
//! Homeostatic set-points define the reference values against which sensed
//! signals are compared. When a signal deviates beyond its set-point,
//! the loop produces an efferent action.

use hkask_types::regulation::QueueDepth;

/// Default minimum energy budget remaining ratio (20%).
///
/// When gas remaining drops below this ratio, the Cybernetics Loop produces
/// a throttle action to reduce consumption.
pub const DEFAULT_ENERGY_MIN_REMAINING_RATIO: f64 = 0.2;

/// Default maximum variety deficit before escalation (100).
///
/// When variety deficit exceeds this value, an algedonic alert is triggered.
pub const DEFAULT_VARIETY_MAX_DEFICIT: f64 = 100.0;

/// Default maximum error rate (30%).
///
/// When the error rate exceeds this ratio, the Cybernetics Loop produces
/// a calibration action.
pub const DEFAULT_ERROR_RATE_MAX: f64 = 0.3;

/// Default maximum connector latency in seconds.
///
/// When connector latency exceeds this threshold, the Cybernetics Loop
/// produces a throttle action.
pub const DEFAULT_CONNECTOR_LATENCY_MAX_SECS: f64 = 30.0;

/// Default communication queue depth threshold for backpressure regulation.
///
/// When the Communication Loop's queue depth exceeds this value,
/// the Cybernetics Loop produces a Throttle(Communication) action.
pub const DEFAULT_COMMUNICATION_BACKPRESSURE_THRESHOLD: QueueDepth =
    QueueDepth::DEFAULT_BACKPRESSURE;

/// Default minimum seam coverage ratio before alert.
///
/// When per-crate coverage drops below its previous snapshot value,
/// Fires an algedonic alert. Default: 0.0 (alert on ANY regression —
/// \[NORMATIVE\] coverage should never go down). (P9 — Homeostatic Self-Regulation).
pub const DEFAULT_SEAM_COVERAGE_MIN: f64 = 0.0;

/// Default maximum number of regulation iterations per cycle.
///
/// Prevents unbounded cascading in the compute→act pipeline.
pub const DEFAULT_MAX_ITERATIONS: u32 = 100;

/// Inference throttle consent mode.
///
/// Controls how the Cybernetics Loop handles low energy budget:
/// - `Off`: No throttle. Regulation logs the event; user manages budget manually.
/// - `Autonomous`: Direct throttle to Inference loop (current behavior).
/// - `CuratorMediated`: Escalate to Curator with budget options.
///   If user doesn't respond within the timeout, apply gentle throttle as fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceThrottleMode {
    /// No automatic throttle. User manages budget manually.
    Off,
    /// Throttle directly — pre-authorized by user (P2 consent via config).
    Autonomous,
    /// Escalate to Curator. Fallback: gentle throttle after timeout.
    CuratorMediated { curator_timeout_secs: u64 },
}

/// Default dampener window in seconds (60s).
///
/// Within this window, repeated identical directives are suppressed.
pub const DEFAULT_DAMPEN_WINDOW_SECS: u64 = 60;

/// Default metacognitive dampener window in seconds (300s).
///
/// Metacognitive overrides are dampened at a longer window.
pub const DEFAULT_METACOGNITIVE_WINDOW_SECS: u64 = 300;

/// Default override cooldown in seconds (120s).
///
/// After any metacognitive override passes dedup, ALL subsequent overrides
/// are suppressed for this duration.
pub const DEFAULT_OVERRIDE_COOLDOWN_SECS: u64 = 120;

/// Default outcome warning threshold (0.50 = 50% success rate).
///
/// When outcome success rate drops below this, a warning alert is emitted.
pub const DEFAULT_OUTCOME_WARNING_THRESHOLD: f64 = 0.50;

/// Default outcome critical threshold (0.25 = 25% success rate).
///
/// When outcome success rate drops below this, a critical alert is emitted.
pub const DEFAULT_OUTCOME_CRITICAL_THRESHOLD: f64 = 0.25;

/// Default guard violation rate maximum (0.20 = 20% of requests blocked).
///
/// When content safety guard violations exceed this rate over a monitoring
/// window, the Curator escalates. Per OWASP LLM Top 10: sustained high
/// violation rates indicate either an active attack (LLM01) or a
/// misconfigured system producing secrets (LLM02/LLM06).
/// Default guard violation rate maximum (0.20 = 20% of requests).
///
/// When content safety guard violations exceed this rate, the Curator
/// escalates. Configurable via SetPointsConfig YAML or
/// `HKASK_GUARD_VIOLATION_RATE_MAX` env var. OWASP LLM01/LLM02/LLM06.
pub const DEFAULT_GUARD_VIOLATION_RATE_MAX: f64 = 0.20;

/// Default stagnation detection threshold (5 cycles).
///
/// After this many consecutive cycles of the same ineffective (metric, action)
/// pair, a `RegulatoryPlateau` alert is triggered.
pub const DEFAULT_STAGNATION_THRESHOLD: u32 = 5;

/// Default stage threshold for ActionDecision: 5% relative worsening.
///
/// When an action worsens its target metric by less than this ratio,
/// it's accepted as noise. Between this and `DEFAULT_BLOCK_WORSENING_RATIO`,
/// it's staged for review.
pub const DEFAULT_STAGE_WORSENING_RATIO: f64 = 0.05;

/// Default block threshold for ActionDecision: 20% relative worsening.
///
/// When an action worsens its target metric by this ratio or more,
/// the (metric, action_type) pair is blocked until Curation intervenes.
pub const DEFAULT_BLOCK_WORSENING_RATIO: f64 = 0.20;

/// Default substitution activation threshold: try alternatives after this
/// many consecutive ineffective cycles (default: 2 — half the stagnation
/// default of 5). When a (metric, action_type) pair hits this count,
/// `compute()` tries the next action in the substitution ladder.
pub const DEFAULT_SUBSTITUTION_AFTER: u32 = 2;

/// Homeostatic set-points for the Cybernetics Loop.
///
/// These define the reference values against which sensed signals
/// are compared. When a signal deviates beyond its set-point,
/// the loop produces an efferent action.
#[derive(Debug, Clone)]
pub struct SetPoints {
    /// Minimum energy budget remaining ratio (0.0-1.0). Default: 0.2 (20% remaining)
    pub gas_min_remaining: f64,
    /// Maximum variety deficit before escalation. Default: 100
    pub variety_max_deficit: f64,
    /// Maximum error rate (0.0-1.0). Default: 0.3 (30% errors)
    pub error_rate_max: f64,
    /// Maximum connector latency in seconds. Default: 30.0
    pub connector_latency_max_secs: f64,
    /// Communication queue depth threshold for backpressure regulation.
    /// When the Communication Loop's queue depth exceeds this value,
    /// CyberneticsLoop produces a Throttle(Communication) action.
    /// Default: 100 messages
    pub communication_backpressure_threshold: QueueDepth,
    /// Minimum seam coverage ratio per crate before seam alert.
    /// When per-crate coverage drops below its previous snapshot,
    /// an algedonic alert fires. Default: 0.0 (any regression alerts).
    pub seam_coverage_min: f64,
    pub fed_sync_latency_warning_ms: u64,
    pub fed_sync_latency_critical_ms: u64,
    pub fed_link_downtime_warning_secs: u64,
    pub fed_link_downtime_critical_secs: u64,
    /// Maximum pause duration before Regulation escalation (hours). Default: 24.
    pub fed_max_pause_duration_hours: u64,
    /// Invitation rate warning threshold (invites/hour). Default: 5.
    pub fed_invitation_rate_warning_per_hour: u64,
    /// Registry divergence warning threshold (entries/sync). Default: 10.
    pub fed_registry_divergence_warning: u64,
    // ── Dampener configuration (v0.30.0) ──
    /// Dampener window for routine directives (seconds). Default: 60.
    pub dampen_window_secs: u64,
    /// Dampener window for metacognitive overrides (seconds). Default: 300.
    pub metacognitive_window_secs: u64,
    /// Override cooldown window after any metacognitive override (seconds). Default: 120.
    pub override_cooldown_secs: u64,
    // ── Outcome thresholds (v0.30.0) ──
    /// Outcome success rate warning threshold. Default: 0.50.
    pub outcome_warning_threshold: f64,
    /// Outcome success rate critical threshold. Default: 0.25.
    pub outcome_critical_threshold: f64,
    // ── Guard thresholds (v0.31.0) ──
    /// Maximum guard violation rate before algedonic alert (0.0-1.0).
    /// When the fraction of requests blocked by content safety guard exceeds
    /// this, the Curator escalates. Default: 0.20 (20% of requests blocked).
    /// Set higher for development, lower for production.
    pub guard_violation_rate_max: f64,
    // ── Loop regulation (v0.30.0) ──
    /// Maximum regulation iterations per cycle. Default: 100.
    pub max_iterations: u32,
    // ── Stagnation detection (v0.31.0, Fermi pattern) ──
    /// Per-metric stagnation thresholds. Key: metric name (snake_case),
    /// value: cycles before RegulatoryPlateau alert. Unlisted metrics
    /// use `DEFAULT_STAGNATION_THRESHOLD` (5).
    pub stagnation_thresholds: std::collections::HashMap<String, u32>,
    /// Action decision stage threshold: max relative worsening before
    /// an action is staged for review (0.0–1.0). Default: 0.05.
    pub stage_worsening_ratio: f64,
    /// Action decision block threshold: min relative worsening to
    /// hard-block an action (0.0–1.0). Default: 0.20.
    pub block_worsening_ratio: f64,
    /// Action substitution ladders. Key: metric name (snake_case),
    /// value: ordered list of action type names to try when the
    /// primary action is ineffective (Fermi model-variant pattern).
    /// Default: empty (no substitution; escalate on plateau).
    pub action_substitutions: std::collections::HashMap<String, Vec<String>>,
    /// Cycles of ineffectiveness before substitution activates.
    /// Default: 2 (half the stagnation threshold so substitution
    /// happens before plateau escalation).
    pub substitution_after: u32,
    // ── Inference throttle consent mode (v0.31.0) ──
    /// How inference throttling decisions are made when energy budget runs low.
    /// Default: Off (user manages budget manually).
    /// Autonomous: pre-authorized by user (P2 consent via config).
    /// CuratorMediated: escalate to Curator with fallback after timeout.
    pub inference_throttle_mode: InferenceThrottleMode,
}

/// Configurable thresholds for Curation decisions (spec coherence, drift).
/// Loaded from YAML via `HKASK_REG_CONFIG` (same pattern as `SetPointsConfig`).
///
/// Type definition lives in `hkask_types::curator`; YAML loading lives here.
pub use hkask_types::curator::CurationThresholdConfig;

/// YAML-configurable set-points. Fields are Optional so partial configs work.
/// Missing fields fall back to the `SetPoints::default()` values.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct SetPointsConfig {
    pub gas_min_remaining: Option<f64>,
    pub variety_max_deficit: Option<f64>,
    pub error_rate_max: Option<f64>,
    pub connector_latency_max_secs: Option<f64>,
    pub communication_backpressure_threshold: Option<QueueDepth>,
    pub seam_coverage_min: Option<f64>,
    pub fed_sync_latency_warning_ms: Option<u64>,
    pub fed_sync_latency_critical_ms: Option<u64>,
    pub fed_link_downtime_warning_secs: Option<u64>,
    pub fed_link_downtime_critical_secs: Option<u64>,
    pub fed_max_pause_duration_hours: Option<u64>,
    pub fed_invitation_rate_warning_per_hour: Option<u64>,
    pub fed_registry_divergence_warning: Option<u64>,
    pub dampen_window_secs: Option<u64>,
    pub metacognitive_window_secs: Option<u64>,
    pub override_cooldown_secs: Option<u64>,
    pub outcome_warning_threshold: Option<f64>,
    pub outcome_critical_threshold: Option<f64>,
    pub guard_violation_rate_max: Option<f64>,
    pub max_iterations: Option<u32>,
    pub stagnation_thresholds: Option<std::collections::HashMap<String, u32>>,
    pub stage_worsening_ratio: Option<f64>,
    pub block_worsening_ratio: Option<f64>,
    pub action_substitutions: Option<std::collections::HashMap<String, Vec<String>>>,
    pub substitution_after: Option<u32>,
    pub inference_throttle_mode: Option<InferenceThrottleMode>,
}

impl SetPointsConfig {
    /// expect: "The system provides configurable regulation thresholds for the cybernetic control loop"
    /// Load set-points from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml_neo::Error> {
        serde_yaml_neo::from_str(yaml)
    }

    /// expect: "The system provides configurable regulation thresholds for the cybernetic control loop"
    /// Load set-points from a YAML file.
    pub fn load_from_file(path: &str) -> Result<Self, std::io::Error> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_yaml(&contents)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

impl Default for SetPoints {
    fn default() -> Self {
        Self {
            gas_min_remaining: DEFAULT_ENERGY_MIN_REMAINING_RATIO,
            variety_max_deficit: DEFAULT_VARIETY_MAX_DEFICIT,
            error_rate_max: DEFAULT_ERROR_RATE_MAX,
            connector_latency_max_secs: DEFAULT_CONNECTOR_LATENCY_MAX_SECS,
            communication_backpressure_threshold: DEFAULT_COMMUNICATION_BACKPRESSURE_THRESHOLD,
            seam_coverage_min: DEFAULT_SEAM_COVERAGE_MIN,
            fed_sync_latency_warning_ms: 5000,
            fed_sync_latency_critical_ms: 30000,
            fed_link_downtime_warning_secs: 3600,
            fed_link_downtime_critical_secs: 86400,
            fed_max_pause_duration_hours: 24,
            fed_invitation_rate_warning_per_hour: 5,
            fed_registry_divergence_warning: 10,
            dampen_window_secs: DEFAULT_DAMPEN_WINDOW_SECS,
            metacognitive_window_secs: DEFAULT_METACOGNITIVE_WINDOW_SECS,
            override_cooldown_secs: DEFAULT_OVERRIDE_COOLDOWN_SECS,
            outcome_warning_threshold: DEFAULT_OUTCOME_WARNING_THRESHOLD,
            outcome_critical_threshold: DEFAULT_OUTCOME_CRITICAL_THRESHOLD,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            guard_violation_rate_max: DEFAULT_GUARD_VIOLATION_RATE_MAX,
            stagnation_thresholds: std::collections::HashMap::new(),
            stage_worsening_ratio: DEFAULT_STAGE_WORSENING_RATIO,
            block_worsening_ratio: DEFAULT_BLOCK_WORSENING_RATIO,
            action_substitutions: std::collections::HashMap::new(),
            substitution_after: DEFAULT_SUBSTITUTION_AFTER,
            inference_throttle_mode: InferenceThrottleMode::Off,
        }
    }
}

impl SetPoints {
    /// expect: "The system provides configurable regulation thresholds for the cybernetic control loop"
    /// Create SetPoints from a config, using defaults for missing fields.
    pub fn from_config(config: &SetPointsConfig) -> Self {
        let defaults = SetPoints::default();
        Self {
            gas_min_remaining: config
                .gas_min_remaining
                .unwrap_or(defaults.gas_min_remaining),
            variety_max_deficit: config
                .variety_max_deficit
                .unwrap_or(defaults.variety_max_deficit),
            error_rate_max: config.error_rate_max.unwrap_or(defaults.error_rate_max),
            connector_latency_max_secs: config
                .connector_latency_max_secs
                .unwrap_or(defaults.connector_latency_max_secs),
            communication_backpressure_threshold: config
                .communication_backpressure_threshold
                .unwrap_or(defaults.communication_backpressure_threshold),
            seam_coverage_min: config
                .seam_coverage_min
                .unwrap_or(defaults.seam_coverage_min),
            fed_sync_latency_warning_ms: config
                .fed_sync_latency_warning_ms
                .unwrap_or(defaults.fed_sync_latency_warning_ms),
            fed_sync_latency_critical_ms: config
                .fed_sync_latency_critical_ms
                .unwrap_or(defaults.fed_sync_latency_critical_ms),
            fed_link_downtime_warning_secs: config
                .fed_link_downtime_warning_secs
                .unwrap_or(defaults.fed_link_downtime_warning_secs),
            fed_link_downtime_critical_secs: config
                .fed_link_downtime_critical_secs
                .unwrap_or(defaults.fed_link_downtime_critical_secs),
            fed_max_pause_duration_hours: config
                .fed_max_pause_duration_hours
                .unwrap_or(defaults.fed_max_pause_duration_hours),
            fed_invitation_rate_warning_per_hour: config
                .fed_invitation_rate_warning_per_hour
                .unwrap_or(defaults.fed_invitation_rate_warning_per_hour),
            fed_registry_divergence_warning: config
                .fed_registry_divergence_warning
                .unwrap_or(defaults.fed_registry_divergence_warning),
            dampen_window_secs: config
                .dampen_window_secs
                .unwrap_or(defaults.dampen_window_secs),
            metacognitive_window_secs: config
                .metacognitive_window_secs
                .unwrap_or(defaults.metacognitive_window_secs),
            override_cooldown_secs: config
                .override_cooldown_secs
                .unwrap_or(defaults.override_cooldown_secs),
            outcome_warning_threshold: config
                .outcome_warning_threshold
                .unwrap_or(defaults.outcome_warning_threshold),
            outcome_critical_threshold: config
                .outcome_critical_threshold
                .unwrap_or(defaults.outcome_critical_threshold),
            max_iterations: config.max_iterations.unwrap_or(defaults.max_iterations),
            guard_violation_rate_max: config
                .guard_violation_rate_max
                .unwrap_or(defaults.guard_violation_rate_max),
            stagnation_thresholds: config
                .stagnation_thresholds
                .clone()
                .unwrap_or(defaults.stagnation_thresholds),
            stage_worsening_ratio: config
                .stage_worsening_ratio
                .unwrap_or(defaults.stage_worsening_ratio),
            block_worsening_ratio: config
                .block_worsening_ratio
                .unwrap_or(defaults.block_worsening_ratio),
            action_substitutions: config
                .action_substitutions
                .clone()
                .unwrap_or(defaults.action_substitutions),
            substitution_after: config
                .substitution_after
                .unwrap_or(defaults.substitution_after),
            inference_throttle_mode: config
                .inference_throttle_mode
                .unwrap_or(defaults.inference_throttle_mode),
        }
    }

    /// expect: "The system provides configurable regulation thresholds for the cybernetic control loop"
    /// Validate set-point invariants.
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("gas_min_remaining", self.gas_min_remaining),
            ("error_rate_max", self.error_rate_max),
            ("seam_coverage_min", self.seam_coverage_min),
            ("guard_violation_rate_max", self.guard_violation_rate_max),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(anyhow::anyhow!("{name} must be in [0.0, 1.0], got {value}"));
            }
        }
        if self.outcome_warning_threshold <= self.outcome_critical_threshold {
            return Err(anyhow::anyhow!(
                "outcome_warning_threshold ({}) must be > outcome_critical_threshold ({})",
                self.outcome_warning_threshold,
                self.outcome_critical_threshold
            ));
        }
        if self.fed_sync_latency_warning_ms >= self.fed_sync_latency_critical_ms {
            return Err(anyhow::anyhow!(
                "fed_sync_latency_warning_ms ({}) must be < fed_sync_latency_critical_ms ({})",
                self.fed_sync_latency_warning_ms,
                self.fed_sync_latency_critical_ms
            ));
        }
        if self.fed_link_downtime_warning_secs >= self.fed_link_downtime_critical_secs {
            return Err(anyhow::anyhow!(
                "fed_link_downtime_warning_secs ({}) must be < fed_link_downtime_critical_secs ({})",
                self.fed_link_downtime_warning_secs,
                self.fed_link_downtime_critical_secs
            ));
        }
        if self.variety_max_deficit <= 0.0 {
            return Err(anyhow::anyhow!(
                "variety_max_deficit must be > 0, got {}",
                self.variety_max_deficit
            ));
        }
        if self.connector_latency_max_secs <= 0.0 {
            return Err(anyhow::anyhow!(
                "connector_latency_max_secs must be > 0, got {}",
                self.connector_latency_max_secs
            ));
        }
        if self.max_iterations == 0 {
            return Err(anyhow::anyhow!("max_iterations must be > 0"));
        }
        if self.stage_worsening_ratio >= self.block_worsening_ratio {
            return Err(anyhow::anyhow!(
                "stage_worsening_ratio ({}) must be < block_worsening_ratio ({})",
                self.stage_worsening_ratio,
                self.block_worsening_ratio
            ));
        }
        if self.substitution_after == 0 {
            return Err(anyhow::anyhow!("substitution_after must be > 0"));
        }
        if self.dampen_window_secs == 0 {
            return Err(anyhow::anyhow!("dampen_window_secs must be > 0"));
        }
        Ok(())
    }
}

/// expect: "The system provides configurable regulation thresholds for the cybernetic control loop"
/// Load set-points from `HKASK_REG_CONFIG` env var, falling back to defaults.
///
/// If `HKASK_REG_CONFIG` is set, reads the YAML file at that path.
/// If unset or the file doesn't exist, returns default set-points.
#[must_use]
pub fn load_set_points() -> SetPoints {
    match std::env::var("HKASK_REG_CONFIG") {
        Ok(path) => match SetPointsConfig::load_from_file(&path) {
            Ok(config) => {
                let points = SetPoints::from_config(&config);
                if let Err(e) = points.validate() {
                    tracing::warn!(
                        target: "reg.config()",
                        path = %path,
                        error = %e,
                        "Loaded Regulation set-points failed validation — falling back to defaults"
                    );
                    return SetPoints::default();
                }
                tracing::info!(
                    target: "reg.config()",
                    path = %path,
                    "Loaded Regulation set-points from config file"
                );
                points
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.config()",
                    path = %path,
                    error = %e,
                    "Failed to load Regulation config file, using defaults"
                );
                SetPoints::default()
            }
        },
        Err(_) => SetPoints::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_set_points_pass_validation() {
        SetPoints::default()
            .validate()
            .expect("defaults must validate");
    }

    #[test]
    fn reject_gas_min_remaining_out_of_range() {
        let mut sp = SetPoints {
            gas_min_remaining: 2.0,
            ..Default::default()
        };
        assert!(sp.validate().is_err());
        sp = SetPoints {
            gas_min_remaining: -0.1,
            ..Default::default()
        };
        assert!(sp.validate().is_err());
    }

    #[test]
    fn reject_inverted_outcome_thresholds() {
        let sp = SetPoints {
            outcome_warning_threshold: 0.2,
            outcome_critical_threshold: 0.5,
            ..Default::default()
        };
        assert!(sp.validate().is_err());
    }

    #[test]
    fn reject_zero_variety_deficit() {
        let sp = SetPoints {
            variety_max_deficit: 0.0,
            ..Default::default()
        };
        assert!(sp.validate().is_err());
    }

    #[test]
    fn reject_zero_max_iterations() {
        let sp = SetPoints {
            max_iterations: 0,
            ..Default::default()
        };
        assert!(sp.validate().is_err());
    }

    #[test]
    fn reject_inverted_fed_latency_thresholds() {
        let sp = SetPoints {
            fed_sync_latency_warning_ms: 50000,
            fed_sync_latency_critical_ms: 5000,
            ..Default::default()
        };
        assert!(sp.validate().is_err());
    }
}
