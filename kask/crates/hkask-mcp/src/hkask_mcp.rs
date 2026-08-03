#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask MCP — MCP runtime and governance
//!
//! Provides the McpRuntime for governed tool dispatch with OCAP,
//! energy budgeting, and cybernetic regulation. This is the heavy
//! runtime layer used by the REPL/API/CLI — MCP server binaries
//! depend on hkask-mcp-server instead.

pub mod runtime;

pub use runtime::{FlatEnergyEstimator, McpRuntime, McpServer, McpTool, ServerStartError};

// ── Canonical MCP server registry ─────────────────────────────────────────
