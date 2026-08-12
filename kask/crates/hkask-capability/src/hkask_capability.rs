#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask Capability — tool dispatch port and taint labels.
//!
//! # No per-call capability gate
//!
//! This crate previously minted `DelegationToken`s that `McpRuntime::invoke`
//! checked against the invoked tool. That gate was removed: every production
//! mint site derived the token's `resource_id` from the same tool name it then
//! passed to `invoke`, so the check compared a value against itself and denied
//! nothing while adding work to every tool call.
//!
//! Capability *separation* is still enforced, at the boundaries that hold a list
//! the caller cannot choose: the per-request `tool_allowlist` on the inference
//! IPC dispatch, each swarm agent card's `mcp_tools` allowlist, and the
//! per-server MCP env/credential allowlists. What remains here is the dispatch
//! port itself plus the FIDES taint labels that drive the information-flow
//! policy check in the manifest executor.

pub mod token_types;
pub mod tool_port;
pub mod tool_taint;

pub use token_types::SYSTEM_MAX_RECURSION;
pub use tool_port::{ToolFuture, ToolInfo, ToolPort, ToolPortError};
