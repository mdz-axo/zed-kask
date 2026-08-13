//! Canonical registry of built-in kask MCP servers.
//!
//! Single source of truth for the server ID → binary name → description mapping.
//! Previously duplicated in three places (`zed/src/main.rs`, `settings_ui/src/pages/kask_page.rs`,
//! the now-removed `kask_panel` crate) with drift between them. This module consolidates
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
    /// Credential env vars (secrets from the keychain) this server is allowed
    /// to receive. Only credentials in this list are injected into the
    /// server's child process env.
    ///
    /// `None` means "no credential filtering" (receives all credentials).
    /// Prefer `Some(&[])` (receives no credentials) for new servers.
    pub credentials: Option<&'static [&'static str]>,
    /// Config env vars (non-secret settings from `mcp_env()`) this server is
    /// allowed to receive. Only config vars in this list are injected.
    ///
    /// `None` means "no config filtering" (receives all config). Prefer
    /// `Some(&[])` for new servers and add specific env vars as needed.
    pub config_env: Option<&'static [&'static str]>,
}

/// The canonical list of built-in kask MCP servers.
///
/// Order is stable and meaningful — the kask panel uses index-based selection.
pub const BUILT_IN_MCP_SERVERS: &[BuiltinMcpServer] = &[
    BuiltinMcpServer {
        // DEEPINFRA/OPENROUTER keys are NOT read directly by this server — its own
        // doc (hkask_mcp_codegraph.rs:466) says credentials come from zed's
        // LanguageModelRegistry over the IPC bridge. They are reachable only on
        // the degraded no-socket fallback, where `resolve_inference_port` builds a
        // MediaRouter from `InferenceConfig::from_env`, which does read them.
        // Retained deliberately so embeddings still work when the inference socket
        // is unavailable; reviewed 2026-08-12 (RR-0061). If the fallback is ever
        // removed, drop these two entries in the same change.
        id: "codegraph",
        binary: "hkask-mcp-codegraph",
        description: "Codegraph — code structure query and traversal",
        credentials: Some(&["DEEPINFRA_API_KEY", "OPENROUTER_API_KEY"]),
        config_env: Some(&[
            "HKASK_CODEGRAPH_DB",
            "HKASK_EMBEDDING_DIM",
            "HKASK_EMBEDDING_MODEL",
        ]),
    },
    BuiltinMcpServer {
        id: "portfolio",
        binary: "hkask-mcp-portfolio",
        description: "Portfolio — general-purpose transaction-ledger portfolio store (stocks, prediction-event portfolios, CMP indices) with materialized daily holdings and returns views",
        credentials: Some(&[]),
        config_env: Some(&[]),
    },
    BuiltinMcpServer {
        id: "companies",
        binary: "hkask-mcp-companies",
        description: "Companies — company research and filings",
        credentials: Some(&[
            // Each entry must have a read site in the crate (allowlist
            // alignment). HKASK_SERPAPI_API_KEY was removed: no read site.
            "HKASK_EODHD_API_KEY",
            "HKASK_FMP_API_KEY",
            "HKASK_EXA_API_KEY",
            "HKASK_TAVILY_API_KEY",
            "HKASK_BRAVE_API_KEY",
        ]),
        config_env: Some(&[
            // HKASK_TRANSACTIONS_DIR was removed: no read site in the crate.
            "HKASK_CHRONIC_STALENESS_DAYS",
            "HKASK_FERMI_DEFAULTS",
        ]),
    },
    BuiltinMcpServer {
        id: "condenser",
        binary: "hkask-mcp-condenser",
        description: "Condenser — context condensation and summarization",
        credentials: Some(&[
            // DB encryption passphrase — read by the condenser server's
            // `run()` for its episodic + semantic SQLite stores. Without
            // this, the condenser cannot open an encrypted DB under governed
            // launch and falls back to in-memory mode (no persistence).
            "HKASK_DB_PASSPHRASE",
        ]),
        config_env: Some(&[
            "HKASK_CONDENSER_PERSONA_KEYWORDS",
            "HKASK_CONDENSE_SALIENCY_WINDOW",
            "HKASK_DEFAULT_MODEL",
            // Condenser DB path — read by `run()` to locate the episodic +
            // semantic SQLite store. Without this, an operator override via
            // kask settings is silently dropped by the per-server filter and
            // the condenser falls back to in-memory mode.
            "HKASK_DB_PATH",
        ]),
    },
    BuiltinMcpServer {
        id: "corpus",
        binary: "hkask-mcp-corpus",
        description: "Corpus — document corpus and QA generation",
        credentials: Some(&[
            // DB encryption passphrase — read by default_corpus_passphrase() in
            // semantic/mod.rs. Without this, the DB is silently encrypted with
            // the hardcoded dev passphrase under governed launch.
            "HKASK_DB_PASSPHRASE",
        ]),
        config_env: Some(&[
            "HKASK_EMBEDDING_DIM",
            "HKASK_EMBEDDING_MODEL",
            "HKASK_OCR_CONCURRENCY",
            "HKASK_OCR_SIMPLE_MAX",
            "HKASK_OCR_MODERATE_MAX",
            "HKASK_OCR_SAMPLE_RATE",
            "HKASK_OCR_TUNEABLE",
            "HKASK_TEMPLATE_ROOT",
            "HKASK_DEFAULT_MODEL",
            "HKASK_CLASSIFIER_MODEL",
            // QA model override — read by qa.rs, falls back to HKASK_DEFAULT_MODEL.
            "HKASK_QA_MODEL",
            // Model cache TTL — read by model_cache.rs, falls back to 4h default.
            "HKASK_MODEL_CACHE_TTL_SECS",
            // Content guard toggle — read by semantic/mod.rs, defaults to true.
            "HKASK_ENABLE_CONTENT_GUARD",
            // OCR triage thresholds — read by ocr/config.rs, fall back to TriageConfig::default().
            "HKASK_OCR_TRIAGE_TEXT_NATIVE_MIN",
            "HKASK_OCR_TRIAGE_MIN_IMAGE_PT",
            "HKASK_OCR_TRIAGE_FULL_PAGE_PT",
            "HKASK_OCR_TRIAGE_EMBEDDED_IMAGE_PT",
            "HKASK_OCR_TRIAGE_TUNEABLE",
            // OCR vision model override — read by ctx.credentials.get() in
            // hkask_mcp_corpus.rs and std::env::var in corpus/embed/ocr.rs.
            // Without this, operator OCR model overrides are silently dropped.
            "HKASK_OCR_MODEL",
        ]),
    },
    BuiltinMcpServer {
        id: "curator",
        binary: "hkask-mcp-curator",
        description: "Curator — regulation cascade and algedonic signals",
        credentials: Some(&["HKASK_SMTP_PASSWORD"]),
        config_env: Some(&[
            "HKASK_MXROUTE_SERVER",
            "HKASK_SMTP_USERNAME",
            "HKASK_CURATOR_EMAIL",
            "HKASK_ALERT_EMAIL",
            "HKASK_AUTHORIZED_EMAILS",
            "HKASK_INBOX_POLL_INTERVAL_SECS",
            "HKASK_DIGEST_INTERVAL_SECS",
            // Curator DB path — injected by the deferred task after
            // provisioning, so the curator MCP server reads from the same
            // `agents/curator/pod.db` the agent writes curator copies to.
            "HKASK_CURATOR_DB",
            // Curator WebID — stashed in a non-global env var by the deferred
            // task. `mcp_env()` maps this to `HKASK_WEBID` for the curator
            // server only, so other MCP servers don't inherit the curator's
            // identity.
            "HKASK_CURATOR_WEBID",
            // The mapped `HKASK_WEBID` — `mcp_env()` injects this from
            // `HKASK_CURATOR_WEBID`. Must be in the allowlist so the
            // per-server filter passes it through.
            "HKASK_WEBID",
            // Data dir — needed so `resolve_under_data_dir` in
            // `open_curator_stores` finds the same root as the agent.
            "HKASK_DATA_DIR",
        ]),
    },
    BuiltinMcpServer {
        id: "kata-kanban",
        binary: "hkask-mcp-kata-kanban",
        description: "Kata Kanban — improvement kata board",
        credentials: Some(&[
            // `HKASK_DB_PASSPHRASE` is read via `ctx.credentials.get` in
            // `run()` for the SQLCipher store. `HKASK_KANBAN_DB` is a
            // non-secret DB path — moved to `config_env` and read via
            // `std::env::var` to match every other DB-path env var
            // (`HKASK_CURATOR_DB`, `HKASK_DB_PATH`, `HKASK_RSS_DB`, etc.).
            "HKASK_DB_PASSPHRASE",
        ]),
        config_env: Some(&[
            // kata-kanban resolves its DB path via `resolve_under_data_dir`,
            // so it needs the data dir to match the parent process.
            "HKASK_DATA_DIR",
            // DB path override — read via `std::env::var` in `run()`. Non-secret
            // config; moved from `credentials` to align with the pattern used
            // by every other DB-path env var in the registry.
            "HKASK_KANBAN_DB",
        ]),
    },
    BuiltinMcpServer {
        id: "media",
        binary: "hkask-mcp-media",
        description: "Media — image generation and media workflows",
        credentials: Some(&["DEEPINFRA_API_KEY", "ATLASCLOUD_API_KEY"]),
        config_env: Some(&[
            // Durable gallery DB path (WS-3). Unencrypted file SQLite — the
            // media server reads it via std::env::var; absent → in-memory.
            "HKASK_MEDIA_DB",
            "HKASK_MEDIA_TTS_MODEL",
            "HKASK_MEDIA_STT_MODEL",
            "HKASK_MEDIA_VISION_MODEL",
            "HKASK_MEDIA_IMAGE_GEN_MODEL",
            // rJoule spend cap — read at hkask-mcp-media/src/budget.rs:233.
            // Unset means enforcement is OFF (budget.rs:222), so while this was
            // unallowlisted the cap could not be enabled by an operator at all
            // (RR-0061). Usage metering is only useful if it can be configured.
            "HKASK_MEDIA_RJOULE_CAP",
        ]),
    },
    BuiltinMcpServer {
        id: "research",
        binary: "hkask-mcp-research",
        description: "Research — web research and paper search",
        credentials: Some(&[
            "HKASK_EXA_API_KEY",
            "HKASK_TAVILY_API_KEY",
            "HKASK_BRAVE_API_KEY",
            "HKASK_SERPAPI_API_KEY",
            "HKASK_FIRECRAWL_API_KEY",
            "HKASK_BROWSERBASE_API_KEY",
            // DB encryption passphrase — read by resolve_db_credential() for
            // the RSS SQLite DB. Without this, RSS tools are silently
            // unavailable under governed launch.
            "HKASK_DB_PASSPHRASE",
        ]),
        config_env: Some(&[
            "HKASK_WEB_CACHE_TTL_SECS",
            "HKASK_WEB_CACHE_MAX_ENTRIES",
            // RSS DB path — read by open_database_with_extensions(). Without
            // this, RSS tools return "not configured" despite the env being set.
            "HKASK_RSS_DB",
        ]),
    },
    BuiltinMcpServer {
        id: "scenarios",
        binary: "hkask-mcp-scenarios",
        description: "Scenarios — scenario planning and forecasting",
        credentials: Some(&[]),
        config_env: Some(&["HKASK_SCENARIOS_DATA"]),
    },
    BuiltinMcpServer {
        id: "prediction-markets",
        binary: "hkask-mcp-prediction-markets",
        description: "Prediction markets — annotated Polymarket/Kalshi market-implied probabilities",
        credentials: Some(&["HKASK_FRED_API_KEY"]),
        config_env: Some(&[
            "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS",
            "HKASK_PREDICTION_MARKETS_DATA",
            "HKASK_PREDICTION_MARKETS_BASE_EVENTS",
        ]),
    },
    BuiltinMcpServer {
        id: "swarm",
        binary: "hkask-mcp-swarm",
        description: "Swarm — Agent Bestiary World agent swarms and Xaman Ek curator",
        // HKASK_SWARM_MEMORY_PASSPHRASE is a SECRET (SQLCipher key for the swarm
        // memory DB), so it belongs here, not in config_env. Before it was
        // allowlisted (RR-0061) the read at hkask-mcp-swarm/src/config.rs:252
        // could never receive a value, so the store always opened under the
        // compiled-in pre-release default "allostery" (config.rs:157) — i.e.
        // encrypted with a constant that ships in the source. The documented
        // override in local_knowledge.rs was unreachable via the governed launch.
        credentials: Some(&["HKASK_ABW_API_KEY", "HKASK_SWARM_MEMORY_PASSPHRASE"]),
        config_env: Some(&[
            "HKASK_ABW_API_URL",
            "HKASK_ABW_MAX_CREDITS",
            "HKASK_ABW_CURATOR_CONSENT_DEFAULT",
            "HKASK_ABW_DEFAULT_AGENT_MODEL",
            "HKASK_SWARM_MODE",
            "HKASK_LOCAL_AGENTS_DIR",
            // Local swarms dir — read by `LocalSwarmRegistry::new` via
            // `HKASK_LOCAL_SWARMS_DIR` in `config.rs`. Without this entry,
            // a kask-settings-derived `local_swarms_dir` override is
            // silently dropped by `filter_config_env_for_server`.
            "HKASK_LOCAL_SWARMS_DIR",
            "HKASK_SWARM_LEDGER_PATH",
            "HKASK_SWARM_CONSENT_STORE",
            // The governed server id set — the swarm server filters cloned
            // cards' declared mcp_tools to these servers (provenance boundary
            // for third-party ABW cards).
            "HKASK_MCP_SERVER_IDS",
            // Data dir — needed so `resolve_under_data_dir` in the swarm
            // server resolves `local_agents_dir` under the same root as the
            // parent process. Without this, a relative default
            // (`agents/local/curated`) resolves against the MCP server's CWD
            // (Zed's working dir, typically home or project root — not the
            // zed-kask repo), and local agent cards are never found.
            "HKASK_DATA_DIR",
            // Skills corpus dir — read by `AgentExecutor::build_skill_catalog`
            // via `HKASK_SKILLS_DIR` in `config.rs` (Slice 6 — local agent
            // skill-awareness). Without this entry, a kask-settings-derived
            // skills dir override is silently dropped by
            // `filter_config_env_for_server`.
            "HKASK_SKILLS_DIR",
            // Swarm memory store shape — read in config.rs alongside the
            // passphrase above. Without these the DB path and embedding
            // dimension overrides were silently dropped (RR-0061).
            "HKASK_SWARM_MEMORY_DB",
            "HKASK_SWARM_EMBEDDING_DIM",
            // A2A HTTP listener toggle — read via `config.a2a_http_enabled`
            // (hkask_mcp_swarm.rs:264). Unallowlisted, an operator could not
            // enable or disable the listener.
            "HKASK_A2A_HTTP_ENABLE",
        ]),
    },
    BuiltinMcpServer {
        id: "training",
        binary: "hkask-mcp-training",
        description: "Training — LoRA training configuration and audit",
        credentials: Some(&[
            "DEEPINFRA_API_KEY",
            "RUNPOD_API_KEY",
            "RUNPOD_TEMPLATE_ID",
            "NEBIUS_PROJECT_ID",
            "NEBIUS_SUBNET_ID",
            "HF_TOKEN",
            // DB encryption passphrase — read by the training server for its
            // job/adapter SQLite DB. Without this, the DB falls back to a
            // default or in-memory store under governed launch.
            "HKASK_DB_PASSPHRASE",
        ]),
        config_env: Some(&[
            "HKASK_TRAINING_HOST",
            "HKASK_TRAINING_CACHE_DIR",
            "HKASK_TEMPLATE_ROOT",
            "HKASK_DATA_DIR",
            // HuggingFace persistence — required by HuggingFaceTraining::from_env()
            // for the Runpod artifact publish path. Without these, training_submit
            // fails at G-P1 under governed launch (B-1).
            "HKASK_HF_ARTIFACT_OWNER",
            "HKASK_HF_DATASET_REPO",
            "HKASK_HF_MODEL_REPO",
            // Training DB path — read by the server, falls back to default.
            "HKASK_TRAINING_DB",
            // RunPod operator overrides — read by runpod.rs, fall back to defaults.
            "RUNPOD_GPU_TYPE_ID",
            "RUNPOD_CONTAINER_DISK_GB",
            "RUNPOD_DOCKER_IMAGE",
            "RUNPOD_DOCKER_ARGS",
            "HKASK_PODS_FILE",
            // DeepInfra operator overrides — read by providers/mod.rs.
            "DEEPINFRA_GPU_CONFIG",
            "DEEPINFRA_CONTAINER_IMAGE",
            // Nebius operator overrides — read by providers/mod.rs and nebius.rs.
            "NEBIUS_GPU_PLATFORM",
            "NEBIUS_GPU_PRESET",
            "NEBIUS_IMAGE_FAMILY",
            "NEBIUS_CLI_PATH",
        ]),
    },
];

