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
//! drops every credential (the config allowlist does not list credential keys). This is
//! the regression the previous two-path design had; do not reintroduce it.

use gpui::AppContext;

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
    pub credentials: &'static [&'static str],
    /// Config env vars (non-secret settings from `mcp_env()`) this server is
    /// allowed to receive. Only config vars in this list are injected.
    pub config_env: &'static [&'static str],
}

/// The canonical list of built-in kask MCP servers.
///
/// Order is stable and meaningful — the kask panel uses index-based selection.
pub const BUILT_IN_MCP_SERVERS: &[BuiltinMcpServer] = &[
    BuiltinMcpServer {
        id: "corpus",
        binary: "hkask-mcp-corpus",
        description: "Corpus — document corpus and QA generation",
        credentials: &[
            // DB encryption passphrase — resolved server-side by
            // resolve_corpus_passphrase() in helpers.rs (fail-closed: an
            // unresolvable passphrase is permission_denied, never an
            // empty-key SQLCipher open).
            "HKASK_DB_PASSPHRASE",
        ],
        config_env: &[
            // Data dir — the corpus server resolves its D28 layout paths
            // (cache, artifacts) under this root via `resolve_under_data_dir`.
            "HKASK_DATA_DIR",
            // Artifacts dir — needed so the corpus server resolves its
            // `corpus-mcp/` artifacts (cache) under the same visible root
            // as the parent process. Without this, an operator
            // `HKASK_ARTIFACTS_DIR` override is silently dropped by
            // `filter_config_env_for_server`.
            "HKASK_ARTIFACTS_DIR",
            "HKASK_EMBEDDING_DIM",
            "HKASK_EMBEDDING_MODEL",
            // Process-wide concurrency ceiling from KaskGeneralSettings —
            // used as the default for embedding concurrency.
            "HKASK_MAX_CONCURRENCY",
            "HKASK_TEMPLATE_ROOT",
            "HKASK_DEFAULT_MODEL",
            "HKASK_CLASSIFIER_MODEL",
            // Dedicated QA generator — no default/chat fallback.
            hkask_inference::model_constants::QA_GENERATION_MODEL_ENV,
            // Legacy consolidation selector; not used by QA generation.
            "HKASK_QA_MODEL",
            // Content guard toggle — read by guard_content() in
            // hkask_mcp_corpus.rs, defaults to true.
            "HKASK_ENABLE_CONTENT_GUARD",
            // OCR triage thresholds — read by ocr/config.rs, fall back to TriageConfig::default().
            "HKASK_OCR_TRIAGE_TEXT_NATIVE_MIN",
            "HKASK_OCR_TRIAGE_MIN_IMAGE_PT",
            "HKASK_OCR_TRIAGE_FULL_PAGE_PT",
            "HKASK_OCR_TRIAGE_EMBEDDED_IMAGE_PT",
            "HKASK_OCR_TRIAGE_TUNEABLE",
            // OCR inference deadline hint — read by ocr_timeout_hint_secs() in
            // ocr/llm_ocr.rs, falls back to 1200s.
            "HKASK_OCR_INFERENCE_TIMEOUT_SECS",
            // Chunk token budget override — read by HkaskSettings::chunk_max_tokens()
            // in hkask-mcp-corpus/src/settings.rs.
            "HKASK_CHUNK_MAX_TOKENS",
            // OCR vision model override — read by ctx.credentials.get() in
            // hkask_mcp_corpus.rs and std::env::var in corpus/embed/ocr.rs.
            // Without this, operator OCR model overrides are silently dropped.
            "HKASK_OCR_MODEL",
        ],
    },
    BuiltinMcpServer {
        id: "curator",
        binary: "hkask-mcp-curator",
        description: "Curator — regulation cascade and algedonic signals",
        credentials: &[
            // SQLCipher passphrase for the curator's sovereign `curator.db`.
            // Without this, `open_curator_stores` cannot decrypt the DB under
            // governed launch and every store-backed tool returns
            // `permission_denied` (escalations, regulation archive, memory).
            // The curator's `run()` reads it via `ctx.credentials.get` with no
            // `std::env::var` fallback, so the registry allowlist is the only
            // delivery path under governed launch.
            "HKASK_DB_PASSPHRASE",
        ],
        config_env: &[
            // NOTE: no email-transport vars here. The curator MCP server has
            // no email sink (it links `hkask-regulation`, not `hkask-email`,
            // and never reads `HKASK_SMTP_*`/`HKASK_MXROUTE_*`/etc.) — the
            // algedonic alert email runs in the EDITOR process
            // (`CuratorAlertEmailSink`, wired from settings at the composition
            // root; the SMTP password comes from the keychain via the
            // editor's own process env). The former child-delivery of these
            // vars was dead surface: zero server-side readers.
            // Curator DB path — injected by the deferred task after
            // provisioning, so the curator MCP server reads from the same
            // `agents/curator/curator.db` the agent writes curator copies to.
            "HKASK_CURATOR_DB",
            // The mapped `HKASK_WEBID` — `mcp_env()` injects this from
            // `HKASK_CURATOR_WEBID` (consumed in the parent process). Must be
            // in the allowlist so the per-server filter passes it through to
            // the curator child only.
            "HKASK_WEBID",
            // Data dir — needed so `resolve_under_data_dir` in
            // `open_curator_stores` finds the same root as the agent.
            "HKASK_DATA_DIR",
            // IPC bridge socket — required for semantic memory recall. The
            // curator server embeds recall queries through zed's
            // LanguageModelRegistry (same routing as corpus/media).
            // Without it, `curator_semantic_search` / `curator_consult`
            // degrade to exact-entity lookup, which never matches a
            // natural-language question.
            "HKASK_INFERENCE_SOCKET",
            // IPC admission-to-completion timeout (published alongside the
            // socket path by `build_mcp_server_env`).
            "HKASK_INFERENCE_TIMEOUT_SECS",
            // ALWAYS-mode distillation pass cadence/idle — emitted by
            // `mcp_env()` from `kask.memory` settings when they differ
            // from the defaults. The curator server's background pass
            // reads them; without the allowlist entry the per-server
            // filter would silently drop an operator override.
            "HKASK_MEMORY_DISTILLATION_CADENCE_SECS",
            "HKASK_MEMORY_DISTILLATION_IDLE_SECS",
            "HKASK_MEMORY_FORGETTING_DAYS",
            // Non-thinking model for the distillation pass's lesson
            // extraction — read by DistillationConfig::from_env in
            // distillation.rs. The port default model is
            // reasoning-mandatory and rejects the pass's non-thinking
            // parameters; without this entry the pass fails per-thread
            // on every generate (observed live 2026-09-09).
            "HKASK_CLASSIFIER_MODEL",
            // Decay constant S (spec §7) — emitted by `mcp_env()` from
            // `kask.memory.memory_life_days` when it differs from the default.
            // Read by `memory_life_days_from_env` at `open_curator_stores`,
            // so the curator server's store decays with the configured S —
            // without the allowlist entry the per-server filter would
            // silently drop the operator's override and decay would revert
            // to the default 180.
            "HKASK_MEMORY_LIFE_DAYS",
            // Embedding model — read by `embed_for_semantic_recall` and the
            // semantic-recall tools (`hkask_mcp_curator.rs`) via
            // `hkask_inference::model_constants::embedding_model()`. Without
            // this entry the per-server filter drops it and every semantic
            // recall degrades to exact-entity lookup with "no embedding
            // model configured" — the exact gap observed live 2026-09-04
            // (the allowlist must align with actual env-var reads).
            "HKASK_EMBEDDING_MODEL",
        ],
    },
    BuiltinMcpServer {
        id: "kata-kanban",
        binary: "hkask-mcp-kata-kanban",
        description: "Kata Kanban — improvement kata board",
        credentials: &[
            // `HKASK_DB_PASSPHRASE` is read via `ctx.credentials.get` in
            // `run()` for the SQLCipher store. `HKASK_KANBAN_DB` is a
            // non-secret DB path — moved to `config_env` and read via
            // `std::env::var` to match every other DB-path env var
            // (`HKASK_CURATOR_DB`, `HKASK_RESEARCH_DB`, etc.).
            "HKASK_DB_PASSPHRASE",
        ],
        config_env: &[
            // kata-kanban resolves its DB path via `resolve_under_data_dir`,
            // so it needs the data dir to match the parent process.
            "HKASK_DATA_DIR",
            // DB path override — read via `std::env::var` in `run()`. Non-secret
            // config; moved from `credentials` to align with the pattern used
            // by every other DB-path env var in the registry.
            "HKASK_KANBAN_DB",
        ],
    },
    BuiltinMcpServer {
        id: "research",
        binary: "hkask-mcp-research",
        description: "Research — web research and paper search",
        credentials: &[
            "HKASK_EXA_API_KEY",
            "HKASK_TAVILY_API_KEY",
            "HKASK_BRAVE_API_KEY",
            "HKASK_SERPAPI_API_KEY",
            "HKASK_FIRECRAWL_API_KEY",
            // DB encryption passphrase — read by resolve_db_passphrase() for
            // the research SQLite DB. Without this, RSS and research-run
            // tools are silently unavailable under governed launch.
            "HKASK_DB_PASSPHRASE",
        ],
        config_env: &[
            // Data dir — needed so the research server's fallback
            // (`resolve_under_data_dir(mcp_server_db("research", "research"))`)
            // resolves under the same root as the parent process when
            // `HKASK_RESEARCH_DB` is unset. The research DB is a database — it
            // stays in the internal data dir; only artifact files and outputs
            // go to the visible artifacts dir.
            "HKASK_DATA_DIR",
            "HKASK_WEB_CACHE_TTL_SECS",
            "HKASK_WEB_CACHE_MAX_ENTRIES",
            // Research DB path — read at server open. Without this, RSS and
            // research-run tools return "not configured" despite the env
            // being set.
            "HKASK_RESEARCH_DB",
            // Embedding model for the semantic duplication tier — read by
            // `hkask_inference::model_constants::embedding_model()` at the
            // construction seam. Emitted unconditionally by
            // `emit_corpus_embedding_env` (consuming servers have no
            // fallback for unset env); without this entry the research
            // server would silently miss the operator's model override.
            "HKASK_EMBEDDING_MODEL",
            // Deep-strategy rerank model — read by
            // `hkask_inference::model_constants::rerank_model()` at call
            // time. Without this, an operator override is silently stripped
            // under governed launch.
            "HKASK_RERANK_MODEL",
        ],
    },
    BuiltinMcpServer {
        id: "scenarios",
        binary: "hkask-mcp-scenarios",
        description: "Scenarios — scenario planning and forecasting",
        credentials: &[],
        config_env: &[
            // Data dir — needed so the scenarios server's fallback
            // (`resolve_under_data_dir(MCP_DIR).join("scenarios")`) resolves
            // under the same root as the parent process when
            // `HKASK_SCENARIOS_DATA` is unset. Without this, an operator
            // `HKASK_DATA_DIR` override is silently dropped.
            "HKASK_DATA_DIR",
            "HKASK_SCENARIOS_DATA",
        ],
    },
    BuiltinMcpServer {
        id: "prediction-markets",
        binary: "hkask-mcp-prediction-markets",
        description: "Prediction markets — annotated Polymarket/Kalshi market-implied probabilities",
        credentials: &["HKASK_FRED_API_KEY"],
        config_env: &[
            // Data dir — needed so the prediction-markets server's fallback
            // (`resolve_under_data_dir(MCP_DIR).join("prediction-markets")`)
            // resolves under the same root as the parent process when
            // `HKASK_PREDICTION_MARKETS_DATA` is unset.
            "HKASK_DATA_DIR",
            "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS",
            "HKASK_PREDICTION_MARKETS_DATA",
        ],
    },
    BuiltinMcpServer {
        id: "media",
        binary: "hkask-mcp-media",
        description: "Media — research evidence ingestion, annotation, transcription, and excerpts",
        // Transcription uses the child-local MediaRouter; SerpApi supplies
        // YouTube discovery metadata. yt-dlp is download-only and needs no key.
        // Vision/chat/embed cross the IPC bridge to zed's LanguageModelRegistry.
        credentials: &[
            "OPENROUTER_API_KEY",
            "DEEPINFRA_API_KEY",
            "HKASK_SERPAPI_API_KEY",
            "HKASK_DB_PASSPHRASE",
        ],
        config_env: &[
            // IPC bridge socket — required for vision/chat/embed routing
            // through zed's LanguageModelRegistry (transcription is child-local).
            "HKASK_INFERENCE_SOCKET",
            // Data dir — needed for the gallery DB path resolution
            // (databases stay in the internal data dir).
            "HKASK_DATA_DIR",
            // Artifacts dir — needed so derived media (user-facing outputs)
            // resolve under `media-mcp/generated/` in the visible artifacts
            // dir, matching the parent process when an operator
            // `HKASK_ARTIFACTS_DIR` override is set.
            "HKASK_ARTIFACTS_DIR",
            // Gallery DB override (optional — defaults to
            // {data_dir}/mcp/media/gallery.db).
            "HKASK_MEDIA_DB",
            // Research model overrides, supplied by settings-layer defaults.
            "HKASK_MEDIA_STT_MODEL",
            "HKASK_MEDIA_VISION_MODEL",
            // Text-layer pass model override (operator process env — no
            // settings field yet; the media server falls back to the
            // vision/analysis model when unset).
            "HKASK_MEDIA_PASS_MODEL",
            // Embedding model — read by `embed_text` via
            // `hkask_inference::model_constants::embedding_model()`. Without
            // this entry the per-server filter drops it and media embedding
            // fails with "no embedding model configured".
            "HKASK_EMBEDDING_MODEL",
        ],
    },
    BuiltinMcpServer {
        id: "experimentation",
        binary: "hkask-mcp-experimentation",
        description: "Experimentation — experiment registry for the sharded experimentation program: registered experiments with pre-registered predictions, variant lineages, fitness records (caller-asserted report references), and selection fossils",
        // HKASK_DB_PASSPHRASE is the ONE SQLCipher passphrase — the experimentation
        // registry opens with it via the canonical resolve_db_passphrase
        // chain (same as curator/kata-kanban/research). A
        // missing passphrase fails startup visibly (Infrastructure), never
        // an empty-key open.
        credentials: &["HKASK_DB_PASSPHRASE"],
        config_env: &[
            // Data dir — needed so the registry's default DB path
            // (`mcp_server_db("experimentation", "experimentation")` under the D28
            // layout) resolves under the same root as the parent process.
            // Without this, an operator HKASK_DATA_DIR override is silently
            // dropped by `filter_config_env_for_server`.
            "HKASK_DATA_DIR",
            // Registry DB path override — read via std::env::var in run()
            // (the same non-secret DB-path pattern as HKASK_KANBAN_DB and
            // HKASK_RESEARCH_DB).
            "HKASK_EXPERIMENTATION_DB",
        ],
    },
];

