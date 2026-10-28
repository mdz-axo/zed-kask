//! RegulationPolicy — data-driven per-metric regulation rules
//!
//! Consolidates the per-metric action mappings, severity thresholds,
//! and classification thresholds that were previously scattered across
//! `cybernetics_loop.rs`. Each `RegulationRule` defines what actions
//! to take when a specific metric deviates in a specific direction.

use crate::loops::{
    ActionDecision, ActionType, Deviation, DeviationDirection, LoopId, RegulationData, SignalMetric,
};

/// Identifies why a regulation action was proposed.
///
/// Replaces string matching in `build_regulation_action` — the compiler
/// now verifies that every policy-table entry has a corresponding dispatch
/// arm (or falls through to the generic `_` arm).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegulationReason {
    VarietyDeficitExceeded,

    ToolReliabilityDegraded,
    /// The memory-store growth breach: sheddable-tier h_mem count (turn
    /// storage + goal events) above `triple_count_max` — the sheddable tiers
    /// have outgrown the ~3-day therapy forgetting-valve cadence. The
    /// additive-by-design knowledge layer is exempt (algedonic card
    /// 33b0eebb, option (A), 2026-10-18). Escalates through the standard
    /// algedonic path (card class `memory_store_growth`); see the
    /// `TripleCount` rule in `RegulationPolicy::default` and the ceiling's
    /// definition at `set_points::DEFAULT_TRIPLE_COUNT_MAX` (algedonic card
    /// c148d75d).
    MemoryStoreGrowth,
    LowConfidenceCountObserved,
    ConsolidationCandidatesObserved,
    PendingEscalationsObserved,
    AlgedonicEventsExceeded,
    /// The in-memory algedonic alert log is approaching its cap. The operator
    /// (or the `algedonic-review` skill) should review and clear reviewed
    /// entries before they are evicted unread.
    AlgedonicLogApproachingCap,
    GoalsStale,
    GoalsExpired,
    MetacognitionCriticalAlerts,
    MemoryLifeLow,
    CircuitBreakerOpen,
    ModelUnavailable,
    ContextServerFleetDegraded,
    OcrSilentFailuresExceeded,
    ExperimentationStuckExperimentsExceeded,
    NativeFirstRateExceeded,
}

impl RegulationReason {
    /// The wire-format string used in `RegulatoryActionParams` and logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::VarietyDeficitExceeded => "variety_deficit_exceeded",

            Self::ToolReliabilityDegraded => "tool_reliability_degraded",
            Self::MemoryStoreGrowth => "memory_store_growth",
            Self::LowConfidenceCountObserved => "low_confidence_count_observed",
            Self::ConsolidationCandidatesObserved => "consolidation_candidates_observed",
            Self::PendingEscalationsObserved => "pending_escalations_observed",
            Self::AlgedonicEventsExceeded => "algedonic_events_exceeded",
            Self::AlgedonicLogApproachingCap => "algedonic_log_approaching_cap",
            Self::GoalsStale => "goals_stale",
            Self::GoalsExpired => "goals_expired",
            Self::MetacognitionCriticalAlerts => "metacognition_critical_alerts",
            Self::MemoryLifeLow => "memory_life_low",
            Self::CircuitBreakerOpen => "circuit_breaker_open",
            Self::ModelUnavailable => "model_unavailable",
            Self::ContextServerFleetDegraded => "context_server_fleet_degraded",
            Self::OcrSilentFailuresExceeded => "ocr_silent_failures_exceeded",
            Self::ExperimentationStuckExperimentsExceeded => {
                "experimentation_stuck_experiments_exceeded"
            }
            Self::NativeFirstRateExceeded => "native_first_rate_exceeded",
        }
    }
}

/// A typed regulation disposition selected for a deviation.
#[derive(Debug, Clone)]
pub(crate) struct ProposedAction {
    pub target: LoopId,
    pub action_type: ActionType,
    pub reason: RegulationReason,
}

