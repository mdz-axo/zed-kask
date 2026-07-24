//! hkask-mcp-curator — binary entrypoint.
//!
//! Thin wrapper around the curator server library.

#![allow(unused_crate_dependencies)] // All deps used in this binary — lint produces false positives

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    let boot =
        hkask_mcp_server::bootstrap_mcp_server("curator", "hkask.mcp.curator", "HKASK_MCP_HOST")
            .await?;
    hkask_mcp_curator::run(boot.userpod, boot.daemon_client).await
}
