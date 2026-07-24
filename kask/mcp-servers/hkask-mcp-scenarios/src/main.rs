//! hkask-mcp-scenarios — binary entrypoint.
//!
//! Thin wrapper around the scenarios server library.

#![allow(unused_crate_dependencies)] // All deps used in the lib — lint produces false positives for the bin

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    let boot = hkask_mcp_server::bootstrap_mcp_server(
        "scenarios",
        "hkask.mcp.scenarios",
        "HKASK_MCP_HOST",
    )
    .await?;
    hkask_mcp_scenarios::run(boot.userpod, boot.daemon_client).await
}
