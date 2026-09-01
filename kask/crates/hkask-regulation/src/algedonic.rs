//! Algedonic alerts — Variety deficit escalation
//!
//! Implements algedonic (pain/pleasure) feedback for cybernetic control.
//! When variety deficit exceeds threshold, alerts are escalated to the Curator/human.
//!
//! Per architecture v0.22.0: Variety deficit >50 → Warning escalation to Curator;
//! deficit >100 → Critical escalation to human. Binary threshold.

use crate::runtime::VarietyTracker;
use chrono::{DateTime, Utc};
use hkask_types::regulation::LedgerHealth;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{error, warn};

/// Default DateTime for serde deserialization
fn default_datetime() -> DateTime<Utc> {
    Utc::now()
}

/// Default expected variety per domain
pub(crate) const DEFAULT_EXPECTED_VARIETY: u64 = 10;

/// Fraction of `max_alerts` at which the approaching-cap signal fires.
/// 0.8 → 160 of 200. Gives the operator a window to review before eviction.
pub(crate) const ALERT_CAP_APPROACHING_FRACTION: f64 = 0.8;

/// Alert severity levels — simple binary threshold classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Informational - deficit detected but below threshold
    Info,
    /// Warning - deficit approaching threshold
    Warning,
    /// Critical - deficit exceeds threshold, escalation required
    Critical,
}

/// Algedonic alert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeAlert {
    pub domain: String,
    pub deficit: u64,
    pub threshold: u64,
    pub severity: AlertSeverity,
    pub escalated: bool,
    #[serde(default = "default_datetime")]
    pub timestamp: DateTime<Utc>,
    pub message: String,
}

/// Sink for sending algedonic alerts via email as a last-resort loop closure
/// when the live channel and persistence are both unavailable (S1->S5 algedonic
/// fallback path).
///
/// Implementations should be non-blocking — spawn async work internally
/// rather than blocking the cybernetics loop.
pub trait AlertEmailSink: Send + Sync + std::fmt::Debug {
    /// Send an alert email. Non-blocking — implementations should spawn
    /// async work rather than blocking the caller.
    ///
    /// Implementations should store a `tokio::runtime::Handle` and use
    /// `handle.spawn(...)` rather than bare `tokio::spawn(...)`, so the
    /// method is safe to call from any thread (including the GPUI
    /// foreground thread, which has no tokio reactor). The sole caller is
    /// `CyberneticsLoop::tick()`, which runs inside `Tokio::spawn`, but
    /// the `Send + Sync` bound on this trait means a future caller could
    /// invoke it from a non-tokio context.
    fn send_alert_email(&self, alert: &RuntimeAlert);
}

/// Sink for persisting algedonic alerts to the reviewable escalation queue.
///
/// This is the primary durable path for alert review: every escalated alert
/// is written here unconditionally (not just as a fallback), so the Curator
/// and user can review pending alerts via the `curator_escalations` MCP tool
/// and resolve/dismiss them with an audit trail. The `RegulationArchive`
/// (`RegulationSink`) remains as a secondary fallback for restart durability
/// when this queue is unavailable.
///
/// Implementations must be non-blocking and best-effort — a failing or missing
/// sink never breaks the regulation loop. The sole caller is
/// `CyberneticsLoop::act` / `verify_impact`, which runs inside `Tokio::spawn`.
pub trait AlertEscalationSink: Send + Sync {
    /// Persist an alert to the reviewable escalation queue.
    ///
    /// `output` is the human-readable alert message; `error_context` is a
    /// serialized JSON blob carrying the structured alert fields (domain,
    /// deficit, threshold, severity) for later triage. `confidence` is 1.0
    /// for Critical, 0.5 for Warning.
    ///
    /// Errors are logged by the caller and never propagated — alert
    /// persistence is best-effort, never a correctness path.
    fn persist_alert(&self, output: &str, confidence: f64, error_context: &str);

