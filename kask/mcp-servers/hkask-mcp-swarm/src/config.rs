//! Swarm server configuration — backend mode and ABW client config.
//!
//! Extracted from the swarm server root. `SwarmConfig` is the validated
//! runtime config built from env vars (`from_env`); `SwarmMode` selects the
//! `Abw` (v1) vs `Local` (v2) backend. Defaults are the single source of truth
//! here and must stay in sync with `KaskSwarmSettings::default()` in
//! `kask_bridge/src/settings.rs` (the bridge emits the `HKASK_ABW_*`/`HKASK_SWARM_*`
//! env vars this reads).

/// Which backend the swarm server talks to.
///
/// `Abw` (default, v1) routes all tools to the Agent Bestiary World REST API.
/// `Local` (v2, §15) routes to zed-kask's local substrate crates
/// (`hkask-ledger`, `hkask-inference`). Both tool sets are
/// available in either mode — the operator chooses the tool explicitly.
/// There is no `Hybrid` routing layer (§15.1.8 — rejected: the operator does
/// the routing by choosing the tool).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum SwarmMode {
    /// Route to Agent Bestiary World (v1 behavior).
    #[default]
    Abw,
    /// Route to local substrate crates (v2, §15).
    Local,
}

impl std::fmt::Display for SwarmMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Abw => write!(f, "abw"),
            Self::Local => write!(f, "local"),
        }
    }
}

impl std::str::FromStr for SwarmMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "abw" => Ok(Self::Abw),
            "local" => Ok(Self::Local),
            other => Err(format!(
                "unknown swarm mode '{other}' — expected 'abw' or 'local'"
            )),
        }
    }
}

/// Runtime configuration for the ABW client. Validated at construction.
///
/// Defaults are the single source of truth; env vars override. No secrets are
/// stored here — `api_key` is the resolved credential value, passed in from
/// the `ServerContext` credentials map at server construction.
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    /// Which backend to route to (§15). Default `Abw` (v1 behavior).
    pub mode: SwarmMode,
    /// ABW API base URL (apex — endpoints are `/api/*` under it).
    pub api_base_url: String,
    /// Resolved ABW API key. `None` = unauthenticated (catalogue-only mode).
    pub api_key: Option<String>,
    /// Per-dispatch credit ceiling for future spend tools (S3 budget gate).
    pub max_credits_per_dispatch: u32,
    /// Whether Xaman Ek sessions may be initiated without per-call opt-in (S5 policy).
    pub curator_consent_default: bool,
    /// Default model id for newly created ABW agents when the caller omits
    /// `model`. Operator-configurable via `HKASK_ABW_DEFAULT_AGENT_MODEL` so
    /// the default is not a code literal that goes stale when the provider
    /// renames/deprecates the model (KA-05).
    pub default_agent_model: String,
    /// Directory containing local agent cards (`<id>/agent_card.json`),
    /// read by `LocalAgentRegistry` in `Local` mode. Default
    /// `agents/local/curated` relative to the working directory.
    pub local_agents_dir: String,
    /// Directory containing local swarms (`<id>/swarm.json`), read/written by
    /// `LocalSwarmRegistry`. Default `agents/local/swarms` relative to the
    /// hKask data directory. The local replica of an ABW workspace roster.
    pub local_swarms_dir: String,
    /// Whether to start the A2A HTTP gateway (loopback JSON-RPC server that
    /// exposes local agents to external A2A clients). Default `false` (opt-in
    /// — it opens a loopback port). Set `HKASK_A2A_HTTP_ENABLE=1` to enable.
    pub a2a_http_enabled: bool,
    /// The governed MCP server ids this server may declare tools for (from
    /// `HKASK_MCP_SERVER_IDS`, the parent's `BUILT_IN_MCP_SERVERS_IDS`).
    /// `None` = no server-side filtering (backward compatible). When set,
    /// `swarm_clone_to_local` drops any cloned card tool whose `server`
    /// segment is not in this set — a third-party ABW card must not extend
    /// the delegated tool surface beyond the operator's own governed servers.
    pub allowed_tool_servers: Option<Vec<String>>,
    /// SQLCipher passphrase for the local swarm semantic-memory store (the
    /// `hkask-memory` `MemoryStore` backing the local knowledge tools). Must
    /// be >=8 chars. Pre-release default `"allostery"` (the kask-wide default for
    /// any user-facing passphrase that isn't an internally generated key) — the
    /// local knowledge tools work out of the box without operator config. Override
    /// via `HKASK_SWARM_MEMORY_PASSPHRASE` for a real secret; if an existing store
    /// was created under a different passphrase, open fails and
    /// `swarm_search_knowledge_local` degrades to an empty result with a
    /// `memory_unconfigured` note (the generate tools proceed unseeded — memory
    /// is an enhancement, not a dependency).
    pub memory_passphrase: String,
    /// On-disk path for the local swarm semantic-memory DB. Default
    /// `<hkask data dir>/swarm_memory.db` (resolved under the data dir so the
    /// server finds it regardless of CWD). Read from `HKASK_SWARM_MEMORY_DB`
    /// (an absolute override is used as-is).
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
        // (`HKASK_ABW_*` / `HKASK_SWARM_*`) from its `Default`; this server reads
        // them in `from_env`. The two `Default` impls are deliberately separate
        // (the server crate does not depend on the bridge crate) to avoid a
        // circular dependency — the duplication is the seam between them. If
        // you change a default here, change it there too, and update the
        // `swarm_settings_default_emits_no_env` test in `settings.rs`.
        // All fields here have a counterpart in `KaskSwarmSettings::default()`.
        // The bridge emits env vars for non-default values; the server reads
        // them in `from_env`. Fields the bridge defaults to empty/false use
        // the server's defaults here as the fallback.
        Self {
            mode: SwarmMode::default(),
            api_base_url: "https://agent-bestiary.world".to_string(),
            api_key: None,
            max_credits_per_dispatch: 50,
            curator_consent_default: false,
            default_agent_model: "claude-haiku-4-5-20251001".to_string(),
            local_agents_dir: "agents/local/curated".to_string(),
            local_swarms_dir: "agents/local/swarms".to_string(),
            a2a_http_enabled: false,
            allowed_tool_servers: None,
            memory_passphrase: "allostery".to_string(),
            memory_db_path: "swarm_memory.db".to_string(),
            embedding_dim: 1024,
        }
    }
}