/// A single regulation rule: when `metric` deviates in `direction`,
/// produce `proposed` actions with the given severity classification.
pub(crate) struct RegulationRule {
    pub metric: SignalMetric,
    pub direction: DeviationDirection,
    /// The proposed actions for this rule (a slice so a rule can propose
    /// more than one; every current rule proposes exactly one).
    pub proposed: &'static [ProposedAction],
}

/// Consolidates all per-metric regulation rules.
///
/// Fuel source: declaration of what actions to propose when a metric
/// deviates. Runtime concerns (substitution ladders, throttle modes)
/// are handled by the caller in `compute()`.
pub(crate) struct RegulationPolicy {
    rules: Vec<RegulationRule>,
}

impl RegulationPolicy {
    /// Build the default regulation policy with all currently-supported rules.
    ///
    /// Covers every `SignalMetric` variant a sensor can emit per ADR-056
    /// (Ashby's Law closure) — pinned by
    /// `every_signal_metric_has_a_rule_or_documented_allowlist_entry`, which
    /// also carries the no-producer variants with their reasons. Metrics
    /// are categorized by cybernetic role:
    /// - **Notify** (observational, no regulation needed)
    /// - **Escalate** (meta-regulatory, route to Curation)
    /// - **Domain-specific** (Calibrate/Throttle/CircuitBreak/Prune)
    pub fn default() -> Self {
        use ActionType::*;
        use DeviationDirection::*;
        use LoopId::*;
        // Explicit SignalMetric imports for the names that also exist as
        // RegulationReason variants — explicit imports shadow the glob and
        // resolve the ambiguity.
        use RegulationReason::*;
        use SignalMetric::{AlgedonicLogApproachingCap, MetacognitionCriticalAlerts, *};

        Self {
            rules: vec![
                // ── Variety (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: VarietyDeficit,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: VarietyDeficitExceeded,
                    }],
                },
                // ── Wallet and Seam Coverage rules removed 2026-08-30 —
                // residuals of the deleted wallet module (219c74b180) and a
                // never-built seam watcher. No sensor ever emitted these
                // metrics; the rules could never fire.
                // ── Tool Reliability (Cybernetics Loop 6) ──
                RegulationRule {
                    metric: ToolReliability,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: ToolReliabilityDegraded,
                    }],
                },
                // ── Memory-store growth (the forgetting-valve breach) → Escalate ──
                // `TripleCount` is the memory-store growth sensor: SHEDDABLE-tier
                // h_mem count (turn storage + goal events — the tiers with
                // forgetting valves) above `triple_count_max` means a valve has
                // stalled past the ~3-day therapy forgetting-valve cadence (the
                // verified curator-process|therapy_forgetting_valve_cadence lesson,
                // confidence 0.891 — measured 2026-10-08: the store grew 218 →
                // 2,863 rows over ~30 days with the lesson unfollowed). The
                // additive-by-design knowledge layer is exempt — its growth is
                // distillation working, not a valve stall (algedonic card
                // 33b0eebb, option (A), 2026-10-18). The
                // breach escalates through the standard algedonic path (card
                // class `memory_store_growth`) so the operator sees it in the
                // next review walk and a therapy session runs — a Notify here
                // repeated the habit failure mode this sensor exists to close
                // (algedonic card c148d75d; cadence decision on card 1c02ef3c,
                // metric (a)). The sensor reads state (count vs ceiling), never
                // events, so a therapy session that reduces the count is the
                // response, not a new breach.
                RegulationRule {
                    metric: TripleCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: MemoryStoreGrowth,
                    }],
                },
                // ── Category A: Observational metrics → Notify (no regulation needed) ──
                RegulationRule {
                    metric: LowConfidenceCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: LowConfidenceCountObserved,
                    }],
                },
                RegulationRule {
                    metric: ConsolidationCandidates,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: ConsolidationCandidatesObserved,
                    }],
                },
                RegulationRule {
                    metric: PendingEscalations,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Notify,
                        reason: PendingEscalationsObserved,
                    }],
                },
                // ── Category B: Meta-regulatory metrics → Escalate to Curation ──
                RegulationRule {
                    metric: AlgedonicEvents,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: AlgedonicEventsExceeded,
                    }],
                },
                RegulationRule {
                    metric: AlgedonicLogApproachingCap,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: RegulationReason::AlgedonicLogApproachingCap,
                    }],
                },
                RegulationRule {
                    metric: GoalStaleCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: GoalsStale,
                    }],
                },
                RegulationRule {
                    metric: GoalExpiredCount,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: GoalsExpired,
                    }],
                },
                RegulationRule {
                    metric: MetacognitionCriticalAlerts,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: RegulationReason::MetacognitionCriticalAlerts,
                    }],
                },
                // MetacognitionVarietyDeficit / ActionIneffective /
                // RegulatoryPlateau / ActionDecisionBlocked rules removed
                // 2026-08-30 — superseded duplicates. MetacognitionVarietyDeficit
                // duplicated VarietyDeficit (same ledger overall_deficit,
                // same Escalate→Curation rule); the three action metrics were
                // superseded by direct plateau/blocked escalations persisted
                // to the review queue and sensed as PendingEscalations.
                // ── Category C: Domain-specific regulation ──
                // MemoryLife (Memory Loop 2) → Escalate
                RegulationRule {
                    metric: MemoryLife,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: MemoryLifeLow,
                    }],
                },
                // CircuitBreakerState (Inference Loop 1) → Escalate.
                // Fast correction already occurred at the dispatch boundary;
                // central regulation reports the persistent degraded state.
                RegulationRule {
                    metric: CircuitBreakerState,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: CircuitBreakerOpen,
                    }],
                },
                // A missing model is not locally calibratable; it requires an
                // operator configuration decision.
                RegulationRule {
                    metric: InferenceModelAvailable,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: ModelUnavailable,
                    }],
                },
                // ContextServerHealth (Cybernetics Loop 6) → Escalate
                //
                // A degraded context-server fleet (servers stuck in Starting
                // or Error) is not something the loop can self-heal — it
                // indicates the foreground executor is starving the stdio
                // transport tasks, or a credential/config failure prevented
                // `initialize`. Escalate to Curation for operator attention.
                RegulationRule {
                    metric: ContextServerHealth,
                    direction: BelowSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: ContextServerFleetDegraded,
                    }],
                },
                // OcrSilentFailures (Cybernetics Loop 6) → Escalate
                //
                // A dead-but-responsive OCR endpoint (HTTP 200 with empty
                // content on every page) is not something the loop
                // can self-heal — the corpus pipeline fails the affected
                // pages with typed errors and quarantines the endpoint via
                // its circuit breaker (there is no fallback backend). Escalate to Curation for operator attention:
                // the endpoint needs fixing (prompt format, RAW_OPENAI_OUTPUT,
                // image encoding) or replacing.
                RegulationRule {
                    metric: OcrSilentFailures,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: OcrSilentFailuresExceeded,
                    }],
                },
                // ── Evolution registry health (Cybernetics Loop 6; §P8.9
                //    step 1) ── A stuck experiment (unresolved past the
                //    stale set point or budget-spent with no verdict) needs
                //    an operator/agent verdict — the loop cannot self-heal
                //                it (the corrective is the evolution protocol:
                //                record a selection or void the experiment).
                //                Escalate to Curation so the stuck ids reach
                //                the reviewable board.
                RegulationRule {
                    metric: ExperimentationStuckExperiments,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: ExperimentationStuckExperimentsExceeded,
                    }],
                },
                // ── Native-first rate (Cybernetics Loop 6) ── Substantive
                // agent turns completing with zero kask instruments is the
                // drift the affordance-injection intervention targets; the
                // loop cannot self-heal prompt-surface behavior — the
                // corrective is operator/harness work (per-turn affordance
                // injection, gates). Escalate to Curation so the reading
                // reaches the reviewable board with its rate/threshold.
                RegulationRule {
                    metric: NativeFirstRate,
                    direction: AboveSetPoint,
                    proposed: &[ProposedAction {
                        target: Curation,
                        action_type: Escalate,
                        reason: NativeFirstRateExceeded,
                    }],
                },
            ],
        }
    }

    /// Find all proposed actions for a given deviation.
    ///
    /// Returns the typed dispositions matching a deviation's metric and direction.
    pub fn decide(&self, dev: &Deviation) -> Vec<&ProposedAction> {
        self.rules
            .iter()
            .filter(|r| r.metric == dev.signal.metric && r.direction == dev.direction)
            .flat_map(|r| r.proposed.iter())
            .collect()
    }
}

