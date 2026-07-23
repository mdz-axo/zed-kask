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
    fn send_alert_email(&self, alert: &RuntimeAlert);
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
    alerts: Vec<RuntimeAlert>,
    /// Outcome success rate warning threshold. Falls back to DEFAULT_OUTCOME_WARNING_THRESHOLD.
    outcome_warning_threshold: f64,
    /// Outcome success rate critical threshold. Falls back to DEFAULT_OUTCOME_CRITICAL_THRESHOLD.
    outcome_critical_threshold: f64,
}

impl AlgedonicManager {
    pub(crate) fn new(threshold: u64, default_expected_variety: u64) -> Self {
        Self {
            threshold,
            default_expected_variety,
            expected_variety: HashMap::new(),
            alerts: Vec::new(),
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

        self.alerts.push(alert);
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

        self.alerts.push(alert);
        self.alerts.last()
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
        let mut mgr = AlgedonicManager::new(100, 10);

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
        let mut mgr = AlgedonicManager::new(100, 10);

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
        let mut mgr = AlgedonicManager::new(100, 10);
        let alert = mgr.check_outcome("hkask-mcp-research", 0.15, 20).unwrap();
        assert!(alert.message.contains("hkask-mcp-research"));
        assert!(alert.message.contains("15.0%"));
        assert!(alert.message.contains("20 operations"));
        assert_eq!(alert.severity, AlertSeverity::Critical);
    }

    #[test]
    fn check_outcome_domain_prefixed_with_outcome() {
        let mut mgr = AlgedonicManager::new(100, 10);
        let alert = mgr.check_outcome("tool", 0.10, 10).unwrap();
        assert!(alert.domain.starts_with("outcome:"));
        assert!(alert.domain.contains("tool"));
    }

    #[test]
    fn set_outcome_thresholds_overrides_defaults() {
        let mut mgr = AlgedonicManager::new(100, 10);
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
}
