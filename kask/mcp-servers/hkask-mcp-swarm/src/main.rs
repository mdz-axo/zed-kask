//! hkask-mcp-swarm — binary entrypoint.
//!
//! Thin wrapper around the swarm server library.

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_swarm::run().await
}
