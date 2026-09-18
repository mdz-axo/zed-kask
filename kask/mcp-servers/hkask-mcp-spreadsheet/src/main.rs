//! hkask-mcp-spreadsheet — binary entrypoint.
//!
//! Thin wrapper around the spreadsheet server library. The server struct and
//! tool methods live in the library for testability.

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_spreadsheet::run().await
}
