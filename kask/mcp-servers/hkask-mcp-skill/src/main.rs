//! hkask-mcp-skill — binary entrypoint.
//!
//! Thin wrapper around the skill server library. The server struct and
//! tool methods live in lib.rs for fuzz testability (P5 Testing Discipline).

#![allow(unused_crate_dependencies)] // All deps used in this binary — lint produces false positives

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    let boot = hkask_mcp_server::bootstrap_mcp_server("skill", "hkask.mcp.skill", "HKASK_MCP_HOST")
        .await?;
    hkask_mcp_skill::run(boot.userpod, boot.daemon_client).await
}
