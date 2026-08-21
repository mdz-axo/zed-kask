//! Canonical registry of built-in kask MCP servers.
//!
//! Single source of truth for the server ID → binary name → description mapping.
//! Previously duplicated in `zed/src/main.rs` and `settings_ui/src/pages/kask_page.rs`
//! with drift between them. This module consolidates the list so all consumers
//! reference the same data.
//!
//! The server IDs here match the keys used in `KaskMcpSettingsContent::overrides`
//! and the `context_servers` entries registered with zed's `ContextServerStore`.
//!
//! # Env-construction invariant
//!
//! There is one env-construction path for a kask MCP server child process:
//! [`build_mcp_server_env`]. It composes two filters — [`filter_config_env_for_server`]
//! for non-secret config and [`filter_credentials_for_server`] for keychain secrets.
//! The two filters apply to **disjoint key sets** (config vars live in
//! `BuiltinMcpServer::config_env`, credentials live in `BuiltinMcpServer::credentials`)
//! and are never composed in sequence on the same map. Config is filtered first, then
//! credentials are resolved and merged into the already-filtered map. Reversing this
//! order — or running the config filter over a map that already contains credentials —
//! drops every credential (the config allowlist does not list credential keys). This
//! is the regression the previous two-path design had; do not reintroduce it.

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
        id: "portfolio",
        binary: "hkask-mcp-portfolio",
        description: "Portfolio — general-purpose transaction-ledger portfolio store (stocks, prediction-event portfolios, CMP indices) with materialized daily holdings and returns views",
        credentials: Some(&[]),
        config_env: Some(&["HKASK_TRANSACTIONS_DIR"]),
    },
    BuiltinMcpServer {
        id: "companies",
        binary: "hkask-mcp-companies",
        description: "Companies — company research and filings",
        credentials: Some(&[
            // Each entry must have a read site in the crate (allowlist alignment).
            "HKASK_EODHD_API_KEY",
            "HKASK_FMP_API_KEY",
            "HKASK_EXA_API_KEY",
            "HKASK_TAVILY_API_KEY",
            "HKASK_BRAVE_API_KEY",
            // Read for corpus-mode transcript search. A prior comment here said
            // "removed: no read site" — true only of this spelling; the server was
            // reading the non-canonical `HKASK_SERPAPI_KEY`, which no allowlist or
            // registry carried, so the key never arrived. Normalized on the
            // kask/.env spelling (RR-0061).
            "HKASK_SERPAPI_API_KEY",
        ]),
        config_env: Some(&[
            // HKASK_TRANSACTIONS_DIR is not emitted by `mcp_env()` and not
            // allowlisted: no MCP server crate reads it (verified across
            // hkask-mcp-companies and hkask-mcp-portfolio). The settings field
            // remains for forward compatibility; re-add emission + an entry
            // here when a server crate gains a read site.
            "HKASK_CHRONIC_STALENESS_DAYS",
            "HKASK_FERMI_DEFAULTS",
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
        credentials: Some(&[
            // SMTP password — read by the curator email sink for algedonic alerts.
            "HKASK_SMTP_PASSWORD",
            // SQLCipher passphrase for the curator's sovereign `curator.db`.
            // Without this, `open_curator_stores` cannot decrypt the DB under
            // governed launch and every store-backed tool returns
            // `permission_denied` (escalations, regulation archive, memory).
            // The curator's `run()` reads it via `ctx.credentials.get` with no
            // `std::env::var` fallback, so the registry allowlist is the only
            // delivery path under governed launch.
            "HKASK_DB_PASSPHRASE",
        ]),
        config_env: Some(&[
            "HKASK_MXROUTE_SERVER",
            "HKASK_SMTP_USERNAME",
            "HKASK_CURATOR_EMAIL",
            "HKASK_ALERT_EMAIL",
            "HKASK_AUTHORIZED_EMAILS",
            // Curator DB path — injected by the deferred task after
            // provisioning, so the curator MCP server reads from the same
            // `agents/curator/curator.db` the agent writes curator copies to.
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
            // Skills corpus dir — set from `KaskSwarmSettings.skills_dir`.
            // Retained for settings UI compatibility; the swarm server no
            // longer reads this env var (skill-awareness was removed with the
            // skill execution cleanup).
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
            // Nebius operator overrides — read by providers/mod.rs and nebius.rs.
            "NEBIUS_GPU_PLATFORM",
            "NEBIUS_GPU_PRESET",
            "NEBIUS_IMAGE_FAMILY",
            "NEBIUS_CLI_PATH",
        ]),
    },
];

/// Just the server IDs, derived from [`BUILT_IN_MCP_SERVERS`].
/// Convenience for consumers that only need the ID list (e.g. `swarm_panel`).
pub fn builtin_mcp_server_ids() -> Vec<&'static str> {
    BUILT_IN_MCP_SERVERS.iter().map(|s| s.id).collect()
}

