//! Request types for hkask-mcp-curator MCP tools.

use hkask_types::AnyJsonValue;
use schemars::{JsonSchema, json_schema};
use serde::{Deserialize, Serialize};

/// The ontology axes `curator_memory_recall` can recall along.
pub const ONTOLOGY_AXES: [&str; 4] = [
    "dc_type",
    "dc_subject",
    "pko_procedure",
    "ontology_namespace",
];

/// The strategies `memory_resolve_contradiction` accepts.
pub const RESOLUTION_STRATEGIES: [&str; 2] = ["forget", "update_confidence"];

/// The sweep modes `curator_memory_backfill_embeddings` accepts.
pub const BACKFILL_MODES: [&str; 2] = ["missing", "model_mismatch"];

// Closed-vocabulary inputs stay strings (the handler names the accepted
// values when it refuses one); the schema enumerates the same lists so a
// caller sees them before the call.
fn optional_ontology_axis_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    json_schema!({"anyOf": [{"type": "string", "enum": ONTOLOGY_AXES}, {"type": "null"}]})
}

fn resolution_strategy_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    json_schema!({"type": "string", "enum": RESOLUTION_STRATEGIES})
}

fn optional_backfill_mode_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    json_schema!({"anyOf": [{"type": "string", "enum": BACKFILL_MODES}, {"type": "null"}]})
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PingRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackfillEmbeddingsRequest {
    /// List the candidates and change nothing. Default false. In
    /// "model_mismatch" mode a dry run still makes one embedding call, to
    /// learn the provider's own name for the configured model.
    pub dry_run: Option<bool>,
    /// Sweep mode. "missing" (default): embed h_mems whose exact canonical
    /// passage has no embedding row. "model_mismatch": re-embed stored
    /// rows whose recorded model matches neither the current requested
    /// embedding model nor its provider-confirmed actual form — the same
    /// two-form predicate the search gate excludes on, so a model
    /// migration's gated-out rows are restored rather than silently
    /// invisible.
    #[schemars(schema_with = "optional_backfill_mode_schema")]
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticSearchRequest {
    /// The question or topic to search for. Must not be blank.
    pub query: String,
    /// Maximum results (default 10; values outside 1-50 are clamped).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FederatedSearchRequest {
    /// The question or topic to search for. Must not be blank.
    pub query: String,
    /// Maximum results (default 10; values outside 1-50 are clamped).
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegQueryRequest {
    /// Optional Regulation path prefix. Matches the exact path or dot-delimited
    /// descendants (for example, `reg.skill` includes `reg.skill.<id>`).
    /// At least one of `namespace` or `window_seconds` must be present: a
    /// request with neither is the truncated-argument signature and is
    /// rejected as `invalid_argument`, not defaulted.
    pub namespace: Option<String>,
    /// Lookback window in seconds (default: 3600 = 1 hour) when `namespace`
    /// is set. Required when `namespace` is absent (see above).
    pub window_seconds: Option<u64>,
    /// Maximum events to return (default 100; must be positive). The
    /// oldest events in the window are returned first; `limit_reached` in
    /// the response is true when more may exist.
    pub limit: Option<usize>,
}

// The enum's doc comment is its schema description: it stays one line.
/// Which rows entity recall returns.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecallType {
    PerspectiveScoped,
    EntityWide,
    #[default]
    Both,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecallRequest {
    /// The exact entity to recall. Give this, or `ontology_axis` with
    /// `ontology_value` — exactly one of the two.
    pub entity: Option<String>,
    /// Recall shape for entity recall: `perspective_scoped` (rows whose
    /// perspective is the curator), `entity_wide` (all h_mems for the
    /// entity), or `both`. Defaults to `both`. Unused with `ontology_axis`.
    #[serde(default)]
    pub recall_shape: MemoryRecallType,
    /// Ontology axis to recall along instead of an entity (P5.4). Requires
    /// `ontology_value`.
    #[schemars(schema_with = "optional_ontology_axis_schema")]
    pub ontology_axis: Option<String>,
    /// The term to match on `ontology_axis` (e.g. `bibo:Article` for
    /// `dc_type`, `omc` for `ontology_namespace`). Requires `ontology_axis`.
    pub ontology_value: Option<String>,
    /// Maximum h_mems to return per scope, newest first (default 100;
    /// values outside 1-500 are clamped). `total` and `limit_reached` in
    /// the response say what the limit cut.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlgedonicLogRequest {
    /// Lookback window in hours (default 24).
    pub hours: Option<u32>,
}

/// Consultation request — a delegated agent asks the curator a question.
/// The curator searches its semantic + episodic memory for relevant
/// fragments and returns them as a structured consultation response.
///
/// This is a memory-grounded consultation, not a full curator agent turn:
/// the server embeds the query and returns stored fragments; it does not
/// generate a response. A synthesized response requires the in-process
/// Curator agent (`CuratorAgentServer`), which lives in the zed process.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CuratorConsultRequest {
    /// The question or topic to consult the curator about. Must not be blank.
    pub query: String,
    /// Maximum fragments per scope (default 5; values outside 1-20 are
    /// clamped).
    pub limit: Option<usize>,
}

// The ownership enum's doc comment is its schema description: it stays
// one line. Its contract — a closed vocabulary; `unknown` is explicit and
// an omitted or invented value is rejected — is pinned by the tests below.
/// Who can act on a reported skill-use failure.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillUseFailureOrigin {
    SkillContract,
    AgentExecution,
    ToolImplementation,
    ProviderTransport,
    EnvironmentOrBaseline,
    OperatorInterruption,
    ExpectedAbsence,
    Unknown,
}

