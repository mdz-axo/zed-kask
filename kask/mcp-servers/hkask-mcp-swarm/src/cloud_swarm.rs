//! ABW cloud swarm tools — split from `cloud_swarm_tools.rs` by capability group.
//!
//! This module is the home for the ABW (Agent Bestiary World) cloud-surface
//! tools. The original `cloud_swarm_tools.rs` was a 2076-line file holding 27 tool
//! methods plus two pure helpers and their tests; this module houses the
//! extracted helpers and the Xaman Ek curator session guard so
//! `cloud_swarm_tools.rs` (the `#[tool_router]` impl) can stay focused on the
//! router assembly and the tool methods themselves.
//!
//! Sub-modules:
//! - `helpers` — pure functions (`build_create_agent_card`,
//!   `extract_execute_response`) with their property tests.
//! - `curator` — the Xaman Ek `CuratorSession` refund guard.

pub mod curator;
pub mod helpers;

// Re-export the pure helpers so `cloud_swarm_tools.rs` and `test_utils` can import
// them from `cloud` without reaching into the sub-module.
pub use helpers::{
    build_agent_update_payload, build_create_agent_card, extract_execute_response,
    unsupported_create_fields, valence_payload,
};
