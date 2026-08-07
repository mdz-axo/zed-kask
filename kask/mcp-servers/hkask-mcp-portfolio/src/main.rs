//! hkask-mcp-portfolio — binary entrypoint.
//!
//! Thin wrapper around the portfolio server library. The server struct and
//! tool methods live in the library for testability.


#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_portfolio::run().await
}
