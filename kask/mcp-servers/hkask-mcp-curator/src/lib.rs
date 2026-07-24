#![forbid(unsafe_code)]
//! hkask-mcp-curator — Curator MCP server library.
//!
//! Exposes the Curator's regulatory surface as MCP tools:
//! system health, escalation management, Regulation observability,
//! cross-pod semantic search, memory recall, spec drift detection,
//! and algedonic event history.

// NOTE: This server is currently stubbed at the module level. The original
// implementation depended on `hkask-storage` and `hkask-services-context`,
// both of which are deletion candidates (T5.7 / T0.6) in the zed-kask merge.
// The tool implementations have been removed until those crates are ported
// or replaced. See `kask/docs/specs/seam-specs.md` for the migration plan.

pub async fn run(
    _userpod: String,
    _daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    Err(hkask_mcp_server::McpError::UnexpectedResponse {
        context: "hkask-mcp-curator".into(),
        detail: "not yet ported — depends on deleted hkask-storage/hkask-services-context (see kask/docs/specs/seam-specs.md T0.6)".into(),
    })
}
