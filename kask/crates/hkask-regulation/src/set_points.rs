//! Set-points and configuration for the Cybernetics Loop.
//!
//! Homeostatic set-points define the reference values against which sensed
//! signals are compared. When a signal deviates beyond its set-point,
//! the loop produces an efferent action.

/// Default maximum variety deficit before escalation (100).
///
/// When variety deficit exceeds this value, an algedonic alert is triggered.
pub const DEFAULT_VARIETY_MAX_DEFICIT: f64 = 100.0;

/// Default maximum number of regulation iterations per cycle.
///
/// Prevents unbounded cascading in the compute→act pipeline.
pub(crate) const DEFAULT_MAX_ITERATIONS: u32 = 100;

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

/// Default tool reliability threshold (0.80 = 80% success rate).
///
/// When the aggregate tool success rate drops below this, the
/// `ToolReliabilitySensor` produces a `ToolReliability` signal and the
/// regulation loop escalates to Curation. This closes the feedback loop
/// that was blind to systematic tool failures (e.g. MCP server timeouts
/// looping for minutes without the regulation loop sensing the deviation).
pub(crate) const DEFAULT_TOOL_RELIABILITY_THRESHOLD: f64 = 0.80;

/// Default ceiling for the native-first rate sensor — the maximum fraction
/// of completed substantive agent turns that may use zero kask instruments
/// before the loop escalates. 0.50: at most half of substantive turns may
/// complete without instruments. The operator's standing drift complaint
/// says the live rate sits well above this, so the first readings are
/// expected to deviate — that is the sensor working, not a
/// misconfiguration; recalibrate once the baseline is known.
pub(crate) const DEFAULT_NATIVE_FIRST_RATE_THRESHOLD: f64 = 0.50;

/// Default maximum regulation cycles retained for history queries.
///
/// Bounds memory growth in long-running sessions. An operator running a
/// Default maximum skill feedback spans retained per skill+phase.
///
/// Bounds memory growth for skill self-improvement signal storage.
pub(crate) const DEFAULT_MAX_SKILL_SPAN_HISTORY: usize = 50;

/// Default maximum algedonic alerts retained in the in-memory log.
///
/// Bounds memory growth in long-running sessions. The log is a diagnostic
/// ring buffer; escalated alerts are delivered to the review board
/// separately, so eviction from this log loses only the diagnostic trail.
/// When the log approaches this cap, review the board and recent log entries;
/// log clearing is a separate maintenance choice, not verification.
pub(crate) const DEFAULT_MAX_ALERTS: usize = 200;

/// Default maximum actionable (Warning-or-Critical) alerts before
/// `AlgedonicEvents` fires — the algedonic-advisory tolerance.
///
/// Default 0: every actionable alert deviates — the pre-2026-10-19 behavior,
/// preserved by option (B) on algedonic card f69162f4 (operator ruling
/// 2026-10-19: the knob exists, the policy is unchanged until the operator
/// sets it via `HKASK_REG_CONFIG` under the key `algedonic_events_max`).
/// Only Critical alerts escalate to the board (`algedonic.rs` —
/// `escalated: severity == AlertSeverity::Critical`), while the metric
/// counts Warning too, so a loop that carries any standing advisory fires
/// under the default — the operator calibrates that tolerance here rather
/// than in a recompile. Bounds: a value at or above `max_alerts` is rejected
/// by `SetPoints::validate` (the actionable count can never reach the log
/// cap, so such a tolerance silently disables the sensor — the same class
/// the `tool_reliability_threshold` bounds check rejects).
pub(crate) const DEFAULT_ALGEDONIC_EVENTS_MAX: usize = 0;

// ── Memory health defaults ──
/// Default max h_mem count before `TripleCount` fires — the memory-store
/// growth ceiling. Counted over the SHEDDABLE tiers (turn storage + goal
/// events — the tiers with forgetting valves); the additive-by-design
/// knowledge layer is exempt (operator ruling on algedonic card 33b0eebb,
/// option (A), 2026-10-18: a size alarm on the knowledge tier fires on
/// normal operation forever — its growth IS distillation working; recall
/// quality is protected by confidence decay, not deletion). Design
/// constraint: the ~3-day therapy forgetting-valve cadence (the verified
/// curator-process|therapy_forgetting_valve_cadence lesson, confidence
/// 0.891 — measured 2026-10-08: the store grew 218 → 2,863 rows over ~30
/// days with the lesson unfollowed; a habit has no enforcement mechanism).
/// The ceiling is sized so a store growing at the measured active-day rate
/// breaches in ~3 days, not ~30: measured 2026-10-18 sheddable steady state
/// 3,415 rows (2,691 turn rows + 724 goal rows), active-day growth 200–600
/// rows/day → 4,000 chosen (the pre-2026-10-18 form counted the whole
/// store — 5,574 rows with 1,861 additive — and fired perpetually on the
/// knowledge tier's by-design growth). Operator-adjustable after each
/// therapy via `HKASK_REG_CONFIG` (`triple_count_max`). The breach
/// escalates (`RegulationReason::MemoryStoreGrowth`, card class
/// `memory_store_growth`). The sensor reads state (sheddable count vs
/// ceiling), never events — a therapy session that reduces the count is the
/// response, not a new breach.
pub(crate) const DEFAULT_TRIPLE_COUNT_MAX: usize = 4_000;
/// Default max low-confidence h_mem count before `LowConfidenceCount` fires.
pub(crate) const DEFAULT_LOW_CONFIDENCE_MAX: usize = 100;
/// Default confidence threshold for `LowConfidenceCount`.
pub(crate) const DEFAULT_LOW_CONFIDENCE_THRESHOLD: f64 = 0.3;
/// Default confidence floor for `ConsolidationCandidates`.
pub(crate) const DEFAULT_CONSOLIDATION_FLOOR: f64 = 0.1;
/// Default max consolidation candidates before `ConsolidationCandidates` fires.
pub(crate) const DEFAULT_CONSOLIDATION_CANDIDATES_MAX: usize = 50;
/// Default minimum memory life in days.
pub(crate) const DEFAULT_MEMORY_LIFE_MIN_DAYS: f64 = 30.0;

