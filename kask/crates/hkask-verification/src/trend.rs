//! Grounding trend report and query scope.
//!
//! The trend report answers the paper's §4.1 question: "is this getting
//! better?" It is aggregated from `GroundingRecord`s in the central ledger
//! by `VerificationStore::grounding_trend`.
//!
//! The lead metric is `delegations_with_zero_nulled` — deletion-resistant
//! (paper Rule 5.4: a scoreboard that counts nulled fields falling can be
//! gamed by recording fewer delegations; counting delegations with zero
//! nulled fields cannot).

use serde::{Deserialize, Serialize};

/// The scope for a grounding trend query. The central ledger is cross-tool
/// and cross-server, so a single query can aggregate across every delegation
/// source or filter to one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendScope {
    /// All delegations across all tools and agents.
    #[default]
    Global,
    /// Delegations for a specific agent.
    ByAgent(String),
    /// Delegations from a specific source tool (e.g. "kanban_task_spawn",
    /// "swarm_delegate_local").
    BySource(String),
}

/// A grounding trend report aggregated across delegations.
///
/// `delegations_without_contract` is the coverage gap (paper §6: coverage
/// is itself a metric, not a pass). A delegation with `had_contract: false`
/// is a coverage gap, not a compliant delegation.
///
/// `delegations_unenforceable` is the "contract existed but could not run"
/// bucket — the contract matched but the output was not a JSON object.
/// The operator's remediation is to fix the agent's system prompt, not to
/// write a contract (paper Rule 5.3: absence ≠ verdict).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundingTrendReport {
    /// Total delegations recorded.
    pub total_delegations: usize,
    /// Delegations for which a grounding contract existed (enforced +
    /// unenforceable). The denominator for `coverage_rate`.
    pub delegations_with_contract: usize,
    /// Delegations for which no grounding contract existed (coverage gap).
    pub delegations_without_contract: usize,
    /// Delegations where the contract existed but grounding could not run
    /// (non-JSON output). `had_contract: true, was_enforced: false`.
    /// Neither zero_nulled nor nulled — absence ≠ verdict (Rule 5.3).
    pub delegations_unenforceable: usize,
    /// Delegations where grounding ran and zero fields were nulled.
    /// The deletion-resistant scoreboard metric (paper Rule 5.4).
    pub delegations_with_zero_nulled: usize,
    /// Delegations where grounding ran and at least one field was nulled.
    pub delegations_with_nulled: usize,
    /// Delegations where grounding ran and at least one narrative leak was
    /// detected.
    pub delegations_with_narrative_leaks: usize,
}

impl GroundingTrendReport {
    /// Fraction of grounded delegations (contract ran) with zero nulled
    /// fields. `None` when no grounded delegations exist (absence ≠ 0 —
    /// paper Rule 5.3).
    pub fn clean_rate(&self) -> Option<f64> {
        let measured = self.delegations_with_zero_nulled + self.delegations_with_nulled;
        if measured == 0 {
            return None;
        }
        Some(self.delegations_with_zero_nulled as f64 / measured as f64)
    }

    /// Fraction of delegations that had a grounding contract. `None` when
    /// no delegations exist.
    pub fn coverage_rate(&self) -> Option<f64> {
        if self.total_delegations == 0 {
            return None;
        }
        Some(self.delegations_with_contract as f64 / self.total_delegations as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_report_clean_rate_none_when_no_measured_delegations() {
        // Absence ≠ 0 (paper Rule 5.3): when no delegations have measured
        // counts, clean_rate is None, not 0.0.
        let report = GroundingTrendReport {
            total_delegations: 2,
            delegations_with_contract: 2,
            delegations_without_contract: 0,
            delegations_unenforceable: 2,
            delegations_with_zero_nulled: 0,
            delegations_with_nulled: 0,
            delegations_with_narrative_leaks: 0,
        };
        assert_eq!(report.clean_rate(), None);
    }

    #[test]
    fn trend_report_clean_rate_some_when_measured() {
        let report = GroundingTrendReport {
            total_delegations: 4,
            delegations_with_contract: 3,
            delegations_without_contract: 1,
            delegations_unenforceable: 0,
            delegations_with_zero_nulled: 2,
            delegations_with_nulled: 1,
            delegations_with_narrative_leaks: 1,
        };
        // clean_rate = 2 / (2 + 1) = 2/3
        assert!((report.clean_rate().unwrap() - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn trend_report_coverage_rate_none_when_no_delegations() {
        let report = GroundingTrendReport::default();
        assert_eq!(report.coverage_rate(), None);
    }

    #[test]
    fn trend_report_coverage_rate_some_when_delegations_exist() {
        let report = GroundingTrendReport {
            total_delegations: 4,
            delegations_with_contract: 3,
            delegations_without_contract: 1,
            ..Default::default()
        };
        assert!((report.coverage_rate().unwrap() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn trend_report_default_is_all_zeros() {
        let report = GroundingTrendReport::default();
        assert_eq!(report.total_delegations, 0);
        assert_eq!(report.delegations_with_contract, 0);
        assert_eq!(report.delegations_without_contract, 0);
        assert_eq!(report.delegations_unenforceable, 0);
        assert_eq!(report.delegations_with_zero_nulled, 0);
        assert_eq!(report.delegations_with_nulled, 0);
        assert_eq!(report.delegations_with_narrative_leaks, 0);
    }
}
