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
pub struct RegStatusRequest {
    pub domain: Option<String>,
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
pub struct TokenListRequest {
    /// Lookback window in seconds (default: 86400 = 24 hours)
    pub window_seconds: Option<u64>,
    /// Optional issuer WebID filter
    pub issuer: Option<String>,
    /// Optional recipient WebID filter
    pub recipient: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UserPodStatusRequest {
    pub userpod_name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryRecallRequest {
    pub entity: String,
    pub memory_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AlgedonicLogRequest {
    pub hours: Option<u32>,
}
