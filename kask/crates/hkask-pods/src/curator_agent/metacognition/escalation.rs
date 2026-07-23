//! Escalation domain types: thresholds, triggers, alerts, and policy.
//! Severity is reused from `hkask_types::curator::EscalationSeverity`.

use std::sync::Arc;

use hkask_types::curator::EscalationSeverity;

/// Default variety deficit threshold for escalation.
pub(crate) const DEFAULT_ESCALATION_VARIETY_DEFICIT: u64 = 100;

/// Default critical alert count threshold for escalation.
pub(crate) const DEFAULT_ESCALATION_CRITICAL_ALERTS: usize = 3;

/// Default bot failure count threshold for escalation.
pub(crate) const DEFAULT_ESCALATION_BOT_FAILURES: usize = 2;

/// Escalation trigger thresholds.
#[derive(Debug, Clone)]
pub(crate) struct EscalationThresholds {
    pub variety_deficit: u64,
    pub critical_alerts: usize,
    pub bot_failures: usize,
}

impl Default for EscalationThresholds {
    fn default() -> Self {
        Self {
            variety_deficit: DEFAULT_ESCALATION_VARIETY_DEFICIT,
            critical_alerts: DEFAULT_ESCALATION_CRITICAL_ALERTS,
            bot_failures: DEFAULT_ESCALATION_BOT_FAILURES,
        }
    }
}

/// The trigger that caused an escalation alert.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EscalationTrigger {
    /// Variety deficit exceeded a threshold.
    VarietyDeficit,
    /// Critical alert count exceeded a threshold.
    CriticalAlerts,
    /// Bot failure count exceeded a threshold.
    BotFailures,
}

/// Alert produced when a threshold is breached.
#[derive(Debug, Clone)]
pub struct EscalationAlert {
    pub trigger: EscalationTrigger,
    pub value: f64,
    pub threshold: f64,
    pub severity: EscalationSeverity,
}

/// Encapsulates escalation threshold logic — independently testable.
/// Algedonic: Warning at threshold/2, Critical at threshold.
pub struct EscalationPolicy {
    thresholds: Arc<std::sync::RwLock<EscalationThresholds>>,
}

impl EscalationPolicy {
    pub(crate) fn new(thresholds: EscalationThresholds) -> Self {
        Self {
            thresholds: Arc::new(std::sync::RwLock::new(thresholds)),
        }
    }

    /// Check all escalation conditions, return active alerts.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — escalation policy classifies variety deficit
    /// \[P4\] Constraining: Clear Boundaries — thresholds define explicit boundaries
    /// pre:  `variety_deficit`, `critical_alerts`, `bot_failures` are
    ///       non-negative numeric values.
    /// post: Returns a `Vec<EscalationAlert>` containing alerts for any
    ///       threshold exceeded: VarietyDeficit (Critical if > threshold,
    ///       Warning if > threshold/2), CriticalAlerts (Critical if ≥
    ///       threshold), BotFailures (Critical if ≥ threshold).
    pub fn check_conditions(
        &self,
        variety_deficit: f64,
        critical_alerts: u64,
        bot_failures: u64,
    ) -> Vec<EscalationAlert> {
        let mut alerts = Vec::new();
        let t = self
            .thresholds
            .read()
            .expect("escalation thresholds lock poisoned");

        let variety_threshold = t.variety_deficit as f64;
        if variety_deficit > variety_threshold {
            alerts.push(EscalationAlert {
                trigger: EscalationTrigger::VarietyDeficit,
                value: variety_deficit,
                threshold: variety_threshold,
                severity: EscalationSeverity::Critical,
            });
        } else if variety_deficit > variety_threshold / 2.0 {
            alerts.push(EscalationAlert {
                trigger: EscalationTrigger::VarietyDeficit,
                value: variety_deficit,
                threshold: variety_threshold,
                severity: EscalationSeverity::Warning,
            });
        }

        let critical_alerts_threshold = t.critical_alerts as f64;
        if critical_alerts >= t.critical_alerts as u64 {
            alerts.push(EscalationAlert {
                trigger: EscalationTrigger::CriticalAlerts,
                value: critical_alerts as f64,
                threshold: critical_alerts_threshold,
                severity: EscalationSeverity::Critical,
            });
        }

        let bot_failures_threshold = t.bot_failures as f64;
        if bot_failures >= t.bot_failures as u64 {
            alerts.push(EscalationAlert {
                trigger: EscalationTrigger::BotFailures,
                value: bot_failures as f64,
                threshold: bot_failures_threshold,
                severity: EscalationSeverity::Critical,
            });
        }

        alerts
    }

    /// Read the current thresholds (for self-calibration old/new reporting).
    #[must_use]
    pub(crate) fn thresholds(&self) -> EscalationThresholds {
        self.thresholds
            .read()
            .expect("escalation thresholds lock poisoned")
            .clone()
    }

    /// Replace the thresholds — used by metacognition self-management to
    /// adjust the Curator's own sensitivity from observed decision quality.
    pub(crate) fn set_thresholds(&self, thresholds: EscalationThresholds) {
        *self
            .thresholds
            .write()
            .expect("escalation thresholds lock poisoned") = thresholds;
    }
}

impl Default for EscalationPolicy {
    fn default() -> Self {
        Self::new(EscalationThresholds::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_are_adjustable_at_runtime() {
        let policy = EscalationPolicy::default();
        let original = policy.thresholds();
        assert_eq!(original.variety_deficit, DEFAULT_ESCALATION_VARIETY_DEFICIT);

        // Self-calibration raises the variety-deficit threshold.
        let mut next = original.clone();
        next.variety_deficit = original.variety_deficit + 10;
        policy.set_thresholds(next);

        let after = policy.thresholds();
        assert_eq!(
            after.variety_deficit,
            DEFAULT_ESCALATION_VARIETY_DEFICIT + 10
        );
        // Other thresholds preserved.
        assert_eq!(after.critical_alerts, original.critical_alerts);
    }

    #[test]
    fn check_conditions_uses_live_thresholds() {
        let policy = EscalationPolicy::default();
        // At default threshold (100), a deficit of 101 is Critical.
        assert!(
            policy
                .check_conditions(101.0, 0, 0)
                .iter()
                .any(|a| a.trigger == EscalationTrigger::VarietyDeficit
                    && a.severity == EscalationSeverity::Critical)
        );

        // Raise the threshold to 200 — now 101 is below Critical (200) but above
        // Warning (100), so it produces a Warning, not Critical. This proves
        // check_conditions reads the live (adjusted) threshold.
        let mut next = policy.thresholds();
        next.variety_deficit = 200;
        policy.set_thresholds(next);
        let alerts = policy.check_conditions(101.0, 0, 0);
        assert!(
            alerts
                .iter()
                .any(|a| a.trigger == EscalationTrigger::VarietyDeficit
                    && a.severity == EscalationSeverity::Warning)
        );
        assert!(
            !alerts
                .iter()
                .any(|a| a.severity == EscalationSeverity::Critical)
        );
    }
}
