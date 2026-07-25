//! hkask-mcp-codegraph — binary entrypoint.
//!
//! Thin wrapper around the codegraph server library.

#![allow(unused_crate_dependencies)]

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_codegraph::run().await
}
