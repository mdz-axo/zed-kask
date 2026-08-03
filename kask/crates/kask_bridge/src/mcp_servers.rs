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
        id: "companies",
        binary: "hkask-mcp-companies",
        description: "Companies — company research and filings",
        credentials: Some(&[
            "HKASK_EODHD_API_KEY",
            "HKASK_FMP_API_KEY",
            "HKASK_EXA_API_KEY",
            "HKASK_TAVILY_API_KEY",
            "HKASK_BRAVE_API_KEY",
            "HKASK_SERPAPI_API_KEY",
        ]),
        config_env: Some(&[
            "HKASK_CHRONIC_STALENESS_DAYS",
            "HKASK_FERMI_DEFAULTS",
            "HKASK_TRANSACTIONS_DIR",
        ]),
    },
    BuiltinMcpServer {
        id: "condenser",
        binary: "hkask-mcp-condenser",
        description: "Condenser — context condensation and summarization",
        credentials: Some(&[]),
        config_env: Some(&[
            "HKASK_CONDENSER_PERSONA_KEYWORDS",
            "HKASK_CONDENSE_SALIENCY_WINDOW",
            "HKASK_DEFAULT_MODEL",
        ]),
    },
    BuiltinMcpServer {
        id: "corpus",
        binary: "hkask-mcp-corpus",
        description: "Corpus — document corpus and QA generation",
        credentials: Some(&["FALAI_API_KEY"]),
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
            // fal.ai docres toggle — read by decimation.rs, falls back to false.
            "HKASK_USE_FAL_DOCRES",
            // Content guard toggle — read by semantic/mod.rs, defaults to true.
            "HKASK_ENABLE_CONTENT_GUARD",
            // OCR triage thresholds — read by ocr/config.rs, fall back to TriageConfig::default().
            "HKASK_OCR_TRIAGE_TEXT_NATIVE_MIN",
            "HKASK_OCR_TRIAGE_MIN_IMAGE_PT",
            "HKASK_OCR_TRIAGE_FULL_PAGE_PT",
            "HKASK_OCR_TRIAGE_EMBEDDED_IMAGE_PT",
            "HKASK_OCR_TRIAGE_TUNEABLE",
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
        credentials: Some(&[]),
        config_env: Some(&[
            // kata-kanban resolves its DB path via `resolve_under_data_dir`,
            // so it needs the data dir to match the parent process.
            "HKASK_DATA_DIR",
        ]),
    },
    BuiltinMcpServer {
        id: "media",
        binary: "hkask-mcp-media",
        description: "Media — image generation and media workflows",
        credentials: Some(&["FALAI_API_KEY", "DEEPINFRA_API_KEY"]),
        config_env: Some(&[
            "HKASK_MEDIA_TTS_MODEL",
            "HKASK_MEDIA_STT_MODEL",
            "HKASK_MEDIA_VISION_MODEL",
            "HKASK_MEDIA_IMAGE_GEN_MODEL",
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
        ]),
        config_env: Some(&[
            // Web cache tunables — read by hkask_mcp_research.rs via ctx.credentials,
            // fall back to DEFAULT_CACHE_TTL_SECS / DEFAULT_CACHE_MAX_ENTRIES.
            "HKASK_WEB_CACHE_TTL_SECS",
            "HKASK_WEB_CACHE_MAX_ENTRIES",
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
        id: "swarm",
        binary: "hkask-mcp-swarm",
        description: "Swarm — Agent Bestiary World agent swarms and Xaman Ek curator",
        credentials: Some(&["HKASK_ABW_API_KEY"]),
        config_env: Some(&[
            "HKASK_ABW_API_URL",
            "HKASK_ABW_MAX_CREDITS",
            "HKASK_ABW_CURATOR_CONSENT_DEFAULT",
            "HKASK_ABW_DEFAULT_AGENT_MODEL",
            "HKASK_SWARM_MODE",
            "HKASK_LOCAL_AGENTS_DIR",
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
        ]),
        config_env: Some(&[
            "HKASK_TRAINING_HOST",
            "HKASK_TRAINING_CACHE_DIR",
            "HKASK_TEMPLATE_ROOT",
            // training resolves its DB path via `resolve_under_data_dir`,
            // so it needs the data dir to match the parent process.
            "HKASK_DATA_DIR",
        ]),
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
    fn swarm_credentials_only_include_abw_key() {
        let all_credentials: Vec<(String, String)> = [
            "HKASK_ABW_API_KEY",
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
        assert_eq!(
            filtered.len(),
            1,
            "swarm server should only receive HKASK_ABW_API_KEY"
        );
        assert_eq!(filtered[0].0, "HKASK_ABW_API_KEY");
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
    // (FALAI_API_KEY, DEEPINFRA_API_KEY), not other inference keys or
    // unrelated secrets. Vision routes through the IPC bridge to zed's
    // LanguageModelRegistry — the media server process never reads
    // TOGETHERAI_API_KEY / OPENROUTER_API_KEY. This pins the allowlist
    // against a future edit that re-widens it. See
    // kask/docs/plans/media-system-refactor.md §6 (F-2).
    #[test]
    fn media_credentials_only_include_used_keys() {
        let all_credentials: Vec<(String, String)> = [
            "DEEPINFRA_API_KEY",
            "FALAI_API_KEY",
            "TOGETHERAI_API_KEY",
            "OPENROUTER_API_KEY",
            "KILOCODE_API_KEY",
            "HKASK_SMTP_PASSWORD",
        ]
        .iter()
        .map(|env| (env.to_string(), "url".to_string()))
        .collect();
        let filtered = filter_credentials_for_server("media", &all_credentials);
        let keys: Vec<&str> = filtered.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys.len(),
            2,
            "media server should only receive FALAI_API_KEY + DEEPINFRA_API_KEY, got {keys:?}"
        );
        assert!(keys.contains(&"FALAI_API_KEY"));
        assert!(keys.contains(&"DEEPINFRA_API_KEY"));
        assert!(
            !keys.contains(&"TOGETHERAI_API_KEY"),
            "media server must not receive TOGETHERAI_API_KEY — it never reads it (vision routes via the IPC bridge)"
        );
        assert!(
            !keys.contains(&"OPENROUTER_API_KEY"),
            "media server must not receive OPENROUTER_API_KEY — vision routes via the IPC bridge, not the media process"
        );
        assert!(
            !keys.contains(&"HKASK_SMTP_PASSWORD"),
            "media server must not receive SMTP credentials"
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
}