    /// Check whether a pending alert with the same condition as `output`
    /// already exists in the escalation queue.
    ///
    /// Used for deduplication at the source: the regulation loop senses the
    /// same deficit every cycle (e.g. an unwired efferent action) and would
    /// otherwise re-escalate every tick. Matching is on the condition key
    /// (`alert_condition` — the reason prefix before the " — " separator),
    /// not the full output: the per-cycle value embedded after the separator
    /// changes every tick, so exact-match dedup never hits for a
    /// persistently re-sensed condition. The caller checks this before
    /// routing an alert — if a pending alert with the same condition exists,
    /// the entire routing (log, live channel, persist, archive) is skipped.
    /// When the operator resolves or dismisses the original, the next cycle
    /// escalates again.
    ///
    /// Default returns `false` (no dedup). Implementations backed by a
    /// durable queue should query for pending alerts with this condition.
    /// Errors are logged by the caller and never propagated.
    fn has_pending_alert(&self, _output: &str) -> bool {
        false
    }

    /// Auto-resolve a pending escalation when the triggering condition has
    /// cleared.
    ///
    /// Called by `verify_impact` when an `Accept` ImpactReport is produced for
    /// a previously-escalated condition — the metric improved, so the
    /// escalation is stale. The implementation should resolve pending
    /// escalations matching the condition key of `output` (`alert_condition`)
    /// with the provided resolution note. Condition matching (not exact
    /// output matching) is required because the persisted escalation's
    /// embedded value differs from the reconstruction's — the two were
    /// sensed in different cycles.
    ///
    /// This closes the stuck-loop pattern: without auto-resolve, the loop
    /// senses a deviation, escalates it, the condition self-resolves, but the
    /// escalation sits in the queue until manual review — the loop spins
    /// indefinitely with zero effectiveness because `verify_impact` produces
    /// no ImpactReport for NoData actions.
    ///
    /// Default is a no-op (no auto-resolve). Implementations backed by a
    /// durable queue should resolve the matching pending escalation.
    /// Errors are logged by the caller and never propagated.
    fn auto_resolve_cleared(&self, _output: &str, _resolution_note: &str) {
        // No-op — auto-resolve is opt-in.
    }
}

impl RuntimeAlert {
    /// Create an alert using binary thresholds. Returns None if domain is empty
    /// or threshold is 0.
    ///
    /// expect: "The system creates algedonic alerts when variety deficit exceeds threshold"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — algedonic feedback loop
    /// \[P4\] Constraining: Clear Boundaries — cap enforcement through binary classification
    /// \[P5\] Constraining: Essentialism — simplest possible threshold model
    /// pre:  domain is non-empty, threshold > 0
    /// post: returns Some(RuntimeAlert) with severity based on deficit vs threshold,
    ///       or None if preconditions violated
    pub fn new(domain: &str, deficit: u64, threshold: u64) -> Option<Self> {
        if domain.is_empty() || threshold == 0 {
            return None;
        }

        let severity = if deficit > threshold {
            AlertSeverity::Critical
        } else if deficit > threshold / 2 {
            AlertSeverity::Warning
        } else {
            AlertSeverity::Info
        };

        let result = Self {
            domain: domain.to_string(),
            deficit,
            threshold,
            severity,
            escalated: severity == AlertSeverity::Critical,
            timestamp: Utc::now(),
            message: format!(
                "Variety deficit {} in domain '{}' (threshold: {})",
                deficit, domain, threshold
            ),
        };
        debug_assert!(
            (result.severity == AlertSeverity::Critical && deficit > threshold)
                || (result.severity == AlertSeverity::Warning
                    && deficit > threshold / 2
                    && deficit <= threshold)
                || (result.severity == AlertSeverity::Info && deficit <= threshold / 2),
            "severity must match deficit vs threshold"
        );
        Some(result)
    }

    /// Check if alert should be escalated.
    ///
    /// expect: "I can check whether an alert warrants escalation to the Curator"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — escalation feedback loop
    /// \[P4\] Constraining: Clear Boundaries — binary threshold boundary check
    /// post: returns true iff severity is Critical
    pub fn should_escalate(&self) -> bool {
        let result = self.escalated;
        debug_assert!(
            result == (self.severity == AlertSeverity::Critical),
            "result must match critical severity"
        );
        result
    }