/// Just the server IDs, derived from [`BUILT_IN_MCP_SERVERS`].
/// Convenience for consumers that only need the ID list (the shadowed-server
/// settings sweep).
pub fn builtin_mcp_server_ids() -> Vec<&'static str> {
    BUILT_IN_MCP_SERVERS.iter().map(|s| s.id).collect()
}

/// IDs of raw `context_servers` entries that shadow a kask built-in server.
///
/// `ContextServerStore::maintain_servers` merges registry descriptors with
/// `or_insert` — a raw settings entry for the same ID silently shadows the
/// kask-managed registration, which is the only path that composes the
/// server's env via `build_mcp_server_env` (credentials, inference socket,
/// data dir). A stale manual entry (e.g. one written by an earlier setup
/// with `env: {}`) therefore strips every credential from the server the
/// agent's tools connect to, with no warning — the operator cannot
/// distinguish "not configured" from "configured but broken." Kask
/// built-ins are configured through `kask.mcp` (`load_default` /
/// `overrides`) and the `HKASK_MCP_{ID}_BIN` binary override; raw
/// `context_servers` entries for their IDs are configuration the kask
/// system must not silently obey.
pub fn shadowed_context_server_entry_ids(
    content: &settings_content::SettingsContent,
) -> Vec<std::sync::Arc<str>> {
    let builtin_ids = builtin_mcp_server_ids();
    content
        .project
        .context_servers
        .keys()
        .filter(|id| builtin_ids.contains(&id.as_ref()))
        .cloned()
        .collect()
}

/// Remove raw `context_servers` entries that shadow kask built-in servers,
/// returning the removed IDs so the caller can warn per entry. Pure over
/// the content — the caller owns the settings write and the logging.
pub fn remove_shadowing_context_server_entries(
    content: &mut settings_content::SettingsContent,
) -> Vec<std::sync::Arc<str>> {
    let shadowed = shadowed_context_server_entry_ids(content);
    for id in &shadowed {
        content.project.context_servers.remove(id);
    }
    shadowed
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
pub(crate) fn find_server(id: &str) -> Option<&'static BuiltinMcpServer> {
    BUILT_IN_MCP_SERVERS.iter().find(|s| s.id == id)
}

/// Filter a list of `(env_var, credential_url)` pairs to only those the
/// specified server is allowed to receive — only env vars in the server's
/// credential allowlist are kept.
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
    credentials
        .iter()
        .filter(|(env_var, _)| server.credentials.contains(&env_var.as_str()))
        .cloned()
        .collect()
}

