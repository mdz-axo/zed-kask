//! hkask-mcp-curator — binary entrypoint.
//!
//! Thin wrapper around the curator server library.

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_curator::run().await
}