    /// Check if alert is critical severity.
    ///
    /// expect: "I can check whether an alert has reached critical severity"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — critical threshold detection
    /// \[P4\] Constraining: Clear Boundaries — severity boundary check
    /// post: returns true iff severity == Critical
    pub fn is_critical(&self) -> bool {
        let result = self.severity == AlertSeverity::Critical;
        debug_assert!(
            result == (self.severity == AlertSeverity::Critical),
            "result must match critical severity"
        );
        result
    }

    /// Check if alert is warning severity.
    ///
    /// expect: "I can check whether an alert is at warning severity"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — warning threshold detection
    /// \[P4\] Constraining: Clear Boundaries — mid-range boundary check
    /// post: returns true iff severity == Warning
    pub fn is_warning(&self) -> bool {
        let result = self.severity == AlertSeverity::Warning;
        debug_assert!(
            result == (self.severity == AlertSeverity::Warning),
            "result must match warning severity"
        );
        result
    }
}

/// Algedonic alert manager
pub(crate) struct AlgedonicManager {
    threshold: u64,
    default_expected_variety: u64,
    expected_variety: HashMap<String, u64>,
    /// Diagnostic alert ring buffer. Capped at `max_alerts`; oldest entries
    /// are evicted on overflow. Escalated (Critical) alerts are persisted to
    /// the `EscalationQueue` separately, so eviction from this log loses only
    /// the diagnostic trail, not the actionable backlog.
    alerts: Vec<RuntimeAlert>,
    /// Maximum alerts retained before oldest are evicted.
    max_alerts: usize,
    /// Outcome success rate warning threshold. Falls back to DEFAULT_OUTCOME_WARNING_THRESHOLD.
    outcome_warning_threshold: f64,
    /// Outcome success rate critical threshold. Falls back to DEFAULT_OUTCOME_CRITICAL_THRESHOLD.
    outcome_critical_threshold: f64,
}

impl AlgedonicManager {
    /// Construct with a custom alert-log cap. Used by `with_set_points` to
    /// thread the operator-configured `max_alerts` through.
    pub(crate) fn with_max_alerts(
        threshold: u64,
        default_expected_variety: u64,
        max_alerts: usize,
    ) -> Self {
        let max_alerts = max_alerts.max(1);
        Self {
            threshold,
            default_expected_variety,
            expected_variety: HashMap::new(),
            alerts: Vec::with_capacity(max_alerts),
            max_alerts,
            outcome_warning_threshold: Self::DEFAULT_OUTCOME_WARNING_THRESHOLD,
            outcome_critical_threshold: Self::DEFAULT_OUTCOME_CRITICAL_THRESHOLD,
        }
    }

    /// Override the outcome quality thresholds from SetPointsConfig.
    ///
    /// expect: "The system escalates variety deficits through binary-threshold algedonic alerting"
    pub(crate) fn set_outcome_thresholds(&mut self, warning: f64, critical: f64) {
        self.outcome_warning_threshold = warning;
        self.outcome_critical_threshold = critical;
    }

    /// Set expected variety for a specific domain.
    ///
    /// expect: "The system escalates variety deficits through binary-threshold algedonic alerting"
    pub(crate) fn set_expected_variety(&mut self, domain: &str, expected: u64) {
        self.expected_variety.insert(domain.to_string(), expected);
    }

