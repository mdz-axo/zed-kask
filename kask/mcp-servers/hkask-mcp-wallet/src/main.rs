//! hkask-mcp-wallet — binary entrypoint.

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_wallet::run().await
}
