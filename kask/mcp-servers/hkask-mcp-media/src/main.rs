//! hkask-mcp-media — binary entrypoint.
//!
//! Thin wrapper around the research-media curation server library. Tool methods
//! live in the library's tools modules so their contracts can be tested directly.

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_media::run().await
}
