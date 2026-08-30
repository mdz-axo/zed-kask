//! Set-points and configuration for the Cybernetics Loop.
//!
//! Homeostatic set-points define the reference values against which sensed
//! signals are compared. When a signal deviates beyond its set-point,
//! the loop produces an efferent action.

use hkask_types::regulation::QueueDepth;

/// Default minimum energy budget remaining ratio (20%).
///
/// When energy remaining drops below this ratio, the Cybernetics Loop produces
/// a throttle action to reduce consumption.
pub(crate) const DEFAULT_ENERGY_MIN_REMAINING_RATIO: f64 = 0.2;

/// Default maximum variety deficit before escalation (100).
///
/// When variety deficit exceeds this value, an algedonic alert is triggered.
pub const DEFAULT_VARIETY_MAX_DEFICIT: f64 = 100.0;

/// Default maximum error rate (30%).
///
/// When the error rate exceeds this ratio, the Cybernetics Loop produces
/// a calibration action.
pub(crate) const DEFAULT_ERROR_RATE_MAX: f64 = 0.3;

/// Default maximum connector latency in seconds.
///
/// When connector latency exceeds this threshold, the Cybernetics Loop
/// produces a throttle action.
pub(crate) const DEFAULT_CONNECTOR_LATENCY_MAX_SECS: f64 = 30.0;

/// Default communication queue depth threshold for backpressure regulation.
///
/// When the Communication Loop's queue depth exceeds this value,
/// the Cybernetics Loop produces a Throttle(Communication) action.
pub(crate) const DEFAULT_COMMUNICATION_BACKPRESSURE_THRESHOLD: QueueDepth =
    QueueDepth::DEFAULT_BACKPRESSURE;

/// Default minimum seam coverage ratio before alert.
///
/// When per-crate coverage drops below its previous snapshot value,
/// Fires an algedonic alert. Default: 0.0 (alert on ANY regression —
/// \[NORMATIVE\] coverage should never go down). (P9 — Homeostatic Self-Regulation).
pub(crate) const DEFAULT_SEAM_COVERAGE_MIN: f64 = 0.0;

/// Default maximum number of regulation iterations per cycle.
///
/// Prevents unbounded cascading in the compute→act pipeline.
pub(crate) const DEFAULT_MAX_ITERATIONS: u32 = 100;

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
pub(crate) const DEFAULT_DAMPEN_WINDOW_SECS: u64 = 60;

/// Default metacognitive dampener window in seconds (300s).
///
/// Metacognitive overrides are dampened at a longer window.
pub(crate) const DEFAULT_METACOGNITIVE_WINDOW_SECS: u64 = 300;

/// Default override cooldown in seconds (120s).
///
/// After any metacognitive override passes dedup, ALL subsequent overrides
/// are suppressed for this duration.
pub(crate) const DEFAULT_OVERRIDE_COOLDOWN_SECS: u64 = 120;

/// Default outcome warning threshold (0.50 = 50% success rate).
///
/// When outcome success rate drops below this, a warning alert is emitted.
pub(crate) const DEFAULT_OUTCOME_WARNING_THRESHOLD: f64 = 0.50;

/// Default outcome critical threshold (0.25 = 25% success rate).
///
/// When outcome success rate drops below this, a critical alert is emitted.
pub(crate) const DEFAULT_OUTCOME_CRITICAL_THRESHOLD: f64 = 0.25;

/// Default stagnation detection threshold (5 cycles).
///
/// After this many consecutive cycles of the same ineffective (metric, action)
/// pair, a regulatory-plateau alert is triggered (persisted to the review
/// queue; the `RegulatoryPlateau` SignalMetric/policy rule was removed
/// 2026-08-30 as a superseded duplicate of this direct path).
pub(crate) const DEFAULT_STAGNATION_THRESHOLD: u32 = 5;

/// Default stage threshold for ActionDecision: 5% relative worsening.
///
/// When an action worsens its target metric by less than this ratio,
/// it's accepted as noise. Between this and `DEFAULT_BLOCK_WORSENING_RATIO`,
/// it's staged for review.
pub(crate) const DEFAULT_STAGE_WORSENING_RATIO: f64 = 0.05;

/// Default block threshold for ActionDecision: 20% relative worsening.
///
/// When an action worsens its target metric by this ratio or more,
/// the (metric, action_type) pair is blocked until Curation intervenes.
pub(crate) const DEFAULT_BLOCK_WORSENING_RATIO: f64 = 0.20;

/// Default substitution activation threshold: try alternatives after this
/// many consecutive ineffective cycles (default: 2 — half the stagnation
/// default of 5). When a (metric, action_type) pair hits this count,
/// `compute()` tries the next action in the substitution ladder.
pub(crate) const DEFAULT_SUBSTITUTION_AFTER: u32 = 2;