impl SkillUseFailureOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SkillContract => "skill_contract",
            Self::AgentExecution => "agent_execution",
            Self::ToolImplementation => "tool_implementation",
            Self::ProviderTransport => "provider_transport",
            Self::EnvironmentOrBaseline => "environment_or_baseline",
            Self::OperatorInterruption => "operator_interruption",
            Self::ExpectedAbsence => "expected_absence",
            Self::Unknown => "unknown",
        }
    }
}

/// Skill-use issue report — filed when an MCP tool call fails or produces
/// unexpected output while a skill runs. Stored as an h_mem under
/// `skill_use_issue:<skill_name>` so the reports are found by
/// `curator_memory_recall` and `curator_semantic_search`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportSkillUseIssueRequest {
    /// The skill manifest ID (e.g. "superforecasting", "scenario-planning").
    /// It becomes part of the entity key: lowercase [a-z0-9:._-] only.
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
    /// Optional granular symptom classification (e.g. "wrong_inputs",
    /// "missing_fields", "timeout", "unexpected_empty_result",
    /// "schema_mismatch"). Ownership belongs in `failure_origin`.
    pub failure_type: Option<String>,
    /// The subsystem that owns the failure. Callers must choose one controlled
    /// value, including explicit `unknown`; omission is rejected.
    pub failure_origin: SkillUseFailureOrigin,
}

#[cfg(test)]
mod tests {
    use super::{ReportSkillUseIssueRequest, SkillUseFailureOrigin};

    /// expect: "Skill-use reports cannot invent new ownership categories."
    /// [P8] Motivating: Semantic Grounding — failure ownership is a closed vocabulary.
    /// [P4] Constraining: Clear Boundaries — unknown ownership is explicit, not a free string.
    /// pre: request JSON carries all required report fields and an invalid failure_origin
    /// post: deserialization rejects the request
    #[test]
    fn skill_use_failure_origin_is_closed_vocabulary() {
        let request = serde_json::json!({
            "skill_name": "program-manager",
            "tool_name": "terminal",
            "step_ordinal": 1,
            "error": "command failed",
            "tool_input": null,
            "failure_type": "invalid_command",
            "failure_origin": "invented_owner"
        });

        let parsed = serde_json::from_value::<ReportSkillUseIssueRequest>(request);
        assert!(
            parsed.is_err(),
            "an invented failure origin must be rejected by the request contract"
        );
    }

