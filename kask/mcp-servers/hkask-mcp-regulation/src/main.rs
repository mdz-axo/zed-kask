//! hkask-mcp-regulation — binary entrypoint.
//!
//! Thin wrapper around the regulation server library.

#![allow(unused_crate_dependencies)]

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    let boot = hkask_mcp_server::bootstrap_mcp_server(
        "regulation",
        "hkask.mcp.regulation",
        "HKASK_MCP_HOST",
    )
    .await?;
    hkask_mcp_regulation::run(boot.userpod, boot.daemon_client).await
}
