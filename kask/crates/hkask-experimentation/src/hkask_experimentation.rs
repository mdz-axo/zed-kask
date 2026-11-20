#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hkask-experimentation — domain store and types for the experiment registry
//! (repair plan §P8).
//!
//! Owns the SQLCipher-backed experiment registry, typed rubric/prediction/seal
//! contracts, and the canonical D28 registry path resolution shared by both the
//! MCP server (`hkask-mcp-experimentation`) and the host health bridge
//! (`kask_bridge::experimentation_health_bridge`).

pub mod store;
pub mod types;

// ── Registry path (shared by the MCP child and the in-host bridge) ────────

/// Resolve the registry DB path: `HKASK_EXPERIMENTATION_DB` when set to a
/// non-empty value, else the per-agent default under the hKask data dir.
///
/// Both the MCP child (`hkask_mcp_experimentation::server::run`) and the
/// in-host health bridge (`kask_bridge::experimentation_health_bridge`) open
/// the registry through this one function — parity is structural, not
/// mirrored: a drift here would read a different registry than the one the
/// tools write.
pub fn registry_path() -> std::path::PathBuf {
    registry_path_from(std::env::var("HKASK_EXPERIMENTATION_DB").ok().as_deref())
}

/// The pure resolution core — testable without process-global env mutation.
pub fn registry_path_from(env_value: Option<&str>) -> std::path::PathBuf {
    match env_value.filter(|value| !value.trim().is_empty()) {
        Some(value) => std::path::PathBuf::from(value),
        None => hkask_types::agent_paths::resolve_under_data_dir(
            &hkask_types::agent_paths::mcp_server_db("experimentation", "experimentation"),
        ),
    }
}

#[cfg(test)]
mod registry_path_tests {
    use super::*;

    /// The parity contract: a non-empty override is honored, and
    /// empty/whitespace/absent all fall to the same per-agent default —
    /// the bridge and the server cannot resolve different registries.
    #[test]
    fn registry_path_override_and_default() {
        let default = registry_path_from(None);
        assert_eq!(default, registry_path_from(Some("")));
        assert_eq!(default, registry_path_from(Some("   ")));
        assert_eq!(
            registry_path_from(Some("/tmp/alt-experimentation.db")),
            std::path::PathBuf::from("/tmp/alt-experimentation.db")
        );
        // The default is the per-agent experimentation registry under the data
        // dir — the same relative path the MCP child serves.
        assert_eq!(
            default,
            hkask_types::agent_paths::resolve_under_data_dir(
                &hkask_types::agent_paths::mcp_server_db("experimentation", "experimentation"),
            )
        );
    }
}