/// Default test coverage floor (0.70 = 70% coverage).
///
/// When the latest trace run's `coverage_pct` drops below this, the
/// Cybernetics Loop's `TestCoverageSensor` produces a signal.
pub(crate) const DEFAULT_COVERAGE_FLOOR: f64 = 0.70;

/// Default mutation score floor (0.50 = 50% of mutants killed).
///
/// When the latest trace run's `mutation_score` drops below this, the
/// Cybernetics Loop's `MutationScoreSensor` produces a signal.
pub(crate) const DEFAULT_MUTATION_SCORE_FLOOR: f64 = 0.50;

/// Default tool reliability threshold (0.80 = 80% success rate).
///
/// When the aggregate tool success rate drops below this, the
/// `ToolReliabilitySensor` produces a `ToolReliability` signal and the
/// regulation loop escalates to Curation. This closes the feedback loop
/// that was blind to systematic tool failures (e.g. MCP server timeouts
/// looping for minutes without the regulation loop sensing the deviation).
pub(crate) const DEFAULT_TOOL_RELIABILITY_THRESHOLD: f64 = 0.80;

/// Default maximum regulation cycles retained for history queries.
///
/// Bounds memory growth in long-running sessions. An operator running a
/// long autonomous swarm may want more history; one on a memory-constrained
/// box may want less.
pub(crate) const DEFAULT_MAX_REGULATION_HISTORY: usize = 100;

/// Default maximum skill feedback spans retained per skill+phase.
///
/// Bounds memory growth for skill self-improvement signal storage.
pub(crate) const DEFAULT_MAX_SKILL_SPAN_HISTORY: usize = 50;

/// Default maximum algedonic alerts retained in the in-memory log.
///
/// Bounds memory growth in long-running sessions. The log is a diagnostic
/// ring buffer; escalated alerts are persisted to the `EscalationQueue`
/// separately, so eviction from this log loses only the diagnostic trail.
/// When the log approaches this cap, the `algedonic-review` skill should be
/// invoked to review and clear reviewed entries.
pub(crate) const DEFAULT_MAX_ALERTS: usize = 200;

// ── Memory health defaults ──
/// Default max h_mem count before `TripleCount` fires.
pub(crate) const DEFAULT_TRIPLE_COUNT_MAX: usize = 10_000;
/// Default max low-confidence h_mem count before `LowConfidenceCount` fires.
pub(crate) const DEFAULT_LOW_CONFIDENCE_MAX: usize = 100;
/// Default confidence threshold for `LowConfidenceCount`.
pub(crate) const DEFAULT_LOW_CONFIDENCE_THRESHOLD: f64 = 0.3;
/// Default confidence floor for `ConsolidationCandidates`.
pub(crate) const DEFAULT_CONSOLIDATION_FLOOR: f64 = 0.1;
/// Default max consolidation candidates before `ConsolidationCandidates` fires.
pub(crate) const DEFAULT_CONSOLIDATION_CANDIDATES_MAX: usize = 50;
/// Default storage usage ratio (h_mem_count / storage_budget).
pub(crate) const DEFAULT_STORAGE_USAGE_MAX_RATIO: f64 = 0.8;
/// Default minimum memory life in days.
pub(crate) const DEFAULT_MEMORY_LIFE_MIN_DAYS: f64 = 30.0;