/// Tracks `(server_id, env_var)` pairs already warned about being missing.
/// `build_mcp_server_env` is called on every `SettingsStore` observer fire
/// (for env-diffing, not just launching), so without deduplication a missing
/// credential spams the log every time any unrelated setting changes. A
/// (server, env var, read-state) triple is warned at most once per process
/// lifetime — the state in the key lets a CHANGED state re-warn (Missing
/// later flipping to ProviderError stays visible) while the same state
/// repeated stays quiet. Restarting Zed resets the set.
static WARNED_MISSING_CREDENTIALS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<(String, String, String)>>,
> = std::sync::OnceLock::new();

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
/// `inference_socket` and `inference_timeout_secs` are parameters, not global
/// reads, so this function is unit-testable without touching
/// `INFERENCE_SOCKET_PATH`. The timeout publishes the server's
/// `stream_completion` establishment deadline to child-process IPC clients so
/// they can align their read deadline; see
/// `hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV`.
pub async fn build_mcp_server_env(
    server_id: &str,
    settings: &crate::KaskSettings,
    credentials_provider: &dyn credentials_provider::CredentialsProvider,
    inference_socket: Option<&str>,
    inference_timeout_secs: Option<u64>,
    cx: &gpui::AsyncApp,
) -> hkask_types::ServerEnv {
    // 1. Config env: build, then filter per-server. `mcp_env()` is the full
    //    unfiltered map; the allowlist is what keeps the curator's email
    let mut env = filter_config_env_for_server(server_id, &settings.mcp_env());
    let catalog = match crate::database_inventory_path() {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(error) => {
            tracing::warn!(%error, "Database inventory route unavailable; child will refuse the empty route");
            String::new()
        }
    };
    env.insert(hkask_storage::DATABASE_CATALOG_ENV.into(), catalog);
    // Update authority before credential awaits: an older env-building future
    // must not re-grant access after a newer settings pass revoked it.
    let loaded =
        settings.mcp.load_default && *settings.mcp.overrides.get(server_id).unwrap_or(&true);
    let tools = settings
        .mcp
        .delegated_tools
        .get(server_id)
        .filter(|_| loaded)
        .cloned()
        .unwrap_or_default();
    if let Some(grant) = crate::delegation_grants::grant_for_server(server_id, &tools) {
        env.insert(
            hkask_types::inference_ipc::TOOL_GRANT_ENV.to_string(),
            grant,
        );
    }

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
    let cred_urls = filter_credentials_for_server(server_id, &crate::credential_urls_for_mcp());
    for (env_var, url) in cred_urls {
        if let Ok(value) = std::env::var(&env_var)
            && !value.is_empty()
        {
            env.insert(env_var, value);
            continue;
        }
        match read_keychain_credential(credentials_provider, server_id, &env_var, &url, cx).await {
            KeychainCredentialRead::Loaded(value) => {
                env.insert(env_var, value);
                continue;
            }
            // The four distinguishable failure states fall through to the
            // per-state warnings below — never a single "not set or empty"
            // message, so the operator can tell "not configured" (Missing)
            // from "configured but broken" (ProviderError, InvalidUtf8,
            // Empty). Each state carries its own remediation.
            other => {
                // Dedup per (server, env var, STATE): `build_mcp_server_env`
                // runs on every SettingsStore observer fire, and a state
                // CHANGE must re-warn (a key fixed after an error flips the
                // message back through Empty→Loaded silently otherwise).
                // Same state twice stays quiet.
                let warned_set = WARNED_MISSING_CREDENTIALS
                    .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
                let state_key = match &other {
                    KeychainCredentialRead::Missing => "missing",
                    KeychainCredentialRead::Empty => "empty",
                    KeychainCredentialRead::InvalidUtf8 => "invalid-utf8",
                    KeychainCredentialRead::ProviderError(_) => "provider-error",
                    KeychainCredentialRead::Loaded(_) => unreachable!("handled above"),
                };
                let is_new = warned_set
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert((
                        server_id.to_string(),
                        env_var.clone(),
                        state_key.to_string(),
                    ));
                if is_new {
                    let message = match &other {
                        KeychainCredentialRead::Missing => format!(
                            "Credential {env_var} for server {server_id} has no keychain entry \
                             at {url} — the server will fail with permission_denied when it \
                             tries to use it. Set the key via the kask settings UI to fix this."
                        ),
                        KeychainCredentialRead::Empty => format!(
                            "Credential {env_var} for server {server_id} is set but empty \
                             (or whitespace-only) at {url} — the server will fail with \
                             permission_denied when it tries to use it. Re-set a non-empty \
                             key via the kask settings UI."
                        ),
                        KeychainCredentialRead::InvalidUtf8 => format!(
                            "Credential {env_var} for server {server_id} at {url} holds \
                             non-UTF-8 bytes — the server will fail with permission_denied \
                             when it tries to use it. The keychain entry is corrupt; \
                             re-enter the key via the kask settings UI."
                        ),
                        KeychainCredentialRead::ProviderError(error) => format!(
                            "Reading credential {env_var} for server {server_id} from \
                             {url} failed: {error} — the keychain may be locked or \
                             unavailable. The server will fail with permission_denied when \
                             it tries to use the credential; retry after unlocking the \
                             keychain."
                        ),
                        KeychainCredentialRead::Loaded(_) => unreachable!("handled above"),
                    };
                    tracing::warn!(
                        target: "reg.mcp",
                        server_id = %server_id,
                        env_var = %env_var,
                        "{message}"
                    );
                }
            }
        }
        // Startup-default passphrases: provision the required "allostery"
        // default before falling through to the missing-credential warning.
        // See `DEFAULT_PASSPHRASE_ENV_VARS` for why this tier exists at all.
        if let Some(passphrase) = provision_default_passphrase(&env_var, cx).await {
            // Mirror into zed's keychain so the primary ctx.credentials tier
            // (and the settings UI's `has_credential`) sees the provisioned
            // value on subsequent reads; the env insertion below serves this
            // launch directly even if the mirror write fails.
            //
            // The write is atomic (oo7 `create_item` with `replace=true`)
            // and only affects the passphrase entry — it cannot clobber
            // other credentials.
            if let Err(error) = credentials_provider
                .write_credentials(&url, "kask", passphrase.as_bytes(), cx)
                .await
            {
                tracing::warn!(
                    target: "reg.mcp",
                    server_id = %server_id,
                    env_var = %env_var,
                    %error,
                    "Failed to mirror provisioned passphrase to CredentialsProvider — \
                     the value is injected into this launch's env, but future reads \
                     will re-provision until the mirror succeeds"
                );
            }
            env.insert(env_var, passphrase);
            continue;
        }
        // Neither the parent shell, the keychain, nor a startup-default
        // passphrase tier provided a value. The per-state warning already
        // fired inside the keychain-read match above (Missing, Empty,
        // InvalidUtf8, ProviderError — each with its own remediation); this
        // fall-through carries no separate message. The MCP server will
        // surface `permission_denied` when it tries to use the credential
        // (see `.rules` — "Missing credentials must surface as
        // `McpToolError::permission_denied`").
    }

    // 3. Inference IPC socket — not in any allowlist; every server may route
    //    inference through zed's LanguageModelRegistry over the IPC bridge.
    if let Some(socket) = inference_socket {
        env.insert(
            hkask_types::inference_ipc::INFERENCE_SOCKET_ENV.to_string(),
            socket.to_string(),
        );
    }

    // 4. Admission-to-completion timeout — publish the server's deadline so
    //    IPC clients can set their read timeout to `server_timeout + grace`
    //    instead of inventing an independent (shorter) one. Without this, a
    //    slow-but-alive provider produces a storm of `BrokenPipe` warnings: the
    //    client times out first, closes its socket, and the server's later
    //    response write hits EPIPE. With this, the client strictly outlasts the
    //    server, so a timed-out inference produces one timeout (the server's).
    if let Some(secs) = inference_timeout_secs {
        env.insert(
            hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV.to_string(),
            secs.to_string(),
        );
    }

    hkask_types::ServerEnv::from_canonical(env)
}

/// Env vars with a required startup-default passphrase.
///
/// The default ("allostery") is a stated first-run requirement: DB-backed
/// MCP servers must open their SQLCipher databases without login or manual
/// keychain configuration. The default used to be provisioned only in zed's
/// deferred task (`provision_agent` + keychain mirrors in
/// `main.rs`), but servers resolve their launch env before the deferred task runs —
/// on a machine that never signs in, the default never landed and every
/// DB-backed server failed with `permission_denied`. Provisioning at the
/// canonical env path guarantees the default regardless of launch ordering.
///
/// All other credentials have NO default — a miss keeps warning so "not
/// configured" stays visible to the operator.
const DEFAULT_PASSPHRASE_ENV_VARS: [&str; 1] = ["HKASK_DB_PASSPHRASE"];

/// The outcome of reading one MCP-server credential from the keychain —
/// four failure states kept distinguishable (the `.ok()??` collapse this
/// replaces made them indistinguishable: a locked keychain, a missing
/// entry, an empty value, and non-UTF-8 bytes all read as "not set",
/// so the operator could not tell "not configured" from "configured but
/// broken" — the broken-feedback-loop class). Modeled on the distinguishable
/// reads in `inference_providers.rs` and `crates/language_model/src/api_key.rs`.
#[derive(Debug, PartialEq, Eq)]
enum KeychainCredentialRead {
    /// A non-empty value, trimmed for injection where trimming applies.
    Loaded(String),
    /// The keychain holds no entry for this URL (`Ok(None)`).
    Missing,
    /// The entry exists but its value is empty — including a value that is
    /// only whitespace, which previously survived the emptiness filter and
    /// was injected as an EMPTY env value after trimming.
    Empty,
    /// The entry exists but its bytes are not valid UTF-8.
    InvalidUtf8,
    /// The keychain provider itself failed (locked, unavailable, I/O) —
    /// carries the error text.
    ProviderError(String),
}

async fn read_keychain_credential(
    credentials_provider: &dyn credentials_provider::CredentialsProvider,
    server_id: &str,
    env_var: &str,
    url: &str,
    cx: &gpui::AsyncApp,
) -> KeychainCredentialRead {
    let (_username, password) = match credentials_provider.read_credentials(url, cx).await {
        Ok(Some((username, password))) => (username, password),
        Ok(None) => return KeychainCredentialRead::Missing,
        Err(error) => {
            return KeychainCredentialRead::ProviderError(error.to_string());
        }
    };
    let value = match String::from_utf8(password) {
        Ok(value) => value,
        Err(_) => return KeychainCredentialRead::InvalidUtf8,
    };
    // The emptiness check is PRE-trim, deliberately: the DB passphrase is
    // exempt from trimming (its exact bytes key the databases) — a
    // post-trim check would classify a whitespace-only passphrase as
    // Loaded, injecting an empty env value. A value that is empty or
    // whitespace-only at the source is empty, whichever credential it is.
    if value.trim().is_empty() {
        return KeychainCredentialRead::Empty;
    }
    KeychainCredentialRead::Loaded(trim_credential_for_injection(server_id, env_var, value))
}

/// Trim surrounding whitespace from a keychain-read credential before env
/// injection. Keychain values are pasted by hand; a trailing newline from a
/// copy-paste is not part of any credential, and it breaks HTTP header
/// construction downstream (observed live 2026-08-30: a 36-byte Firecrawl
/// key whose last byte was `\n`, failing every request from the long-lived
/// server instance with "failed to parse header value" while fresh spawns
/// — whose `env -i $(...)` harness shell-word-split the newline away —
/// succeeded). The DB passphrase is exempt: its exact bytes key the
/// SQLCipher databases, so trimming it would make every DB unopenable.
fn trim_credential_for_injection(server_id: &str, env_var: &str, value: String) -> String {
    if DEFAULT_PASSPHRASE_ENV_VARS.contains(&env_var) {
        return value;
    }
    let trimmed = value.trim().to_string();
    if trimmed != value {
        tracing::warn!(
            target: "reg.mcp",
            server_id = %server_id,
            env_var = %env_var,
            "Keychain credential has surrounding whitespace — trimmed at injection. \
             Re-set the key via the kask settings UI to fix it at the source."
        );
    }
    trimmed
}