/// Extract (deficit, threshold) from a `RegulationData` variant.
/// Extract the metric value and threshold from a `RegulationData` variant.
///
/// Returns `Some((value, threshold))` for variants that carry a quantitative
/// value/threshold pair, `None` for `NoData` and variants without quantitative
/// data. The caller uses the `None` case to fall back to the action's reason
/// string for the alert message — avoiding the misleading "Variety deficit 0
/// exceeds threshold 0" message that the previous `(0, 0)` fallback produced
/// for non-variety alerts.
///
/// Fractional scalars are unit-scaled before the u64 conversion — a bare
/// `as u64` cast truncates (0.80 → 0), which produced the live-observed
/// "tool_reliability_degraded — value 0 exceeds threshold 0" escalation:
/// a positive 0.80 floor displayed as 0. Rates and ratios scale to whole
/// percent (0.80 → 80), latency seconds to whole milliseconds, and
/// count-valued scalars round to the nearest integer.
pub(crate) fn extract_deficit_threshold(data: &RegulationData) -> Option<(u64, u64)> {
    match data {
        RegulationData::VarietyDeficitExceeded { deficit, threshold } => {
            Some((rounded_count(*deficit), rounded_count(*threshold)))
        }

        RegulationData::ToolReliabilityDegraded {
            reliability,
            threshold,
        } => Some((percent_of(*reliability), percent_of(*threshold))),
        RegulationData::NativeFirstRateExceeded { rate, threshold } => {
            Some((percent_of(*rate), percent_of(*threshold)))
        }

        RegulationData::ContextServerFleetHealth {
            healthy_count,
            total_count,
        } => Some((*total_count - *healthy_count, *total_count)),
        RegulationData::OcrSilentFailuresExceeded { count, threshold } => {
            Some((rounded_count(*count), rounded_count(*threshold)))
        }
        RegulationData::ExperimentationStuckExperimentsExceeded { count, threshold } => {
            Some((rounded_count(*count), rounded_count(*threshold)))
        }
        RegulationData::NoData => None,
    }
}