/// Homeostatic set-points for the Cybernetics Loop.
///
/// These define the reference values against which sensed signals
/// are compared. When a signal deviates beyond its set-point,
/// the loop produces an efferent action.
#[derive(Debug, Clone)]
pub struct SetPoints {
    /// Minimum energy budget remaining ratio (0.0-1.0). Default: 0.2 (20% remaining)
    pub energy_min_remaining: f64,
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
    // ── Loop regulation (v0.30.0) ──
    /// Maximum regulation iterations per cycle. Default: 100.
    pub max_iterations: u32,
    // ── Stagnation detection (v0.31.0, Fermi pattern) ──
    /// Per-metric stagnation thresholds. Key: metric name (snake_case),
    /// value: cycles before the regulatory-plateau alert. Unlisted metrics
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
    // ── Trace-derived quality floors (v0.32.0) ──
    /// Minimum test coverage fraction before the Cybernetics Loop alerts.
    /// Read from the latest trace run's `metrics.json` `coverage_pct`.
    /// Default: 0.70.
    pub coverage_floor: f64,
    /// Minimum mutation score fraction before the Cybernetics Loop alerts.
    /// Read from the latest trace run's `metrics.json` `mutation_score`.
    /// Default: 0.50.
    pub mutation_score_floor: f64,
    /// Minimum tool reliability (success rate) before the Cybernetics Loop
    /// escalates. Sensed from `RegulationLedger::outcome_success_rate`.
    /// Default: 0.80.
    pub tool_reliability_threshold: f64,
    // ── History retention (v0.33.0) ──
    /// Maximum regulation cycles retained for history queries.
    /// Default: 100.
    pub max_regulation_history: usize,
    /// Maximum skill feedback spans retained per skill+phase.
    /// Default: 50.
    pub max_skill_span_history: usize,
    /// Maximum algedonic alerts retained in the in-memory log before oldest
    /// entries are evicted. Default: 200. When the log approaches this cap,
    /// the cybernetics loop emits an `AlgedonicLogApproachingCap` signal.
    pub max_alerts: usize,
    // ── Memory health set-points (v0.34.0) ──
    /// Maximum h_mem count before `TripleCount` fires. Default: 10_000
    /// (matches `DEFAULT_STORAGE_BUDGET`).
    pub triple_count_max: usize,
    /// Maximum low-confidence h_mem count before `LowConfidenceCount` fires.
    /// Default: 100.
    pub low_confidence_max: usize,
    /// Confidence threshold for `LowConfidenceCount`. H_mems at or below this
    /// value are counted. Default: 0.3.
    pub low_confidence_threshold: f64,
    /// Confidence floor for `ConsolidationCandidates`. H_mems at or below this
    /// value are deletion candidates. Default: 0.1 (lower than
    /// `low_confidence_threshold` — candidates are near-deletion, not just
    /// low-confidence).
    pub consolidation_floor: f64,
    /// Maximum consolidation candidates before `ConsolidationCandidates` fires.
    /// Default: 50.
    pub consolidation_candidates_max: usize,
    /// Maximum storage usage ratio (h_mem_count / storage_budget) before
    /// `StorageUsage` fires. 0.0–1.0. Default: 0.8 (80% full).
    pub storage_usage_max_ratio: f64,
    /// Minimum memory life in days. Below this, `MemoryLife` fires.
    /// Default: 30.0 (a memory life shorter than 30 days is too aggressive).
    pub memory_life_min_days: f64,
}

/// YAML-configurable set-points. Fields are Optional so partial configs work.
/// Missing fields fall back to the `SetPoints::default()` values.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub(crate) struct SetPointsConfig {
    pub energy_min_remaining: Option<f64>,
    pub variety_max_deficit: Option<f64>,
    pub error_rate_max: Option<f64>,
    pub connector_latency_max_secs: Option<f64>,
    pub communication_backpressure_threshold: Option<QueueDepth>,
    pub seam_coverage_min: Option<f64>,
    pub dampen_window_secs: Option<u64>,
    pub metacognitive_window_secs: Option<u64>,
    pub override_cooldown_secs: Option<u64>,
    pub outcome_warning_threshold: Option<f64>,
    pub outcome_critical_threshold: Option<f64>,
    pub max_iterations: Option<u32>,
    pub stagnation_thresholds: Option<std::collections::HashMap<String, u32>>,
    pub stage_worsening_ratio: Option<f64>,
    pub block_worsening_ratio: Option<f64>,
    pub action_substitutions: Option<std::collections::HashMap<String, Vec<String>>>,
    pub substitution_after: Option<u32>,
    pub inference_throttle_mode: Option<InferenceThrottleMode>,
    pub coverage_floor: Option<f64>,
    pub mutation_score_floor: Option<f64>,
    pub tool_reliability_threshold: Option<f64>,
    pub max_regulation_history: Option<usize>,
    pub max_skill_span_history: Option<usize>,
    pub max_alerts: Option<usize>,
    pub triple_count_max: Option<usize>,
    pub low_confidence_max: Option<usize>,
    pub low_confidence_threshold: Option<f64>,
    pub consolidation_floor: Option<f64>,
    pub consolidation_candidates_max: Option<usize>,
    pub storage_usage_max_ratio: Option<f64>,
    pub memory_life_min_days: Option<f64>,
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
            energy_min_remaining: DEFAULT_ENERGY_MIN_REMAINING_RATIO,
            variety_max_deficit: DEFAULT_VARIETY_MAX_DEFICIT,
            error_rate_max: DEFAULT_ERROR_RATE_MAX,
            connector_latency_max_secs: DEFAULT_CONNECTOR_LATENCY_MAX_SECS,
            communication_backpressure_threshold: DEFAULT_COMMUNICATION_BACKPRESSURE_THRESHOLD,
            seam_coverage_min: DEFAULT_SEAM_COVERAGE_MIN,
            dampen_window_secs: DEFAULT_DAMPEN_WINDOW_SECS,
            metacognitive_window_secs: DEFAULT_METACOGNITIVE_WINDOW_SECS,
            override_cooldown_secs: DEFAULT_OVERRIDE_COOLDOWN_SECS,
            outcome_warning_threshold: DEFAULT_OUTCOME_WARNING_THRESHOLD,
            outcome_critical_threshold: DEFAULT_OUTCOME_CRITICAL_THRESHOLD,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            stagnation_thresholds: std::collections::HashMap::new(),
            stage_worsening_ratio: DEFAULT_STAGE_WORSENING_RATIO,
            block_worsening_ratio: DEFAULT_BLOCK_WORSENING_RATIO,
            action_substitutions: std::collections::HashMap::new(),
            substitution_after: DEFAULT_SUBSTITUTION_AFTER,
            inference_throttle_mode: InferenceThrottleMode::Off,
            coverage_floor: DEFAULT_COVERAGE_FLOOR,
            mutation_score_floor: DEFAULT_MUTATION_SCORE_FLOOR,
            tool_reliability_threshold: DEFAULT_TOOL_RELIABILITY_THRESHOLD,
            max_regulation_history: DEFAULT_MAX_REGULATION_HISTORY,
            max_skill_span_history: DEFAULT_MAX_SKILL_SPAN_HISTORY,
            max_alerts: DEFAULT_MAX_ALERTS,
            triple_count_max: DEFAULT_TRIPLE_COUNT_MAX,
            low_confidence_max: DEFAULT_LOW_CONFIDENCE_MAX,
            low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            consolidation_floor: DEFAULT_CONSOLIDATION_FLOOR,
            consolidation_candidates_max: DEFAULT_CONSOLIDATION_CANDIDATES_MAX,
            storage_usage_max_ratio: DEFAULT_STORAGE_USAGE_MAX_RATIO,
            memory_life_min_days: DEFAULT_MEMORY_LIFE_MIN_DAYS,
        }
    }
}

