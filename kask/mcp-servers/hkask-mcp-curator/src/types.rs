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

/// Dismiss all pending escalations matching a given output string.
///
/// Used to clear runaway escalation floods from a single broken feedback
/// loop (e.g. an unwired efferent action that the regulation loop senses
/// every cycle) in one operation, rather than dismissing each duplicate
/// individually. Only pending escalations with an exact `output` match are
/// dismissed — this will not collapse distinct alerts that happen to share
/// a prefix.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EscalationDismissByPatternRequest {
    /// The exact `output` string to match against pending escalations.
    pub output: String,
    /// The dismissal reason recorded for each dismissed escalation.
    pub reason: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BackfillEmbeddingsRequest {
    /// List the candidates that would be embedded, without embedding
    /// anything. Default false.
    pub dry_run: Option<bool>,
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

/// Recall shape for `curator_memory_recall`.
///
/// - `perspective_scoped`: h_mems written by the curator (first-person
///   turn history). Uses `query_for_deduped` with the curator's WebID.
/// - `entity_wide`: all h_mems for the entity regardless of perspective.
///   Uses `query_deduped` (no perspective filter).
/// - `both`: return both recall shapes in separate JSON keys.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecallType {
    PerspectiveScoped,
    EntityWide,
    #[default]
    Both,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryRecallRequest {
    pub entity: String,
    /// Recall shape: `perspective_scoped` (curator's turns), `entity_wide`
    /// (all h_mems for the entity), or `both`. Defaults to `both`.
    #[serde(default)]
    pub recall_shape: MemoryRecallType,
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
// loop). See `kask/docs/architecture/memory-system-specification.md`
// (consolidation + hygiene sections).
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
/// (forget — delete the row).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryResolveContradictionRequest {
    /// The h_mem IDs involved in the contradiction.
    pub h_mem_ids: Vec<String>,
    /// The resolution strategy: "forget" (delete the dissonant h_mem from
    /// the database — memories are forgotten, not expired) or
    /// "update_confidence" (lower confidence on the contradicted one).
    pub strategy: String,
    /// The h_mem ID to act on (the one to forget or update). For
    /// "update_confidence", the new confidence value must be provided.
    pub target_h_mem_id: String,
    /// For "update_confidence": the new confidence value. Ignored for other
    /// strategies.
    pub new_confidence: Option<f64>,
    /// Evidence-grounding: the reason for this resolution (must cite the
    /// contradiction observed).
    pub reason: String,
}

// ── Memory hygiene tools (age prune + dedup) ───────────────────────────
//
// The consolidation service handles confidence-based cleanup and budget
// pruning. These tools add the two missing axes: age-based hard-delete
// (memory_life_days is used for decay, never for deletion) and
// near-duplicate string dedup (recall-time dedup is an exact-EAV hash
// filter, not fuzzy value dedup).
// Both are deterministic, non-LLM, and operator-invoked.

/// Prune h_mems older than a specified age.
///
/// Hard-deletes h_mems whose observation timestamp (`valid_from`) is older
/// than `max_age_days`. Optionally spares h_mems that have been recalled
/// within the last `spare_recalled_within_days` days — actively-used
/// memories survive even if they are old. This is distinct from confidence
/// decay (which lowers recall weight but never deletes) and from
/// confidence-based consolidation (which deletes low-confidence h_mems).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryPruneRequest {
    /// Maximum age in days. h_mems older than this are candidates for deletion.
    pub max_age_days: i64,
    /// If set, spare h_mems recalled within this many days. An h_mem that was
    /// recalled recently stays even if it is old — the decay clock was reset,
    /// so it is still active in the recall path.
    pub spare_recalled_within_days: Option<i64>,
    /// Prune ALL layers including knowledge-layer rows (operator rulings,
    /// verified status, reified lessons). Opt-in: the default scope is turn
    /// storage only — the episodic forgetting valve without the power to
    /// destroy durable knowledge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_layers: Option<bool>,
}

/// Deduplicate h_mems by normalized string value.
///
/// Scans the curator's h_mems and groups them by (entity, attribute,
/// normalized_value). For each group with 2+ near-duplicate values, the
/// highest-confidence h_mem is kept and the rest are expired (soft-delete
/// via `valid_to`). Non-string values are skipped — structural dedup is
/// the recall-time exact-EAV filter's job.
///
/// Normalization: lowercase, strip punctuation, collapse whitespace.
/// "AAPL." and "aapl" are treated as duplicates.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryDedupRequest {
    /// Maximum h_mems to scan. Defaults to 10_000 if omitted. The scan is
    /// bounded to prevent unbounded memory reads on very large stores.
    pub limit: Option<usize>,
}

/// Extract candidate semantic memories from a thread's turn history.
///
/// This is the on-demand version of Agno's ALWAYS-mode learning: instead
/// of automatically extracting memories after every turn (which requires
/// a background LLM call and careful rate-limiting), the curator or operator
/// calls this tool to extract candidate memories from a specific thread's
/// turns. The tool returns the candidates — it does NOT insert them.
/// Insertion still goes through `memory_insert`, which requires evidence
/// citation (the turn h_mem ID), preserving the evidence-grounding invariant.
///
/// The tool queries the curator's memory for all h_mems with entity
/// `chat:thread:<thread_id>`, returns their IDs and content, and suggests
/// candidate (entity, attribute, value) triples the curator might extract.
/// The curator reviews and inserts the ones worth keeping.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryExtractRequest {
    /// The thread id whose turns to extract candidates from.
    pub thread_id: String,
}
