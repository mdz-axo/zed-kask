//! DAMPEN — Suppress repeated directives within a configurable time window
//!
//! Implements the DAMPEN messenger function (4.3: FILTER+RECONCILE) from the
//! 6-loop architecture. The Curation→Cybernetics→Curation
//! feedback cycle can produce repeated identical directives. DAMPEN prevents
//! the same directive from being issued within a configurable time window.
//!
//! # Why this lives in the Regulation crate
//!
//! Dampening is a Cybernetics regulation function — it prevents oscillation
//! in the Curation↔Cybernetics feedback cycle. As such, it is owned by the
//! Cybernetics loop and lives in `hkask-regulation`, the crate responsible for
//! homeostatic self-regulation. The dampener operates on `CuratorDirective`
//! data, but its purpose is regulatory, not curatorial: it is a FILTER
//! function that enforces the cybernetic stability of the system.
//!
//! # How it works
//!
//! Two dampening layers:
//!
//! 1. **Per-fingerprint dedup** — When a directive is issued, the dampener
//!    records a "fingerprint" (type + target) with a timestamp. If the same
//!    fingerprint is seen again within the standard window, the directive is
//!    suppressed. This prevents repeated identical directives.
//!
//! 2. **Override cooldown** — After any metacognitive override
//!    (`override_energy_budget`, `seek_more_evidence`) passes the fingerprint
//!    dedup, ALL subsequent overrides are suppressed for the cooldown period
//!    (default 120s), regardless of type or target. This prevents override
//!    oscillation: a different override targeting a different agent cannot
//!    bypass the cooldown by changing its fingerprint.

use hkask_types::CuratorDirective;
use hkask_types::WebID;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::Duration;

/// Default dampening window: 60 seconds.
///
/// Within this window, the same directive (same type + target) will be
/// suppressed to prevent feedback oscillation.
pub(crate) const DEFAULT_DAMPEN_WINDOW: Duration = Duration::from_secs(60);

/// Metacognitive override dampening window: 300 seconds.
///
/// Metacognitive overrides (`OverrideEnergyBudget`, `SeekMoreEvidence`) represent
/// higher-order reflective interventions and are dampened at a longer window
/// to prevent premature re-issuance while still allowing genuine re-triggering.
pub(crate) const METACOGNITIVE_DAMPEN_WINDOW: Duration = Duration::from_secs(300);

/// Default override cooldown: 120 seconds.
///
/// After any metacognitive override passes the per-fingerprint dedup check,
/// ALL subsequent overrides are suppressed for this duration regardless of
/// type or target. This prevents override oscillation — the scenario where
/// the Curation↔Cybernetics feedback loop rapidly fires different overrides.
pub(crate) const DEFAULT_OVERRIDE_COOLDOWN: Duration = Duration::from_secs(120);

/// A fingerprint that identifies a directive for dampening.
///
/// Two directives with the same fingerprint will be suppressed if the
/// second arrives within the dampening window.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DirectiveFingerprint {
    /// The directive variant name (e.g., "calibrate_threshold", "override_energy_budget").
    variant: String,
    /// The target agent (if applicable).
    target: Option<WebID>,
}

/// Inner state protected by a single lock to eliminate TOCTOU races.
struct DampenerState {
    /// Recent directive fingerprints with their last-seen timestamps
    seen: HashMap<DirectiveFingerprint, std::time::Instant>,
    /// Timestamp of the last metacognitive override that passed dedup.
    /// After any override passes, ALL subsequent overrides are suppressed
    /// for `override_cooldown` seconds. This prevents override oscillation.
    last_override: Option<std::time::Instant>,
}