/// Provision the startup-default passphrase for `env_var`, if it has one.
///
/// Runs the keystore chain (env override → existing keychain entry → default
/// "allostery" stored on first run) on the background executor — keyring I/O
/// is synchronous D-Bus and must not block the foreground thread. Returns the
/// resolved passphrase, or `None` when `env_var` has no startup default or
/// provisioning failed (the caller then falls through to the
/// missing-credential warning, which names the env var).
async fn provision_default_passphrase(env_var: &str, cx: &gpui::AsyncApp) -> Option<String> {
    if !DEFAULT_PASSPHRASE_ENV_VARS.contains(&env_var) {
        return None;
    }
    let env_var = env_var.to_string();
    let env_var_for_log = env_var.clone();
    let provisioned = cx
        .background_spawn(async move { crate::identity::provision_db_passphrase() })
        .await;
    match provisioned {
        Ok(passphrase) if !passphrase.is_empty() => Some(passphrase),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(
                target: "reg.mcp",
                env_var = %env_var_for_log,
                %error,
                "Failed to provision the default passphrase — the server will \
                 fail with permission_denied until a passphrase is provisioned \
                 (sign in, or set the env var / keychain entry)"
            );
            None
        }
    }
}

/// Filter a base config env map (`mcp_env()` output) to only the env vars the
/// specified server is allowed to receive — only env vars in the server's
/// config allowlist are kept.
///
/// This prevents the curator's email config (`HKASK_SMTP_USERNAME`,
/// `HKASK_MXROUTE_SERVER`, etc.) from being injected into servers that don't
#[must_use]
pub(crate) fn filter_config_env_for_server(
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
    config_env
        .iter()
        .filter(|(env_var, _)| server.config_env.contains(&env_var.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockCredentialsProvider;

    /// The experimentation server's registration entry is pinned: id, binary, the
    /// SQLCipher passphrase credential, and the config allowlist aligned with
    /// the crate's actual env reads (HKASK_DATA_DIR for the D28 default DB
    /// path, HKASK_EXPERIMENTATION_DB for the override). A drift here breaks launch
    /// under governed env before any tool runs — the allowlist is the only
    /// delivery path for the passphrase (no std::env fallback tier in the
    /// child).
    #[test]
    fn experimentation_registration_entry_is_pinned() {
        let server = BUILT_IN_MCP_SERVERS
            .iter()
            .find(|s| s.id == "experimentation")
            .expect("experimentation server registered");
        assert_eq!(server.binary, "hkask-mcp-experimentation");
        let expected_credentials: &[&str] = &["HKASK_DB_PASSPHRASE"];
        let expected_config: &[&str] = &["HKASK_DATA_DIR", "HKASK_EXPERIMENTATION_DB"];
        assert_eq!(server.credentials, expected_credentials);
        assert_eq!(server.config_env, expected_config);
    }

    #[test]
    fn qa_generation_setting_round_trips_and_reaches_only_its_consumer() {
        use crate::KaskSettings;
        use hkask_inference::model_constants::QA_GENERATION_MODEL_ENV;
        let mut content = settings::KaskSettingsContent::default();
        let models = content.models.get_or_insert_default();
        models.default_model = Some("OpenRouter/z-ai/glm-5.3-flash".into());
        models.qa_generation_model = Some("OpenRouter/~openai/gpt-sol-latest".into());
        let saved = serde_json::to_string(&content).expect("serialize settings content");
        let reloaded = serde_json::from_str::<settings::KaskSettingsContent>(&saved)
            .expect("reload settings content");
        let settings: KaskSettings = reloaded.into();
        assert_eq!(
            settings.models.qa_generation_model,
            "OpenRouter/~openai/gpt-sol-latest"
        );
        let env = settings.mcp_env();
        for server in BUILT_IN_MCP_SERVERS {
            let filtered = filter_config_env_for_server(server.id, &env);
            assert_eq!(
                filtered.get(QA_GENERATION_MODEL_ENV).map(String::as_str),
                (server.id == "corpus").then_some("OpenRouter/~openai/gpt-sol-latest"),
                "{}",
                server.id
            );
        }
        // Clearing the setting removes it from the restart-diff environment.
        let cleared: settings::KaskSettingsContent =
            serde_json::from_str(r#"{"models":{"qa_generation_model":""}}"#)
                .expect("cleared setting");
        let cleared: KaskSettings = cleared.into();
        assert!(!cleared.mcp_env().contains_key(QA_GENERATION_MODEL_ENV));
        assert!(
            !KaskSettings::default()
                .mcp_env()
                .contains_key(QA_GENERATION_MODEL_ENV)
        );
        assert!(
            KaskSettings::default()
                .models
                .qa_generation_model
                .is_empty()
        );
    }

    /// The rerank default (operator ruling 2026-09-11) must reach the research
    /// server's filtered env, and only the research server — the same
    /// consumer-scoping discipline the QA generator follows.
    #[test]
    fn rerank_default_reaches_only_the_research_server() {
        use hkask_inference::model_constants::DEFAULT_RERANK_MODEL;
        let settings: crate::KaskSettings = settings::KaskSettingsContent::default().into();
        assert_eq!(settings.models.rerank_model, DEFAULT_RERANK_MODEL);
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_RERANK_MODEL").map(String::as_str),
            Some(DEFAULT_RERANK_MODEL)
        );
        for server in BUILT_IN_MCP_SERVERS {
            let filtered = filter_config_env_for_server(server.id, &env);
            assert_eq!(
                filtered.get("HKASK_RERANK_MODEL").map(String::as_str),
                (server.id == "research").then_some(DEFAULT_RERANK_MODEL),
                "{}",
                server.id
            );
        }
    }

    fn server_by_id(id: &str) -> &'static BuiltinMcpServer {
        BUILT_IN_MCP_SERVERS
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("server '{id}' not in BUILT_IN_MCP_SERVERS"))
    }

    /// The startup-default gate: both passphrase env vars are in
    /// `DEFAULT_PASSPHRASE_ENV_VARS`, so `provide_default_passphrase`
    /// resolves it (env → keychain → first-run "allostery") at MCP
    /// launch time. This is what makes "stores never start down" true at
    /// startup — remove it from the list and the gate regresses.
    #[test]
    fn startup_default_passphrase_gate_includes_db_passphrase() {
        assert!(DEFAULT_PASSPHRASE_ENV_VARS.contains(&"HKASK_DB_PASSPHRASE"));
    }

    /// The derived fns must match the main registry — this pins the single-source
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
            s.credentials.to_vec(),
            vec!["HKASK_FRED_API_KEY"],
            "prediction-markets credentials allowlist drifted — HKASK_FRED_API_KEY \
             is read in run() for live reference-level fetches; add a credential \
             only with a read site in hkask-mcp-prediction-markets"
        );
        // Read site: std::env::var("HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS")
        // in `run()` (with a malformed-value warn, not silent fallback).
        // HKASK_DATA_DIR is read transitively via `resolve_under_data_dir` →
        // `resolve_data_dir` in hkask-types (the server's fallback when
        // HKASK_PREDICTION_MARKETS_DATA is unset). Allowlisted so an
        // operator `HKASK_DATA_DIR` override is not silently dropped by
        // `filter_config_env_for_server`.
        assert_eq!(
            s.config_env.to_vec(),
            vec![
                "HKASK_DATA_DIR",
                "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS",
                "HKASK_PREDICTION_MARKETS_DATA",
            ],
            "prediction-markets config_env allowlist drifted — every entry must \
             have a read site in hkask-mcp-prediction-markets (HKASK_DATA_DIR is \
             read transitively via resolve_under_data_dir)"
        );
    }

    // The curator server should receive only the DB passphrase, no data
    // service API keys and no SMTP password (it has no email sink — the
    // alert email runs in the editor process). This pins the blast-radius
    // reduction.
    #[test]
    fn curator_credentials_do_not_include_data_service_keys() {
        let all_credentials: Vec<(String, String)> = ["HKASK_SMTP_PASSWORD", "OPENROUTER_API_KEY"]
            .iter()
            .map(|env| (env.to_string(), "url".to_string()))
            .collect();
        let filtered = filter_credentials_for_server("curator", &all_credentials);
        let env_vars: Vec<&str> = filtered.iter().map(|(k, _)| k.as_str()).collect();
        assert!(
            !env_vars.contains(&"HKASK_SMTP_PASSWORD"),
            "curator has no email sink — it must not receive HKASK_SMTP_PASSWORD"
        );
        assert!(
            !env_vars.contains(&"OPENROUTER_API_KEY"),
            "curator should NOT receive OPENROUTER_API_KEY"
        );
    }

    /// expect: [P7] Research model choices reach only media; creative model
    /// variables cannot re-enable retired pipelines through child injection.
    #[test]
    fn media_models_are_research_only_and_round_trip_to_child() {
        let content: settings_content::KaskMediaSettingsContent = serde_json::from_value(
            serde_json::json!({"stt_model":"OpenRouter/research-stt", "vision_model":"OpenRouter/research-vision"}),
        ).expect("research media content");
        let mut settings = crate::KaskSettings::default();
        settings.media = content.into();
        let env = settings.mcp_env();
        let media_env = filter_config_env_for_server("media", &env);
        for key in ["HKASK_MEDIA_STT_MODEL", "HKASK_MEDIA_VISION_MODEL"] {
            assert_eq!(media_env.get(key), env.get(key));
            assert!(media_env.contains_key(key));
            for server in BUILT_IN_MCP_SERVERS
                .iter()
                .filter(|server| server.id != "media")
            {
                assert!(!filter_config_env_for_server(server.id, &env).contains_key(key));
            }
        }
        let cleared: settings_content::KaskMediaSettingsContent =
            serde_json::from_value(serde_json::json!({"stt_model":"", "vision_model":""}))
                .expect("explicitly cleared models");
        settings.media = cleared.into();
        let cleared_env = settings.mcp_env();
        for key in ["HKASK_MEDIA_STT_MODEL", "HKASK_MEDIA_VISION_MODEL"] {
            assert!(!cleared_env.contains_key(key));
        }
        let retired: std::collections::HashMap<String, String> = [
            "HKASK_MEDIA_TTS_MODEL",
            "HKASK_MEDIA_IMAGE_GEN_MODEL",
            "HKASK_MEDIA_VIDEO_MODEL",
        ]
        .into_iter()
        .map(|key| (key.to_string(), "retired".to_string()))
        .collect();
        assert!(filter_config_env_for_server("media", &retired).is_empty());
    }

    // The Reduct.cloud integration was removed (2026-10-22, operator-authorized;
    // DIVERGENCE.md D9/D5): no Data Services descriptor and no child allowlist
    // may carry its key — a dormant third-party secret with no read path is
    // worse than a logged one-time delete (the ABW precedent).
    #[test]
    fn reduct_key_reaches_no_child() {
        assert!(
            crate::DATA_SERVICES
                .iter()
                .all(|entry| entry.credential_key != "reduct_api_key"),
            "the Reduct.cloud removal is complete — no Data Services descriptor may carry its key"
        );
        for server in BUILT_IN_MCP_SERVERS.iter() {
            assert!(
                !server.credentials.contains(&"REDUCT_API_KEY"),
                "the Reduct.cloud integration is removed — its API key must not reach the {} server",
                server.id
            );
        }
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

    // The curator server must NOT receive email config vars — it has no
    // email sink (no server-side reader); the alert email runs in the
    // editor process. The former delivery was dead surface.
    #[test]
    fn curator_config_env_excludes_email_settings() {
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
        assert!(
            !filtered.contains_key("HKASK_SMTP_USERNAME"),
            "curator has no email sink — email transport vars must not reach it"
        );
        assert!(
            !filtered.contains_key("HKASK_MXROUTE_SERVER"),
            "curator has no email sink — email transport vars must not reach it"
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

    /// fal.ai is deprecated for the corpus server (the OCR docres path was
    /// removed). The corpus
    /// server no longer reads `FALAI_API_KEY` or `HKASK_USE_FAL_DOCRES`. Pin
    /// the removal so a future re-add is caught (`.rules` "tests must pin
    /// deliberate deviations").
    #[test]
    fn corpus_allowlist_excludes_deprecated_fal_ai() {
        let corpus = server_by_id("corpus");
        let creds = corpus.credentials;
        assert!(
            !creds.contains(&"FALAI_API_KEY"),
            "FALAI_API_KEY must not be granted to the corpus server — fal.ai docres is removed"
        );
        let cfg = corpus.config_env;
        assert!(
            !cfg.contains(&"HKASK_USE_FAL_DOCRES"),
            "HKASK_USE_FAL_DOCRES must not be granted to the corpus server — fal.ai docres is removed"
        );
    }

    // ── read-alignment history and current per-server pins ─────────
    //
    // This block began when only 5 of 13 then-registered servers had a
    // read-alignment test. The retired training server's large secret grant
    // helped expose both drift directions: under-grants silently drop operator
    // overrides; over-grants hand secrets to processes that never read them.
    //
    // Each expectation below was derived by grepping the server's own source for
    // `std::env::var` / `ctx.credentials.get` sites, so the assertion is against
    // observed reads rather than intent.

    #[test]
    fn curator_allowlist_matches_actual_reads() {
        let s = server_by_id("curator");
        // Secret reads: ctx.credentials.get("HKASK_DB_PASSPHRASE")
        // (SQLCipher curator.db) — the only credential the server reads.
        // The curator MCP server has NO email sink (it links
        // `hkask-regulation`, not `hkask-email`); the algedonic alert email
        // runs in the editor process, whose sink is wired from settings and
        // reads the SMTP password from the keychain via the editor's own
        // process env — never from MCP child delivery.
        let creds = s.credentials;
        assert!(
            !creds.contains(&"HKASK_SMTP_PASSWORD"),
            "the curator server has no email sink — it must not receive \
             HKASK_SMTP_PASSWORD (secrets are granted only where read)"
        );
        assert!(
            creds.contains(&"HKASK_DB_PASSPHRASE"),
            "curator must receive HKASK_DB_PASSPHRASE to open its SQLCipher curator.db \
             under governed launch — the run() reads it from ctx.credentials \
             with no std::env::var fallback"
        );
        assert!(
            !s.config_env.is_empty(),
            "curator config_env should not be empty — the server reads the curator \
             DB path, memory-life and distillation model settings from it"
        );
        // HKASK_MEMORY_LIFE_DAYS: read by `memory_life_days_from_env` at
        // `open_curator_stores` (hkask_mcp_curator.rs) — the decay constant
        // from `kask.memory.memory_life_days`, emitted by
        // `emit_curator_distillation_env`. Without this allowlist entry the
        // per-server filter silently drops the operator's override and decay
        // reverts to the default 180 while the regulation sensor keeps
        // reporting the configured value — the exact misreport this check exists to end.
        assert!(
            s.config_env.contains(&"HKASK_MEMORY_LIFE_DAYS"),
            "curator must receive HKASK_MEMORY_LIFE_DAYS so the configured \
             decay constant reaches the store"
        );
        // The embedding model must be allowlisted: `embed_for_semantic_recall`
        // and the semantic-recall tools read HKASK_EMBEDDING_MODEL via the env
        // accessor with no fallback — an unallowlisted entry is silently
        // dropped by the per-server filter and every semantic recall degrades
        // (the live gap observed 2026-09-04).
        let s = server_by_id("curator");
        assert!(
            s.config_env.contains(&"HKASK_EMBEDDING_MODEL"),
            "curator must receive HKASK_EMBEDDING_MODEL for semantic recall"
        );
    }

    #[test]
    fn research_allowlist_matches_actual_reads() {
        let s = server_by_id("research");
        // ctx.credentials.get sites: EXA, TAVILY, BRAVE, SERPAPI, FIRECRAWL.
        // HKASK_DB_PASSPHRASE is read for the RSS store.
        let creds = s.credentials;
        for key in [
            "HKASK_EXA_API_KEY",
            "HKASK_TAVILY_API_KEY",
            "HKASK_BRAVE_API_KEY",
            "HKASK_SERPAPI_API_KEY",
            "HKASK_FIRECRAWL_API_KEY",
        ] {
            assert!(
                creds.contains(&key),
                "research reads {key} via ctx.credentials.get but it is not \
                 allowlisted — the provider would be silently unavailable"
            );
        }
        assert!(
            s.config_env.contains(&"HKASK_RESEARCH_DB"),
            "research reads HKASK_RESEARCH_DB via std::env::var but it is not allowlisted"
        );
        assert!(
            s.config_env.contains(&"HKASK_EMBEDDING_MODEL"),
            "research reads HKASK_EMBEDDING_MODEL via model_constants::embedding_model() \
             but it is not allowlisted — the semantic duplication tier would silently \
             degrade to the shingle floor despite an operator override"
        );
        assert!(
            s.config_env.contains(&"HKASK_DATA_DIR"),
            "research resolves its default research DB via resolve_under_data_dir \
             but HKASK_DATA_DIR is not allowlisted — an operator override \
             would be silently dropped"
        );
        assert!(
            !s.config_env.contains(&"HKASK_ARTIFACTS_DIR"),
            "research no longer resolves the artifacts dir — the InlineTable \
             presentation was removed with the spreadsheet capability \
             (2026-10-15); re-add the entry only with a read site in \
             hkask-mcp-research"
        );
        assert!(
            s.config_env.contains(&"HKASK_RERANK_MODEL"),
            "research reads HKASK_RERANK_MODEL via std::env::var (rerank model \
             resolution) but it is not allowlisted — an operator override \
             would be silently stripped under governed launch"
        );
    }

    #[test]
    fn scenarios_allowlist_matches_actual_reads() {
        let s = server_by_id("scenarios");
        // No secret read sites at all — the server is storage-only.
        assert!(
            s.credentials.is_empty(),
            "scenarios has no credential read site; granting one would be an \
             unjustified secret grant"
        );
        // HKASK_DATA_DIR is read transitively via `resolve_under_data_dir`
        // → `resolve_data_dir` in hkask-types (the server's fallback when
        // HKASK_SCENARIOS_DATA is unset). Allowlisted so an operator
        // `HKASK_DATA_DIR` override is not silently dropped by
        // `filter_config_env_for_server`.
        assert_eq!(
            s.config_env.to_vec(),
            vec!["HKASK_DATA_DIR", "HKASK_SCENARIOS_DATA"],
            "scenarios config_env allowlist drifted — HKASK_SCENARIOS_DATA is \
             its direct std::env::var read; HKASK_DATA_DIR is read transitively \
             via resolve_under_data_dir"
        );
    }

    /// The training server was removed (training is outside the co-researcher
    /// mandate). Pin its absence so a reintroduction is a deliberate decision
    /// with its own registry entry, not an accidental revival.
    #[test]
    fn training_server_is_not_registered() {
        assert!(
            !BUILT_IN_MCP_SERVERS.iter().any(|s| s.id == "training"),
            "the training server was removed — reintroducing it needs a new \
             registry entry, build-list line and settings surface"
        );
        // The training-only child grants must not reappear. RunPod itself is
        // intentionally absent from this list: its API key remains live in the
        // native D29 OCR provider, but no MCP child consumes it now.
        const RETIRED_TRAINING_CREDENTIALS: &[&str] = &["HF_TOKEN", "RUNPOD_API_KEY"];
        const RETIRED_TRAINING_CONFIG: &[&str] = &[
            "HKASK_TRAINING_HOST",
            "HKASK_TRAINING_CACHE_DIR",
            "HKASK_TRAINING_DB",
            "HKASK_HF_ARTIFACT_OWNER",
            "HKASK_HF_DATASET_REPO",
            "HKASK_HF_MODEL_REPO",
            "RUNPOD_TEMPLATE_ID",
            "RUNPOD_GPU_TYPE_ID",
            "RUNPOD_CONTAINER_DISK_GB",
            "RUNPOD_DOCKER_IMAGE",
            "RUNPOD_DOCKER_ARGS",
            "HKASK_PODS_FILE",
            "NEBIUS_PROJECT_ID",
            "NEBIUS_SUBNET_ID",
            "NEBIUS_GPU_PLATFORM",
            "NEBIUS_GPU_PRESET",
            "NEBIUS_IMAGE_FAMILY",
            "NEBIUS_CLI_PATH",
        ];
        for server in BUILT_IN_MCP_SERVERS {
            for key in RETIRED_TRAINING_CREDENTIALS {
                assert!(
                    !server.credentials.contains(key),
                    "{} still grants retired training credential {key}",
                    server.id
                );
            }
            for key in RETIRED_TRAINING_CONFIG {
                assert!(
                    !server.config_env.contains(key),
                    "{} still grants retired training config {key}",
                    server.id
                );
            }
        }
        assert!(
            !crate::credential_urls_for_mcp()
                .iter()
                .any(|(env_var, _)| env_var == "RUNPOD_API_KEY"),
            "RunPod OCR is native-only after training removal; its key must not \
             be injected into or restart MCP children"
        );
    }

    /// Every secret-shaped credential grant must be justified by a read site.
    /// This is the generic backstop for the 8 servers without a bespoke test:
    /// it cannot verify each name, but it catches the case where a credential
    /// allowlist grows without the registry comment that documents why.
    #[test]
    fn every_credential_grant_is_secret_shaped_or_documented() {
        for server in BUILT_IN_MCP_SERVERS {
            for key in server.credentials {
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
        full_config.insert("HKASK_RESEARCH_DB".to_string(), "/research.db".to_string());
        // A credential-shaped key that lives in `credentials`, not `config_env`.
        let credential_keys = ["HKASK_SMTP_PASSWORD", "OPENROUTER_API_KEY"];

        for server in BUILT_IN_MCP_SERVERS {
            // Simulate `build_mcp_server_env`'s filter sequence:
            //   1. config filtered by `config_env` allowlist
            //   2. credentials merged (filtered by `credentials` allowlist)
            //   3. inference socket added
            //   4. inference timeout added
            let mut env = filter_config_env_for_server(server.id, &full_config);
            for key in server.credentials {
                if credential_keys.contains(&key) {
                    env.insert(key.to_string(), "secret-value".to_string());
                }
            }
            env.insert(
                hkask_types::inference_ipc::INFERENCE_SOCKET_ENV.to_string(),
                "/tmp/sock".to_string(),
            );
            env.insert(
                hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV.to_string(),
                "300".to_string(),
            );

            let allowed: std::collections::HashSet<&str> = std::collections::HashSet::from_iter(
                server
                    .config_env
                    .iter()
                    .copied()
                    .chain(server.credentials.iter().copied())
                    .chain(std::iter::once(
                        hkask_types::inference_ipc::INFERENCE_SOCKET_ENV,
                    ))
                    .chain(std::iter::once(
                        hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV,
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
    /// default). The `OPENROUTER_API_KEY` env var is set and removed within
    /// this test only.
    #[test]
    fn shell_set_credential_survives_env_clear() {
        // SAFETY: Single-threaded test context. Set a shell value for the
        // OpenRouter key, simulating an operator who exports it in their
        // shell.
        unsafe { std::env::set_var("OPENROUTER_API_KEY", "shell-secret-value") };

        // Mirror the credential loop from `build_mcp_server_env`:
        //   if parent env has a non-empty value → insert into env, continue
        //   else → read from keychain (simulated as None here)
        let mut env = std::collections::HashMap::<String, String>::new();
        let cred_urls = filter_credentials_for_server("media", &crate::credential_urls_for_mcp());
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
            env.get("OPENROUTER_API_KEY").map(|v| v.as_str()),
            Some("shell-secret-value"),
            "shell-set OPENROUTER_API_KEY must be inserted into the env map so it \
             survives the governed path's cmd.env_clear() — the prior `continue` \
             skipped it, and the child received neither the shell value nor the \
             keychain value"
        );

        // SAFETY: Clean up the test env var.
        unsafe { std::env::remove_var("OPENROUTER_API_KEY") };
    }

    // ── build_mcp_server_env: composed-path filter-order tests ──────────
    //
    // P18: the existing `build_mcp_server_env_composition_respects_allowlists`
    // test simulates the filter sequence synchronously — it never calls the
    // real `build_mcp_server_env`. The comment at L386-394 warns that both
    // prior bugs were invisible because the filter helpers were exercised in
    // isolation, never the composed path. These tests call the real composed
    // function with a mock `CredentialsProvider` and pin the load-bearing
    // filter order: config filtered first, then credentials merged into the
    // already-filtered map (the two filters apply to disjoint key sets).

    /// expect: "Settings unload revokes delegated authority before stopping the child" [P1]
    #[test]
    fn main_unload_revokes_before_stop() {
        let main = include_str!("../../../../crates/zed/src/main.rs");
        assert!(main.contains("for server_id in to_stop {\n                kask_bridge::revoke_delegation_grant(server_id);\n                runtime.stop_server(server_id).await;"));
    }

    static ENV_TEST_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

    /// expect: "Only loaded children receive stable parent-held grants, and unload cannot remint authority" [P1]
    #[tokio::test]
    async fn delegated_grants_follow_settings_and_launch_lifecycle() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let provider = MockCredentialsProvider::new(
            crate::credential_urls_for_mcp()
                .into_iter()
                .map(|(_, url)| (url, b"disposable-test-secret".to_vec()))
                .collect(),
        );
        let context = gpui::TestAppContext::single();
        let cx = context.to_async();
        let content: settings_content::KaskSettingsContent = serde_json::from_value(
            serde_json::json!({"mcp":{"delegated_tools":{"kata-kanban":["research/rss_search"]}}}),
        )
        .expect("settings");
        let mut settings = crate::KaskSettings::from(content);
        assert!(
            crate::KaskSettings::default()
                .mcp
                .delegated_tools
                .is_empty()
        );
        let schema = serde_json::to_value(schemars::schema_for!(
            settings_content::KaskMcpSettingsContent
        ))
        .expect("schema");
        assert!(schema["properties"]["delegated_tools"].is_object());
        let first =
            build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
        let token = first
            .get(hkask_types::inference_ipc::TOOL_GRANT_ENV)
            .expect("token")
            .to_string();
        assert!(crate::delegation_grants::parent_allows(
            Some(&token),
            "research/rss_search"
        ));
        assert!(!crate::delegation_grants::parent_allows(
            Some(&token),
            "research/rss_fetch"
        ));
        let again =
            build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
        assert_eq!(
            again.get(hkask_types::inference_ipc::TOOL_GRANT_ENV),
            Some(token.as_str())
        );
        let other = build_mcp_server_env("portfolio", &settings, &provider, None, None, &cx).await;
        assert!(
            other
                .get(hkask_types::inference_ipc::TOOL_GRANT_ENV)
                .is_none()
        );
        settings.mcp.overrides.insert("kata-kanban".into(), false);
        crate::revoke_delegation_grant("kata-kanban");
        let unloaded =
            build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
        assert!(
            unloaded
                .get(hkask_types::inference_ipc::TOOL_GRANT_ENV)
                .is_none(),
            "observer env diff must not remint an unloaded grant"
        );
        assert!(!crate::delegation_grants::parent_allows(
            Some(&token),
            "research/rss_search"
        ));
        settings.mcp.overrides.insert("kata-kanban".into(), true);
        let reloaded =
            build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
        assert_ne!(
            reloaded.get(hkask_types::inference_ipc::TOOL_GRANT_ENV),
            Some(token.as_str())
        );
        settings
            .mcp
            .delegated_tools
            .insert("kata-kanban".into(), vec!["research/*".into()]);
        let invalid =
            build_mcp_server_env("kata-kanban", &settings, &provider, None, None, &cx).await;
        assert!(
            invalid
                .get(hkask_types::inference_ipc::TOOL_GRANT_ENV)
                .is_none()
        );
        crate::revoke_delegation_grant("kata-kanban");
    }

    #[tokio::test]
    async fn build_mcp_server_env_forwards_allowlisted_operator_overrides() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let settings = crate::KaskSettings::default();
        let secrets: std::collections::HashMap<String, Vec<u8>> = crate::credential_urls_for_mcp()
            .into_iter()
            .map(|(_, url)| (url, b"disposable-test-secret".to_vec()))
            .collect();
        let provider = MockCredentialsProvider::new(secrets);
        unsafe {
            std::env::set_var("HKASK_CURATOR_DB", "/tmp/test-curator-override.db");
        }
        let cx = gpui::TestAppContext::single().to_async();
        let curator_env =
            build_mcp_server_env("curator", &settings, &provider, None, None, &cx).await;
        unsafe {
            std::env::remove_var("HKASK_CURATOR_DB");
        }
        assert_eq!(
            curator_env.get("HKASK_CURATOR_DB"),
            Some("/tmp/test-curator-override.db"),
            "HKASK_CURATOR_DB set by main.rs must survive build_mcp_server_env for curator"
        );
    }

    /// A-4: the four keychain-read failure states are distinguishable —
    /// Loaded / Missing / Empty / InvalidUtf8 / ProviderError each classify
    /// distinctly (the `.ok()??` collapse this replaces made them read as
    /// "not set"), and a whitespace-only value classifies as Empty rather
    /// than injecting an empty env value. The mock's `failures` injection is
    /// what makes ProviderError testable at all — the prior mock always
    /// returned `Ok`.
    #[tokio::test]
    async fn keychain_credential_failure_states_are_distinguishable() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let cx = gpui::TestAppContext::single().to_async();
        let (_, passphrase_url) = crate::credential_urls_for_mcp()
            .into_iter()
            .find(|(env_var, _)| env_var == "HKASK_DB_PASSPHRASE")
            .expect("HKASK_DB_PASSPHRASE has a credential URL");

        // SAFETY: clear the shell tier so every case exercises the keychain
        // branch. Single-threaded test under ENV_TEST_LOCK.
        unsafe { std::env::remove_var("HKASK_DB_PASSPHRASE") };

        // Missing: no keychain entry at all.
        let provider = MockCredentialsProvider::new(std::collections::HashMap::new());
        let read = read_keychain_credential(
            &provider,
            "curator",
            "HKASK_DB_PASSPHRASE",
            &passphrase_url,
            &cx,
        )
        .await;
        assert_eq!(read, KeychainCredentialRead::Missing, "{read:?}");

        // Loaded: a real non-empty value.
        let mut secrets = std::collections::HashMap::new();
        secrets.insert(
            passphrase_url.clone(),
            b"keychain-secret-passphrase".to_vec(),
        );
        let provider = MockCredentialsProvider::new(secrets);
        let read = read_keychain_credential(
            &provider,
            "curator",
            "HKASK_DB_PASSPHRASE",
            &passphrase_url,
            &cx,
        )
        .await;
        assert_eq!(
            read,
            KeychainCredentialRead::Loaded("keychain-secret-passphrase".to_string()),
            "{read:?}"
        );

        // Empty: an entry whose value is the empty string.
        let mut secrets = std::collections::HashMap::new();
        secrets.insert(passphrase_url.clone(), Vec::new());
        let provider = MockCredentialsProvider::new(secrets);
        let read = read_keychain_credential(
            &provider,
            "curator",
            "HKASK_DB_PASSPHRASE",
            &passphrase_url,
            &cx,
        )
        .await;
        assert_eq!(read, KeychainCredentialRead::Empty, "{read:?}");

        // Empty (whitespace-only): a value that survives the emptiness filter
        // but trims to nothing — previously injected as an EMPTY env value.
        let mut secrets = std::collections::HashMap::new();
        secrets.insert(passphrase_url.clone(), b"  \t \n".to_vec());
        let provider = MockCredentialsProvider::new(secrets);
        let read = read_keychain_credential(
            &provider,
            "curator",
            "HKASK_DB_PASSPHRASE",
            &passphrase_url,
            &cx,
        )
        .await;
        assert_eq!(read, KeychainCredentialRead::Empty, "{read:?}");

        // InvalidUtf8: an entry with corrupt bytes. The passphrase is exempt
        // from trimming but not from UTF-8 validation — a non-UTF-8 value can
        // never be injected as a credential.
        let mut secrets = std::collections::HashMap::new();
        secrets.insert(passphrase_url.clone(), vec![0xff_u8, 0xfe, 0xfd]);
        let provider = MockCredentialsProvider::new(secrets);
        let read = read_keychain_credential(
            &provider,
            "curator",
            "HKASK_DB_PASSPHRASE",
            &passphrase_url,
            &cx,
        )
        .await;
        assert_eq!(read, KeychainCredentialRead::InvalidUtf8, "{read:?}");

        // ProviderError: the keychain itself fails (locked, unavailable).
        let mut failures = std::collections::HashMap::new();
        failures.insert(passphrase_url.clone(), "keychain locked".to_string());
        let provider = MockCredentialsProvider {
            secrets: std::collections::HashMap::new(),
            failures,
        };
        let read = read_keychain_credential(
            &provider,
            "curator",
            "HKASK_DB_PASSPHRASE",
            &passphrase_url,
            &cx,
        )
        .await;
        assert_eq!(
            read,
            KeychainCredentialRead::ProviderError("keychain locked".to_string()),
            "{read:?}"
        );
    }

    #[test]
    fn all_mcp_servers_allowlists_match_source_env_reads() {
        fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let entries = std::fs::read_dir(dir)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()));
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect_rs_files(&path, out);
                } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if !name.ends_with("_tests.rs")
                        && name != "tests.rs"
                        && name != "test_support.rs"
                    {
                        out.push(path);
                    }
                }
            }
        }

        let mcp_servers_root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mcp-servers");
        let prefixes = [
            "std::env::var(\"",
            "env::var(\"",
            "std::env::var_os(\"",
            "env::var_os(\"",
            "parse_env_warn(\"",
            "parse_env_u64(\"",
            "required_env(\"",
            "configured(\"",
            "resolve_model(\"",
            "resolve_credential(\"",
            "credentials.get(\"",
        ];

        for server in BUILT_IN_MCP_SERVERS {
            let src_dir = mcp_servers_root.join(server.binary).join("src");
            let mut files = Vec::new();
            collect_rs_files(&src_dir, &mut files);

            let mut read_vars = std::collections::BTreeSet::new();
            for file in files {
                let content = std::fs::read_to_string(&file)
                    .unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()));
                // Stop at the first #[cfg(test)] marker so test-only env vars are ignored.
                let prod_content = content.split("#[cfg(test)]").next().unwrap_or(&content);
                for prefix in &prefixes {
                    let mut rest = prod_content;
                    while let Some(idx) = rest.find(prefix) {
                        rest = &rest[idx + prefix.len()..];
                        if let Some(end) = rest.find('"') {
                            let var_name = &rest[..end];
                            if var_name != "HOME" && var_name != "PATH" {
                                read_vars.insert(var_name.to_string());
                            }
                            rest = &rest[end + 1..];
                        }
                    }
                }
                // Media model env constants in `hkask_mcp_media::models`.
                for (const_ref, env_name) in [
                    ("configured(STT_ENV)", "HKASK_MEDIA_STT_MODEL"),
                    ("configured(VISION_ENV)", "HKASK_MEDIA_VISION_MODEL"),
                    ("configured(PASS_ENV)", "HKASK_MEDIA_PASS_MODEL"),
                ] {
                    if prod_content.contains(const_ref) {
                        read_vars.insert(env_name.to_string());
                    }
                }
            }

            let allowed: std::collections::BTreeSet<&str> = server
                .credentials
                .iter()
                .copied()
                .chain(server.config_env.iter().copied())
                .collect();

            for var in &read_vars {
                assert!(
                    allowed.contains(var.as_str()),
                    "MCP server '{}' ({}) reads env var '{}' in production src/, \
                     but it is missing from BuiltinMcpServer::{{credentials, config_env}}",
                    server.id,
                    server.binary,
                    var
                );
            }
        }
    }

    /// Pin the load-bearing filter order in `build_mcp_server_env`: config is
    /// filtered first, then credentials are resolved and merged into the
    /// already-filtered map. The two filters apply to disjoint key sets — a
    /// credential key must survive the config filter even if a config var of
    /// the same name existed in the unfiltered `mcp_env()` map. The prior Path
    /// B bug dropped every credential because the config filter ran on the
    /// credential map too.
    ///
    /// This test constructs a `KaskSettings` whose `mcp_env()` emits a config
    /// var that collides with a credential key for the `curator` server, then
    /// asserts the credential value (from the mock keychain) survives in the
    /// composed output — not the config value, and not absent.
    #[tokio::test]
    async fn build_mcp_server_env_composed_path_credential_survives_config_filter() {
        let _guard = ENV_TEST_LOCK.lock().await;
        // The curator server's `credentials` allowlist includes
        // `HKASK_DB_PASSPHRASE`. We inject a mock keychain secret
        // for its credential URL and assert it lands in the composed env.
        let settings = crate::KaskSettings::default();
        // Collect the (env_var, url) pairs the curator server would receive.
        let cred_urls = filter_credentials_for_server("curator", &crate::credential_urls_for_mcp());
        // Find the DB passphrase URL; if absent, the test setup is
        // stale relative to the registry and we fail loudly.
        let passphrase_url = cred_urls
            .iter()
            .find(|(env_var, _)| env_var == "HKASK_DB_PASSPHRASE")
            .map(|(_, url)| url.clone())
            .expect("HKASK_DB_PASSPHRASE must be in the curator credentials allowlist");

        let mut secrets = std::collections::HashMap::new();
        secrets.insert(
            passphrase_url.clone(),
            b"keychain-secret-passphrase".to_vec(),
        );
        let provider = MockCredentialsProvider::new(secrets);

        // SAFETY: Ensure no shell env var leaks into the test — the keychain
        // branch must be the one that injects the value. Single-threaded test.
        unsafe { std::env::remove_var("HKASK_DB_PASSPHRASE") };

        let cx = gpui::TestAppContext::single().to_async();
        let env = build_mcp_server_env(
            "curator",
            &settings,
            &provider,
            Some("/tmp/sock"),
            Some(300),
            &cx,
        )
        .await;

        // The credential must survive the config filter and land in the
        // composed env with the keychain value — not be dropped by the config
        // filter running on the credential map (the Path B regression).
        assert_eq!(
            env.get("HKASK_DB_PASSPHRASE"),
            Some("keychain-secret-passphrase"),
            "credential must survive the config filter in the composed path — \
             the prior Path B bug dropped every credential because the config \
             filter ran on the credential map too"
        );

        // The inference socket must also be injected (it is added last,
        // outside both filters).
        assert_eq!(
            env.get(hkask_types::inference_ipc::INFERENCE_SOCKET_ENV),
            Some("/tmp/sock"),
            "inference socket must be injected into the composed env"
        );

        // The inference timeout must also be injected — it is published
        // alongside the socket so child IPC clients can align their read
        // deadline with the server's `inference_timeout`. Without this, a
        // slow-but-alive provider produces a `BrokenPipe` warn storm (client
        // times out first, server's later write hits EPIPE).
        assert_eq!(
            env.get(hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV),
            Some("300"),
            "inference timeout must be injected into the composed env so IPC \
             clients can align their read deadline with the server's"
        );
    }

    /// Pin the other half of the filter order: a config var that is NOT in
    /// the server's `config_env` allowlist must NOT survive the composed path,
    /// even if it collides with a credential key. This is the Path A
    /// regression — the full unfiltered `mcp_env()` map leaked because
    /// `extend` only overwrote allowed keys.
    ///
    /// We use the `scenarios` server, whose `config_env` allowlist is
    /// `["HKASK_DATA_DIR", "HKASK_SCENARIOS_DATA"]`
    /// and `credentials` allowlist is `[]` (empty). A config var outside the
    /// allowlist (e.g. `HKASK_MXROUTE_SERVER`,
    /// which only the curator server may receive) must be filtered out by the
    /// composed path.
    #[tokio::test]
    async fn build_mcp_server_env_composed_path_filters_unallowed_config() {
        let _guard = ENV_TEST_LOCK.lock().await;
        // Build settings whose `mcp_env()` emits a curator-only config var.
        // We can't easily mutate `mcp_env()` directly, so we rely on the
        // real `KaskSettings::default()` — if it emits `HKASK_MXROUTE_SERVER`,
        // the composed path must filter it out for the scenarios server. If
        // the default does not emit it, the test still pins the invariant:
        // whatever `mcp_env()` emits, only allowlisted keys survive.
        let settings = crate::KaskSettings::default();
        let provider = MockCredentialsProvider {
            secrets: std::collections::HashMap::new(),
            failures: std::collections::HashMap::new(),
        };

        let cx = gpui::TestAppContext::single().to_async();
        let env = build_mcp_server_env("scenarios", &settings, &provider, None, None, &cx).await;

        // The scenarios server's allowlist is
        // `[HKASK_DATA_DIR, HKASK_SCENARIOS_DATA]`.
        // `HKASK_MXROUTE_SERVER` (curator email config) must not survive —
        // if it does, the config filter was bypassed (Path A regression).
        assert_eq!(
            env.get("HKASK_MXROUTE_SERVER"),
            None,
            "unallowlisted config var must not survive the composed path — \
             the prior Path A bug leaked the full mcp_env() map"
        );
        assert_eq!(
            env.get("HKASK_SMTP_USERNAME"),
            None,
            "curator email config must not leak into the scenarios server"
        );
    }

    // ── startup-default passphrase tests ───────────────────────────────
    //
    // The "allostery" default is a stated first-run requirement: DB-backed
    // servers must open their SQLCipher databases without login or manual
    // keychain configuration. These tests pin the wiring that guarantees it
    // at the canonical env path — the vars that have a default, and that
    // non-default credentials never silently gain one.

    /// Pin the requirement surface: exactly the two passphrase vars carry a
    /// startup default, and both are reachable through the credential loop
    /// for the servers that need them (so the provisioning tier actually
    /// fires — a var with a default but no credential URL would never reach
    /// `provision_default_passphrase`).
    #[test]
    fn default_passphrase_env_vars_pin_the_requirement() {
        assert_eq!(
            DEFAULT_PASSPHRASE_ENV_VARS,
            ["HKASK_DB_PASSPHRASE"],
            "the startup-default passphrase set is a stated requirement — \
             the ONE DB passphrase defaults to 'allostery' on first run and \
             every SQLCipher DB (curator, kanban, research, \
             experimentation) opens with it; changing this set is a requirements \
             change, not a refactor"
        );

        // Both vars must be in the credential URL list so the loop reaches
        // the provisioning tier for servers that allowlist them.
        let urls = crate::credential_urls_for_mcp();
        for env_var in DEFAULT_PASSPHRASE_ENV_VARS {
            assert!(
                urls.iter().any(|(var, _)| var == env_var),
                "{env_var} must have a credential URL — otherwise the credential \
                 loop never reaches provision_default_passphrase for it"
            );
        }

        // And at least one DB-backed server must allowlist the DB passphrase —
        // pins that the default actually reaches a consumer.
        let corpus_urls = filter_credentials_for_server("corpus", &urls);
        assert!(
            corpus_urls
                .iter()
                .any(|(var, _)| var == "HKASK_DB_PASSPHRASE"),
            "the corpus server must allowlist HKASK_DB_PASSPHRASE so the \
             startup default is provisioned for its launch env"
        );
    }

    /// Pin that non-default credentials never silently gain a default: a
    /// missing API key must return `None` (falling through to the
    /// missing-credential warning) rather than provisioning a value.
    /// "Not configured" must stay visible for credentials that have no
    /// required default.
    #[tokio::test]
    async fn provision_default_passphrase_returns_none_for_non_default_vars() {
        let cx = gpui::TestAppContext::single().to_async();
        assert!(
            provision_default_passphrase("HKASK_ABW_API_KEY", &cx)
                .await
                .is_none(),
            "HKASK_ABW_API_KEY has no startup default — a miss must fall through \
             to the missing-credential warning, not be silently provisioned"
        );
    }

    fn stdio_entry() -> settings_content::ContextServerSettingsContent {
        use settings_content::ContextServerCommand;
        settings_content::ContextServerSettingsContent::Stdio {
            enabled: true,
            remote: false,
            command: ContextServerCommand {
                path: "/bin/true".into(),
                args: Vec::new(),
                env: None,
                timeout: None,
            },
        }
    }

    /// The shadowing fix: a raw `context_servers` entry whose ID matches a
    /// kask built-in server is removed (and reported), because it silently
    /// overrides the managed registration that injects credentials and the
    /// inference socket. This is the exact pattern that shipped the
    /// keyless research server: a stale `env: {}` entry shadowed the
    /// keyed registration with no warning anywhere.
    #[test]
    fn remove_shadowing_context_server_entries_removes_builtins_and_keeps_others() {
        let mut content = settings_content::SettingsContent::default();
        content
            .project
            .context_servers
            .insert("research".into(), stdio_entry());
        content
            .project
            .context_servers
            .insert("some-other-server".into(), stdio_entry());

        let removed = remove_shadowing_context_server_entries(&mut content);

        assert_eq!(
            removed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            vec!["research"],
            "only the kask built-in entry is removed — the caller warns per ID"
        );
        assert!(
            !content.project.context_servers.contains_key("research"),
            "the shadowing entry must be gone so the managed registration wins"
        );
        assert!(
            content
                .project
                .context_servers
                .contains_key("some-other-server"),
            "non-kask entries are the user's own servers — never touched"
        );
    }

    /// No shadowing entries → nothing removed, nothing to warn about. Pins
    /// that the removal is a no-op on a clean configuration (the common
    /// case on every settings change).
    #[test]
    fn remove_shadowing_context_server_entries_is_noop_without_builtins() {
        let mut content = settings_content::SettingsContent::default();
        content
            .project
            .context_servers
            .insert("some-other-server".into(), stdio_entry());

        let removed = remove_shadowing_context_server_entries(&mut content);

        assert!(
            removed.is_empty(),
            "a clean configuration must not produce removals or warnings"
        );
        assert_eq!(content.project.context_servers.len(), 1);
    }

    /// Detection must work on content parsed from real settings-file text,
    /// not just on hand-constructed `SettingsContent`s — the live path is
    /// `raw_user_settings()` → `UserSettingsContent::parse_json` output. If
    /// the parse routes `context_servers` somewhere other than
    /// `content.project.context_servers`, the removal silently no-ops.
    #[test]
    fn shadowed_detection_works_on_parsed_file_content() {
        use settings_content::RootUserSettings;

        let text = r#"{
            "context_servers": {
                "research": {"command": "/bin/true", "args": [], "env": {}},
                "my-own-server": {"command": "/bin/true", "args": [], "env": {}}
            }
        }"#;
        let (parsed, _status) = settings_content::UserSettingsContent::parse_json(text);
        let user = parsed.expect("settings text must parse");
        let shadowed = shadowed_context_server_entry_ids(&user.content);
        assert_eq!(
            shadowed.iter().map(|id| id.as_ref()).collect::<Vec<_>>(),
            vec!["research"],
            "detection must find the shadowing entry in parsed file content"
        );
    }

    /// The removal write goes through `edits_for_update`, which diffs
    /// `serde_json::to_value(old)` against `to_value(new)`. If the
    /// serialization of `UserSettingsContent` drops `context_servers`, the
    /// diff is empty, `new_text == old_text`, and D32's no-op-write skip
    /// silently swallows the removal — the file keeps the shadowing entry
    /// forever with no error anywhere. This pins that the round-trip
    /// preserves the entry so the removal produces a real edit.
    #[test]
    fn user_settings_serialization_preserves_context_servers_for_edit_diff() {
        use settings_content::RootUserSettings;

        let text = r#"{
            "context_servers": {
                "research": {"command": "/bin/true", "args": [], "env": {}}
            }
        }"#;
        let (parsed, _status) = settings_content::UserSettingsContent::parse_json(text);
        let user = parsed.expect("settings text must parse");

        let old_value = serde_json::to_value(&user).expect("serialize");
        assert!(
            old_value
                .get("context_servers")
                .and_then(|v| v.get("research"))
                .is_some(),
            "serialization must preserve context_servers — otherwise the edit diff \
             is empty and the removal write is silently skipped"
        );

        let mut new_user = user;
        let removed = remove_shadowing_context_server_entries(&mut new_user.content);
        assert_eq!(removed.len(), 1);
        let new_value = serde_json::to_value(&new_user).expect("serialize");
        assert!(
            new_value.get("context_servers").is_none()
                || new_value["context_servers"].get("research").is_none(),
            "after removal the serialized entry must be gone — the diff drives the file edit"
        );
    }

    /// A pasted trailing newline must not survive keychain→env injection
    /// (the live Firecrawl failure), and the DB passphrase must never be
    /// trimmed — its exact bytes key the SQLCipher databases.
    #[test]
    fn credential_trim_strips_whitespace_but_never_the_passphrase() {
        let dirty_key = trim_credential_for_injection(
            "research",
            "HKASK_FIRECRAWL_API_KEY",
            "fc-0123456789abcdef0123456789abcdef\n".to_string(),
        );
        assert_eq!(dirty_key, "fc-0123456789abcdef0123456789abcdef");

        let clean_key = trim_credential_for_injection(
            "research",
            "HKASK_FIRECRAWL_API_KEY",
            "fc-0123456789abcdef0123456789abcdef".to_string(),
        );
        assert_eq!(clean_key, "fc-0123456789abcdef0123456789abcdef");

        let passphrase = trim_credential_for_injection(
            "curator",
            "HKASK_DB_PASSPHRASE",
            "pass phrase \n".to_string(),
        );
        assert_eq!(passphrase, "pass phrase \n");
    }
}