    /// Check variety counter and generate alert using binary thresholds.
    ///
    /// expect: "The system escalates variety deficits through binary-threshold algedonic alerting"
    /// pre: counter is a valid VarietyTracker; domain is non-empty
    /// post: returns Some(&RuntimeAlert) if the active domain's deficit
    ///       exceeds expected, None if healthy or idle (no observations
    ///       in the current window)
    pub(crate) fn check(
        &mut self,
        counter: &VarietyTracker,
        domain: &str,
    ) -> Option<&RuntimeAlert> {
        // Idle gate: a domain with no observations in the current window
        // is at rest, not deficient. Without this gate every check on an
        // idle domain pushes a max-deficit alert into the diagnostic log,
        // growing it by the full expected variety per idle domain per
        // cycle.
        if counter.variety() == 0 {
            return None;
        }
        let expected = self
            .expected_variety
            .get(domain)
            .copied()
            .unwrap_or(self.default_expected_variety);
        let deficit = counter.deficit(expected);

        let alert = RuntimeAlert::new(domain, deficit, self.threshold)
            .unwrap_or_else(|| {
                // Preconditions violated (empty domain or zero threshold);
                // create a safe fallback Info alert.
                RuntimeAlert {
                    domain: domain.to_string(),
                    deficit,
                    threshold: self.threshold.max(1),
                    severity: AlertSeverity::Info,
                    escalated: false,
                    timestamp: Utc::now(),
                    message: format!(
                        "Variety deficit {} in domain '{}' (threshold: {} — fallback, preconditions violated)",
                        deficit, domain, self.threshold.max(1)
                    ),
                }
            });

        if alert.should_escalate() {
            error!(
                target: "reg.alert",
                domain = %alert.domain,
                deficit = alert.deficit,
                threshold = alert.threshold,
                "ALGEDONIC ALERT - Escalation required"
            );
        } else if alert.is_warning() {
            warn!(
                target: "reg.alert",
                domain = %alert.domain,
                deficit = alert.deficit,
                "Variety deficit approaching threshold"
            );
        }

        self.push_alert(alert);
        self.alerts.last()
    }

    /// Get the configured default threshold.
    ///
    /// expect: "The system escalates variety deficits through binary-threshold algedonic alerting"
    pub(crate) fn default_threshold(&self) -> u64 {
        self.threshold
    }

    /// Get all alerts.
    ///
    /// expect: "The system escalates variety deficits through binary-threshold algedonic alerting"
    pub(crate) fn alerts(&self) -> &[RuntimeAlert] {
        &self.alerts
    }

    /// Get critical alerts only.
    ///
    /// expect: "The system escalates variety deficits through binary-threshold algedonic alerting"
    pub(crate) fn critical_alerts(&self) -> Vec<&RuntimeAlert> {
        self.alerts.iter().filter(|a| a.is_critical()).collect()
    }

    /// Current total variety deficit across live trackers — a level, not an
    /// accumulation.
    ///
    /// Each tracked domain contributes `expected − observed` distinct states
    /// for the current window; idle domains (empty window) contribute zero
    /// via `VarietyTracker::deficit`. This is what `LedgerHealth::
    /// overall_deficit` reports. The previous implementation summed the
    /// alert log, whose entries accumulate one per check cycle — a
    /// monotonically growing integral that no threshold could ever clear,
    /// and a channel through which outcome-quality alerts (deficit =
    /// failure-rate %) leaked into the variety metric. The log remains
    /// diagnostics-only.
    ///
    /// expect: "The system escalates variety deficits through binary-threshold algedonic alerting"
    pub(crate) fn current_total_deficit(&self, counters: &HashMap<String, VarietyTracker>) -> u64 {
        counters
            .iter()
            .map(|(domain, tracker)| {
                let expected = self
                    .expected_variety
                    .get(domain)
                    .copied()
                    .unwrap_or(self.default_expected_variety);
                tracker.deficit(expected)
            })
            .sum()
    }

    // ── Outcome Quality Checking ──

    /// Default outcome success rate warning threshold (50%).
    pub(crate) const DEFAULT_OUTCOME_WARNING_THRESHOLD: f64 = 0.50;
    /// Default outcome success rate critical threshold (25%).
    pub(crate) const DEFAULT_OUTCOME_CRITICAL_THRESHOLD: f64 = 0.25;

