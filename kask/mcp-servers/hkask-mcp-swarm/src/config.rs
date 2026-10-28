//! Swarm server configuration.
//!
//! Extracted from the swarm server root. `SwarmConfig` is the validated
//! runtime config built from env vars (`from_env`). Defaults are the single
//! source of truth here and must stay in sync with
//! `KaskSwarmSettings::default()` in `kask_bridge/src/settings.rs` (the
//! bridge emits the `HKASK_SWARM_*` env vars this reads).

/// Runtime configuration for the swarm server (local substrate only).
///
/// Defaults are the single source of truth; env vars override.
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    /// Operator-selected default model for local agents without a per-card
    /// model. Configured through `HKASK_SWARM_DEFAULT_AGENT_MODEL`; empty
    /// means local execution uses the host session's default. Local cards
    /// remain unpinned so changing this setting takes effect on the next
    /// swarm server restart; an explicit card model wins.
    pub default_agent_model: String,
    /// Directory containing local agent cards (`<id>/agent_card.json`),
    /// read by `LocalAgentRegistry`. Default `mcp/swarm/agents/curated`
    /// relative to the hKask data directory — the swarm server owns these
    /// cards, so they live under its per-server subtree per the
    /// Standardized Artifact Storage layout. (Previously
    /// `agents/local/curated` at the data root top level, which broke the
    /// per-server subtree invariant.)
    pub local_agents_dir: String,
    /// Directory containing local swarms (`<id>/swarm.json`), read/written by
    /// `LocalSwarmRegistry`. Default `mcp/swarm/swarms` relative to the
    /// hKask data directory.
    pub local_swarms_dir: String,
    /// Whether to start the A2A HTTP gateway (loopback JSON-RPC server that
    /// exposes local agents to external A2A clients). Default `false` (opt-in
    /// — it opens a loopback port). Set `HKASK_A2A_HTTP_ENABLE=1` to enable.
    pub a2a_http_enabled: bool,
    /// The governed MCP server ids local agent cards may declare tools for
    /// (from `HKASK_MCP_SERVER_IDS`, the parent's `BUILT_IN_MCP_SERVERS_IDS`).
    /// `None` = no server-side filtering. When set, agent create/reconfigure
    /// drops any declared tool whose `server` segment is not in this set — a
    /// card must not extend the delegated tool surface beyond the operator's
    /// own governed servers.
    pub allowed_tool_servers: Option<Vec<String>>,
    /// SQLCipher passphrase for the local swarm semantic-memory store (the
    /// `hkask-memory` `MemoryStore` backing the local knowledge tools). Must
    /// be >=8 chars. This is the ONE shared kask DB passphrase — resolved
    /// from `HKASK_DB_PASSPHRASE` (delivered as a credential via the
    /// governed-launch allowlist), same as curator/kanban/research.
    /// If empty or too short, `LazyLocalMemory::get_or_init` returns an error
    /// and `swarm_search_knowledge_local` degrades to an empty result with a
    /// `memory_unconfigured` note (the generate tools proceed unseeded — memory
    /// is an enhancement, not a dependency).
    pub memory_passphrase: String,
    /// On-disk path for the local swarm semantic-memory DB. Default
    /// `<hkask data dir>/mcp/swarm/memory.db` (resolved under the data dir
    /// so the server finds it regardless of CWD). Read from
    /// `HKASK_SWARM_MEMORY_DB` (an absolute override is used as-is).
    pub memory_db_path: String,
    /// Embedding vector dimension for the semantic-memory embedding store.
    /// Only relevant if the embedding-search path is used; the EAV-retrieval
    /// path (`query_deduped`) used by `swarm_search_knowledge_local` does not
    /// depend on it. Default 1024. Read from `HKASK_SWARM_EMBEDDING_DIM`.
    pub embedding_dim: usize,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        // These defaults MUST stay in sync with `KaskSwarmSettings::default()` in
        // `kask/crates/kask_bridge/src/settings.rs`. The bridge emits env vars
        // (`HKASK_SWARM_*`) from its `Default`; this server reads them in
        // `from_env`. The two `Default` impls are deliberately separate
        // (the server crate does not depend on the bridge crate) to avoid a
        // circular dependency — the duplication is the seam between them. If
        // you change a default here, change it there too, and update the
        // `swarm_settings_default_emits_no_env` test in `settings.rs`.
        // All fields here have a counterpart in `KaskSwarmSettings::default()`
        // EXCEPT `memory_passphrase` — there is no bridge-side passphrase
        // setting; the value arrives via the `HKASK_DB_PASSPHRASE` credential
        // (resolved in `run()` from ctx.credentials → env → keychain). There
        // is no last-resort default: an unresolvable passphrase degrades
        // (memory_unconfigured), never falls back open.
        Self {
            default_agent_model: String::new(),
            local_agents_dir: "mcp/swarm/agents/curated".to_string(),
            local_swarms_dir: "mcp/swarm/swarms".to_string(),
            a2a_http_enabled: false,
            allowed_tool_servers: None,
            // No default-passphrase fallback: an unresolvable HKASK_DB_PASSPHRASE
            // degrades (memory_unconfigured) rather than opening a DB on the
            // source-known DEFAULT_PASSPHRASE — the presence-IS-the-toggle
            // contract every sibling server upholds fail-closed. First-run
            // provisioning writes the default into the KEYCHAIN, so the
            // resolver finds it there; the config default stays empty by
            // design (security-audit run 1, R-01, card 295f2859).
            memory_passphrase: String::new(),
            memory_db_path: "mcp/swarm/memory.db".to_string(),
            embedding_dim: 1024,
        }
    }
}