/// Compose the alert message for a native-Escalate action from its typed
/// data and reason — the single source of truth for this format.
///
/// `route_action_as_alert` delivers this string to the board; its condition
/// prefix identifies the open card for repeat comments. Callers use this
/// helper so alert identity cannot drift with formatting.
///
/// The verb follows the variant's bad direction (see
/// `RegulationData::below_threshold_is_bad`): floor metrics read
/// "fell below", ceiling metrics read "exceeds". Variants without a
/// threshold pair fall back to the advisory form.
pub(crate) fn alert_message(data: &RegulationData, reason: &str) -> String {
    match extract_deficit_threshold(data) {
        Some((deficit, threshold)) => {
            let verb = if data.below_threshold_is_bad() {
                "fell below"
            } else {
                "exceeds"
            };
            format!(
                "{} — value {} {} threshold {}",
                reason, deficit, verb, threshold
            )
        }
        None => format!("{} — regulatory escalation", reason),
    }
}

/// Extract the stable condition key from an alert message composed by
/// [`alert_message`].
///
/// `alert_message` embeds the per-cycle value ("{reason} — value {v} …" or
/// "{reason} — regulatory escalation"), so two messages for the same
/// persistently re-sensed condition differ every cycle and never
/// exact-match. Dedup, supersede, and auto-resolve must key on the
/// condition — the reason prefix before the " — " separator. Messages
/// without the separator are their own condition (exact match, the
/// previous behavior).
pub fn alert_condition(message: &str) -> &str {
    match message.find(" — ") {
        Some(idx) => &message[..idx],
        None => message,
    }
}