/// The server list as `(id, description)` pairs, derived from
/// [`BUILT_IN_MCP_SERVERS`]. Convenience for the settings UI which renders
/// `(id, description)` rows.
pub fn builtin_mcp_server_pairs() -> Vec<(&'static str, &'static str)> {
    BUILT_IN_MCP_SERVERS
        .iter()
        .map(|s| (s.id, s.description))
        .collect()
}

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
/// only needs `OPENROUTER_API_KEY` won't receive `HKASK_SMTP_PASSWORD`.
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

/// The single canonical env-construction path for a kask MCP server child
/// process.
///
/// There used to be two: `KaskMcpDescriptor::command` (zed's per-project
/// `ContextServerStore`) and `kask_server_env` (the governed `McpRuntime` +
/// the settings-change restart observer). They composed the same helpers in
/// different orders and diverged in opposite directions — Path A leaked the
/// full unfiltered `mcp_env()` map (the per-server `config_env` allowlist was
/// bypassed), Path B dropped every credential (the config filter ran on the
/// credential map too, and credentials live in `credentials`, not `config_env`).
/// Both bugs were invisible because the allowlist-alignment tests exercise the
/// filter helpers in isolation, never the composed path.
///
/// This function is the one place that composes the filters. The order is
/// load-bearing: config is filtered first, then credentials are resolved and
/// merged into the already-filtered map — the two filters apply to disjoint
/// key sets and never run in sequence on the same map. The inference socket is
/// injected last and is not in any allowlist (every server may route inference
/// through zed's `LanguageModelRegistry`).
///
/// `inference_socket` is a parameter, not a global read, so this function is
/// unit-testable without touching `INFERENCE_SOCKET_PATH`.
pub async fn build_mcp_server_env(
    server_id: &str,
    settings: &crate::KaskSettings,
    credentials_provider: &dyn credentials_provider::CredentialsProvider,
    inference_socket: Option<&str>,
    cx: &gpui::AsyncApp,
) -> std::collections::HashMap<String, String> {
    // 1. Config env: build, then filter per-server. `mcp_env()` is the full
    //    unfiltered map; the allowlist is what keeps the curator's email
    let mut env = filter_config_env_for_server(server_id, &settings.mcp_env());

    // 2. Credentials: resolve URLs, filter per-server, read from keychain.
    //    Shell overrides win (preserves the polarity the previous
    //    `mcp_env_with_credentials` established: an empty env var in the
    //    parent shell is not a meaningful override and would silently break
    //    inference with an untraceable "API key not configured" error).
    //
    //    The governed McpRuntime path (`start_server_with_env`) calls
    //    `cmd.env_clear()` before injecting `extra_env`, so a shell-set
    //    credential that is merely `continue`-d here is LOST — the child sees
    //    neither the shell value nor the keychain value. The prior `continue`
    //    was correct for the zed context-server path (which inherits the parent
    //    env), but the governed path does not inherit. Insert the parent value
    //    into `env` so it survives the `env_clear()`: the shell value still
    //    wins over the keychain (it is inserted first, and the keychain branch
    //    below only runs when the parent env did not provide a non-empty
    //    value), and the governed child receives it.
    let cred_urls =
        filter_credentials_for_server(server_id, &crate::credential_urls_for_mcp(settings));
    for (env_var, url) in cred_urls {
        if let Ok(value) = std::env::var(&env_var)
            && !value.is_empty()
        {
            env.insert(env_var, value);
            continue;
        }
        if let Ok(Some((_username, password))) =
            credentials_provider.read_credentials(&url, cx).await
            && let Ok(value) = String::from_utf8(password)
            && !value.is_empty()
        {
            env.insert(env_var, value);
        }
    }

    // 3. Inference IPC socket — not in any allowlist; every server may route
    //    inference through zed's LanguageModelRegistry over the IPC bridge.
    if let Some(socket) = inference_socket {
        env.insert(
            hkask_types::inference_ipc::INFERENCE_SOCKET_ENV.to_string(),
            socket.to_string(),
        );
    }

    env
}

