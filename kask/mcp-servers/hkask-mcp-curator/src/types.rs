//! Request types for hkask-mcp-curator MCP tools.

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PingRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EscalationResolveRequest {
    pub id: String,
    pub resolution: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EscalationDismissRequest {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SemanticSearchRequest {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RegQueryRequest {
    /// Regulation namespace prefix to filter by (e.g., "reg.sovereignty", "reg.contract")
    pub namespace: Option<String>,
    /// Lookback window in seconds (default: 3600 = 1 hour)
    pub window_seconds: Option<u64>,
    /// Maximum events to return (default: 100)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryRecallRequest {
    pub entity: String,
    pub memory_type: Option<String>,
    /// Optional ontology axis to recall along instead of the entity (P5.4).
    /// One of `dc_type`, `dc_subject`, `pko_procedure`, `ontology_namespace`.
    /// When set, `ontology_value` supplies the term and `entity` is ignored.
    pub ontology_axis: Option<String>,
    /// The term to match on `ontology_axis` (e.g. `bibo:Article` for
    /// `dc_type`, `fibo` for `ontology_namespace`).
    pub ontology_value: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AlgedonicLogRequest {
    pub hours: Option<u32>,
}

/// Consultation request — a swarm agent asks the curator a question.
/// The curator searches its semantic + episodic memory for relevant
/// fragments and returns them as a structured consultation response.
///
/// This is a memory-grounded consultation, not a full curator agent turn —
/// the curator MCP server has no inference port. A full inference-grounded
/// response requires the in-process Curator agent (`CuratorAgentServer`),
/// which lives in the zed process, not in this MCP server. The tool's
/// description makes this clear so the calling agent knows what it gets.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CuratorConsultRequest {
    /// The question or topic to consult the curator about.
    pub query: String,
    /// Maximum number of memory fragments to return per store (default: 5).
    pub limit: Option<usize>,
}

/// Skill-use issue report — submitted by a skill's `on_failure` config when
/// an MCP tool call fails or produces unexpected output. The Curator
/// collects these reports across skills and invocations to identify patterns
/// (e.g. "dcf_valuation fails 40% of the time when growth_rate > 0.3") and
/// issue CuratorDirectives to evolve the MCP tool.
///
/// The report is stored as an episodic h_mem in the curator's memory store
/// with entity `skill_use_issue:<skill_name>` so it is queryable via
/// `curator_memory_recall` and `curator_semantic_search`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportSkillUseIssueRequest {
    /// The skill manifest ID (e.g. "superforecasting", "scenario-builder").
    pub skill_name: String,
    /// The MCP tool name that failed or produced unexpected output
    /// (e.g. "market_match", "scenario_score").
    pub tool_name: String,
    /// The step ordinal in the skill's cascade where the failure occurred.
    pub step_ordinal: u32,
    /// The error message or unexpected output description.
    pub error: String,
    /// Optional: the input that was sent to the tool (JSON string).
    pub tool_input: Option<String>,
    /// Optional: classification of the failure (e.g. "wrong_inputs",
    /// "missing_fields", "timeout", "unexpected_empty_result",
    /// "schema_mismatch").
    pub failure_type: Option<String>,
}

// ── Curator memory edit tools (Priority 5) ───────────────────────────────
//
// These tools give the curator agent write access to its own memory, with
// evidence-grounding and confidence-floor constraints. User threads cannot
// write to memory directly — only the curator (the one agent with a feedback
// loop). See `kask/docs/plans/memory-system-improvements.md` Priority 5.
//
// Grounding: Dunning's Cassandra quandary (`138299529:16-17`) — poor
// performers can't evaluate which memories are worth writing. MemGPT (Packer
// et al., 2023) — OS-style memory management with permission boundaries.

/// Insert a new semantic memory into the curator's store.
///
/// The memory starts at confidence 0.5 (the floor — NOT the model's
/// self-assessed confidence). Confidence is calibrated by subsequent
/// Brier-scored outcomes, not by self-assessment.
///
/// Evidence-grounding: the `evidence_h_mem_id` field must cite a specific
/// episodic h_mem ID that supports this memory. The tool rejects inserts
/// without a citation. This is Dunning's structured-reflection principle:
/// the model must ground its assertion in evidence, not free-associate.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryInsertRequest {
    /// The entity (subject) of the memory — e.g. "company:AAPL", "skill:superforecasting".
    pub entity: String,
    /// The attribute (predicate) of the memory — e.g. "revenue_trend", "calibration_gap".
    pub attribute: String,
    /// The value (object) of the memory, as a JSON string.
    pub value: serde_json::Value,
    /// The episodic h_mem ID that supports this memory (evidence-grounding
    /// requirement). The tool rejects inserts without a citation.
    pub evidence_h_mem_id: String,
    /// Optional: a human-readable note explaining the reasoning behind this
    /// memory. Stored in the h_mem's value as a `_note` field.
    pub note: Option<String>,
}

/// Update an existing memory's confidence via Bayesian combination.
///
/// The new confidence is combined with the existing confidence using
/// log-odds (Bayesian) pooling — not replacement. This means:
/// - Two independent sources saying p=0.8 → combined ≈ 0.94 (consensus strengthens)
/// - One saying p=0.8 and another p=0.2 → combined = 0.5 (conflict dampens)
///
/// This is the calibration mechanism: confidence is adjusted by outcome
/// feedback, not by self-assessment.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryUpdateRequest {
    /// The h_mem ID to update.
    pub h_mem_id: String,
    /// The new confidence value (0.0–1.0). Will be Bayesian-combined with
    /// the existing confidence, not replaced.
    pub new_confidence: f64,
    /// Optional: a new value to replace the existing one (bitemporal update —
    /// the old version is closed, a new version is inserted).
    pub new_value: Option<serde_json::Value>,
    /// Optional: reason for the confidence update (e.g. "Brier score 0.12 on
    /// resolved forecast", "contradicted by newer observation").
    pub reason: Option<String>,
}

/// Resolve a contradiction between two or more memories.
///
/// This is the therapy process tool — it resolves cognitive dissonance in
/// the memory store by expiring, updating, or deleting contradictory h_mems.
/// Requires operator approval (the curator proposes; the operator approves).
///
/// Grounding: Festinger's three dissonance resolution strategies
/// (`Universal_Principles_of_Design:39`): reduce importance (lower
/// confidence), add consonant (insert reconciling memory), remove dissonant
/// (expire/delete).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryResolveContradictionRequest {
    /// The h_mem IDs involved in the contradiction.
    pub h_mem_ids: Vec<String>,
    /// The resolution strategy: "expire" (soft-delete the lower-confidence
    /// one), "update_confidence" (lower confidence on the contradicted one),
    /// or "delete" (hard-delete — use sparingly).
    pub strategy: String,
    /// The h_mem ID to act on (the one to expire/update/delete). For
    /// "update_confidence", the new confidence value must be provided.
    pub target_h_mem_id: String,
    /// For "update_confidence": the new confidence value. Ignored for other
    /// strategies.
    pub new_confidence: Option<f64>,
    /// Evidence-grounding: the reason for this resolution (must cite the
    /// contradiction observed).
    pub reason: String,
}