/// Resolve `local_agents_dir` against the hKask data directory.
///
/// A relative path (the default `mcp/swarm/agents/curated`) is joined under the
/// data dir resolved by `hkask_types::agent_paths::resolve_under_data_dir` —
/// this ensures the MCP server finds the same agent cards regardless of where
/// the parent process spawned it (the swarm server inherits Zed's CWD, which
/// is typically the user's home or project root — not the zed-kask repo). An
/// absolute path (operator-set via `HKASK_LOCAL_AGENTS_DIR`) is used as-is.
///
/// Extracted from `from_env` as a pure function so the resolution logic is
/// testable without manipulating process env vars (this crate is
/// `#![forbid(unsafe_code)]`, so `std::env::set_var` is unavailable in tests).
pub fn resolve_local_agents_dir(local_agents_dir: &str) -> String {
    if std::path::Path::new(local_agents_dir).is_absolute() {
        local_agents_dir.to_string()
    } else {
        hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(local_agents_dir))
            .to_string_lossy()
            .to_string()
    }
}

/// Resolve `local_swarms_dir` against the hKask data directory. Same rule as
/// [`resolve_local_agents_dir`]: a relative path (the default
/// `mcp/swarm/swarms`) is joined under the data dir; an absolute path
/// (operator-set via `HKASK_LOCAL_SWARMS_DIR`) is used as-is.
pub fn resolve_local_swarms_dir(local_swarms_dir: &str) -> String {
    if std::path::Path::new(local_swarms_dir).is_absolute() {
        local_swarms_dir.to_string()
    } else {
        hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(local_swarms_dir))
            .to_string_lossy()
            .to_string()
    }
}

/// Non-empty env filter — extracted from `from_env` as a pure function so
/// tests can pin the empty-env fall-through without mutating the process env
/// (this crate has `#![forbid(unsafe_code)]`, so `std::env::set_var` is
/// unavailable in tests). Preserves `from_env`'s resolution chain:
/// `env-var → keychain → default`.
fn non_empty_or_env(input: Option<String>, default: String) -> String {
    input.filter(|s| !s.trim().is_empty()).unwrap_or(default)
}

use hkask_mcp_server::parse_env_warn;

impl SwarmConfig {
    /// Build from environment.
    pub fn from_env() -> Self {
        let default = Self::default();
        let default_agent_model = std::env::var("HKASK_SWARM_DEFAULT_AGENT_MODEL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default.default_agent_model);
        let local_agents_dir = std::env::var("HKASK_LOCAL_AGENTS_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default.local_agents_dir);
        let local_agents_dir = resolve_local_agents_dir(&local_agents_dir);
        let local_swarms_dir = std::env::var("HKASK_LOCAL_SWARMS_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default.local_swarms_dir);
        let local_swarms_dir = resolve_local_swarms_dir(&local_swarms_dir);
        let a2a_http_enabled = parse_env_warn("HKASK_A2A_HTTP_ENABLE", default.a2a_http_enabled);
        let allowed_tool_servers = std::env::var("HKASK_MCP_SERVER_IDS")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            });
        let memory_passphrase = non_empty_or_env(
            hkask_mcp_server::server::resolve_db_passphrase(&std::collections::HashMap::new()).ok(),
            default.memory_passphrase,
        );
        let memory_db_raw = std::env::var("HKASK_SWARM_MEMORY_DB")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default.memory_db_path);
        let memory_db_path = if std::path::Path::new(&memory_db_raw).is_absolute() {
            memory_db_raw
        } else {
            hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(&memory_db_raw))
                .to_string_lossy()
                .to_string()
        };
        let embedding_dim_parsed =
            parse_env_warn("HKASK_SWARM_EMBEDDING_DIM", default.embedding_dim);
        let embedding_dim = if embedding_dim_parsed == 0 {
            tracing::warn!(
                target: "hkask.mcp.env",
                key = "HKASK_SWARM_EMBEDDING_DIM",
                "value must be > 0 — falling back to default"
            );
            default.embedding_dim
        } else {
            embedding_dim_parsed
        };
        Self {
            default_agent_model,
            local_agents_dir,
            local_swarms_dir,
            a2a_http_enabled,
            allowed_tool_servers,
            memory_passphrase,
            memory_db_path,
            embedding_dim,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Degrade closed, never fall back open: the config default is EMPTY —
    /// an unresolvable HKASK_DB_PASSPHRASE must trigger the
    /// memory_unconfigured degradation, never open a DB on the source-known
    /// DEFAULT_PASSPHRASE (security-audit run 1, R-01; the keychain tier
    /// carries the first-run default, so the resolver finds it there).
    #[test]
    fn default_memory_passphrase_is_empty_never_the_source_known_default() {
        let passphrase = SwarmConfig::default().memory_passphrase;
        assert!(passphrase.is_empty(), "config default must be empty");
        assert_ne!(
            passphrase,
            hkask_keystore::passphrase::DEFAULT_PASSPHRASE,
            "never fall back open to the source-shipped default"
        );
    }

    /// `non_empty_or_env` treats empty/whitespace env vars the same as
    /// absent — falls through to `default`. The from-env chain in
    /// `from_env` calls this helper; the test pins the fall-through without
    /// mutating the process env (crate has `#![forbid(unsafe_code)]`).
    #[test]
    fn env_fallback_helper_treats_empty_as_unset() {
        assert_eq!(
            non_empty_or_env(Some(String::new()), "fallback".to_string()),
            "fallback".to_string(),
            "empty env var must fall through"
        );
        assert_eq!(
            non_empty_or_env(Some("  ".to_string()), "fallback".to_string()),
            "fallback".to_string(),
            "whitespace-only env var must fall through"
        );
        assert_eq!(
            non_empty_or_env(Some("real".to_string()), "fallback".to_string()),
            "real".to_string(),
            "non-empty env var must win"
        );
    }
}
