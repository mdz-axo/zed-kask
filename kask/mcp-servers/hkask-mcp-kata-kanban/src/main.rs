//! hkask-mcp-kata-kanban — binary entrypoint.
//!
//! Thin wrapper around the kanban server library. The server struct and
//! tool methods live in lib.rs for fuzz testability (P5 Testing Discipline).


#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_kata_kanban::run().await
}
