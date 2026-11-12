//! hkask-mcp-nostr-relay — binary entrypoint.
//!
//! Thin wrapper around the relay server library. The server, its tools, the
//! accept layer and the admission rule live in the library for testability.

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_nostr_relay::run().await
}
