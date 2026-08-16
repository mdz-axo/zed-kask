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
    /// The step ordinal in the skill's flowdef where the failure occurred.
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

/// Request the grounding trend from the central verification ledger.
/// Answers the paper's §4.1 question: "is this getting better?" The
/// trend aggregates across all delegations (cross-tool, cross-server) or
/// filters by agent/source depending on `scope`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GroundingTrendToolRequest {
    /// Scope: "global" (all delegations), "by_agent" (filter by agent_id),
    /// or "by_source" (filter by source tool like "kanban_task_spawn").
    /// Default: "global".
    #[serde(default)]
    pub scope: Option<String>,
    /// Agent id to filter by when `scope == "by_agent"`.
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Source tool to filter by when `scope == "by_source"`.
    #[serde(default)]
    pub source: Option<String>,
}

/// Request recent grounding violations from the central verification ledger.
/// Returns delegations with nulled fields or narrative leaks since `since`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GroundingViolationsToolRequest {
    /// ISO 8601 timestamp — return violations at or after this time.
    /// Default: 24 hours ago.
    #[serde(default)]
    pub since: Option<String>,
    /// Scope: same as `GroundingTrendToolRequest.scope`.
    #[serde(default)]
    pub scope: Option<String>,
    /// Agent id to filter by when `scope == "by_agent"`.
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Source tool to filter by when `scope == "by_source"`.
    #[serde(default)]
    pub source: Option<String>,
}

/// Request a grounding coverage report from the central verification ledger.
/// Reports which agent types have grounding contracts vs. which have
/// delegations but no contract (the coverage gap, paper §6).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GroundingCoverageToolRequest {}
