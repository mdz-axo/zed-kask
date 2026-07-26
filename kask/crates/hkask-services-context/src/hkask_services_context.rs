#![forbid(unsafe_code)]
//! hKask Context Service — governance, regulation store, and storage guards.
//!
//! Stripped to the modules the MCP servers need (governance + guards).
//! The daemon/Matrix/identity surface was deleted with the cloud-server migration.

// Used via derive macros (serde/thiserror/async_trait) — invisible to unused_crate_dependencies lint
#![allow(unused_crate_dependencies)]

pub mod governance;
pub mod mcp_server_guard;
pub mod storage_guard;
