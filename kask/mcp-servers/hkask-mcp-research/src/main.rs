//! hkask-mcp-research — binary entrypoint.
//!
//! Thin wrapper around the research server library. The server struct and
//! tool methods live in lib.rs for fuzz testability (P5 Testing Discipline).

#![allow(unused_crate_dependencies)] // All deps used in this binary — lint produces false positives

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    let boot =
        hkask_mcp_server::bootstrap_mcp_server("research", "hkask.mcp.research", "HKASK_MCP_HOST")
            .await?;
    hkask_mcp_research::run(boot.userpod, boot.daemon_client).await
}
