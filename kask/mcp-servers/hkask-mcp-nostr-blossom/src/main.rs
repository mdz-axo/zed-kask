//! hkask-mcp-nostr-blossom — binary entrypoint.

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_nostr_blossom::run().await
}
