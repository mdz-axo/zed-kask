//! Core Regulation (Cybernetic Nervous System) types for hKask
//!
//! Core spans: reg.curation.*, reg.memory.encode.*
//!
//! Domain-specific spans have moved to their respective domain crates.
//!
//! `CANONICAL_NAMESPACES` (in `event.rs`) is the single source of truth for
//! **canonical** Regulation spans — the essential, regulation record-eligible spans that are
//! `SpanNamespace`-validated, `SpanCategory`-categorized, and loop-connected.
//! The `reg.*` prefix is reserved for canonical spans: every `reg.*` tracing
//! target MUST be registered in `CANONICAL_NAMESPACES`. **Performative**
//! telemetry (per PRINCIPLES §9.1) uses `hkask.*` tracing targets (e.g.
//! `hkask.cli`, `hkask.training.job.submit`), NOT `reg.*`; those are observability
//! logs, not loop variables, and `SpanNamespace::new` rejects them.

use serde::{Deserialize, Serialize};

// ── Domain newtypes (P2.3) ──────────────────────────────────────────────────

/// Communication queue depth for backpressure regulation.
///
/// Newtype wrapper that prevents accidental confusion with other numeric
/// thresholds in `SetPoints` (energy, variety deficit, error rate).
///
/// Defined in hkask-types (substrate crate) because it is shared across
/// hkask-regulation (SetPoints, cybernetics loop) and hkask-agents
/// (communication loop).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct QueueDepth(pub f64);

impl QueueDepth {
    /// Default backpressure threshold: 100 messages.
    pub const DEFAULT_BACKPRESSURE: QueueDepth = QueueDepth(100.0);
}

// Regulation Health — Observability data struct

/// Regulation health status
///
/// Pure data struct — construction logic (`reg_health_check`) lives in
/// hkask-regulation where it has access to `AlgedonicManager`.
#[derive(Debug, Clone)]
pub struct LedgerHealth {
    pub overall_deficit: u64,
    pub critical_count: usize,
    pub warning_count: usize,
    pub healthy: bool,
    /// Session-level EMA of domain variety (survives window resets).
    /// 0.0 when no domains have been tracked.
    pub variety_ema: f64,
    /// Number of alerts currently in the in-memory algedonic log.
    /// The log is a capped ring buffer (default 200); when this approaches
    /// `alert_log_cap`, the operator should run the `algedonic-review` skill
    /// to review and clear reviewed entries.
    pub alert_log_count: usize,
    /// The configured cap for the in-memory algedonic log.
    pub alert_log_cap: usize,
    /// `true` when the alert log is ≥ 80% of the cap. The cybernetics loop
    /// emits an `AlgedonicLogApproachingCap` signal when this is true so the
    /// operator (or the `algedonic-review` skill) can review before eviction.
    pub alert_log_approaching_cap: bool,
}

/// Regulation loop health — the Curator's window into regulatory effectiveness.
///
/// Aggregated from `ImpactReport` decisions across regulation cycles.
/// Enables the metacognition loop to answer: "are our regulatory actions working?"
#[derive(Debug, Clone, Default)]
pub struct RegulationHealth {
    /// Total regulation cycles recorded.
    pub total_cycles: u64,
    /// Actions accepted (improved or within noise tolerance).
    pub accepted: u64,
    /// Actions staged for review (moderately ineffective).
    pub staged: u64,
    /// Actions blocked (severely counterproductive).
    pub blocked: u64,
}

impl RegulationHealth {
    /// Ratio of accepted actions to total (0.0–1.0). 1.0 if no actions recorded.
    pub fn effectiveness(&self) -> f64 {
        let total = self.accepted + self.staged + self.blocked;
        if total == 0 {
            1.0
        } else {
            self.accepted as f64 / total as f64
        }
    }
}

// ── RegulationSpan — Core Regulation Span Identifiers ────────────────────────────────────

/// Core Regulation span identifiers — spans that are constructed in 2+ crates from
/// different dependency domains (the "cross-cutting concern" test).
///
/// backup, ACP, curator, etc.) have moved to their respective domain crates
/// as enums implementing [`ObservableSpan`](crate::ObservableSpan).
///
/// `CANONICAL_NAMESPACES` (in `event.rs`) is the single source of truth for
/// **canonical** Regulation spans — essential spans that are `SpanNamespace`-validated,
/// `SpanCategory`-categorized, and connected to a cybernetic loop. The `reg.*`
/// prefix is reserved for these canonical spans: every `reg.*` tracing target
/// MUST be registered. Per PRINCIPLES §9.1, performative telemetry uses
/// `hkask.*` tracing targets (not `reg.*`); those are observability logs, not
/// loop variables, and `SpanNamespace::new` rejects them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RegulationSpan {
    /// Curation loop operations — registry sync, pod sync, directive issuance.
    Curation,
    /// Memory encoding operations.
    MemoryEncode,
}

impl RegulationSpan {
    /// Emit a typed Regulation span event through the `tracing` infrastructure.
    ///
    /// Enforces the canonical Regulation emission convention (PRINCIPLES.md §9.2):
    /// - `target` = `"reg"` root namespace (full domain in `reg_domain` field)
    /// - `reg_domain` = `self.as_str()` (e.g. `"reg.tool.media"`)
    /// - `operation` = the verb describing what occurred (e.g. `"invoked"`)
    /// - message = `"REG"` (required for downstream regulation record parsing)
    ///
    /// Callers that need additional structured fields can attach them by
    /// entering a child [`mod@tracing::span`] before calling `emit()`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use hkask_types::regulation::RegulationSpan;
    ///
    /// RegulationSpan::Curation.emit("invoked");
    /// ```
    pub fn emit(&self, operation: &str) {
        tracing::info!(
            target: "reg",
            reg_domain = %self.as_str(),
            operation = %operation,
            "REG",
        );
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid RegulationSpan variant
    /// post: returns the canonical namespace string (e.g. "reg.tool.web_search"); output matches CANONICAL_NAMESPACES byte-for-byte
    ///
    /// This output must match regulation record serialization strings byte-for-byte
    /// (P8 — Semantic Grounding).
    pub fn as_str(&self) -> &'static str {
        match self {
            RegulationSpan::Curation => "reg.curation",
            RegulationSpan::MemoryEncode => "reg.memory.encode",
        }
    }
}

impl std::fmt::Display for RegulationSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl crate::observable_span::ObservableSpan for RegulationSpan {
    fn as_str(&self) -> &'static str {
        RegulationSpan::as_str(self)
    }

    fn emit(&self, operation: &str) {
        RegulationSpan::emit(self, operation);
    }
}

impl std::str::FromStr for RegulationSpan {
    type Err = ();

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  s is a string matching a canonical RegulationSpan namespace
    /// post: returns Ok(RegulationSpan) for canonical strings; Err(()) for unknown strings
    ///
    /// Only strings matching canonical `RegulationSpan` namespaces parse
    /// successfully. Unknown strings return `Err(())`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "reg.curation" => Ok(RegulationSpan::Curation),
            "reg.memory.encode" => Ok(RegulationSpan::MemoryEncode),
            _ => Err(()),
        }
    }
}