/// DAMPEN — Suppress repeated directives within a configurable time window.
///
/// This implements the DAMPEN Cybernetics regulation function that prevents
/// feedback oscillation in the Curation→Cybernetics→Curation cycle.
///
/// # OCAP Discipline
///
/// The dampener does not change directives — it only decides whether to
/// pass them through or suppress them. It is a pure FILTER function.
pub(crate) struct Dampener {
    /// Combined state (seen fingerprints + last override timestamp) under
    /// a single lock to eliminate the TOCTOU race between fingerprint check
    /// and override cooldown check.
    state: Mutex<DampenerState>,
    /// Standard dampening window for routine directives
    window: Duration,
    /// Extended dampening window for metacognitive overrides (used for eviction)
    metacognitive_window: Duration,
    /// Cooldown window after a metacognitive override passes dedup.
    /// Default: 120 seconds. Within this window, ALL overrides are suppressed
    /// regardless of type or target.
    override_cooldown: Duration,
}

impl Dampener {
    /// Create a new dampener with the default 60-second window and 120-second
    /// override cooldown.
    ///
    /// expect: "The system prevents regulation loop stagnation through cooldown dampening and substitution tracking"
    pub(crate) fn new() -> Self {
        Self::with_window(DEFAULT_DAMPEN_WINDOW)
    }

    /// Create a new dampener with a custom standard dampening window.
    ///
    /// The metacognitive window defaults to 300 seconds.
    /// The override cooldown defaults to 120 seconds.
    ///
    /// expect: "The system prevents regulation loop stagnation through cooldown dampening and substitution tracking"
    pub(crate) fn with_window(window: Duration) -> Self {
        Self {
            state: Mutex::new(DampenerState {
                seen: HashMap::new(),
                last_override: None,
            }),
            window,
            metacognitive_window: METACOGNITIVE_DAMPEN_WINDOW,
            override_cooldown: DEFAULT_OVERRIDE_COOLDOWN,
        }
    }

    /// Create a new dampener with fully configurable windows from SetPointsConfig.
    ///
    /// All three windows can be overridden via YAML configuration.
    ///
    /// expect: "The system prevents regulation loop stagnation through cooldown dampening and substitution tracking"
    pub(crate) fn with_windows(
        dampen_window: Duration,
        metacognitive_window: Duration,
        override_cooldown: Duration,
    ) -> Self {
        Self {
            state: Mutex::new(DampenerState {
                seen: HashMap::new(),
                last_override: None,
            }),
            window: dampen_window,
            metacognitive_window,
            override_cooldown,
        }
    }

    /// [NORMATIVE] Check if a directive should be dampened (suppressed). (P9 — Homeostatic Self-Regulation).
    ///
    /// Two dampening layers are applied in order:
    ///
    /// 1. **Per-fingerprint dedup** — if the same (type, target) directive
    ///    was seen within the standard window, suppress.
    ///
    /// 2. **Override cooldown** — for metacognitive overrides only: if any
    ///    override passed dedup within the cooldown period, suppress ALL
    ///    subsequent overrides regardless of type or target.
    ///
    /// If neither layer suppresses the directive, the fingerprint is recorded
    /// and (for overrides) the override timestamp is set.
    ///
    /// Uses a single `parking_lot::Mutex` lock acquisition to eliminate the
    /// TOCTOU race between fingerprint check and override cooldown check.
    ///
    /// expect: "The system prevents regulation loop stagnation through cooldown dampening and substitution tracking"
    pub(crate) fn should_dampen_directive(&self, directive: &CuratorDirective) -> bool {
        let fingerprint = DirectiveFingerprint {
            variant: directive.variant_name().to_string(),
            target: directive.agent_target(),
        };
        let now = std::time::Instant::now();
        let mut state = self.state.lock();
        let max_window = self.window.max(self.metacognitive_window);
        state
            .seen
            .retain(|_, last_seen| now.duration_since(*last_seen) < max_window);

        if let Some(last_seen) = state.seen.get(&fingerprint)
            && now.duration_since(*last_seen) < self.window
        {
            return true;
        }

        if directive.is_metacognitive() {
            if let Some(last) = state.last_override
                && now.duration_since(last) < self.override_cooldown
            {
                return true;
            }
            state.last_override = Some(now);
        }
        state.seen.insert(fingerprint, now);
        false
    }
}

