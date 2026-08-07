//! hkask-mcp-scenarios — binary entrypoint.
//!
//! Thin wrapper around the scenarios server library.


#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_scenarios::run().await
}