    #[test]
    fn omitted_skill_use_failure_origin_is_rejected() {
        let request = serde_json::json!({
            "skill_name": "program-manager",
            "tool_name": "terminal",
            "step_ordinal": 1,
            "error": "command failed",
            "tool_input": null,
            "failure_type": "invalid_command"
        });
        let parsed = serde_json::from_value::<ReportSkillUseIssueRequest>(request);
        assert!(
            parsed.is_err(),
            "callers must send explicit unknown rather than rely on an omitted legacy field"
        );
    }

    #[test]
    fn skill_use_failure_origins_round_trip_the_closed_wire_vocabulary() {
        for (wire, expected) in [
            ("skill_contract", SkillUseFailureOrigin::SkillContract),
            ("agent_execution", SkillUseFailureOrigin::AgentExecution),
            (
                "tool_implementation",
                SkillUseFailureOrigin::ToolImplementation,
            ),
            (
                "provider_transport",
                SkillUseFailureOrigin::ProviderTransport,
            ),
            (
                "environment_or_baseline",
                SkillUseFailureOrigin::EnvironmentOrBaseline,
            ),
            (
                "operator_interruption",
                SkillUseFailureOrigin::OperatorInterruption,
            ),
            ("expected_absence", SkillUseFailureOrigin::ExpectedAbsence),
            ("unknown", SkillUseFailureOrigin::Unknown),
        ] {
            let parsed: SkillUseFailureOrigin =
                serde_json::from_value(serde_json::json!(wire)).expect("allowed origin");
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_str(), wire);
        }
    }
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
/// self-assessed confidence). No process raises or lowers it afterwards
/// except a `memory_update` call: goal and forecast scores do not write
/// confidence (memory-system-specification.md §11, row 4).
///
/// Evidence-grounding: the `evidence_h_mem_id` field must cite a specific
/// episodic h_mem ID that supports this memory. The tool rejects inserts
/// without a citation. This is Dunning's structured-reflection principle:
/// the model must ground its assertion in evidence, not free-associate.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryInsertRequest {
    /// The entity (subject) of the memory — e.g. "company:AAPL", "skill:superforecasting".
    pub entity: String,
    /// The attribute (predicate) of the memory — e.g. "revenue_trend", "calibration_gap".
    pub attribute: String,
    /// The value (object) of the memory, as an arbitrary JSON value.
    ///
    /// Supply complete JSON text. The shared [`AnyJsonValue`] contract decodes
    /// it before storage; objects, arrays, scalars and literal strings retain
    /// their types. For example, an object is `"{\"text\":\"lesson\"}"`.
    pub value: AnyJsonValue,
    /// The episodic h_mem ID that supports this memory (evidence-grounding
    /// requirement). The tool rejects inserts without a citation.
    pub evidence_h_mem_id: String,
    /// Optional: a human-readable note explaining the reasoning behind this
    /// memory. Stored in the h_mem's value as a `_note` field; a value that
    /// is not a JSON object is stored as `{"value": ..., "_note": ...}`.
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
#[serde(deny_unknown_fields)]
pub struct MemoryUpdateRequest {
    /// The h_mem ID to update.
    pub h_mem_id: String,
    /// The new confidence value, 0.0 to 1.0 inclusive (a value outside the
    /// range is refused). Bayesian-combined with the existing confidence,
    /// not replaced: 0.5 leaves it unchanged. A memory whose combined
    /// confidence is at or below the editor's consolidation floor
    /// (`kask.memory.confidence_floor`, default 0.3) is deleted by the
    /// editor's next consolidation pass.
    pub new_confidence: f64,
    /// Optional: a new value to replace the existing one. Missing recall text
    /// is derived automatically and the canonical passage is re-embedded.
    /// The old row and replacement plus vector change in one transaction;
    /// failure preserves the old memory. Use the returned replacement h_mem_id
    /// for subsequent updates.
    ///
    /// Supply complete JSON text for the replacement value. Omit this field
    /// to change confidence only; JSON text `"null"` represents a null value.
    pub new_value: Option<AnyJsonValue>,
    /// Optional: reason for the confidence update (e.g. "Brier score 0.12 on
    /// resolved forecast", "contradicted by newer observation").
    pub reason: Option<String>,
}

