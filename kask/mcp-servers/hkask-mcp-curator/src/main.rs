//! hkask-mcp-curator — binary entrypoint.
//!
//! Thin wrapper around the curator server library.

#![allow(unused_crate_dependencies)] // All deps used in this binary — lint produces false positives

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_curator::run().await
}
