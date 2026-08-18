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

/// Default maximum alerts retained in the in-memory `AlgedonicManager.alerts`
/// log before oldest entries are evicted. Bounds memory growth in long-running
/// sessions — the log is a diagnostic ring buffer, not an audit archive
/// (escalated alerts are persisted to the `EscalationQueue` for durable review).
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
    #[allow(dead_code)]
    pub(crate) fn new(threshold: u64, default_expected_variety: u64) -> Self {
        Self::with_max_alerts(
            threshold,
            default_expected_variety,
            crate::set_points::DEFAULT_MAX_ALERTS,
        )
    }

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
    /// post: returns Some(&RuntimeAlert) if deficit exceeds expected, None if healthy
    pub(crate) fn check(
        &mut self,
        counter: &VarietyTracker,
        domain: &str,
    ) -> Option<&RuntimeAlert> {
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

    /// Get total deficit across all alerts.
    ///
    /// expect: "The system escalates variety deficits through binary-threshold algedonic alerting"
    pub(crate) fn total_deficit(&self) -> u64 {
        self.alerts.iter().map(|a| a.deficit).sum()
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
    #[allow(dead_code)]
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
pub(crate) fn reg_health_check(manager: &AlgedonicManager, variety_ema: f64) -> LedgerHealth {
    LedgerHealth {
        overall_deficit: manager.total_deficit(),
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

    //
    // TASK 1 cybernetic property: when deficit exceeds threshold, severity
    // must be Critical. When deficit > threshold/2 but ≤ threshold, severity
    // must be Warning. When deficit ≤ threshold/2, severity must be Info.
    #[test]
    fn binary_threshold_classifies_critical_and_warning() {
        let threshold = 100;

        // deficit = 150 → > threshold → Critical
        let critical = RuntimeAlert::new("test", 150, threshold).unwrap();
        assert_eq!(critical.severity, AlertSeverity::Critical);
        assert!(critical.escalated);

        // deficit = 75 → > threshold/2 but ≤ threshold → Warning
        let warning = RuntimeAlert::new("test", 75, threshold).unwrap();
        assert_eq!(warning.severity, AlertSeverity::Warning);
        assert!(!warning.escalated);

        // deficit = 25 → ≤ threshold/2 → Info
        let info = RuntimeAlert::new("test", 25, threshold).unwrap();
        assert_eq!(info.severity, AlertSeverity::Info);
        assert!(!info.escalated);
    }

    //
    // TASK 1 cybernetic property: AlgedonicManager must track variety per domain
    // independently, so a deficit in one domain does not suppress alerts in another.
    #[test]
    fn algedonic_manager_accumulates_alerts_across_domains() {
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 200);

        // Domain A: low variety (5 distinct states, expected 10 → deficit 5)
        let mut tracker_a = VarietyTracker::new();
        for i in 0..5 {
            tracker_a.increment(&format!("state_{}", i));
        }

        // Domain B: very low variety (1 distinct state, expected 10 → deficit 9)
        let mut tracker_b = VarietyTracker::new();
        tracker_b.increment("only_state");

        mgr.check(&tracker_a, "domain_a");
        mgr.check(&tracker_b, "domain_b");

        // Both domains should have alerts
        assert!(
            !mgr.alerts().is_empty(),
            "Should accumulate alerts per domain"
        );
        // Domain B should be more severe (higher deficit)
        let total = mgr.total_deficit();
        assert!(total >= 5 + 9, "Total deficit should reflect both domains");
    }

    //
    // Outcome quality tracking: success_rate < 0.25 → Critical,
    // < 0.50 → Warning, ≥ 0.50 → healthy (no alert).
    #[test]
    fn check_outcome_classifies_success_rate_correctly() {
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 200);

        // Critical: 20% success rate (80% failure)
        let alert = mgr.check_outcome("test_domain", 0.20, 10);
        assert!(alert.is_some(), "20% success rate should trigger alert");
        assert_eq!(alert.unwrap().severity, AlertSeverity::Critical);

        // Warning: 40% success rate (60% failure)
        let alert = mgr.check_outcome("test_domain", 0.40, 10);
        assert!(alert.is_some(), "40% success rate should trigger alert");
        assert_eq!(alert.unwrap().severity, AlertSeverity::Warning);

        // Healthy: 60% success rate
        let alert = mgr.check_outcome("test_domain", 0.60, 10);
        assert!(alert.is_none(), "60% success rate should be healthy");

        // Healthy: 100% success rate
        let alert = mgr.check_outcome("test_domain", 1.0, 10);
        assert!(alert.is_none(), "100% success rate should be healthy");
    }

    #[test]
    fn check_outcome_alert_message_includes_domain_and_rate() {
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 200);
        let alert = mgr.check_outcome("hkask-mcp-research", 0.15, 20).unwrap();
        assert!(alert.message.contains("hkask-mcp-research"));
        assert!(alert.message.contains("15.0%"));
        assert!(alert.message.contains("20 operations"));
        assert_eq!(alert.severity, AlertSeverity::Critical);
    }

    #[test]
    fn check_outcome_domain_prefixed_with_outcome() {
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 200);
        let alert = mgr.check_outcome("tool", 0.10, 10).unwrap();
        assert!(alert.domain.starts_with("outcome:"));
        assert!(alert.domain.contains("tool"));
    }

    #[test]
    fn set_outcome_thresholds_overrides_defaults() {
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 200);
        // Set custom thresholds: warning at 0.80, critical at 0.60
        mgr.set_outcome_thresholds(0.80, 0.60);

        // 70% success → below custom warning (0.80) but above custom critical (0.60) → Warning
        let alert = mgr.check_outcome("test", 0.70, 10);
        assert!(
            alert.is_some(),
            "70% should trigger warning with custom thresholds"
        );
        assert_eq!(alert.unwrap().severity, AlertSeverity::Warning);

        // 50% success → below custom critical (0.60) → Critical
        let alert = mgr.check_outcome("test", 0.50, 10);
        assert!(
            alert.is_some(),
            "50% should trigger critical with custom thresholds"
        );
        assert_eq!(alert.unwrap().severity, AlertSeverity::Critical);

        // 85% success → above custom warning (0.80) → healthy
        let alert = mgr.check_outcome("test", 0.85, 10);
        assert!(
            alert.is_none(),
            "85% should be healthy with custom thresholds"
        );
    }

    // ── Alert cap tests ──────────────────────────────────────────────────

    #[test]
    fn alert_log_caps_at_max_alerts() {
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 5);
        let mut tracker = VarietyTracker::new();
        tracker.increment("state");

        // Push 10 alerts into a cap-5 log.
        for _ in 0..10 {
            mgr.check(&tracker, "domain");
        }

        // The log must not exceed the cap.
        assert_eq!(mgr.alert_count(), 5, "log must cap at max_alerts");
        assert_eq!(mgr.max_alerts(), 5);
    }

    #[test]
    fn alert_log_evicts_oldest_on_overflow() {
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 3);

        // Push 4 alerts — the first should be evicted.
        for i in 0..4 {
            let mut t = VarietyTracker::new();
            t.increment(&format!("state_{i}"));
            mgr.check(&t, &format!("domain_{i}"));
        }

        // The log should have the last 3 alerts (domains 1, 2, 3).
        assert_eq!(mgr.alert_count(), 3);
        let domains: Vec<&str> = mgr.alerts().iter().map(|a| a.domain.as_str()).collect();
        assert!(
            !domains.contains(&"domain_0"),
            "oldest alert must be evicted"
        );
        assert!(
            domains.contains(&"domain_3"),
            "newest alert must be retained"
        );
    }

    #[test]
    fn log_approaching_cap_fires_at_80_percent() {
        // Cap = 10, approaching threshold = 8 (80%).
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 10);
        let mut tracker = VarietyTracker::new();
        tracker.increment("state");

        // 7 alerts → not approaching (7 < 8).
        for _ in 0..7 {
            mgr.check(&tracker, "domain");
        }
        assert!(!mgr.log_approaching_cap(), "7/10 should not be approaching");

        // 8th alert → approaching (8 >= 8).
        mgr.check(&tracker, "domain");
        assert!(mgr.log_approaching_cap(), "8/10 should be approaching");
    }

    #[test]
    fn clear_reviewed_retains_unresolved_critical() {
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 200);

        // Push a mix: Info, Warning, escalated Critical, un-escalated Critical.
        let info = RuntimeAlert {
            domain: "info_domain".to_string(),
            deficit: 10,
            threshold: 100,
            severity: AlertSeverity::Info,
            escalated: false,
            timestamp: Utc::now(),
            message: "info".to_string(),
        };
        let warning = RuntimeAlert {
            domain: "warning_domain".to_string(),
            deficit: 60,
            threshold: 100,
            severity: AlertSeverity::Warning,
            escalated: false,
            timestamp: Utc::now(),
            message: "warning".to_string(),
        };
        let escalated_critical = RuntimeAlert {
            domain: "escalated_critical".to_string(),
            deficit: 150,
            threshold: 100,
            severity: AlertSeverity::Critical,
            escalated: true,
            timestamp: Utc::now(),
            message: "escalated critical".to_string(),
        };
        let unresolved_critical = RuntimeAlert {
            domain: "unresolved_critical".to_string(),
            deficit: 150,
            threshold: 100,
            severity: AlertSeverity::Critical,
            escalated: false,
            timestamp: Utc::now(),
            message: "unresolved critical".to_string(),
        };
        mgr.push_alert(info);
        mgr.push_alert(warning);
        mgr.push_alert(escalated_critical);
        mgr.push_alert(unresolved_critical);

        // Clear reviewed — retain unresolved Critical (not escalated).
        mgr.clear_reviewed(true);

        // Only the unresolved Critical should survive.
        assert_eq!(mgr.alert_count(), 1);
        assert_eq!(mgr.alerts()[0].domain, "unresolved_critical");
    }

    #[test]
    fn clear_reviewed_false_clears_all() {
        let mut mgr = AlgedonicManager::with_max_alerts(100, 10, 200);
        let mut tracker = VarietyTracker::new();
        tracker.increment("state");
        mgr.check(&tracker, "domain");
        assert!(!mgr.alerts().is_empty());

        mgr.clear_reviewed(false);
        assert!(
            mgr.alerts().is_empty(),
            "retain_unresolved=false must clear all"
        );
    }
}
