//! hkask-mcp-docproc — binary entrypoint.
//!
//! Thin wrapper around the document processing server library. The server struct
//! tool methods live in lib.rs for fuzz testability (P5 Testing Discipline).

#![allow(unused_crate_dependencies)] // All deps used in this binary — lint produces false positives

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    let boot =
        hkask_mcp_server::bootstrap_mcp_server("docproc", "hkask.mcp.docproc", "HKASK_MCP_HOST")
            .await?;
    hkask_mcp_docproc::run(boot.userpod, boot.daemon_client).await
}
