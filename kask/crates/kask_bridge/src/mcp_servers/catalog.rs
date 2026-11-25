//! Canonical built-in MCP server catalog and ContextServerStore shadowing rules.

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
            // The consolidation floor (`kask.memory.confidence_floor`) —
            // emitted by `mcp_env()` on difference from the default and
            // read by the write tools' floor check (CU-31 b): a confidence
            // that lands at or below it is a deferred delete, and the
            // response says so. Without the allowlist entry the per-server
            // filter would silently drop it and floor-crossing writes would
            // stop warning.
            "HKASK_MEMORY_CONFIDENCE_FLOOR",
            // Embedding model — read by `embed_for_semantic_recall` and the
            // semantic-recall tools (`hkask_mcp_curator.rs`) via
            // `hkask_inference::model_constants::embedding_model()`. Without
            // this entry the per-server filter drops it and every semantic
            // recall degrades to exact-entity lookup with "no embedding
            // model configured" — the exact gap observed live 2026-09-04
            // (the allowlist must align with actual env-var reads).
            "HKASK_EMBEDDING_MODEL",
            // zed-kask: D100's named follow-up (operator-approved
            // 2026-11-06) — the private leg's LOCAL embedding model for
            // the query-embed routing: private-call queries embed through
            // this model only; absent/empty is the surfaced degrade.
            hkask_inference::model_constants::PRIVATE_EMBEDDING_MODEL_ENV,
            // CU-19 (b): the private leg's LOCAL classifier model — the
            // distillation pass routes a batch holding private-marked turns
            // through this model only; unset/empty skips such batches with a
            // surfaced warn. Without the allowlist entry the per-server
            // filter would silently drop it and private batches would
            // generate through the configured cloud classifier — the exact
            // crossing this exists to end.
            hkask_inference::model_constants::PRIVATE_CLASSIFIER_MODEL_ENV,
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
            // Optional — the Semantic Scholar provider is registered
            // keyless (free tier); the free key lifts the shared
            // ~1 req/s limit (read via ctx.credentials.get in
            // build_provider_pool).
            "HKASK_SEMANTIC_SCHOLAR_API_KEY",
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
            // zed-kask: D100 (operator-approved 2026-11-06) — the private
            // leg's LOCAL embedding model for the duplication tier on
            // private-provenance calls (the same admission the curator
            // server carries; the emission is unconditional, the
            // admission is the per-server allowlist). Without this entry
            // a private call's semantic-duplication tier would degrade
            // to the shingle floor instead of routing locally.
            "HKASK_PRIVATE_EMBEDDING_MODEL",
            // Deep-strategy rerank model — read by
            // `hkask_inference::model_constants::rerank_model()` at call
            // time. Without this, an operator override is silently stripped
            // under governed launch.
            "HKASK_RERANK_MODEL",
            // zed-kask: D100 §8.1 routed shape (operator ruling 2026-11-06)
            // — the private leg's LOCAL rerank model for private-provenance
            // deep-search rerank, read by
            // `hkask_inference::model_constants::private_rerank_model()` at
            // the construction seam. Without this entry a private call's
            // rerank stage would surface the heuristic skip instead of
            // routing locally.
            hkask_inference::model_constants::PRIVATE_RERANK_MODEL_ENV,
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
            "HKASK_DECISION_MODEL",
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
            // Cross-check STT fleet (transcript-reel Phase 1b) — read by the
            // media server's models module (CROSS_CHECK_STT_ENV) to list the
            // fleet in model_list; per-call overrides address each entry.
            "HKASK_MEDIA_CROSS_CHECK_STT_MODELS",
            "HKASK_MEDIA_VISION_MODEL",
            // Media role models (settings > kask > models) — the audio pass
            // resolves per-call → HKASK_MEDIA_SOUND_ANNOTATION_MODEL →
            // NotConfigured (audio-only models never route via the shared
            // pass-model env); the grounded tagging pass resolves per-call →
            // HKASK_MEDIA_TAGGING_MODEL → the vision default.
            "HKASK_MEDIA_SOUND_ANNOTATION_MODEL",
            "HKASK_MEDIA_TAGGING_MODEL",
            // Additional operator-authorized read-only gallery/media roots
            // (kask.media.allowed_gallery_roots, JSON array of paths; empty by
            // default — SEC-01 Option B).
            "HKASK_MEDIA_ALLOWED_GALLERY_ROOTS",
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
    BuiltinMcpServer {
        id: "nostr-relay",
        binary: "hkask-mcp-nostr-relay",
        description: "Nostr relay — a loopback archive of allowlisted authors' public events",
        // No credentials, by design: the relay verifies signatures and never
        // signs. It holds no Nostr key and no database passphrase (its LMDB
        // archive is unencrypted and holds public events only).
        credentials: &[],
        config_env: &[
            // Data dir — the archive lives at `mcp/nostr-relay/events` under
            // it (`default_store_path` in the server).
            "HKASK_DATA_DIR",
            // Operator policy, read once at startup by `Config::from_env`.
            // Source: `kask.nostr.relay` settings only (`emit_nostr_relay_env`
            // emits all three for every settings state, so the editor's own
            // environment never supplies them). No tool can change them.
            "HKASK_NOSTR_RELAY_PORT",
            "HKASK_NOSTR_RELAY_AUTHORS",
            "HKASK_NOSTR_RELAY_SYNC_RELAYS",
        ],
    },
    BuiltinMcpServer {
        id: "nostr-client",
        binary: "hkask-mcp-nostr-client",
        description: "Nostr client — reads the configured Nostr identity and events from the local relay and the seed relays; holds no key and cannot sign",
        // No credentials, by design, and pinned by
        // `nostr_client_registration_entry_is_pinned`: this server parses
        // untrusted relay text, and a Nostr key cannot be rotated or
        // recovered. The identity key never enters an MCP child; signing
        // belongs to the editor process.
        credentials: &[],
        config_env: &[
            // Data dir — the relay server announces its listener at
            // `mcp/nostr-relay/listener.json` under it, and this server
            // reads the announcement there. Without it an operator
            // HKASK_DATA_DIR override would send the two servers to
            // different directories and the client would never find the
            // local relay.
            "HKASK_DATA_DIR",
            // The public identity, seed relays and Blossom media servers,
            // read once at startup by `Config::from_env`. Public data; no tool
            // can change them.
            "HKASK_NOSTR_CLIENT_NPUB",
            "HKASK_NOSTR_CLIENT_SEED_RELAYS",
            "HKASK_NOSTR_CLIENT_BLOSSOM_SERVERS",
            "HKASK_NOSTR_BLOSSOM_PUBLIC_URL",
        ],
    },
    BuiltinMcpServer {
        id: "nostr-blossom",
        binary: "hkask-mcp-nostr-blossom",
        description: "Nostr Blossom sidecar — local content-addressed media server (BUD-01/BUD-02/BUD-06) with streaming storage and reference-counted delete",
        credentials: &[],
        config_env: &[
            "HKASK_DATA_DIR",
            "HKASK_NOSTR_BLOSSOM_PORT",
            "HKASK_NOSTR_BLOSSOM_MAX_BYTES",
            "HKASK_NOSTR_BLOSSOM_PUBLIC_URL",
            "HKASK_NOSTR_BLOSSOM_AUTHORS",
        ],
    },
    BuiltinMcpServer {
        id: "wallet",
        binary: "hkask-mcp-wallet",
        description: "Cashu + Lightning wallet (D110) — SQLCipher proof store, NUT-04/05/07/08/09/11/12/13/19, BOLT11 validation, and editor-gated spend approval",
        // No credentials via launch env, by design: SQLCipher DB passphrase
        // is retrieved over the authenticated HKASK_INFERENCE_SOCKET handshake
        // (`host/wallet_db_unlock`), while the BIP-39 seed and NWC URI never
        // leave the editor keychain.
        credentials: &[],
        config_env: &[
            "HKASK_DATA_DIR",
            "HKASK_WALLET_NETWORK",
            "HKASK_WALLET_MAINNET_ENABLED",
            "HKASK_WALLET_PER_TX_CAP_SATS",
            "HKASK_WALLET_DAILY_CAP_SATS",
            "HKASK_WALLET_MAX_FEE_SATS",
            "HKASK_WALLET_ALLOWED_MINTS",
            "HKASK_WALLET_DEFAULT_MINT",
            "HKASK_WALLET_NWC_LABEL",
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
