#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask MCP — MCP runtime and tool dispatch
//!
//! Provides the McpRuntime for metered tool dispatch with runaway-loop breaking
//! and cybernetic regulation. This is the heavy runtime layer used by the
//! REPL/API/CLI — MCP server binaries depend on hkask-mcp-server instead.
//!
//! `invoke` performs no per-call authorization; see
//! `tool_port::ToolPort::invoke` for why the prior capability gate was removed. Authority lives in
//! the inference IPC `tool_allowlist` and the per-server MCP env allowlists.

pub mod runtime;
pub mod tool_port;

pub use runtime::{McpRuntime, McpTool, set_spawn_runtime};
pub use tool_port::{ToolFuture, ToolInfo, ToolPort, ToolPortError};

// ── Canonical MCP server registry ─────────────────────────────────────────
