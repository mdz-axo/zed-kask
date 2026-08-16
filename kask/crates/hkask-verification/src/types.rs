//! Shared verification types.
//!
//! `GroundingRecord` is the append-only record stored in the central
//! grounding ledger. Each delegation writes a new record — the trend query
//! reads all records and aggregates them.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::grounding::{GroundingResult, ProvenanceTag};

/// A full grounding record stored in the central ledger. Append-only —
/// each delegation writes a new record. This is the source of truth for
/// grounding status, trend analysis, and the curator's feedback loop.
///
/// The `source` field identifies which MCP server/tool produced this
/// delegation ("kanban_task_spawn", "swarm_delegate_local", etc.), enabling
/// cross-tool aggregation and per-tool trend analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingRecord {
    /// UUID identifying this specific delegation (for linking with
    /// `LocalDelegateResult` if needed in the future).
    pub delegation_id: String,
    /// Which MCP server/tool produced this delegation.
    pub source: String,
    /// The agent that was delegated to.
    pub agent_id: String,
    /// The agent's type (determines which grounding contract applies).
    pub agent_type: String,
    /// When the delegation was grounded.
    pub timestamp: DateTime<Utc>,
    /// Whether a grounding contract existed for this agent_type.
    /// `false` = coverage gap (paper §6: coverage is itself a metric).
    pub had_contract: bool,
    /// Fields nulled as Unsourced (empty if clean or no contract).
    pub nulled_fields: Vec<String>,
    /// Narrative leaks detected (empty if clean or no contract).
    pub narrative_leaks: Vec<(String, String)>,
    /// Per-field provenance tags (empty if no contract).
    pub provenance: HashMap<String, ProvenanceTag>,
}

impl GroundingRecord {
    /// Construct from a `GroundingResult` after enforcement.
    pub fn from_result(
        source: &str,
        agent_id: &str,
        agent_type: &str,
        result: &GroundingResult,
    ) -> Self {
        let delegation_id = uuid::Uuid::new_v4().to_string();
        Self {
            delegation_id,
            source: source.to_string(),
            agent_id: agent_id.to_string(),
            agent_type: agent_type.to_string(),
            timestamp: Utc::now(),
            had_contract: true,
            nulled_fields: result.nulled_fields.clone(),
            narrative_leaks: result.narrative_leaks.clone(),
            provenance: result.provenance.clone(),
        }
    }

    /// Construct a coverage-gap record (no contract for this agent_type).
    /// `had_contract: false` — the gap is visible in the trend, not silently
    /// treated as compliant (paper §6: coverage is itself a metric).
    pub fn coverage_gap(source: &str, agent_id: &str, agent_type: &str) -> Self {
        let delegation_id = uuid::Uuid::new_v4().to_string();
        Self {
            delegation_id,
            source: source.to_string(),
            agent_id: agent_id.to_string(),
            agent_type: agent_type.to_string(),
            timestamp: Utc::now(),
            had_contract: false,
            nulled_fields: Vec::new(),
            narrative_leaks: Vec::new(),
            provenance: HashMap::new(),
        }
    }

    /// True if this delegation was clean (contract ran, zero violations).
    pub fn is_clean(&self) -> bool {
        self.had_contract && self.nulled_fields.is_empty() && self.narrative_leaks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::GroundingResult;

    #[test]
    fn from_result_records_violations() {
        let result = GroundingResult {
            nulled_fields: vec!["deliverable_path".to_string()],
            narrative_leaks: vec![("/src/x".to_string(), "deliverable_path".to_string())],
            ..Default::default()
        };
        let record =
            GroundingRecord::from_result("kanban_task_spawn", "task_agent", "task", &result);
        assert!(record.had_contract);
        assert_eq!(record.source, "kanban_task_spawn");
        assert_eq!(record.agent_id, "task_agent");
        assert_eq!(record.agent_type, "task");
        assert_eq!(record.nulled_fields, vec!["deliverable_path".to_string()]);
        assert!(!record.is_clean(), "record with nulled fields is not clean");
    }

    #[test]
    fn coverage_gap_is_not_clean() {
        // Absence ≠ verdict (paper Rule 5.3): a coverage-gap record is NOT
        // clean — it has no contract. `is_clean` requires `had_contract: true`.
        let record =
            GroundingRecord::coverage_gap("swarm_delegate_local", "researcher", "research");
        assert!(!record.had_contract);
        assert!(
            !record.is_clean(),
            "coverage gap must not be reported as clean"
        );
        assert!(record.nulled_fields.is_empty());
        assert!(record.narrative_leaks.is_empty());
    }

    #[test]
    fn clean_record_is_clean() {
        let result = GroundingResult::default();
        let record =
            GroundingRecord::from_result("kanban_task_spawn", "task_agent", "task", &result);
        assert!(record.is_clean());
    }

    #[test]
    fn delegation_ids_are_unique() {
        let result = GroundingResult::default();
        let a = GroundingRecord::from_result("s", "a", "task", &result);
        let b = GroundingRecord::from_result("s", "a", "task", &result);
        assert_ne!(a.delegation_id, b.delegation_id, "UUIDs must be unique");
    }
}