impl Default for Dampener {
    fn default() -> Self {
        Self::new()
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// STAGNATION DETECTOR — detect regulatory plateaus (Fermi pattern)
// ═════════════════════════════════════════════════════════════════════════════

/// Tracks (deviation, action) pattern repetition across regulation cycles.
///
/// Fermi-inspired early-stopping pattern: when the same deviation→action
/// pattern repeats without metric improvement, the regulator has converged
/// to a wrong attractor (Conant-Ashby violation). This detector identifies
/// plateaus and signals for metacognitive escalation.
///
/// Supports per-metric thresholds via `per_metric_thresholds`.
/// Fallback: `default_threshold` (default: 5 cycles).
pub(crate) struct StagnationDetector {
    /// Key: (metric_name, action_type). Value: consecutive ineffective cycles.
    history: Mutex<HashMap<(String, String), u32>>,
    /// Default cycles before reporting plateau.
    default_threshold: u32,
    /// Per-metric threshold overrides. Key: metric name (snake_case).
    per_metric_thresholds: HashMap<String, u32>,
}

impl StagnationDetector {
    /// expect: "The system prevents regulation loop stagnation through cooldown dampening and substitution tracking"
    pub(crate) fn new(default_threshold: u32) -> Self {
        Self {
            history: Mutex::new(HashMap::new()),
            default_threshold,
            per_metric_thresholds: HashMap::new(),
        }
    }

    /// Set per-metric threshold overrides from SetPoints configuration.
    ///
    /// expect: "The system prevents regulation loop stagnation through cooldown dampening and substitution tracking"
    pub(crate) fn with_per_metric_thresholds(mut self, thresholds: HashMap<String, u32>) -> Self {
        self.per_metric_thresholds = thresholds;
        self
    }

    /// Get the effective threshold for a given metric.
    fn threshold_for(&self, metric_name: &str) -> u32 {
        self.per_metric_thresholds
            .get(metric_name)
            .copied()
            .unwrap_or(self.default_threshold)
    }

    /// Record a regulatory action for a specific metric. Returns true if
    /// this (metric, action) pair has repeated enough times to indicate
    /// a plateau.
    ///
    /// If the action's decision is Accept, the counter is reset.
    /// If Stage or Block, the counter increments toward the threshold.
    ///
    /// expect: "The system prevents regulation loop stagnation through cooldown dampening and substitution tracking"
    pub(crate) fn record_and_check(
        &self,
        metric_name: &str,
        action_type: &str,
        accepted: bool,
    ) -> bool {
        let key = (metric_name.to_string(), action_type.to_string());
        let mut history = self.history.lock();

        if accepted {
            // Action was accepted — reset the counter.
            history.remove(&key);
            return false;
        }

        let threshold = self.threshold_for(metric_name);
        let count = history.entry(key).or_insert(0);
        *count += 1;
        *count >= threshold
    }

    /// Get the effective threshold for a metric (for alert messages).
    ///
    /// expect: "The system prevents regulation loop stagnation through cooldown dampening and substitution tracking"
    pub(crate) fn threshold_for_metric(&self, metric_name: &str) -> u32 {
        self.threshold_for(metric_name)
    }

    /// Get the current ineffective count for a (metric, action_type) pair
    /// without incrementing. Returns 0 if the pair has no history or was
    /// recently reset by an accepted action.
    ///
    /// Used by `compute()` to decide whether to substitute an action
    /// *before* it's produced.
    ///
    /// expect: "The system prevents regulation loop stagnation through cooldown dampening and substitution tracking"
    pub(crate) fn ineffective_count(&self, metric_name: &str, action_type: &str) -> u32 {
        let key = (metric_name.to_string(), action_type.to_string());
        let history = self.history.lock();
        history.get(&key).copied().unwrap_or(0)
    }
}
