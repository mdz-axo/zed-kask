//! Tool input schema helpers — re-exported from `hkask-types`.
//!
//! The canonical implementation lives in [`hkask_types::tool_schema`] so that
//! pure domain crates (e.g. `hkask-condenser`) can use [`AnyJsonValue`] and
//! [`find_boolean_schema_positions`] without depending on `hkask-mcp-server`,
//! which drags in `rmcp`, `reqwest`, `hkask-keystore`, `hkask-storage`, and
//! `tracing-subscriber` as transitive deps. This module re-exports the same
//! items so existing `use hkask_mcp_server::{AnyJsonValue,
//! find_boolean_schema_positions};` imports keep working without changes.
//!
//! [`hkask_types::tool_schema`]: hkask_types::tool_schema

pub use hkask_types::tool_schema::{AnyJsonValue, find_boolean_schema_positions};
