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
