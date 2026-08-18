//! Curator types — CuratorHandle (capability handle) and CuratorDirective (Curation → Cybernetics).
//!
//! Lives in hkask-types as the single source of truth for curator types.
//!
//! Per the Authority DAG: Curation (Loop 5) → Cybernetics (Loop 6).
//! The Curator's types must live in a crate that both Curator (hkask-agents)
//! and Cybernetics (hkask-regulation) can depend on without inversion.
//!
//! NOTE: hkask-types must NOT depend on hkask-capability (cycle prevention).
//! Methods that need capability tokens (e.g., `issue_consolidation_token`) live
//! as free functions or extension traits in crates that have the capability dep.

use crate::id::WebID;

// ── CuratorHandle — Loop 5 capability handle ────────────────────────────────

/// The Curator's capability handle. Single agent — the user's
/// counterpart in the zed-kask agent panel. Can read all loop state and write
/// governance/observability policy.
///
/// **Singleton invariant:** There is exactly one Curator per hKask system.
/// `CurationLoop` owns the single `CuratorHandle` instance; all other code
/// accesses it through `CuratorContext::handle()`. Construct via
/// `CuratorHandle::system()` — the `new(WebID)` constructor is `pub(crate)`
/// to prevent external callers from creating additional handles.
#[derive(Clone)]
pub struct CuratorHandle {
    curator_id: WebID,
}

impl CuratorHandle {
    /// Create the system CuratorHandle using the system WebID.
    ///
    /// The Curator is a singleton — the user's counterpart in the zed-kask
    /// agent panel. This constructor enforces that convention by deriving the ID from
    /// the "curator" persona.
    pub fn system() -> Self {
        Self {
            curator_id: WebID::from_persona(b"curator"),
        }
    }

    pub fn curator_id(&self) -> &WebID {
        &self.curator_id
    }
}

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
///
/// Per ARL IP-3: when the Curation Confidence Gate is in the transition zone
/// (0.3 < R̄ < 0.8), the regulated response is `SeekMoreEvidence`, which
/// is routed through Cybernetics to the Inference Loop.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorDirective {
    CalibrateThreshold {
        domain: String,
        new_threshold: u64,
    },
    UpdateCapabilities {
        agent: WebID,
        additions: Vec<String>,
        removals: Vec<String>,
    },
    /// Override energy budget beyond Cybernetics set-points.
    ///
    /// This is the Curation-level metacognitive override. Cybernetics
    /// uses `ActionType::AdjustEnergyBudget` for automatic within-bounds
    /// regulation. Curation uses `OverrideEnergyBudget` to exceed bounds.
    OverrideEnergyBudget {
        agent: WebID,
        new_budget: u64,
    },
    /// IP-3: Confidence-gated metacognitive directive.
    /// The Curator requests additional evidence to increase confidence
    /// in a pending decision. Routed through Cybernetics to Inference.
    SeekMoreEvidence {
        /// The decision context requiring more evidence.
        context: String,
        /// Which evidence channel to verify (from sensitivity analysis).
        /// E.g., "llm_confidence", "template_match", "validation_result".
        channel: String,
        /// Current R̄ from the Curation Confidence Gate.
        confidence: String,
    },
    /// Replenish an agent's energy budget by a specific amount.
    ///
    /// Used when an agent has exhausted its budget but Curation
    /// \[NORMATIVE\] determines it should continue operating. This is the Curator's (P9 — Homeostatic Self-Regulation).
    /// ability to inject energy into the system, analogous to Ethereum's
    /// gas refund mechanism but governed by human/curator authority.
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
    /// Evolve an MCP tool's input schema based on skill-use feedback.
    ///
    /// This is the Phase 3 co-evolution directive: the Curator analyzes
    /// skill-use reports (Phase 2.2) and `reg.tool.*` spans, identifies
    /// schema mismatches or missing inputs, and issues a directive to
    /// evolve the tool's schema. The directive does not directly modify the
    /// schema (MCP tool schemas are compiled Rust structs); it records the
    /// evolution request in the regulation ledger so a developer or
    /// automated migration agent can act on it.
    EvolveMcpToolSchema {
        /// MCP server name (e.g., "hkask-mcp-companies").
        server_name: String,
        /// Tool name on that server (e.g., "dcf_valuation").
        tool_name: String,
        /// Type of schema evolution requested.
        evolution_type: SchemaEvolutionType,
        /// Field name to add/remove/rename/change.
        field_name: String,
        /// New field type (for add_field/change_type), or new field name (for
        /// rename_field). Omitted for remove_field.
        new_type: Option<String>,
        /// Why this evolution is needed — grounded in skill-use reports.
        rationale: String,
        /// Evidence summary (which skill, which step, what failure).
        evidence: String,
    },
}