/// Homeostatic set-points for the Cybernetics Loop.
///
/// These define the reference values against which sensed signals
/// are compared. When a signal deviates beyond its set-point,
/// the loop produces an efferent action.
#[derive(Debug, Clone)]
pub struct SetPoints {
    /// Maximum variety deficit before escalation. Default: 100
    pub variety_max_deficit: f64,

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

    /// Minimum tool reliability (success rate) before the Cybernetics Loop
    /// escalates. Sensed from `RegulationLedger::outcome_breakdown` via the
    /// minimum-sample-floored aggregate (`aggregate_tool_reliability`).
    /// Default: 0.80.
    pub tool_reliability_threshold: f64,
    /// Maximum fraction of completed substantive agent turns that may use
    /// zero kask instruments before the native-first sensor deviates.
    /// Default: 0.50.
    pub native_first_rate_threshold: f64,
    // ── History retention (v0.33.0) ──
    /// Maximum skill feedback spans retained per skill+phase.
    /// Default: 50.
    pub max_skill_span_history: usize,
    /// Maximum algedonic alerts retained in the in-memory log before oldest
    /// entries are evicted. Default: 200. When the log approaches this cap,
    /// the cybernetics loop emits an `AlgedonicLogApproachingCap` signal.
    pub max_alerts: usize,
    /// Maximum actionable (Warning-or-Critical) alerts before
    /// `AlgedonicEvents` fires — the algedonic-advisory tolerance.
    /// Default: 0 (every actionable alert deviates — the pre-2026-10-19
    /// behavior; option (B) on algedonic card f69162f4 exposes the knob,
    /// it does not decide the policy). See `DEFAULT_ALGEDONIC_EVENTS_MAX`
    /// for the measured basis and bounds.
    pub algedonic_events_max: usize,
    // ── Memory health set-points (v0.34.0) ──
    /// Maximum h_mem count before `TripleCount` fires — the memory-store
    /// growth ceiling, counted over the SHEDDABLE tiers (turn storage +
    /// goal events; the additive-by-design knowledge layer is exempt —
    /// operator ruling on algedonic card 33b0eebb, option (A), 2026-10-18).
    /// Default: 4_000 (sized to the ~3-day therapy forgetting-valve cadence
    /// against the measured 2026-10-18 sheddable steady state of 3,415
    /// rows — 2,691 turn rows + 724 goal rows; see `DEFAULT_TRIPLE_COUNT_MAX`
    /// for the measured basis). A breach escalates as `memory_store_growth`
    /// (algedonic card c148d75d) — the count-based monitoring reference
    /// now reaches the review board; forgetting itself stays time-based
    /// and distillation-gated (operator ruling 2026-09-04).
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
    /// Minimum memory life in days. Below this, `MemoryLife` fires.
    /// Default: 30.0 (a memory life shorter than 30 days is too aggressive).
    pub memory_life_min_days: f64,
}

