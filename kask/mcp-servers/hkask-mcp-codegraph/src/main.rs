//! hkask-mcp-codegraph — binary entrypoint.
//!
//! Thin wrapper around the codegraph server library.

#![allow(unused_crate_dependencies)]

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    let boot = hkask_mcp_server::bootstrap_mcp_server(
        "codegraph",
        "hkask.mcp.codegraph",
        "HKASK_MCP_HOST",
    )
    .await?;
    hkask_mcp_codegraph::run(boot.userpod, boot.daemon_client).await
}
