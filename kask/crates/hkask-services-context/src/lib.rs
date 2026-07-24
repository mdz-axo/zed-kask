#![forbid(unsafe_code)]
//! hKask Context Service — AgentService, PerAgentMemory, and loop construction.
//!
//! Extracted from `hkask-services`.

// Used via derive macros (serde/thiserror/async_trait) — invisible to unused_crate_dependencies lint
#![allow(unused_crate_dependencies)]

mod context_impl;
pub mod governance;
pub mod infra;
pub mod mcp_server_guard;
pub mod reg_store_slo_provider;
pub mod regulation;
pub mod storage;
pub mod storage_guard;
pub use context_impl::{AgentService, PerAgentMemory};