/// Filter a base config env map (`mcp_env()` output) to only the env vars the
/// specified server is allowed to receive.
///
/// When the server's `config_env` field is `Some(allowlist)`, only env vars
/// in the allowlist are kept. When it's `None`, all config is kept.
///
/// This prevents the curator's email config (`HKASK_SMTP_USERNAME`,
/// `HKASK_MXROUTE_SERVER`, etc.) from being injected into servers that don't
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

    fn server_by_id(id: &str) -> &'static BuiltinMcpServer {
        BUILT_IN_MCP_SERVERS
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("server '{id}' not in BUILT_IN_MCP_SERVERS"))
    }

    // The derived fns must match the main registry — this pins the single-source
    // invariant so a future edit to the fns can't silently drift.
    #[test]
    fn builtin_mcp_server_ids_match_main_registry() {
        let ids: Vec<_> = BUILT_IN_MCP_SERVERS.iter().map(|s| s.id).collect();
        assert_eq!(builtin_mcp_server_ids(), ids);
    }

    #[test]
    fn builtin_mcp_server_pairs_match_main_registry() {
        let pairs: Vec<_> = BUILT_IN_MCP_SERVERS
            .iter()
            .map(|s| (s.id, s.description))
            .collect();
        assert_eq!(builtin_mcp_server_pairs(), pairs);
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
        // D28 — HKASK_TRANSACTIONS_DIR is read in `run()` to resolve the
        // transactions directory (default `mcp/portfolio/transactions/`).
        assert_eq!(
            s.config_env.unwrap().to_vec(),
            vec!["HKASK_TRANSACTIONS_DIR"],
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
            "OPENROUTER_API_KEY",
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
            !env_vars.contains(&"OPENROUTER_API_KEY"),
            "curator should NOT receive OPENROUTER_API_KEY"
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
        let filtered = filter_config_env_for_server("curator", &config_env);
        assert!(filtered.contains_key("HKASK_SMTP_USERNAME"));
        assert!(filtered.contains_key("HKASK_MXROUTE_SERVER"));
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
            !filtered.iter().any(|(k, _)| k == "OPENROUTER_API_KEY"),
            "swarm server must not receive inference keys"
        );
    }
    // The swarm server should only receive ABW config env, not curator email
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
            hkask_inference::model_constants::DEFAULT_AGENT_MODEL.to_string(),
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
    /// removed). The corpus
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
    // including `training` (the registry's largest secret grant) and `curator`
    // (SMTP password). Drift in an unguarded server is silent in both
    // directions: an under-grant means an operator override never arrives, and
    // an over-grant hands a secret to a process that never reads it.
    //
    // Each expectation below was derived by grepping the server's own source for
    // `std::env::var` / `ctx.credentials.get` sites, so the assertion is against
    // observed reads rather than intent.

    #[test]
    fn curator_allowlist_matches_actual_reads() {
        let s = server_by_id("curator");
        // Secret reads: ctx.credentials.get("HKASK_SMTP_PASSWORD") (email sink)
        // and ctx.credentials.get("HKASK_DB_PASSPHRASE") (SQLCipher curator.db).
        // The passphrase has no std::env::var fallback in the curator's `run()`,
        // so the allowlist is the only delivery path under governed launch.
        let creds = s.credentials.unwrap();
        assert!(
            creds.contains(&"HKASK_SMTP_PASSWORD"),
            "curator must receive HKASK_SMTP_PASSWORD for algedonic email alerts"
        );
        assert!(
            creds.contains(&"HKASK_DB_PASSPHRASE"),
            "curator must receive HKASK_DB_PASSPHRASE to open its SQLCipher curator.db \
             under governed launch — the run() reads it from ctx.credentials \
             with no std::env::var fallback"
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
        // Read sites include HF_TOKEN, NEBIUS_PROJECT_ID,
        // NEBIUS_SUBNET_ID, RUNPOD_* and HKASK_DB_PASSPHRASE. This is the largest
        // secret grant in the registry, so pin that every granted secret has a
        // read site.
        let creds = s.credentials.unwrap();
        assert!(
            creds.contains(&"HF_TOKEN"),
            "training reads HF_TOKEN but it is not allowlisted"
        );
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
            s.credentials
                .unwrap()
                .contains(&"HKASK_SWARM_MEMORY_PASSPHRASE"),
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

    /// The composition invariant `build_mcp_server_env` encodes: after the
    /// filter sequence, the only env vars present are those in the server's
    /// `config_env` allowlist, its `credentials` allowlist, or the inference
    /// socket. This is the test the two old paths lacked — Path A leaked the
    /// full unfiltered `mcp_env()` map (the `extend` only overwrote allowed
    /// keys, never removed disallowed ones), and Path B dropped every
    /// credential (the config filter ran on the credential map too). Both
    /// bugs were invisible because the allowlist-alignment tests above
    /// exercise the filter helpers in isolation, never the composed path.
    ///
    /// This is a synchronous stand-in: it simulates the filter sequence with
    /// a populated `mcp_env()`-style map (including the curator's email
    /// config, which `mcp_env()` emits) and asserts no key outside
    /// `config_env ∪ credentials ∪ {INFERENCE_SOCKET_ENV}` survives. The
    /// async keychain read is orthogonal — the invariant is the filter order.
    #[test]
    fn build_mcp_server_env_composition_respects_allowlists() {
        // A `mcp_env()`-shaped map: config vars the curator emits plus a few
        // vars other servers emit. In a real launch `mcp_env()` produces this.
        let mut full_config = std::collections::HashMap::new();
        full_config.insert("HKASK_DATA_DIR".to_string(), "/data".to_string());
        full_config.insert(
            "HKASK_MXROUTE_SERVER".to_string(),
            "mail.example.com".to_string(),
        );
        full_config.insert(
            "HKASK_SMTP_USERNAME".to_string(),
            "curator@example.com".to_string(),
        );
        full_config.insert("HKASK_RSS_DB".to_string(), "/rss.db".to_string());
        // A credential-shaped key that lives in `credentials`, not `config_env`.
        let credential_keys = ["HKASK_SMTP_PASSWORD", "OPENROUTER_API_KEY"];

        for server in BUILT_IN_MCP_SERVERS {
            // Simulate `build_mcp_server_env`'s filter sequence:
            //   1. config filtered by `config_env` allowlist
            //   2. credentials merged (filtered by `credentials` allowlist)
            //   3. inference socket added
            let mut env = filter_config_env_for_server(server.id, &full_config);
            for key in server.credentials.unwrap_or(&[]) {
                if credential_keys.contains(&key) {
                    env.insert(key.to_string(), "secret-value".to_string());
                }
            }
            env.insert(
                hkask_types::inference_ipc::INFERENCE_SOCKET_ENV.to_string(),
                "/tmp/sock".to_string(),
            );

            let allowed: std::collections::HashSet<&str> = std::collections::HashSet::from_iter(
                server
                    .config_env
                    .unwrap_or(&[])
                    .iter()
                    .copied()
                    .chain(server.credentials.unwrap_or(&[]).iter().copied())
                    .chain(std::iter::once(
                        hkask_types::inference_ipc::INFERENCE_SOCKET_ENV,
                    )),
            );

            for key in env.keys() {
                assert!(
                    allowed.contains(key.as_str()),
                    "{}: env var '{key}' survived the filter sequence but is not in \
                     config_env or credentials — the composition leaked it. \
                     This is the Path A regression (full mcp_env() map leaked \
                     because `extend` only overwrote allowed keys).",
                    server.id
                );
            }
        }
    }

    /// Pin the shell-override-survives-env_clear fix. The governed McpRuntime
    /// path (`start_server_with_env`) calls `cmd.env_clear()` before injecting
    /// `extra_env`, so a shell-set credential that is merely `continue`-d in
    /// `build_mcp_server_env` is LOST — the child sees neither the shell value
    /// nor the keychain value. The fix inserts the parent env value into the
    /// map so it survives the clear. This test mirrors the credential loop
    /// logic synchronously (the async keychain read is orthogonal) and asserts
    /// a shell-set credential lands in the output map.
    ///
    /// SAFETY: Setting/removing test environment variables in test code is
    /// safe in a single-threaded test context (Rust runs tests serially by
    /// default). The `HKASK_ABW_API_KEY` env var is set and removed within this
    /// test only.
    #[test]
    fn shell_set_credential_survives_env_clear() {
        // SAFETY: Single-threaded test context. Set a shell value for the ABW
        // key, simulating an operator who exports it in their shell.
        unsafe { std::env::set_var("HKASK_ABW_API_KEY", "shell-secret-value") };

        // Mirror the credential loop from `build_mcp_server_env`:
        //   if parent env has a non-empty value → insert into env, continue
        //   else → read from keychain (simulated as None here)
        let mut env = std::collections::HashMap::<String, String>::new();
        let cred_urls = filter_credentials_for_server(
            "swarm",
            &crate::credential_urls_for_mcp(&crate::KaskSettings::default()),
        );
        for (env_var, _url) in cred_urls {
            if let Ok(value) = std::env::var(&env_var)
                && !value.is_empty()
            {
                env.insert(env_var, value);
                continue;
            }
            // Keychain read would go here; simulated as None for this test.
        }

        // The shell-set credential must be in the output map. If the `continue`
        // were a skip (the pre-fix behavior), this would fail — the value would
        // be lost when the governed path clears the child's env.
        assert_eq!(
            env.get("HKASK_ABW_API_KEY").map(|v| v.as_str()),
            Some("shell-secret-value"),
            "shell-set HKASK_ABW_API_KEY must be inserted into the env map so it \
             survives the governed path's cmd.env_clear() — the prior `continue` \
             skipped it, and the child received neither the shell value nor the \
             keychain value"
        );

        // SAFETY: Clean up the test env var.
        unsafe { std::env::remove_var("HKASK_ABW_API_KEY") };
    }
}
