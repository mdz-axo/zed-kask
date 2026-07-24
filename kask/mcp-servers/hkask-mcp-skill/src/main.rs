//! hkask-mcp-skill — binary entrypoint.
//!
//! Thin wrapper around the skill server library. The server struct and
//! tool methods live in lib.rs for fuzz testability (P5 Testing Discipline).
//!
//! Port-ified (T0.6): the concrete `InferencePort` implementation lived in
//! the deleted `hkask-inference` crate. It is now a `kask_bridge`/`KaskCore`
//! responsibility (T5.1). The standalone binary cannot construct a port
//! without the bridge, so it returns an error directing callers to the
//! in-process path. Production runs the server in-process via `KaskCore`,
//! not as a standalone binary.

#![allow(unused_crate_dependencies)]

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    let _boot = hkask_mcp_server::bootstrap_mcp_server("skill", "hkask.mcp.skill", "HKASK_MCP_HOST")
        .await?;
    Err(hkask_mcp_server::McpError::UnexpectedResponse {
        context: "hkask-mcp-skill standalone binary".into(),
        detail: "InferencePort must be provided by kask_bridge (T5.1); run via KaskCore in-process".into(),
    })
}
