//! STUB (T0.6): original server depended on deleted crate `hkask-services-kata-kanban`.
//! Functionality moves to `kask_bridge`/`KaskCore` (T5.7). Re-implement over ports.
//!
//! Original module provided 18 MCP tools for kanban board and task management
//! backed by `KanbanService` over `HMemStore`. Both dependencies are deletion
//! candidates (T5.7) and have been removed from the workspace. The tool
//! implementations, `KanbanServer` struct, and `default_columns()` helper are
//! intentionally dropped pending re-implementation against the new ports.
//!
//! See `kask/docs/specs/seam-specs.md` (T0.6) for the migration plan.

#![forbid(unsafe_code)]
#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

pub mod pko;
pub mod types;

/// Run the kanban MCP server (used by binary target).
///
/// Currently a stub: returns an error immediately because the underlying
/// `hkask-services-kata-kanban` crate has been deleted (T0.6). Re-enable by
/// re-implementing the server over `kask_bridge`/`KaskCore` ports (T5.7).
pub async fn run(
    _userpod: String,
    _daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    Err(hkask_mcp_server::McpError::UnexpectedResponse {
        context: "hkask-mcp-kata-kanban".into(),
        detail:
            "not yet ported — depends on deleted hkask-services-kata-kanban \
             (see kask/docs/specs/seam-specs.md T0.6)"
                .into(),
    })
}