    /// Check outcome quality and generate alert if success rate is degraded.
    ///
    /// Uses binary thresholds on success_rate (higher is better, so we invert):
    /// - success_rate < critical_threshold → Critical
    /// - success_rate < warning_threshold → Warning
    /// - success_rate ≥ warning_threshold → Info (healthy)
    ///
    /// Thresholds come from the instance fields (defaulting to 0.50/0.25),
    /// which can be overridden via `set_outcome_thresholds()` from SetPointsConfig.
    ///
    /// expect: "The system escalates variety deficits through binary-threshold algedonic alerting"
    pub(crate) fn check_outcome(
        &mut self,
        domain: &str,
        success_rate: f64,
        total_ops: u64,
    ) -> Option<&RuntimeAlert> {
        let severity = if success_rate < self.outcome_critical_threshold {
            AlertSeverity::Critical
        } else if success_rate < self.outcome_warning_threshold {
            AlertSeverity::Warning
        } else {
            return None; // Healthy — no alert needed
        };

        let alert = RuntimeAlert {
            domain: format!("outcome:{domain}"),
            deficit: ((1.0 - success_rate) * 100.0) as u64, // failure rate as "deficit"
            threshold: ((1.0 - self.outcome_warning_threshold) * 100.0) as u64,
            severity,
            escalated: severity == AlertSeverity::Critical,
            timestamp: Utc::now(),
            message: format!(
                "Outcome success rate {:.1}% in domain '{}' ({} operations, {} failures)",
                success_rate * 100.0,
                domain,
                total_ops,
                total_ops.saturating_sub((success_rate * total_ops as f64) as u64),
            ),
        };

        if alert.should_escalate() {
            error!(
                target: "reg.outcome",
                domain = %domain,
                success_rate = %format!("{:.1}%", success_rate * 100.0),
                total_ops = total_ops,
                "OUTCOME ALERT - Critical failure rate"
            );
        } else {
            warn!(
                target: "reg.outcome",
                domain = %domain,
                success_rate = %format!("{:.1}%", success_rate * 100.0),
                total_ops = total_ops,
                "Outcome success rate degraded"
            );
        }

        self.push_alert(alert);
        self.alerts.last()
    }

    /// Push an alert onto the log, evicting the oldest entry if the cap is
    /// reached. Called by both `check` and `check_outcome` — the single
    /// chokepoint for log growth.
    fn push_alert(&mut self, alert: RuntimeAlert) {
        if self.alerts.len() >= self.max_alerts {
            self.alerts.remove(0);
        }
        self.alerts.push(alert);
    }

    /// Number of alerts currently in the log.
    pub(crate) fn alert_count(&self) -> usize {
        self.alerts.len()
    }

    /// Number of escalated alerts currently in the log — routed toward the
    /// durable `EscalationQueue` but not yet resolved. The cybernetics loop
    /// senses this as `PendingEscalations`.
    pub(crate) fn escalated_alert_count(&self) -> usize {
        self.alerts.iter().filter(|alert| alert.escalated).count()
    }

    /// The configured alert-log cap.
    pub(crate) fn max_alerts(&self) -> usize {
        self.max_alerts
    }

    /// Whether the alert log is approaching the cap (≥
    /// `ALERT_CAP_APPROACHING_FRACTION * max_alerts`). When true, the
    /// cybernetics loop should emit an `AlgedonicLogApproachingCap` signal
    /// so the operator (or the `algedonic-review` skill) can review and
    /// clear reviewed entries before they are evicted unread.
    pub(crate) fn log_approaching_cap(&self) -> bool {
        let threshold = (self.max_alerts as f64 * ALERT_CAP_APPROACHING_FRACTION) as usize;
        self.alerts.len() >= threshold
    }

    /// Clear reviewed alerts from the log. Called by the `algedonic-review`
    /// skill (via `RegulationLedger::clear_reviewed_alerts`) after the
    /// operator has reviewed the log and the escalated alerts have been
    /// persisted to the `EscalationQueue`. Retains unresolved Critical
    /// alerts that have not yet been persisted to the escalation queue —
    /// clearing those would lose the live signal.
    ///
    /// `retain_unresolved` controls what survives: when `true` (the default
    /// from `RegulationLedger`), only Info and Warning alerts and already-
    /// escalated Critical alerts are cleared. When `false`, the entire log
    /// is cleared (used by `session_reset`).
    pub(crate) fn clear_reviewed(&mut self, retain_unresolved: bool) {
        if retain_unresolved {
            // Retain Critical alerts that have not been escalated yet —
            // these are the live signals the operator needs to act on.
            // Everything else (Info, Warning, escalated Critical) has been
            // reviewed or persisted and can be cleared.
            self.alerts.retain(|a| a.is_critical() && !a.escalated);
        } else {
            self.alerts.clear();
        }
    }
}