/// Resolve `local_agents_dir` against the hKask data directory.
///
/// A relative path (the default `agents/local/curated`) is joined under the
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
/// `agents/local/swarms`) is joined under the data dir; an absolute path
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

impl SwarmConfig {
    /// Build from environment, returning the config plus any warnings about
    /// degraded operation (missing key → catalogue-only mode).
    pub fn from_env(api_key: Option<String>) -> (Self, Option<String>) {
        let default = Self::default();
        let mode = std::env::var("HKASK_SWARM_MODE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.parse().ok())
            .unwrap_or(default.mode);
        let api_base_url = std::env::var("HKASK_ABW_API_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default.api_base_url);
        let max_credits_per_dispatch = std::env::var("HKASK_ABW_MAX_CREDITS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default.max_credits_per_dispatch);
        let curator_consent_default = std::env::var("HKASK_ABW_CURATOR_CONSENT_DEFAULT")
            .ok()
            .and_then(|s| s.trim().to_lowercase().parse::<bool>().ok())
            .unwrap_or(default.curator_consent_default);
        let default_agent_model = std::env::var("HKASK_ABW_DEFAULT_AGENT_MODEL")
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
        let a2a_http_enabled = std::env::var("HKASK_A2A_HTTP_ENABLE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.trim().to_lowercase().parse::<bool>().ok())
            .unwrap_or(default.a2a_http_enabled);
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
        let memory_passphrase = std::env::var("HKASK_SWARM_MEMORY_PASSPHRASE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default.memory_passphrase);
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
        let embedding_dim = std::env::var("HKASK_SWARM_EMBEDDING_DIM")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .filter(|d| *d > 0)
            .unwrap_or(default.embedding_dim);
        let warning = if api_key.is_none() && mode == SwarmMode::Abw {
            Some(
                "HKASK_ABW_API_KEY not set and mode=abw — swarm server in catalogue-only mode; \
                 authenticated tools (get_swarm, execute_agent, curate) will return Auth errors"
                    .to_string(),
            )
        } else if mode == SwarmMode::Local {
            // In local mode, the ABW key is irrelevant — no warning needed.
            // But warn if the local agents dir doesn't exist or is empty, so
            // the operator doesn't silently run with zero agents (the
            // startup-failure-signal rule).
            if !std::path::Path::new(&local_agents_dir).exists() {
                Some(format!(
                    "HKASK_SWARM_MODE=local but local agents dir '{local_agents_dir}' does not exist \
                     — local tools will return zero agents. Create the directory and add \
                     agent cards (<id>/agent_card.json), or set HKASK_LOCAL_AGENTS_DIR."
                ))
            } else {
                None
            }
        } else {
            None
        };
        (
            Self {
                mode,
                api_base_url,
                api_key,
                max_credits_per_dispatch,
                curator_consent_default,
                default_agent_model,
                local_agents_dir,
                local_swarms_dir,
                a2a_http_enabled,
                allowed_tool_servers,
                memory_passphrase,
                memory_db_path,
                embedding_dim,
            },
            warning,
        )
    }
}