/// Scale a rate or ratio in [0.0, 1.0] to whole percent, rounding to
/// nearest.
///
/// A bare `as u64` cast truncates (0.80 → 0); percent scaling preserves
/// the set-point's magnitude so an alert never displays a threshold of 0
/// for a positive floor.
fn percent_of(value: f64) -> u64 {
    (value * 100.0).round() as u64
}

/// Round a count-valued f64 to the nearest integer.
fn rounded_count(value: f64) -> u64 {
    value.round() as u64
}

/// Classify an action's impact decision using Fermi's three-tier gate.
///
/// - `worsening`: relative adverse movement from the baseline (0.0 if improved or unchanged).
/// - `stage_ratio`: below this → Accept (noise).
/// - `block_ratio`: at or above this → Block (hard reject).
/// - Between → Stage (escalate for review).
pub(crate) fn classify_decision(
    worsening: f64,
    stage_ratio: f64,
    block_ratio: f64,
) -> ActionDecision {
    debug_assert!(
        stage_ratio <= block_ratio,
        "stage_worsening_ratio ({stage_ratio}) must be <= block_worsening_ratio ({block_ratio})"
    );
    if worsening >= block_ratio {
        ActionDecision::Block
    } else if worsening < stage_ratio {
        ActionDecision::Accept
    } else {
        ActionDecision::Stage
    }
}

/// Severity of a blocked action, graded against the block threshold the way
/// `RuntimeAlert::new` grades a deficit against its threshold: a block is at
/// least a Warning, and Critical once the worsening exceeds twice the block
/// threshold. Before this, every block was Critical and severity could not
/// order the operator's attention.
pub(crate) fn block_severity(worsening: f64, block_ratio: f64) -> crate::algedonic::AlertSeverity {
    if worsening > 2.0 * block_ratio {
        crate::algedonic::AlertSeverity::Critical
    } else {
        crate::algedonic::AlertSeverity::Warning
    }
}

/// Minimum attempts on each side before a proportion drop may Block.
pub(crate) const MIN_BLOCK_SAMPLE: u64 = 10;
/// A drop must exceed this many standard errors of the difference to Block.
pub(crate) const BLOCK_STD_ERRORS: f64 = 2.0;

/// Why a proportion drop cannot support a Block verdict, or `None` when the
/// evidence is sufficient. Uses the binomial standard error of the
/// difference, √(p₁(1−p₁)/n₁ + p₂(1−p₂)/n₂). Unknown sample sizes are
/// insufficient: an unverifiable sample is never treated as a large one.
pub(crate) fn insufficient_block_sample(
    before: f64,
    after: f64,
    sizes: Option<(u64, u64)>,
) -> Option<String> {
    let Some((n_before, n_after)) = sizes else {
        return Some("sample sizes not recorded for this metric".to_string());
    };
    if n_before < MIN_BLOCK_SAMPLE || n_after < MIN_BLOCK_SAMPLE {
        return Some(format!(
            "n_before={n_before}, n_after={n_after}; at least {MIN_BLOCK_SAMPLE} attempts per run required"
        ));
    }
    let variance = |p: f64, n: u64| p.clamp(0.0, 1.0) * (1.0 - p.clamp(0.0, 1.0)) / n as f64;
    let std_error = (variance(before, n_before) + variance(after, n_after)).sqrt();
    let delta = (after - before).abs();
    if delta <= BLOCK_STD_ERRORS * std_error {
        return Some(format!(
            "|delta|={delta:.3} within {BLOCK_STD_ERRORS} standard errors ({std_error:.3}) at n_before={n_before}, n_after={n_after}"
        ));
    }
    None
}

#[cfg(test)]
mod sample_guard_tests;
#[cfg(test)]
mod tests;