/// Construct LedgerHealth from the algedonic manager's current state.
///
/// `overall_deficit` is the caller-computed current level from the live
/// variety trackers (`AlgedonicManager::current_total_deficit`) — not the
/// manager's alert history, which accumulates and can only grow.
pub(crate) fn reg_health_check(
    manager: &AlgedonicManager,
    variety_ema: f64,
    overall_deficit: u64,
) -> LedgerHealth {
    LedgerHealth {
        overall_deficit,
        critical_count: manager.critical_alerts().len(),
        warning_count: manager.alerts().iter().filter(|a| a.is_warning()).count(),
        healthy: manager.critical_alerts().is_empty(),
        variety_ema,
        alert_log_count: manager.alert_count(),
        alert_log_cap: manager.max_alerts(),
        alert_log_approaching_cap: manager.log_approaching_cap(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::VarietyTracker;
    use std::collections::HashMap;

    fn manager() -> AlgedonicManager {
        AlgedonicManager::with_max_alerts(20, DEFAULT_EXPECTED_VARIETY, 200)
    }

    /// Idle domains produce no alert and no log growth — a domain at rest
    /// is not variety-deficient. Pins the idle gate in `check`.
    #[test]
    fn check_returns_none_for_idle_domain() {
        let mut mgr = manager();
        let idle = VarietyTracker::new();
        assert!(mgr.check(&idle, "media").is_none());
        assert_eq!(mgr.alert_count(), 0, "idle check must not push an alert");
    }

    /// `VarietyTracker::deficit` reads zero for an idle window even against
    /// a non-zero expected variety.
    #[test]
    fn deficit_is_zero_for_idle_window() {
        let tracker = VarietyTracker::new();
        assert_eq!(tracker.deficit(DEFAULT_EXPECTED_VARIETY), 0);
    }

    /// An active domain with fewer distinct states than expected reports
    /// the gap — the genuine Ashby signal the sensor exists to carry.
    #[test]
    fn deficit_reports_gap_for_active_domain() {
        let mut tracker = VarietyTracker::new();
        tracker.increment("state_a");
        tracker.increment("state_b");
        assert_eq!(tracker.deficit(10), 8);
    }

    /// `current_total_deficit` is a level: it reports the live per-domain
    /// gaps and does not grow as alerts accumulate in the log. This pins
    /// the fix for the monotonic log-sum that made every threshold trip
    /// forever.
    #[test]
    fn current_total_deficit_is_a_level_not_a_log_sum() {
        let mut mgr = manager();
        let mut active = VarietyTracker::new();
        active.increment("state_a");
        active.increment("state_b");
        let counters = HashMap::from([
            ("active".to_string(), active.clone()),
            ("idle".to_string(), VarietyTracker::new()),
        ]);

        // Re-check the active domain many times — each check pushes an
        // alert into the diagnostic log, but the current deficit must not
        // move.
        for _ in 0..10 {
            mgr.check(&active, "active");
        }
        assert!(mgr.alert_count() > 0, "sanity: the log grew");
        assert_eq!(
            mgr.current_total_deficit(&counters),
            8,
            "deficit is the live gap (10 expected − 2 observed), not the log sum"
        );
    }

    /// `reg_health_check` reports the caller-computed level, not the
    /// manager's alert history.
    #[test]
    fn reg_health_check_reports_passed_deficit() {
        let mut mgr = manager();
        let mut tracker = VarietyTracker::new();
        tracker.increment("state_a");
        let counters = HashMap::from([("domain".to_string(), tracker.clone())]);
        mgr.check(&tracker, "domain");

        let health = reg_health_check(&mgr, 0.0, mgr.current_total_deficit(&counters));
        assert_eq!(health.overall_deficit, 9);
    }
}