/// Resolve a contradiction between two or more memories.
///
/// This is the therapy process tool — it resolves cognitive dissonance in
/// the memory store by forgetting or updating contradictory h_mems.
/// Requires operator approval (the curator proposes; the operator approves).
///
/// Grounding: Festinger's three dissonance resolution strategies
/// (`Universal_Principles_of_Design:39`): reduce importance (lower
/// confidence), add consonant (insert reconciling memory), remove dissonant
/// (forget — delete the row).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryResolveContradictionRequest {
    /// The h_mem IDs involved in the contradiction.
    pub h_mem_ids: Vec<String>,
    /// The resolution strategy: "forget" (delete the dissonant h_mem from
    /// the database) or "update_confidence" (set the contradicted one's
    /// confidence to `new_confidence`).
    #[schemars(schema_with = "resolution_strategy_schema")]
    pub strategy: String,
    /// The h_mem ID to act on (the one to forget or update). For
    /// "update_confidence", the new confidence value must be provided.
    pub target_h_mem_id: String,
    /// For "update_confidence": the confidence to set, 0.0 to 1.0. It
    /// replaces the stored confidence (memory_update combines instead).
    /// A value at or below the editor's consolidation floor
    /// (`kask.memory.confidence_floor`, default 0.3) means the editor's
    /// next consolidation pass deletes the memory. Ignored for "forget".
    pub new_confidence: Option<f64>,
    /// Evidence-grounding: the reason for this resolution (must cite the
    /// contradiction observed). Must not be blank.
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
#[serde(deny_unknown_fields)]
pub struct MemoryPruneRequest {
    /// Maximum age in days (positive). h_mems older than this are
    /// candidates for deletion.
    pub max_age_days: i64,
    /// If set (positive), spare h_mems recalled within this many days. An
    /// h_mem that was recalled recently stays even if it is old — the decay
    /// clock was reset, so it is still active in the recall path.
    pub spare_recalled_within_days: Option<i64>,
    /// Prune ALL layers including knowledge-layer rows (operator rulings,
    /// verified status, reified lessons). Opt-in: the default scope is turn
    /// storage only — the episodic forgetting valve without the power to
    /// destroy durable knowledge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_layers: Option<bool>,
    /// If set, prune ONLY h_mems whose entity starts with one of these
    /// prefixes (e.g. ["skill_use_issue:"] for the incident log). Opt-in
    /// scoped valve: the default scope stays turn storage only, and
    /// all_layers=true stays the explicit full-store opt-in. An empty list
    /// is rejected — a silent no-op prune is a broken feedback loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefixes: Option<Vec<String>>,
}

/// Deduplicate h_mems by normalized string value.
///
/// Scans the curator's h_mems and groups them by (entity, attribute,
/// normalized_value). For each group with 2+ near-duplicate values, the
/// highest-confidence h_mem is kept and the rest are deleted.
/// Non-string values are skipped — structural dedup is
/// the recall-time exact-EAV filter's job.
///
/// Normalization: lowercase, strip punctuation, collapse whitespace.
/// "AAPL." and "aapl" are treated as duplicates.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryDedupRequest {
    /// Maximum h_mems to scan, newest first. Defaults to 10_000 if omitted.
    /// `limit_reached` in the response is true when older rows may have
    /// been left unscanned.
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
/// The tool reads the h_mems stored under exactly
/// `curator:thread:<thread_id>` and returns their IDs and content. The
/// curator reviews them and inserts the lessons worth keeping.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryExtractRequest {
    /// The complete thread id whose turns to list. Must not be blank; a
    /// partial id matches nothing.
    pub thread_id: String,
}
