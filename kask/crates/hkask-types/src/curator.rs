//! Curator types — CuratorHandle (capability handle) and CuratorDirective (Curation → Cybernetics).
//!
//! Lives in hkask-types as the single source of truth for curator types.
//!
//! Per the Authority DAG: Curation (Loop 5) → Cybernetics (Loop 6).
//! The Curator's types must live in a crate that both Curator (hkask-agents)
//! and Cybernetics (hkask-regulation) can depend on without inversion.
//!
//! NOTE: hkask-types must NOT depend on hkask-mcp (cycle prevention —
//! the ToolPort dispatch types live there since the Q6 fold).
//! Methods that need capability tokens (e.g., `issue_consolidation_token`) live
//! as free functions or extension traits in crates that have the capability dep.

use crate::id::WebID;

// ── CuratorDirective — Curation → Cybernetics directives ────────────────────

/// Severity level for domain escalations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationSeverity {
    /// Informational — no action needed.
    Info,
    /// Warning — attention recommended.
    Warning,
    /// Critical — immediate attention required.
    Critical,
}

impl std::fmt::Display for EscalationSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Directives the Curator issues to Cybernetics.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorDirective {
    CalibrateThreshold {
        domain: String,
        new_threshold: u64,
    },
    /// Override energy budget beyond Cybernetics set-points.
    ///
    /// This is the Curation-level metacognitive override for an agent's
    /// governed per-tick call cap. It is distinct from inference resilience.
    OverrideEnergyBudget {
        agent: WebID,
        new_budget: u64,
    },
    /// Replenish an agent's energy budget by a specific amount.
    ///
    /// Used when an agent has exhausted its budget but Curation
    /// \[NORMATIVE\] determines it should continue operating. This is the
    /// Curator's ability to inject energy into the system (P9 — Homeostatic
    /// Self-Regulation), governed by human/curator authority.
    ReplenishBudget {
        agent: WebID,
        amount: u64,
        /// Priority weight for replenishment scaling (0.0–1.0).
        /// When present, Cybernetics scales the replenishment amount by this priority.
        /// Defaults to 1.0 (full replenishment).
        priority: Option<f64>,
    },
    /// Clear a Curation override on an agent's energy budget.
    ///
    /// Removes the agent from the active-overrides registry so that
    /// normal replenishment resumes. This is the inverse of `OverrideEnergyBudget`.
    ClearOverride {
        agent: WebID,
    },
    /// Escalate a domain-level variety deficit or quality threat to the user.
    ///
    /// Narrows the variety gap: the system has ~500 distinct operations but
    /// only ~6 regulatory action types. This directive lets the Curator
    /// surface domain-specific concerns with severity and evidence for
    /// human review.
    EscalateDomain {
        /// Domain identifier (e.g., "inference", "storage").
        domain: String,
        /// Severity of the escalation.
        severity: EscalationSeverity,
        /// Human-readable summary of the evidence.
        evidence: String,
    },
}

impl CuratorDirective {
    /// Returns the snake_case variant name for logging and fingerprinting.
    pub fn variant_name(&self) -> &'static str {
        match self {
            CuratorDirective::CalibrateThreshold { .. } => "calibrate_threshold",
            CuratorDirective::OverrideEnergyBudget { .. } => "override_energy_budget",
            CuratorDirective::ReplenishBudget { .. } => "replenish_budget",
            CuratorDirective::ClearOverride { .. } => "clear_override",
            CuratorDirective::EscalateDomain { .. } => "escalate_domain",
        }
    }

    /// Returns the agent targeted by this directive, if applicable.
    ///
    /// Directives that target a domain rather than an agent
    /// (e.g., `CalibrateThreshold`, `EscalateDomain`) return `None`.
    pub fn agent_target(&self) -> Option<WebID> {
        match self {
            CuratorDirective::CalibrateThreshold { .. } => None,
            CuratorDirective::OverrideEnergyBudget { agent, .. } => Some(*agent),
            CuratorDirective::ReplenishBudget { agent, .. } => Some(*agent),
            CuratorDirective::ClearOverride { agent } => Some(*agent),
            CuratorDirective::EscalateDomain { .. } => None,
        }
    }

    /// Whether this directive is a metacognitive override.
    ///
    /// Metacognitive overrides are higher-order Curation interventions that
    /// reconfigure Cybernetics regulation itself. They are subject to the
    /// override cooldown in addition to per-fingerprint dedup, because
    /// override oscillation is especially destabilizing.
    pub fn is_metacognitive(&self) -> bool {
        matches!(self, CuratorDirective::OverrideEnergyBudget { .. })
    }
}

