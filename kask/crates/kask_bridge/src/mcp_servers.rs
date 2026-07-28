//! Canonical registry of built-in kask MCP servers.
//!
//! Single source of truth for the server ID → binary name → description mapping.
//! Previously duplicated in three places (`zed/src/main.rs`, `settings_ui/src/pages/kask_page.rs`,
//! `kask_panel/src/kask_panel.rs`) with drift between them. This module consolidates
//! the list so all consumers reference the same data.
//!
//! The server IDs here match the keys used in `KaskMcpSettingsContent::overrides`
//! and the `context_servers` entries registered with zed's `ContextServerStore`.

/// A built-in kask MCP server descriptor.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinMcpServer {
    /// The server ID used in settings (`kask.mcp.overrides`) and as the
    /// `ContextServerId` when registering with zed's `ContextServerStore`.
    pub id: &'static str,
    /// The binary name (without path) of the MCP server executable.
    /// Resolved via `HKASK_MCP_{ID}_BIN` env var or PATH lookup at launch time.
    pub binary: &'static str,
    /// Human-readable description shown in the settings UI and kask panel.
    pub description: &'static str,
}

/// The canonical list of built-in kask MCP servers.
///
/// Order is stable and meaningful — the kask panel uses index-based selection.
pub const BUILT_IN_MCP_SERVERS: &[BuiltinMcpServer] = &[
    BuiltinMcpServer {
        id: "codegraph",
        binary: "hkask-mcp-codegraph",
        description: "Codegraph — code structure query and traversal",
    },
    BuiltinMcpServer {
        id: "companies",
        binary: "hkask-mcp-companies",
        description: "Companies — company research and filings",
    },
    BuiltinMcpServer {
        id: "condenser",
        binary: "hkask-mcp-condenser",
        description: "Condenser — context condensation and summarization",
    },
    BuiltinMcpServer {
        id: "corpus",
        binary: "hkask-mcp-corpus",
        description: "Corpus — document corpus and QA generation",
    },
    BuiltinMcpServer {
        id: "curator",
        binary: "hkask-mcp-curator",
        description: "Curator — regulation cascade and algedonic signals",
    },
    BuiltinMcpServer {
        id: "kata-kanban",
        binary: "hkask-mcp-kata-kanban",
        description: "Kata Kanban — improvement kata board",
    },
    BuiltinMcpServer {
        id: "media",
        binary: "hkask-mcp-media",
        description: "Media — image generation and media workflows",
    },
    BuiltinMcpServer {
        id: "research",
        binary: "hkask-mcp-research",
        description: "Research — web research and paper search",
    },
    BuiltinMcpServer {
        id: "scenarios",
        binary: "hkask-mcp-scenarios",
        description: "Scenarios — scenario planning and forecasting",
    },
    BuiltinMcpServer {
        id: "training",
        binary: "hkask-mcp-training",
        description: "Training — LoRA training configuration and audit",
    },
];

/// Just the server IDs, as a static slice of `&str`.
/// Convenience for consumers that only need the ID list (e.g. `kask_panel`).
pub const BUILT_IN_MCP_SERVERS_IDS: &[&str] = &[
    "codegraph",
    "companies",
    "condenser",
    "corpus",
    "curator",
    "kata-kanban",
    "media",
    "research",
    "scenarios",
    "training",
];

/// The server list as `(id, description)` pairs.
/// Convenience for the settings UI which renders `(id, description)` rows.
pub const BUILT_IN_MCP_SERVERS_PAIRS: &[(&str, &str)] = &[
    (
        "codegraph",
        "Codegraph — code structure query and traversal",
    ),
    ("companies", "Companies — company research and filings"),
    (
        "condenser",
        "Condenser — context condensation and summarization",
    ),
    ("corpus", "Corpus — document corpus and QA generation"),
    (
        "curator",
        "Curator — regulation cascade and algedonic signals",
    ),
    ("kata-kanban", "Kata Kanban — improvement kata board"),
    ("media", "Media — image generation and media workflows"),
    ("research", "Research — web research and paper search"),
    ("scenarios", "Scenarios — scenario planning and forecasting"),
    (
        "training",
        "Training — LoRA training configuration and audit",
    ),
];

/// Look up a server by ID.
#[must_use]
pub fn find_server(id: &str) -> Option<&'static BuiltinMcpServer> {
    BUILT_IN_MCP_SERVERS.iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_servers_have_unique_ids() {
        let mut ids: Vec<&str> = BUILT_IN_MCP_SERVERS.iter().map(|s| s.id).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate server IDs found");
    }

    #[test]
    fn all_binaries_follow_naming_convention() {
        for s in BUILT_IN_MCP_SERVERS {
            assert!(
                s.binary.starts_with("hkask-mcp-"),
                "binary '{}' does not follow 'hkask-mcp-*' convention",
                s.binary
            );
        }
    }

    #[test]
    fn find_server_returns_known_ids() {
        assert!(find_server("codegraph").is_some());
        assert!(find_server("kata-kanban").is_some());
        assert!(find_server("nonexistent").is_none());
    }

    // The derived arrays below are hand-maintained convenience views over
    // BUILT_IN_MCP_SERVERS. Without these tests they can silently drift the
    // moment a server is added to BUILT_IN_MCP_SERVERS without updating the
    // derived slices (the settings UI / kask panel would then drop the new
    // server while the runtime registry served it).
    #[test]
    fn ids_slice_matches_main_registry() {
        let expected: Vec<&str> = BUILT_IN_MCP_SERVERS.iter().map(|s| s.id).collect();
        let actual: Vec<&str> = BUILT_IN_MCP_SERVERS_IDS.to_vec();
        assert_eq!(
            actual, expected,
            "BUILT_IN_MCP_SERVERS_IDS is out of sync with BUILT_IN_MCP_SERVERS"
        );
    }

    #[test]
    fn pairs_slice_matches_main_registry() {
        let expected: Vec<(&str, &str)> = BUILT_IN_MCP_SERVERS
            .iter()
            .map(|s| (s.id, s.description))
            .collect();
        let actual: Vec<(&str, &str)> = BUILT_IN_MCP_SERVERS_PAIRS.to_vec();
        assert_eq!(
            actual, expected,
            "BUILT_IN_MCP_SERVERS_PAIRS is out of sync with BUILT_IN_MCP_SERVERS"
        );
    }
}