impl SetPoints {
    /// expect: "The system provides configurable regulation thresholds for the cybernetic control loop"
    /// Create SetPoints from a config, using defaults for missing fields.
    pub(crate) fn from_config(config: &SetPointsConfig) -> Self {
        let defaults = SetPoints::default();
        Self {
            energy_min_remaining: config
                .energy_min_remaining
                .unwrap_or(defaults.energy_min_remaining),
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
            coverage_floor: config.coverage_floor.unwrap_or(defaults.coverage_floor),
            mutation_score_floor: config
                .mutation_score_floor
                .unwrap_or(defaults.mutation_score_floor),
            tool_reliability_threshold: config
                .tool_reliability_threshold
                .unwrap_or(defaults.tool_reliability_threshold),
            max_regulation_history: config
                .max_regulation_history
                .unwrap_or(defaults.max_regulation_history),
            max_skill_span_history: config
                .max_skill_span_history
                .unwrap_or(defaults.max_skill_span_history),
            max_alerts: config.max_alerts.unwrap_or(defaults.max_alerts),
            triple_count_max: config.triple_count_max.unwrap_or(defaults.triple_count_max),
            low_confidence_max: config
                .low_confidence_max
                .unwrap_or(defaults.low_confidence_max),
            low_confidence_threshold: config
                .low_confidence_threshold
                .unwrap_or(defaults.low_confidence_threshold),
            consolidation_floor: config
                .consolidation_floor
                .unwrap_or(defaults.consolidation_floor),
            consolidation_candidates_max: config
                .consolidation_candidates_max
                .unwrap_or(defaults.consolidation_candidates_max),
            storage_usage_max_ratio: config
                .storage_usage_max_ratio
                .unwrap_or(defaults.storage_usage_max_ratio),
            memory_life_min_days: config
                .memory_life_min_days
                .unwrap_or(defaults.memory_life_min_days),
        }
    }

    /// expect: "The system provides configurable regulation thresholds for the cybernetic control loop"
    /// Validate set-point invariants.
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("energy_min_remaining", self.energy_min_remaining),
            ("error_rate_max", self.error_rate_max),
            ("seam_coverage_min", self.seam_coverage_min),
            ("coverage_floor", self.coverage_floor),
            ("mutation_score_floor", self.mutation_score_floor),
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
///
#[must_use]
pub fn load_set_points() -> SetPoints {
    let points = match std::env::var("HKASK_REG_CONFIG") {
        Ok(path) => match SetPointsConfig::load_from_file(&path) {
            Ok(config) => {
                let points = SetPoints::from_config(&config);
                if let Err(e) = points.validate() {
                    tracing::warn!(
                        target: "hkask.config",
                        path = %path,
                        error = %e,
                        "Loaded Regulation set-points failed validation — falling back to defaults"
                    );
                    return SetPoints::default();
                }
                tracing::info!(
                    target: "hkask.config",
                    path = %path,
                    "Loaded Regulation set-points from config file"
                );
                points
            }
            Err(e) => {
                tracing::warn!(
                    target: "hkask.config",
                    path = %path,
                    error = %e,
                    "Failed to load Regulation config file, using defaults"
                );
                SetPoints::default()
            }
        },
        Err(_) => SetPoints::default(),
    };
    points
}