/// YAML-configurable set-points. Fields are Optional so partial configs work.
/// Missing fields fall back to the `SetPoints::default()` values.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub(crate) struct SetPointsConfig {
    pub variety_max_deficit: Option<f64>,

    pub dampen_window_secs: Option<u64>,
    pub metacognitive_window_secs: Option<u64>,
    pub override_cooldown_secs: Option<u64>,
    pub outcome_warning_threshold: Option<f64>,
    pub outcome_critical_threshold: Option<f64>,
    pub max_iterations: Option<u32>,
    pub stagnation_thresholds: Option<std::collections::HashMap<String, u32>>,
    pub stage_worsening_ratio: Option<f64>,
    pub block_worsening_ratio: Option<f64>,

    pub tool_reliability_threshold: Option<f64>,
    pub native_first_rate_threshold: Option<f64>,
    pub max_skill_span_history: Option<usize>,
    pub max_alerts: Option<usize>,
    pub algedonic_events_max: Option<usize>,
    pub triple_count_max: Option<usize>,
    pub low_confidence_max: Option<usize>,
    pub low_confidence_threshold: Option<f64>,
    pub consolidation_floor: Option<f64>,
    pub consolidation_candidates_max: Option<usize>,
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
            variety_max_deficit: DEFAULT_VARIETY_MAX_DEFICIT,

            dampen_window_secs: DEFAULT_DAMPEN_WINDOW_SECS,
            metacognitive_window_secs: DEFAULT_METACOGNITIVE_WINDOW_SECS,
            override_cooldown_secs: DEFAULT_OVERRIDE_COOLDOWN_SECS,
            outcome_warning_threshold: DEFAULT_OUTCOME_WARNING_THRESHOLD,
            outcome_critical_threshold: DEFAULT_OUTCOME_CRITICAL_THRESHOLD,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            stagnation_thresholds: std::collections::HashMap::new(),
            stage_worsening_ratio: DEFAULT_STAGE_WORSENING_RATIO,
            block_worsening_ratio: DEFAULT_BLOCK_WORSENING_RATIO,

            tool_reliability_threshold: DEFAULT_TOOL_RELIABILITY_THRESHOLD,
            native_first_rate_threshold: DEFAULT_NATIVE_FIRST_RATE_THRESHOLD,
            max_skill_span_history: DEFAULT_MAX_SKILL_SPAN_HISTORY,
            max_alerts: DEFAULT_MAX_ALERTS,
            algedonic_events_max: DEFAULT_ALGEDONIC_EVENTS_MAX,
            triple_count_max: DEFAULT_TRIPLE_COUNT_MAX,
            low_confidence_max: DEFAULT_LOW_CONFIDENCE_MAX,
            low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            consolidation_floor: DEFAULT_CONSOLIDATION_FLOOR,
            consolidation_candidates_max: DEFAULT_CONSOLIDATION_CANDIDATES_MAX,
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
            variety_max_deficit: config
                .variety_max_deficit
                .unwrap_or(defaults.variety_max_deficit),

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

            tool_reliability_threshold: config
                .tool_reliability_threshold
                .unwrap_or(defaults.tool_reliability_threshold),
            native_first_rate_threshold: config
                .native_first_rate_threshold
                .unwrap_or(defaults.native_first_rate_threshold),
            max_skill_span_history: config
                .max_skill_span_history
                .unwrap_or(defaults.max_skill_span_history),
            max_alerts: config.max_alerts.unwrap_or(defaults.max_alerts),
            algedonic_events_max: config
                .algedonic_events_max
                .unwrap_or(defaults.algedonic_events_max),
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
            memory_life_min_days: config
                .memory_life_min_days
                .unwrap_or(defaults.memory_life_min_days),
        }
    }

    /// expect: "The system provides configurable regulation thresholds for the cybernetic control loop"
    /// Validate set-point invariants.
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("outcome_warning_threshold", self.outcome_warning_threshold),
            (
                "outcome_critical_threshold",
                self.outcome_critical_threshold,
            ),
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

        if self.dampen_window_secs == 0 {
            return Err(anyhow::anyhow!("dampen_window_secs must be > 0"));
        }
        // A 0.0 reliability floor silently disables the check: the sensor
        // emits nothing when the set-point is 0 (every aggregate is >= 0), so
        // an operator cannot distinguish "all tools healthy" from "check
        // disabled". A floor above 1.0 is unsatisfiable — every aggregate
        // (<= 1.0) would deviate every tick. Reject both so load_set_points
        // falls back to the 0.80 default instead of running a blind sensor.
        if !(0.0 < self.tool_reliability_threshold && self.tool_reliability_threshold <= 1.0) {
            return Err(anyhow::anyhow!(
                "tool_reliability_threshold must be in (0.0, 1.0], got {}",
                self.tool_reliability_threshold
            ));
        }
        // A 1.0 native-first ceiling silently disables the sensor: every
        // observed rate is <= 1.0, so a 1.0 set point can never be exceeded
        // and the sensor reads "healthy" forever — the mirror image of the
        // 0.0 reliability floor above. 0.0 is strict but meaningful (any
        // native-first substantive turn deviates). Reject the ceiling and
        // negatives so load_set_points falls back to the 0.50 default
        // instead of running a blind sensor.
        if !(0.0 <= self.native_first_rate_threshold && self.native_first_rate_threshold < 1.0) {
            return Err(anyhow::anyhow!(
                "native_first_rate_threshold must be in [0.0, 1.0), got {}",
                self.native_first_rate_threshold
            ));
        }
        // An `algedonic_events_max` at or above `max_alerts` can never be
        // exceeded — the actionable count cannot reach the log cap — so it
        // silently disables the `AlgedonicEvents` sensor. Reject it so
        // `load_set_points` falls back to the default instead of running a
        // blind sensor (the same class the `tool_reliability_threshold`
        // bounds check rejects).
        if self.algedonic_events_max >= self.max_alerts {
            return Err(anyhow::anyhow!(
                "algedonic_events_max ({}) must be < max_alerts ({}) — at or above the log cap the sensor is silently disabled",
                self.algedonic_events_max,
                self.max_alerts
            ));
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

#[cfg(test)]
mod tests;
