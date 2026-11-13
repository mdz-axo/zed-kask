//! hkask-mcp-nostr-client — binary entrypoint.
//!
//! Thin wrapper around the client server library. The server, its tools and
//! its configuration live in the library for testability.

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_nostr_client::run().await
}