// ── CurationThresholdConfig — spec coherence/drift thresholds ───────────────

fn default_coherence_threshold() -> f64 {
    0.7
}
fn default_drift_threshold() -> f64 {
    0.5
}

/// Configurable thresholds for Curation decisions (spec coherence, drift).
///
/// Lives in `hkask-types`, not `hkask-regulation`
/// (curation regulates cybernetics — its config belongs in the foundation layer).
/// YAML loading remains in `hkask-regulation` (requires `serde_yaml_neo`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CurationThresholdConfig {
    #[serde(default = "default_coherence_threshold")]
    pub coherence_threshold: f64,
    #[serde(default = "default_drift_threshold")]
    pub drift_threshold: f64,
}

/// Validation error for [`CurationThresholdConfig`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CurationThresholdError {
    #[error("coherence_threshold must be finite and in [0.0, 1.0], got {0}")]
    InvalidCoherenceThreshold(f64),
    #[error("drift_threshold must be finite and in [0.0, 1.0], got {0}")]
    InvalidDriftThreshold(f64),
    #[error(
        "drift_threshold ({drift}) must be strictly less than coherence_threshold ({coherence})"
    )]
    InvertedBounds { drift: f64, coherence: f64 },
}

impl CurationThresholdConfig {
    /// Validate that both thresholds are finite, within `[0.0, 1.0]`, and
    /// strictly ordered (`drift_threshold < coherence_threshold`).
    pub fn validate(&self) -> Result<(), CurationThresholdError> {
        if !self.coherence_threshold.is_finite() || !(0.0..=1.0).contains(&self.coherence_threshold)
        {
            return Err(CurationThresholdError::InvalidCoherenceThreshold(
                self.coherence_threshold,
            ));
        }
        if !self.drift_threshold.is_finite() || !(0.0..=1.0).contains(&self.drift_threshold) {
            return Err(CurationThresholdError::InvalidDriftThreshold(
                self.drift_threshold,
            ));
        }
        if self.drift_threshold >= self.coherence_threshold {
            return Err(CurationThresholdError::InvertedBounds {
                drift: self.drift_threshold,
                coherence: self.coherence_threshold,
            });
        }
        Ok(())
    }
}

impl Default for CurationThresholdConfig {
    fn default() -> Self {
        Self {
            coherence_threshold: default_coherence_threshold(),
            drift_threshold: default_drift_threshold(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curation_threshold_config_default_and_boundaries_validate() {
        assert!(CurationThresholdConfig::default().validate().is_ok());
        assert!(
            CurationThresholdConfig {
                coherence_threshold: 1.0,
                drift_threshold: 0.0,
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn curation_threshold_config_rejects_non_finite_out_of_range_and_inverted_bounds() {
        for bad in [
            CurationThresholdConfig {
                coherence_threshold: f64::NAN,
                drift_threshold: 0.5,
            },
            CurationThresholdConfig {
                coherence_threshold: 0.7,
                drift_threshold: f64::INFINITY,
            },
            CurationThresholdConfig {
                coherence_threshold: 0.7,
                drift_threshold: f64::NEG_INFINITY,
            },
            CurationThresholdConfig {
                coherence_threshold: 1.01,
                drift_threshold: 0.5,
            },
            CurationThresholdConfig {
                coherence_threshold: 0.7,
                drift_threshold: -0.01,
            },
            CurationThresholdConfig {
                coherence_threshold: 0.5,
                drift_threshold: 0.5,
            },
            CurationThresholdConfig {
                coherence_threshold: 0.4,
                drift_threshold: 0.6,
            },
        ] {
            assert!(bad.validate().is_err(), "expected invalid config: {bad:?}");
        }
    }
}
