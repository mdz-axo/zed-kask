//! hkask-mcp-scenarios — binary entrypoint.
//!
//! Thin wrapper around the scenarios server library.

#![allow(unused_crate_dependencies)] // All deps used in the lib — lint produces false positives for the bin

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_scenarios::run().await
}