/// Type of MCP tool schema evolution requested by the Curator.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaEvolutionType {
    /// Add a new input field to the tool's schema.
    AddField,
    /// Remove an existing input field.
    RemoveField,
    /// Rename an existing input field.
    RenameField,
    /// Change the type of an existing input field.
    ChangeType,
}

impl CuratorDirective {
    /// Returns the snake_case variant name for logging and fingerprinting.
    pub fn variant_name(&self) -> &'static str {
        match self {
            CuratorDirective::CalibrateThreshold { .. } => "calibrate_threshold",
            CuratorDirective::UpdateCapabilities { .. } => "update_capabilities",
            CuratorDirective::OverrideEnergyBudget { .. } => "override_energy_budget",
            CuratorDirective::SeekMoreEvidence { .. } => "seek_more_evidence",
            CuratorDirective::ReplenishBudget { .. } => "replenish_budget",
            CuratorDirective::ClearOverride { .. } => "clear_override",
            CuratorDirective::EscalateDomain { .. } => "escalate_domain",
            CuratorDirective::EvolveMcpToolSchema { .. } => "evolve_mcp_tool_schema",
        }
    }

    /// Returns the agent targeted by this directive, if applicable.
    ///
    /// Directives that target a domain rather than an agent
    /// (e.g., `CalibrateThreshold`, `SeekMoreEvidence`) return `None`.
    pub fn agent_target(&self) -> Option<WebID> {
        match self {
            CuratorDirective::CalibrateThreshold { .. } => None,
            CuratorDirective::UpdateCapabilities { agent, .. } => Some(*agent),
            CuratorDirective::OverrideEnergyBudget { agent, .. } => Some(*agent),
            CuratorDirective::SeekMoreEvidence { .. } => None,
            CuratorDirective::ReplenishBudget { agent, .. } => Some(*agent),
            CuratorDirective::ClearOverride { agent } => Some(*agent),
            CuratorDirective::EscalateDomain { .. } => None,
            CuratorDirective::EvolveMcpToolSchema { .. } => None,
        }
    }

    /// Whether this directive is a metacognitive override.
    ///
    /// Metacognitive overrides are higher-order Curation interventions that
    /// reconfigure Cybernetics regulation itself. They are subject to the
    /// override cooldown in addition to per-fingerprint dedup, because
    /// override oscillation is especially destabilizing.
    pub fn is_metacognitive(&self) -> bool {
        matches!(
            self,
            CuratorDirective::OverrideEnergyBudget { .. }
                | CuratorDirective::SeekMoreEvidence { .. }
        )
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
/// Consolidated from `hkask-regulation/src/types/curation.rs` into `hkask-types`
/// (curation regulates cybernetics — its config belongs in the foundation layer).
/// YAML loading remains in `hkask-regulation` (requires `serde_yaml_neo`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CurationThresholdConfig {
    #[serde(default = "default_coherence_threshold")]
    pub coherence_threshold: f64,
    #[serde(default = "default_drift_threshold")]
    pub drift_threshold: f64,
}

impl Default for CurationThresholdConfig {
    fn default() -> Self {
        Self {
            coherence_threshold: default_coherence_threshold(),
            drift_threshold: default_drift_threshold(),
        }
    }
}