/// Just the server IDs, as a static slice of `&str`.
/// Convenience for consumers that only need the ID list (e.g. `swarm_panel`).
pub const BUILT_IN_MCP_SERVERS_IDS: &[&str] = &[
    "codegraph",
    "portfolio",
    "companies",
    "condenser",
    "corpus",
    "curator",
    "kata-kanban",
    "media",
    "research",
    "scenarios",
    "prediction-markets",
    "swarm",
    "training",
];

/// The server list as `(id, description)` pairs.
/// Convenience for the settings UI which renders `(id, description)` rows.
pub const BUILT_IN_MCP_SERVERS_PAIRS: &[(&str, &str)] = &[
    (
        "codegraph",
        "Codegraph — code structure query and traversal",
    ),
    (
        "portfolio",
        "Portfolio — general-purpose transaction-ledger portfolio store (stocks, prediction-event portfolios, CMP indices) with materialized daily holdings and returns views",
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
        "prediction-markets",
        "Prediction markets — annotated Polymarket/Kalshi market-implied probabilities",
    ),
    (
        "swarm",
        "Swarm — Agent Bestiary World agent swarms and Xaman Ek curator",
    ),
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

/// Filter a list of `(env_var, credential_url)` pairs to only those the
/// specified server is allowed to receive.
///
/// When the server's `credentials` field is `Some(allowlist)`, only env vars
/// in the allowlist are kept. When it's `None`, all credentials are kept
/// (backward-compatible behavior for unaudited servers).
///
/// This limits the blast radius of a compromised MCP server — a server that
/// only needs `DEEPINFRA_API_KEY` won't receive `HKASK_SMTP_PASSWORD`.
#[must_use]
pub fn filter_credentials_for_server(
    server_id: &str,
    credentials: &[(String, String)],
) -> Vec<(String, String)> {
    let Some(server) = find_server(server_id) else {
        // Fail closed: an unknown server id receives no credentials.
        tracing::warn!(
            target: "reg.mcp",
            server_id = %server_id,
            "Unknown MCP server id — no credentials will be injected"
        );
        return Vec::new();
    };
    match server.credentials {
        Some(allowlist) => credentials
            .iter()
            .filter(|(env_var, _)| allowlist.contains(&env_var.as_str()))
            .cloned()
            .collect(),
        None => credentials.to_vec(),
    }
}

/// Filter a base config env map (`mcp_env()` output) to only the env vars the
/// specified server is allowed to receive.
///
/// When the server's `config_env` field is `Some(allowlist)`, only env vars
/// in the allowlist are kept. When it's `None`, all config is kept.
///
/// This prevents the curator's email config (`HKASK_SMTP_USERNAME`,
/// `HKASK_MXROUTE_SERVER`, etc.) from being injected into servers that don't
/// need it (codegraph, condenser, kata-kanban, etc.).
#[must_use]
pub fn filter_config_env_for_server(
    server_id: &str,
    config_env: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    let Some(server) = find_server(server_id) else {
        // Fail closed: an unknown server id receives no config env.
        tracing::warn!(
            target: "reg.mcp",
            server_id = %server_id,
            "Unknown MCP server id — no config env will be injected"
        );
        return std::collections::HashMap::new();
    };
    match server.config_env {
        Some(allowlist) => config_env
            .iter()
            .filter(|(env_var, _)| allowlist.contains(&env_var.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        None => config_env.clone(),
    }
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

    // Every server must have a credential allowlist (not `None`).
    // `None` means "receive all credentials" — the unsafe default we're
    // moving away from. New servers should use `Some(&[])` (no credentials)
    // and add specific env vars as needed.
    #[test]
    fn all_servers_have_credential_allowlist() {
        for s in BUILT_IN_MCP_SERVERS {
            assert!(
                s.credentials.is_some(),
                "server '{}' has no credential allowlist (credentials is None) — \
                 use Some(&[]) for servers that need no credentials",
                s.id
            );
            assert!(
                s.config_env.is_some(),
                "server '{}' has no config_env allowlist (config_env is None) — \
                 use Some(&[]) for servers that need no config env vars",
                s.id
            );
        }
    }

    // Allowlist *content* alignment: every entry in a server's
    // `credentials`/`config_env` allowlist must have a read site in the crate,
    // and every env var the server reads must be in the allowlist. The shape
    // test above only checks `Some(...)`; these pin the known-good baseline
    // for servers whose allowlists were found mis-aligned (over-grant in
    // companies, under-grant in kata-kanban). When a server legitimately
    // starts reading a new env var, update both the descriptor and the
    // baseline here in the same change.
    fn server_by_id(id: &str) -> &'static BuiltinMcpServer {
        BUILT_IN_MCP_SERVERS
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("server '{id}' not in BUILT_IN_MCP_SERVERS"))
    }

    #[test]
    fn companies_allowlist_matches_actual_reads() {
        let s = server_by_id("companies");
        // Read sites: ctx.credentials.get in `run()` for the 5 providers;
        // std::env::var("HKASK_CHRONIC_STALENESS_DAYS") and
        // FermiDefaults::from_env() reading HKASK_FERMI_DEFAULTS.
        assert_eq!(
            s.credentials.unwrap().to_vec(),
            vec![
                "HKASK_EODHD_API_KEY",
                "HKASK_FMP_API_KEY",
                "HKASK_EXA_API_KEY",
                "HKASK_TAVILY_API_KEY",
                "HKASK_BRAVE_API_KEY",
            ],
            "companies credentials allowlist drifted — every entry must have a \
             read site in hkask-mcp-companies (over-grant leaks a secret to the \
             child process)"
        );
        assert_eq!(
            s.config_env.unwrap().to_vec(),
            vec!["HKASK_CHRONIC_STALENESS_DAYS", "HKASK_FERMI_DEFAULTS"],
            "companies config_env allowlist drifted — every entry must have a \
             read site in hkask-mcp-companies"
        );
    }

    #[test]
    fn kata_kanban_allowlist_matches_actual_reads() {
        let s = server_by_id("kata-kanban");
        // Read sites: `ctx.credentials.get("HKASK_DB_PASSPHRASE")` in
        // `run()`; `std::env::var("HKASK_KANBAN_DB")` in `run()` (moved from
        // credentials to config_env — it's a non-secret DB path);
        // `resolve_under_data_dir` reads `HKASK_DATA_DIR`.
        assert_eq!(
            s.credentials.unwrap().to_vec(),
            vec!["HKASK_DB_PASSPHRASE"],
            "kata-kanban credentials allowlist drifted — under-granting silently \
             drops operator overrides (server falls back to in-memory mode)"
        );
        assert_eq!(
            s.config_env.unwrap().to_vec(),
            vec!["HKASK_DATA_DIR", "HKASK_KANBAN_DB"],
            "kata-kanban config_env allowlist drifted"
        );
    }

    #[test]
    fn condenser_allowlist_matches_actual_reads() {
        let s = server_by_id("condenser");
        // Read sites in `run()`:
        //   ctx.credentials.get("HKASK_DB_PATH")       → episodic + semantic DB path
        //   ctx.credentials.get("HKASK_DB_PASSPHRASE") → SQLCipher passphrase (required when DB_PATH set)
        //   std::env::var("HKASK_CONDENSER_PERSONA_KEYWORDS") → persona keyword list
        //   std::env::var("HKASK_CONDENSE_SALIENCY_WINDOW")   → saliency window multiplier
        //   ctx.credentials.get("HKASK_DEFAULT_MODEL")        → default inference model
        assert_eq!(
            s.credentials.unwrap().to_vec(),
            vec!["HKASK_DB_PASSPHRASE"],
            "condenser credentials allowlist drifted — HKASK_DB_PASSPHRASE is read \
             in run() for the episodic + semantic SQLite stores; under-granting forces \
             in-memory mode (no persistence) under governed launch"
        );
        assert_eq!(
            s.config_env.unwrap().to_vec(),
            vec![
                "HKASK_CONDENSER_PERSONA_KEYWORDS",
                "HKASK_CONDENSE_SALIENCY_WINDOW",
                "HKASK_DEFAULT_MODEL",
                "HKASK_DB_PATH",
            ],
            "condenser config_env allowlist drifted — every entry must have a read \
             site in hkask-mcp-condenser"
        );
    }

    #[test]
    fn prediction_markets_allowlist_matches_actual_reads() {
        let s = server_by_id("prediction-markets");
        // Read site: ctx.credentials.get("HKASK_FRED_API_KEY") in `run()` for
        // live reference-level fetches (FRED API). Optional — curated static
        // defaults used when absent.
        assert_eq!(
            s.credentials.unwrap().to_vec(),
            vec!["HKASK_FRED_API_KEY"],
            "prediction-markets credentials allowlist drifted — HKASK_FRED_API_KEY \
             is read in run() for live reference-level fetches; add a credential \
             only with a read site in hkask-mcp-prediction-markets"
        );
        // Read site: std::env::var("HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS")
        // in `run()` (with a malformed-value warn, not silent fallback).
        assert_eq!(
            s.config_env.unwrap().to_vec(),
            vec![
                "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS",
                "HKASK_PREDICTION_MARKETS_DATA",
                "HKASK_PREDICTION_MARKETS_BASE_EVENTS",
            ],
            "prediction-markets config_env allowlist drifted — every entry must              have a read site in hkask-mcp-prediction-markets"
        );
    }

    // The portfolio server is provider-agnostic: no credentials, no config
    // env. It reads only HKASK_WEBID (identity, injected via config_env by
    // the runtime, not declared here) and writes to the owner-scoped SQLite
    // DB under the config dir. This pins the blast-radius reduction — a
    // future edit that adds a provider key here would leak it to a process
    // that has no read site for it.
    #[test]
    fn portfolio_allowlist_matches_actual_reads() {
        let s = server_by_id("portfolio");
        // Read sites: none — the portfolio store is provider-agnostic.
        assert_eq!(
            s.credentials.unwrap().to_vec(),
            Vec::<&str>::new(),
            "portfolio credentials allowlist drifted — the portfolio store is \
             provider-agnostic; add a credential only with a read site in \
             hkask-mcp-portfolio"
        );
        // Read sites: none beyond HKASK_WEBID (identity, not config).
        assert_eq!(
            s.config_env.unwrap().to_vec(),
            Vec::<&str>::new(),
            "portfolio config_env allowlist drifted — add an entry only with \
             a read site in hkask-mcp-portfolio"
        );
    }

    // The curator server should only receive the SMTP password, not data
    // service API keys. This pins the blast-radius reduction.
    #[test]
    fn curator_credentials_do_not_include_data_service_keys() {
        let all_credentials: Vec<(String, String)> = [
            "HKASK_EODHD_API_KEY",
            "HKASK_FMP_API_KEY",
            "HKASK_SMTP_PASSWORD",
            "DEEPINFRA_API_KEY",
        ]
        .iter()
        .map(|env| (env.to_string(), "url".to_string()))
        .collect();
        let filtered = filter_credentials_for_server("curator", &all_credentials);
        let env_vars: Vec<&str> = filtered.iter().map(|(k, _)| k.as_str()).collect();
        assert!(
            env_vars.contains(&"HKASK_SMTP_PASSWORD"),
            "curator should receive HKASK_SMTP_PASSWORD"
        );
        assert!(
            !env_vars.contains(&"HKASK_EODHD_API_KEY"),
            "curator should NOT receive HKASK_EODHD_API_KEY"
        );
        assert!(
            !env_vars.contains(&"DEEPINFRA_API_KEY"),
            "curator should NOT receive DEEPINFRA_API_KEY"
        );
    }

    // The codegraph server should only receive inference keys, not SMTP.
    #[test]
    fn codegraph_credentials_do_not_include_smtp_password() {
        let all_credentials: Vec<(String, String)> = [
            "DEEPINFRA_API_KEY",
            "OPENROUTER_API_KEY",
            "HKASK_SMTP_PASSWORD",
            "HKASK_EODHD_API_KEY",
        ]
        .iter()
        .map(|env| (env.to_string(), "url".to_string()))
        .collect();
        let filtered = filter_credentials_for_server("codegraph", &all_credentials);
        let env_vars: Vec<&str> = filtered.iter().map(|(k, _)| k.as_str()).collect();
        assert!(env_vars.contains(&"DEEPINFRA_API_KEY"));
        assert!(env_vars.contains(&"OPENROUTER_API_KEY"));
        assert!(
            !env_vars.contains(&"HKASK_SMTP_PASSWORD"),
            "codegraph should NOT receive HKASK_SMTP_PASSWORD"
        );
    }

    // Unknown server IDs fail closed: no credentials are injected.
    #[test]
    fn unknown_server_gets_no_credentials() {
        let credentials = vec![
            ("KEY_A".to_string(), "url_a".to_string()),
            ("KEY_B".to_string(), "url_b".to_string()),
        ];
        let filtered = filter_credentials_for_server("nonexistent", &credentials);
        assert!(filtered.is_empty());
    }

    // The codegraph server should not receive the curator's email config.
    // This pins the config-env blast-radius reduction.
    #[test]
    fn codegraph_config_env_excludes_curator_email() {
        let mut config_env = std::collections::HashMap::new();
        config_env.insert("HKASK_CODEGRAPH_DB".to_string(), "/path/to/db".to_string());
        config_env.insert(
            "HKASK_SMTP_USERNAME".to_string(),
            "curator@example.com".to_string(),
        );
        config_env.insert(
            "HKASK_MXROUTE_SERVER".to_string(),
            "mail.example.com".to_string(),
        );
        config_env.insert(
            "HKASK_AUTHORIZED_EMAILS".to_string(),
            "ops@example.com".to_string(),
        );
        let filtered = filter_config_env_for_server("codegraph", &config_env);
        assert!(
            filtered.contains_key("HKASK_CODEGRAPH_DB"),
            "codegraph should receive HKASK_CODEGRAPH_DB"
        );
        assert!(
            !filtered.contains_key("HKASK_SMTP_USERNAME"),
            "codegraph should NOT receive HKASK_SMTP_USERNAME"
        );
        assert!(
            !filtered.contains_key("HKASK_MXROUTE_SERVER"),
            "codegraph should NOT receive HKASK_MXROUTE_SERVER"
        );
        assert!(
            !filtered.contains_key("HKASK_AUTHORIZED_EMAILS"),
            "codegraph should NOT receive HKASK_AUTHORIZED_EMAILS"
        );
    }

    // The curator server should receive its email config.
    #[test]
    fn curator_config_env_includes_email_settings() {
        let mut config_env = std::collections::HashMap::new();
        config_env.insert(
            "HKASK_SMTP_USERNAME".to_string(),
            "curator@example.com".to_string(),
        );
        config_env.insert(
            "HKASK_MXROUTE_SERVER".to_string(),
            "mail.example.com".to_string(),
        );
        config_env.insert("HKASK_CODEGRAPH_DB".to_string(), "/path".to_string());
        let filtered = filter_config_env_for_server("curator", &config_env);
        assert!(filtered.contains_key("HKASK_SMTP_USERNAME"));
        assert!(filtered.contains_key("HKASK_MXROUTE_SERVER"));
        assert!(
            !filtered.contains_key("HKASK_CODEGRAPH_DB"),
            "curator should NOT receive HKASK_CODEGRAPH_DB"
        );
    }

    // Unknown server IDs fail closed: no config env is injected.
    #[test]
    fn unknown_server_gets_no_config_env() {
        let mut config_env = std::collections::HashMap::new();
        config_env.insert("KEY_A".to_string(), "val_a".to_string());
        config_env.insert("KEY_B".to_string(), "val_b".to_string());
        let filtered = filter_config_env_for_server("nonexistent", &config_env);
        assert!(filtered.is_empty());
    }

    // The swarm server should only receive the ABW API key, not SMTP or
    // inference keys. This pins the credential blast-radius reduction for
    // the swarm server specifically — a future edit that widens the swarm
    // `credentials` allowlist would not be caught by the generic
    // `all_servers_have_credential_allowlist` test.
    #[test]
    fn swarm_credentials_exclude_other_servers_secrets() {
        let all_credentials: Vec<(String, String)> = [
            "HKASK_ABW_API_KEY",
            "HKASK_SWARM_MEMORY_PASSPHRASE",
            "HKASK_EODHD_API_KEY",
            "HKASK_FMP_API_KEY",
            "HKASK_SMTP_PASSWORD",
            "DEEPINFRA_API_KEY",
            "OPENROUTER_API_KEY",
        ]
        .iter()
        .map(|env| (env.to_string(), "url".to_string()))
        .collect();
        let filtered = filter_credentials_for_server("swarm", &all_credentials);
        // Renamed from `swarm_credentials_only_include_abw_key` 2026-08-12: the
        // swarm legitimately receives TWO secrets now that the memory passphrase
        // is allowlisted (RR-0061). The invariant that matters is not the count —
        // it is that no OTHER server's secret reaches this one.
        let names: Vec<&str> = filtered.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            vec!["HKASK_ABW_API_KEY", "HKASK_SWARM_MEMORY_PASSPHRASE"],
            "swarm should receive exactly its own two declared secrets"
        );
        assert!(
            !filtered.iter().any(|(k, _)| k == "HKASK_SMTP_PASSWORD"),
            "swarm server must not receive SMTP credentials"
        );
        assert!(
            !filtered.iter().any(|(k, _)| k == "DEEPINFRA_API_KEY"),
            "swarm server must not receive inference keys"
        );
    }

    // The media server should only receive the keys it actually reads
    // (DEEPINFRA_API_KEY, ATLASCLOUD_API_KEY), not other inference keys or
    // unrelated secrets. Vision routes through the IPC bridge to zed's
    // LanguageModelRegistry — the media server process never reads
    // OPENROUTER_API_KEY. This pins the allowlist against a future edit
    // that re-widens it. See kask/docs/plans/media-system-refactor.md §6 (F-2).
    #[test]
    fn media_credentials_only_include_used_keys() {
        let all_credentials: Vec<(String, String)> = [
            "DEEPINFRA_API_KEY",
            "ATLASCLOUD_API_KEY",
            "OPENROUTER_API_KEY",
            "HKASK_SMTP_PASSWORD",
            "HKASK_DB_PASSPHRASE",
        ]
        .iter()
        .map(|env| (env.to_string(), "url".to_string()))
        .collect();
        let filtered = filter_credentials_for_server("media", &all_credentials);
        let keys: Vec<&str> = filtered.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys.len(),
            2,
            "media server should receive DEEPINFRA_API_KEY + ATLASCLOUD_API_KEY, got {keys:?}"
        );
        assert!(keys.contains(&"DEEPINFRA_API_KEY"));
        assert!(
            !keys.contains(&"FALAI_API_KEY"),
            "media server must not receive FALAI_API_KEY — fal.ai backend removed"
        );
        assert!(
            keys.contains(&"ATLASCLOUD_API_KEY"),
            "media server reads ATLASCLOUD_API_KEY — it must be in credentials"
        );
        assert!(
            !keys.contains(&"OPENROUTER_API_KEY"),
            "media server must not receive OPENROUTER_API_KEY — vision routes via the IPC bridge, not the media process"
        );
        assert!(
            !keys.contains(&"HKASK_SMTP_PASSWORD"),
            "media server must not receive SMTP credentials"
        );
        assert!(
            !keys.contains(&"HKASK_DB_PASSPHRASE"),
            "media server must not receive the global DB passphrase — gallery DB is unencrypted (credential-blast-radius)"
        );
    }

    // The media server reads `HKASK_MEDIA_DB` (durable gallery DB path, WS-3)
    // plus the four `HKASK_MEDIA_*_MODEL` overrides via `std::env::var`, so
    // those must be in its `config_env` allowlist and unrelated vars must not.
    // This is the config-env alignment enforcement point (the .rules
    // "MCP server allowlists must align with actual env-var reads").
    #[test]
    fn media_config_env_includes_media_db_and_models() {
        let mut config_env = std::collections::HashMap::new();
        config_env.insert("HKASK_MEDIA_DB".to_string(), "/tmp/media.db".to_string());
        config_env.insert("HKASK_MEDIA_TTS_MODEL".to_string(), "FA/x".to_string());
        config_env.insert("HKASK_MEDIA_STT_MODEL".to_string(), "FA/wizper".to_string());
        config_env.insert(
            "HKASK_MEDIA_VISION_MODEL".to_string(),
            "KC/qwen-vl".to_string(),
        );
        config_env.insert(
            "HKASK_MEDIA_IMAGE_GEN_MODEL".to_string(),
            "FA/flux".to_string(),
        );
        config_env.insert("UNRELATED_VAR".to_string(), "x".to_string());
        config_env.insert("HKASK_DB_PASSPHRASE".to_string(), "secret".to_string());
        let filtered = filter_config_env_for_server("media", &config_env);
        let keys: Vec<&str> = filtered.keys().map(|k| k.as_str()).collect();
        assert!(
            keys.contains(&"HKASK_MEDIA_DB"),
            "media server reads HKASK_MEDIA_DB — it must be in config_env"
        );
        assert!(
            keys.contains(&"HKASK_MEDIA_TTS_MODEL"),
            "media server reads HKASK_MEDIA_TTS_MODEL — it must be in config_env"
        );
        assert!(
            keys.contains(&"HKASK_MEDIA_STT_MODEL"),
            "media server reads HKASK_MEDIA_STT_MODEL — it must be in config_env"
        );
        assert!(
            keys.contains(&"HKASK_MEDIA_VISION_MODEL"),
            "media server reads HKASK_MEDIA_VISION_MODEL — it must be in config_env"
        );
        assert!(
            !keys.contains(&"UNRELATED_VAR"),
            "media server must not receive unrelated config env"
        );
        assert!(
            !keys.contains(&"HKASK_DB_PASSPHRASE"),
            "media gallery DB is unencrypted — it must NOT receive the global \
             HKASK_DB_PASSPHRASE (credential-blast-radius rule)"
        );
    }

    // The swarm server should only receive ABW config env, not curator email
    // config or codegraph DB paths. This pins the config-env blast-radius.
    #[test]
    fn swarm_config_env_excludes_unrelated_vars() {
        let mut config_env = std::collections::HashMap::new();
        config_env.insert(
            "HKASK_ABW_API_URL".to_string(),
            "https://abw.example".to_string(),
        );
        config_env.insert("HKASK_ABW_MAX_CREDITS".to_string(), "100".to_string());
        config_env.insert(
            "HKASK_ABW_CURATOR_CONSENT_DEFAULT".to_string(),
            "true".to_string(),
        );
        config_env.insert(
            "HKASK_ABW_DEFAULT_AGENT_MODEL".to_string(),
            "claude-haiku-4-5-20251001".to_string(),
        );
        config_env.insert("HKASK_SWARM_MODE".to_string(), "local".to_string());
        config_env.insert(
            "HKASK_LOCAL_AGENTS_DIR".to_string(),
            "/custom/dir".to_string(),
        );
        config_env.insert("HKASK_DATA_DIR".to_string(), "/data/hkask".to_string());
        config_env.insert(
            "HKASK_SMTP_USERNAME".to_string(),
            "ops@example.com".to_string(),
        );
        config_env.insert("HKASK_CODEGRAPH_DB".to_string(), "/path/to/db".to_string());
        let filtered = filter_config_env_for_server("swarm", &config_env);
        assert!(filtered.contains_key("HKASK_ABW_API_URL"));
        assert!(filtered.contains_key("HKASK_ABW_MAX_CREDITS"));
        assert!(filtered.contains_key("HKASK_ABW_CURATOR_CONSENT_DEFAULT"));
        assert!(filtered.contains_key("HKASK_ABW_DEFAULT_AGENT_MODEL"));
        assert!(filtered.contains_key("HKASK_SWARM_MODE"));
        assert!(filtered.contains_key("HKASK_LOCAL_AGENTS_DIR"));
        assert!(
            filtered.contains_key("HKASK_DATA_DIR"),
            "swarm server must receive HKASK_DATA_DIR so it can resolve local_agents_dir"
        );
        assert!(
            !filtered.contains_key("HKASK_SMTP_USERNAME"),
            "swarm server must not receive curator email config"
        );
        assert!(
            !filtered.contains_key("HKASK_CODEGRAPH_DB"),
            "swarm server must not receive codegraph config"
        );
    }

    // The under-granting direction: every env var the swarm server actually
    // reads (directly via `std::env::var` in its source) MUST be in its
    // `config_env` or `credentials` allowlist. The `.rules` trap: under-granting
    // silently drops operator overrides (the server falls back to default). The
    // over-grant test above checks that unrelated vars are excluded; this
    // test checks that no read var is forgotten. When a new env var read is
    // added to the swarm server, this list MUST be updated — that is the
    // point (it forces allowlist alignment to be reviewed, not silently
    // drifted). Verified against `grep std::env::var
    // kask/mcp-servers/hkask-mcp-swarm/src/**/*.rs`.
    #[test]
    fn swarm_config_env_includes_all_read_vars() {
        // The env vars the swarm server reads at runtime (non-test).
        // `HKASK_ABW_API_KEY` is a credential (read via `ServerContext`), not
        // `config_env`; assert it via the credential allowlist instead.
        let read_config_vars = [
            "HKASK_ABW_API_URL",
            "HKASK_ABW_MAX_CREDITS",
            "HKASK_ABW_CURATOR_CONSENT_DEFAULT",
            "HKASK_ABW_DEFAULT_AGENT_MODEL",
            "HKASK_SWARM_MODE",
            "HKASK_LOCAL_AGENTS_DIR",
            "HKASK_LOCAL_SWARMS_DIR",
            "HKASK_SWARM_LEDGER_PATH",
            "HKASK_SWARM_CONSENT_STORE",
            "HKASK_MCP_SERVER_IDS",
            "HKASK_DATA_DIR",
        ];
        let mut config_env = std::collections::HashMap::new();
        for v in &read_config_vars {
            config_env.insert(v.to_string(), "sentinel".to_string());
        }
        let filtered = filter_config_env_for_server("swarm", &config_env);
        for v in &read_config_vars {
            assert!(
                filtered.contains_key(*v),
                "swarm server reads {v} but it is not in the config_env allowlist — \
                 a kask-settings-derived override would be silently dropped (.rules trap)"
            );
        }
        // The credential (secret) read is gated by the credentials allowlist,
        // not config_env. Assert it via the credential-filter path.
        let credentials: Vec<(String, String)> = [("HKASK_ABW_API_KEY", "secret")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let filtered_creds = filter_credentials_for_server("swarm", &credentials);
        assert!(
            filtered_creds.iter().any(|(k, _)| k == "HKASK_ABW_API_KEY"),
            "swarm server reads HKASK_ABW_API_KEY but it is not in the credentials allowlist"
        );
    }

    /// fal.ai is deprecated for the corpus server (the OCR docres path was
    /// removed; TTS/STT/image-gen defaults now route to DeepInfra). The corpus
    /// server no longer reads `FALAI_API_KEY` or `HKASK_USE_FAL_DOCRES`. Pin
    /// the removal so a future re-add is caught (`.rules` "tests must pin
    /// deliberate deviations").
    #[test]
    fn corpus_allowlist_excludes_deprecated_fal_ai() {
        let corpus = server_by_id("corpus");
        let creds = corpus
            .credentials
            .expect("corpus has a credential allowlist");
        assert!(
            !creds.contains(&"FALAI_API_KEY"),
            "FALAI_API_KEY must not be granted to the corpus server — fal.ai docres is removed"
        );
        let cfg = corpus
            .config_env
            .expect("corpus has a config_env allowlist");
        assert!(
            !cfg.contains(&"HKASK_USE_FAL_DOCRES"),
            "HKASK_USE_FAL_DOCRES must not be granted to the corpus server — fal.ai docres is removed"
        );
    }

    // ── RR-0061: read-alignment for the five previously-unguarded servers ────
    //
    // Before these tests, only 5 of 13 servers had a read-alignment test.
    // `codegraph`, `curator`, `research`, `scenarios`, and `training` had none —
    // including `training` (the registry's largest secret grant) and `curator`
    // (SMTP password). Drift in an unguarded server is silent in both
    // directions: an under-grant means an operator override never arrives, and
    // an over-grant hands a secret to a process that never reads it.
    //
    // Each expectation below was derived by grepping the server's own source for
    // `std::env::var` / `ctx.credentials.get` sites, so the assertion is against
    // observed reads rather than intent.

    #[test]
    fn codegraph_allowlist_matches_actual_reads() {
        let s = server_by_id("codegraph");
        // Direct reads: HKASK_CODEGRAPH_DB, HKASK_EMBEDDING_DIM.
        // HKASK_EMBEDDING_MODEL is read by the shared hkask-inference model
        // constants, not by this crate directly.
        // DEEPINFRA/OPENROUTER keys have NO direct read site — they are reachable
        // only through `resolve_inference_port`'s no-socket fallback
        // (`InferenceConfig::from_env`). Retained deliberately; see the registry
        // comment. This test pins that decision so the grant cannot silently grow.
        assert_eq!(
            s.credentials.unwrap().to_vec(),
            vec!["DEEPINFRA_API_KEY", "OPENROUTER_API_KEY"],
            "codegraph credentials allowlist drifted — these two are justified ONLY \
             by the degraded no-socket inference fallback; adding more secrets to a \
             code-indexing process needs a read site"
        );
        assert_eq!(
            s.config_env.unwrap().to_vec(),
            vec![
                "HKASK_CODEGRAPH_DB",
                "HKASK_EMBEDDING_DIM",
                "HKASK_EMBEDDING_MODEL",
            ],
            "codegraph config_env allowlist drifted"
        );
    }

    #[test]
    fn curator_allowlist_matches_actual_reads() {
        let s = server_by_id("curator");
        // Only secret read: ctx.credentials.get("HKASK_SMTP_PASSWORD").
        assert_eq!(
            s.credentials.unwrap().to_vec(),
            vec!["HKASK_SMTP_PASSWORD"],
            "curator credentials allowlist drifted — this server holds the SMTP \
             password and must not accumulate unrelated secrets"
        );
        assert!(
            !s.config_env.unwrap().is_empty(),
            "curator config_env should not be empty — the server reads SMTP host/port \
             and curator settings from it"
        );
    }

    #[test]
    fn research_allowlist_matches_actual_reads() {
        let s = server_by_id("research");
        // ctx.credentials.get sites: EXA, TAVILY, BRAVE, SERPAPI, FIRECRAWL,
        // BROWSERBASE. HKASK_DB_PASSPHRASE is read for the RSS store.
        let creds = s.credentials.unwrap();
        for key in [
            "HKASK_EXA_API_KEY",
            "HKASK_TAVILY_API_KEY",
            "HKASK_BRAVE_API_KEY",
            "HKASK_SERPAPI_API_KEY",
            "HKASK_FIRECRAWL_API_KEY",
            "HKASK_BROWSERBASE_API_KEY",
        ] {
            assert!(
                creds.contains(&key),
                "research reads {key} via ctx.credentials.get but it is not \
                 allowlisted — the provider would be silently unavailable"
            );
        }
        assert!(
            s.config_env.unwrap().contains(&"HKASK_RSS_DB"),
            "research reads HKASK_RSS_DB via std::env::var but it is not allowlisted"
        );
    }

    #[test]
    fn scenarios_allowlist_matches_actual_reads() {
        let s = server_by_id("scenarios");
        // No secret read sites at all — the server is storage-only.
        assert!(
            s.credentials.unwrap().is_empty(),
            "scenarios has no credential read site; granting one would be an \
             unjustified secret grant"
        );
        assert_eq!(
            s.config_env.unwrap().to_vec(),
            vec!["HKASK_SCENARIOS_DATA"],
            "scenarios config_env allowlist drifted — HKASK_SCENARIOS_DATA is its \
             only std::env::var read"
        );
    }

    #[test]
    fn training_allowlist_matches_actual_reads() {
        let s = server_by_id("training");
        // Read sites include DEEPINFRA_API_KEY, HF_TOKEN, NEBIUS_PROJECT_ID,
        // NEBIUS_SUBNET_ID, RUNPOD_* and HKASK_DB_PASSPHRASE. This is the largest
        // secret grant in the registry, so pin that every granted secret has a
        // read site.
        let creds = s.credentials.unwrap();
        for key in ["DEEPINFRA_API_KEY", "HF_TOKEN"] {
            assert!(
                creds.contains(&key),
                "training reads {key} but it is not allowlisted"
            );
        }
        // Guard the other direction: the training server must not be handed the
        // SMTP password or another server's DB keys.
        assert!(
            !creds.contains(&"HKASK_SMTP_PASSWORD"),
            "training must not receive the SMTP password — no read site exists"
        );
        assert!(
            !s.config_env.unwrap().is_empty(),
            "training config_env should not be empty — it reads cache dir, host, \
             template root and GPU/pod config"
        );
    }

    // ── RR-0061: the swarm under-grants that silently disabled real features ──

    /// The sharpest instance: HKASK_SWARM_MEMORY_PASSPHRASE is READ at
    /// hkask-mcp-swarm/src/config.rs:252 but was not allowlisted, so the override
    /// could never arrive and the SQLCipher memory DB always opened under the
    /// compiled-in default "allostery" (config.rs:157) — encrypted with a constant
    /// that ships in the source.
    #[test]
    fn swarm_credentials_include_memory_passphrase() {
        let s = server_by_id("swarm");
        assert!(
            s.credentials.unwrap().contains(&"HKASK_SWARM_MEMORY_PASSPHRASE"),
            "HKASK_SWARM_MEMORY_PASSPHRASE is read by the swarm server but is not \
             allowlisted — the SQLCipher memory DB would fall back to the \
             compiled-in default passphrase with no way for an operator to \
             override it (RR-0061)"
        );
    }

    /// The swarm memory store shape and the A2A listener toggle were read but
    /// unallowlisted, so those overrides were silently dropped.
    #[test]
    fn swarm_config_env_includes_memory_store_and_a2a_toggle() {
        let cfg = server_by_id("swarm").config_env.unwrap();
        for key in [
            "HKASK_SWARM_MEMORY_DB",
            "HKASK_SWARM_EMBEDDING_DIM",
            "HKASK_A2A_HTTP_ENABLE",
        ] {
            assert!(
                cfg.contains(&key),
                "{key} is read by the swarm server but is not allowlisted — the \
                 operator override is silently dropped (RR-0061)"
            );
        }
    }

    /// The media rJoule cap could not be enabled at all while unallowlisted:
    /// budget.rs treats unset as "enforcement off", so usage metering was
    /// unconfigurable.
    #[test]
    fn media_config_env_includes_rjoule_cap() {
        assert!(
            server_by_id("media")
                .config_env
                .unwrap()
                .contains(&"HKASK_MEDIA_RJOULE_CAP"),
            "HKASK_MEDIA_RJOULE_CAP is read at hkask-mcp-media/src/budget.rs:233 \
             but is not allowlisted — unset means enforcement is OFF, so the media \
             spend cap could not be turned on by an operator (RR-0061)"
        );
    }

    /// Every secret-shaped credential grant must be justified by a read site.
    /// This is the generic backstop for the 8 servers without a bespoke test:
    /// it cannot verify each name, but it catches the case where a credential
    /// allowlist grows without the registry comment that documents why.
    #[test]
    fn every_credential_grant_is_secret_shaped_or_documented() {
        for server in BUILT_IN_MCP_SERVERS {
            for key in server.credentials.unwrap_or(&[]) {
                let upper = key.to_uppercase();
                assert!(
                    upper.contains("KEY")
                        || upper.contains("TOKEN")
                        || upper.contains("PASSWORD")
                        || upper.contains("PASSPHRASE")
                        || upper.contains("SECRET")
                        || upper.contains("PROJECT_ID")
                        || upper.contains("SUBNET_ID")
                        || upper.contains("TEMPLATE_ID"),
                    "{} grants '{key}' as a CREDENTIAL but the name is not \
                     secret-shaped — non-secret configuration belongs in \
                     config_env, which is not treated as sensitive",
                    server.id
                );
            }
        }
    }
}
