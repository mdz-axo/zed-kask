//! hkask-mcp-media — binary entrypoint.
//!
//! Thin wrapper around the media generation server library. The server struct
//! tool methods live in lib.rs for fuzz testability (P5 Testing Discipline).

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_media::run().await
}
