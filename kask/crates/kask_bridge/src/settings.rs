//! Kask settings — the `"kask"` section in zed-kask's settings.json (D9a).
//!
//! This struct holds kask-unique, **non-secret** config. API keys and other
//! secrets live in the OS keychain via `CredentialsProvider` (D9b), not here.
//!
//! Registered with zed's settings system so it appears in the settings schema
//! and can be edited in the settings UI.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::{RegisterSetting, Settings};
use settings_content::{
    KaskCompaniesSettingsContent, KaskCondenserSettingsContent, KaskCorpusSettingsContent,
    KaskCuratorEmailSettingsContent, KaskCuratorSettingsContent, KaskDataServiceSettingsContent,
    KaskGeneralSettingsContent, KaskMcpSettingsContent,
    KaskMediaSettingsContent, KaskMemorySettingsContent, KaskModelsSettingsContent,
    KaskPredictionMarketsSettingsContent, KaskResearchSettingsContent,
    KaskScenariosSettingsContent, KaskSettingsContent, KaskSwarmSettingsContent,
    KaskToolRouterSettingsContent, KaskTrainingSettingsContent,
};

use collections::HashMap;

/// Kask-specific settings (the `"kask"` section in settings.json).
///
/// Non-secret configuration for hKask features: MCP server load set,
/// data-service toggles, curator/regulation/memory/condenser settings.
/// API keys are stored in the keychain via `CredentialsProvider` (D9b).
///
/// `Default` is the single source of truth for every subsection's defaults.
/// `From<KaskSettingsContent>` delegates to each subsection's `Default` via
/// `unwrap_or(default.field)`. Do not add `#[serde(default = ...)]` attributes
/// here — `KaskSettings` is never deserialized directly (the settings system
/// deserializes `SettingsContent` and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default, RegisterSetting)]
pub struct KaskSettings {
    /// Kask data directory — the root for all kask databases, agent state,
    /// and file-based stores. When empty, `mcp_env()` resolves a default
    /// via `hkask_types::agent_paths::resolve_data_dir()` (HKASK_DATA_DIR
    /// env var → XDG_DATA_HOME/hkask → ~/.local/share/hkask) and injects
    /// it as `HKASK_DATA_DIR` for every MCP server. This ensures servers
    /// always receive a consistent data directory without requiring the
    /// operator to set environment variables manually.
    pub data_dir: String,

    /// Kask-wide general configuration: global inference concurrency + batching.
    pub general: KaskGeneralSettings,

    /// MCP server configuration — which of the 12 built-in servers to load.
    pub mcp: KaskMcpSettings,

    /// Data service toggles (non-secret — API keys are in the keychain).
    pub data_services: KaskDataServiceSettings,

    /// Curator configuration.
    pub curator: KaskCuratorSettings,

    /// Memory consolidation and recall configuration.
    pub memory: KaskMemorySettings,

    /// Condenser configuration for context management in inference threads.
    pub condenser: KaskCondenserSettings,

    /// Research MCP server configuration.
    pub research: KaskResearchSettings,

    /// Companies MCP server configuration.
    pub companies: KaskCompaniesSettings,

    /// Corpus MCP server configuration.
    pub corpus: KaskCorpusSettings,

    /// Media MCP server configuration.
    pub media: KaskMediaSettings,

    /// Scenarios MCP server configuration.
    pub scenarios: KaskScenariosSettings,
    /// Prediction-markets data-service configuration.
    pub prediction_markets: KaskPredictionMarketsSettings,

    /// Swarm (Agent Bestiary World) MCP server configuration.
    pub swarm: KaskSwarmSettings,

    /// Training MCP server configuration.
    pub training: KaskTrainingSettings,

    /// Kask-wide model configuration: default, embedding, and classifier models.
    pub models: KaskModelsSettings,

    /// Tool-router thresholds for narrowing the MCP tool set on complex or
    /// tool-directed requests. Defaults match the historical
    /// `LazyToolRouter::new()` hardcoded values (`0.30` / `40`).
    pub tool_router: KaskToolRouterSettings,

    /// Inference provider toggles (non-secret — API keys are in the keychain).
}

/// Kask-wide general configuration: global inference concurrency + batching.
/// The limiter is process-global (one `Arc` shared across all consumers —
/// corpus OCR, MCP tool calls). See `kask_bridge::concurrency`
/// for the wiring and the limiter impl.
///
/// `Default` is the single source of truth for defaults — `From<Content>`
/// reads from it via `unwrap_or(default.field)`. Do not add
/// `#[serde(default = ...)]` attributes here; `KaskSettings` is never
/// deserialized directly (the settings system deserializes `SettingsContent`
/// and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskGeneralSettings {
    /// Maximum concurrent cloud inference provider calls across the whole
    /// process. Default 96. Providers throttle at different levels;
    /// OpenRouter scales to this ceiling.
    pub max_concurrency: u32,

    /// Concurrency step — the ramp origin and increment. The limiter starts
    /// at `concurrency_step` permits and adds `concurrency_step` per ramp
    /// tick on success until `max_concurrency` or a throttle. Default 4.
    pub concurrency_step: u32,
}

impl Default for KaskGeneralSettings {
    fn default() -> Self {
        Self {
            max_concurrency: 96,
            concurrency_step: 4,
        }
    }
}

/// MCP server load configuration.
///
/// `Default` is the single source of truth for defaults — `From<Content>` reads
/// from it via `unwrap_or(default.field)`. Do not add `#[serde(default = ...)]`
/// attributes here; `KaskSettings` is never deserialized directly (the settings
/// system deserializes `SettingsContent` and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskMcpSettings {
    /// Whether to load the default MCP server set (12 servers).
    /// Set to `false` to disable all kask MCP servers.
    pub load_default: bool,

    /// Per-server overrides (e.g. `"curator": false` to unload the curator MCP).
    pub overrides: HashMap<String, bool>,
}

impl Default for KaskMcpSettings {
    fn default() -> Self {
        Self {
            load_default: true,
            overrides: HashMap::default(),
        }
    }
}

/// Data service toggles. API keys are in the keychain, not here.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskDataServiceSettings {
    /// Enable EODHD (historical price data).
    pub eodhd_enabled: bool,

    /// Enable FMP (Financial Modeling Prep).
    pub fmp_enabled: bool,

    /// Enable Exa (research search).
    pub exa_enabled: bool,

    /// Enable Tavily (research search).
    pub tavily_enabled: bool,

    /// Enable Brave Search.
    pub brave_enabled: bool,

    /// Enable RunPod (GPU cloud for training).
    pub runpod_enabled: bool,

    /// Enable Nebius (GPU cloud for training).
    pub nebius_enabled: bool,
}

/// Inference provider toggles. API keys are in the keychain, not here.
///
/// When a provider is enabled, the composition root mirrors its API key to
/// kask MCP servers (see `credential_urls_for_mcp`).
///
/// `Default` returns all-false (pure, no side effects). The env-var-based
/// auto-enable logic lives in `From<KaskInferenceProvidersSettingsContent>`,
/// which is the only production path. This keeps `Default` deterministic for
/// tests and `KaskSettings::default()`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskInferenceProvidersSettings {
    /// Enable OpenRouter (unified API for 200+ models).
    pub openrouter_enabled: bool,

/// Curator configuration.
///
/// `Default` is the single source of truth for defaults — `From<Content>` reads
/// from it via `unwrap_or(default.field)`. Do not add `#[serde(default = ...)]`
/// attributes here; `KaskSettings` is never deserialized directly (the settings
/// system deserializes `SettingsContent` and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskCuratorSettings {
    /// Whether the Curator agent is always-on (runs regulation loops in background).
    pub always_on: bool,

    /// Algedonic signal threshold (0.0–1.0).
    pub algedonic_threshold: f64,

    /// Curator email configuration (outbound algedonic alerts via MXroute).
    /// When `None` or unconfigured, the alert email sink falls back to the
    /// log-only sink (`LogAlertEmailSink` in `crates/zed/src/main.rs`).
    pub email: KaskCuratorEmailSettings,
}

impl Default for KaskCuratorSettings {
    fn default() -> Self {
        Self {
            always_on: true,
            algedonic_threshold: 0.8,
            email: KaskCuratorEmailSettings::default(),
        }
    }
}

/// Curator email configuration (non-secret fields).
///
/// The SMTP password is stored in the OS keychain under
/// `kask://credentials/hkask_smtp_password`, not here. The composition root
/// reads it from the keychain and injects it as `HKASK_SMTP_PASSWORD` into
/// MCP server child processes.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskCuratorEmailSettings {
    /// MXroute server hostname (e.g. "tuesday.mxrouting.net").
    pub mxroute_server: String,

    /// Full email address used for SMTP auth and the `From` header.
    pub smtp_username: String,

    /// From address (defaults to `smtp_username` when empty).
    pub curator_email: String,

    /// Alert recipient (defaults to `smtp_username` when empty).
    pub alert_email: String,

    /// Comma-separated list of senders authorized to reply with curator
    /// commands (P12 allowlist). Empty means inbound replies are rejected.
    pub authorized_emails: Vec<String>,
}

/// Memory consolidation and recall configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskMemorySettings {
    /// Consolidation cadence in seconds (0 = disabled).
    pub consolidation_cadence_secs: u64,

    /// Confidence floor for memory retention (0.0–1.0).
    pub confidence_floor: f64,

    /// Maximum number of memory snippets to retrieve for context injection.
    pub recall_limit: u32,

    /// Minimum confidence for a memory to be injected into context (0.0–1.0).
    pub recall_min_confidence: f64,

    /// Whether to automatically inject recalled memories into prompts.
    pub auto_inject: bool,

    /// Number of recent turns from the invoking thread to include as
    /// short-term context for skill execution. 0 disables short-term
    /// injection (skill execution runs isolated, as before). Default: 6.
    pub cascade_short_term_turns: u32,

    /// Saliency floor for cascade memory recall. A memory chunk is injected
    /// only if `relevance_score * confidence >= saliency_floor`. Default:
    /// 0.3 (same as `recall_min_confidence`).
    pub cascade_memory_saliency_floor: f64,

    /// Maximum memory chunks to inject into skill execution, after merging
    /// across all participant stores (user, curator, swarm). Default: 5.
    pub cascade_memory_max_chunks: u32,

    /// Maximum tokens per turn for cascade short-term context. Each turn
    /// exceeding this budget is condensed via the local algorithmic
    /// condenser (TF-IDF word-rank for conversation, flashrank for other
    /// content), then truncated to the token cap if still over. 0 disables
    /// condensation (raw turn text is passed verbatim). Default: 512.
    pub cascade_turn_token_cap: u32,
}

impl Default for KaskMemorySettings {
    fn default() -> Self {
        Self {
            consolidation_cadence_secs: 300,
            confidence_floor: 0.3,
            recall_limit: 5,
            recall_min_confidence: 0.3,
            auto_inject: true,
            cascade_short_term_turns: 6,
            cascade_memory_saliency_floor: 0.3,
            cascade_memory_max_chunks: 5,
            cascade_turn_token_cap: 512,
        }
    }
}

/// Condenser configuration for context management in inference threads.
///
/// Controls how tool results are compressed before entering the message
/// history, and what compression profile to use.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskCondenserSettings {
    /// Compression profile: "heavy", "normal", "soft", or "light".
    /// - Heavy: 10% retention, 30 max lines — aggressive compression
    /// - Normal: 20% retention, 80 max lines — balanced
    /// - Soft: 60% retention, 200 max lines — light touch
    /// - Light: 95% retention, no max — near-passthrough
    pub profile: String,

    /// Whether to automatically compress tool results before they enter
    /// the message history. When false, tool results are stored verbatim.
    pub auto_compress_tool_results: bool,

    /// Persona keywords for saliency scoring (comma-separated in settings.json).
    /// Used by the condenser's word_rank algorithm to prioritize lines
    /// relevant to the user's domain.
    pub persona_keywords: Vec<String>,

    /// Saliency window multiplier for thread summarization.
    /// Controls the max_tokens budget: saliency_window * 100, clamped [150, 2000].
    pub saliency_window: u32,
}

impl Default for KaskCondenserSettings {
    fn default() -> Self {
        Self {
            profile: "normal".to_string(),
            auto_compress_tool_results: false,
            persona_keywords: Vec::default(),
            saliency_window: 5,
        }
    }
}

/// Research MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskResearchSettings {
    /// RSS database path for persistent feed storage. When empty, the server
    /// resolves a default path under the hKask data directory.
    pub rss_db: String,
}

/// Companies MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskCompaniesSettings {
    /// Chronic staleness threshold in days for superforecasting learning state.
    pub chronic_staleness_days: u32,

    /// Fermi decomposition defaults as JSON (growth + margin question arrays).
    /// When empty, uses hardcoded defaults.
    pub fermi_defaults: String,

    /// Directory for portfolio transaction files (CSV/JSON). The portfolio
    /// dashboard auto-loads any new files from this directory. When empty,
    /// defaults to `<kask_data_dir>/transactions/`.
    pub transactions_dir: String,
}

/// Corpus MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskCorpusSettings {
    /// Embedding dimensionality (must match the embedding model's output).
    pub embedding_dim: u32,

    /// Embedding model. Defaults to the kask router constant
    /// (`hkask_inference::model_constants::DEFAULT_EMBEDDING_MODEL`).
    pub embedding_model: String,

    /// OCR concurrency — number of pages sent to the vision model in parallel.
    pub ocr_concurrency: u32,

    /// OCR simple threshold (0.0–1.0). Pages below this are processed simply.
    pub ocr_simple_max: f64,

    /// OCR moderate threshold (0.0–1.0). Pages above simple but below this
    /// are processed with moderate pipeline.
    pub ocr_moderate_max: f64,

    /// OCR moderate sample rate (0.0–1.0). Fraction of moderate pages sampled.
    pub ocr_sample_rate: f64,

    /// Whether OCR tuneable mode is enabled.
    pub ocr_tuneable: bool,

    /// Template root directory for Jinja2 templates.
    pub template_root: String,
}

impl Default for KaskCorpusSettings {
    fn default() -> Self {
        Self {
            embedding_dim: 1024,
            embedding_model: default_embedding_model(),
            ocr_concurrency: 4,
            ocr_simple_max: 0.05,
            ocr_moderate_max: 0.15,
            ocr_sample_rate: 0.10,
            ocr_tuneable: true,
            template_root: "kask/registry".to_string(),
        }
    }
}

fn default_embedding_model() -> String {
    hkask_inference::model_constants::DEFAULT_EMBEDDING_MODEL.to_string()
}

/// Media MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskMediaSettings {
    /// TTS model override (provider-prefixed, e.g. "ollama/kokoro").
    pub tts_model: String,

    /// STT model override (provider-prefixed, e.g. "ollama/whisper-large-v3").
    pub stt_model: String,

    /// Vision model override (e.g., "OpenRouter/qwen/qwen3-vl-235b-a22b-instruct").
    pub vision_model: String,

    /// Image generation model override (provider-prefixed).
    pub image_gen_model: String,
}

/// Prediction-markets MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskPredictionMarketsSettings {
    /// Data directory for the calibration journal. When empty, in-memory.
    pub data_dir: String,
    /// Cache TTL in seconds for market-data responses (0 = server default).
    pub cache_ttl_secs: u64,
    /// Base-event registry: "domain:series,..." pairs for CMP construction.
    pub base_events: String,
}

/// Scenarios MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskScenariosSettings {
    /// Data directory for scenario persistence. When empty, uses in-memory.
    pub data_dir: String,
}

/// Swarm (Agent Bestiary World) MCP server configuration.
///
/// The API key is a secret — it lives in the keychain under
/// `kask://credentials/hkask_abw_api_key`, injected as `HKASK_ABW_API_KEY`.
/// Only non-secret config lives here.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskSwarmSettings {
    /// Which backend to route to (v2 §15). Default `Abw` (v1 behavior).
    /// `Local` routes to zed-kask's local substrate crates.
    pub mode: SwarmModeConfig,

    /// ABW API base URL override. When empty, uses the default
    /// (`https://agent-bestiary.world`).
    pub api_url: String,

    /// Per-dispatch credit ceiling for spend tools (the S3 budget gate).
    /// Dispatches estimated above this are refused before any credit is spent.
    pub max_credits_per_dispatch: u32,

    /// Whether Xaman Ek curator calls may be initiated without a per-call
    /// consent token (S5 policy). Default `false` — sending task content to
    /// a third-party curator requires explicit opt-in per the plan's §3.7.
    /// When `false`, `swarm_xaman` requires a `consent_token` (action "curate").
    /// When `true`, the operator has globally opted in and the token is optional.
    pub curator_consent_default: bool,

    /// Directory containing local agent cards (`<id>/agent_card.json`),
    /// read by `LocalAgentRegistry` in `Local` mode. When empty, uses the
    /// default `agents/local/curated`.
    pub local_agents_dir: String,

    /// Directory containing local swarms (`<id>/swarm.json`), read/written by
    /// `LocalSwarmRegistry` - the local replica of an ABW workspace roster.
    /// When empty, uses the default `agents/local/swarms`.
    pub local_swarms_dir: String,

    /// Directory containing the zed-kask skill corpus (`.agents/skills/`),
    /// read by `AgentExecutor::build_skill_catalog` to inject skill
    /// descriptions into the local agent's system prompt (Slice 6 — local
    /// agent skill-awareness). When empty, skill-awareness is disabled (the
    /// agent runs skill-blind). Set from the project's `.agents/skills/`
    /// directory.
    pub skills_dir: String,

    /// Default model id for newly created ABW agents when the caller omits
    /// `model` (KA-05). When empty, uses the server default
    /// (`claude-haiku-4-5-20251001`).
    pub default_agent_model: String,

    /// Whether to start the A2A HTTP gateway (loopback JSON-RPC server that
    /// exposes local agents to external A2A clients). Default `false`
    /// (opt-in — it opens a loopback port).
    pub a2a_http_enabled: bool,

    /// SQLCipher passphrase for the local swarm semantic-memory store. Must
    /// be >=8 chars. When empty, uses the pre-release default `"allostery"`.
    pub memory_passphrase: String,

    /// On-disk path for the local swarm semantic-memory DB. When empty, uses
    /// the default `<hkask data dir>/swarm_memory.db`.
    pub memory_db_path: String,

    /// Embedding vector dimension for the semantic-memory embedding store.
    /// Default 1024.
    pub embedding_dim: usize,
}

/// Mirror of `SwarmMode` in the server crate, kept separate to avoid a
/// circular dependency (the bridge crate does not depend on the server
/// crate). The two enums MUST stay in sync — see the `Default` impl comment
/// on `KaskSwarmSettings`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SwarmModeConfig {
    /// Route to Agent Bestiary World (v1 behavior).
    #[default]
    Abw,
    /// Route to local substrate crates (v2, §15).
    Local,
}

impl std::fmt::Display for SwarmModeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Abw => write!(f, "abw"),
            Self::Local => write!(f, "local"),
        }
    }
}

impl From<settings_content::SwarmModeContent> for SwarmModeConfig {
    fn from(c: settings_content::SwarmModeContent) -> Self {
        match c {
            settings_content::SwarmModeContent::Abw => Self::Abw,
            settings_content::SwarmModeContent::Local => Self::Local,
        }
    }
}

impl Default for KaskSwarmSettings {
    fn default() -> Self {
        // These defaults MUST stay in sync with `SwarmConfig::default()` in
        // `kask/mcp-servers/hkask-mcp-swarm/src/config.rs`. The bridge emits
        // env vars (`HKASK_ABW_*` / `HKASK_SWARM_*`) from this `Default` via
        // `mcp_env()`; the server reads them in `SwarmConfig::from_env`. The
        // two `Default` impls are deliberately separate (the server crate
        // does not depend on the bridge crate) to avoid a circular dependency
        // — the duplication is the seam between them. If you change a default
        // here, change it there too, and update the
        // `swarm_settings_default_emits_no_env` test below.
        Self {
            mode: SwarmModeConfig::default(),
            api_url: String::new(),
            max_credits_per_dispatch: 50,
            curator_consent_default: false,
            local_agents_dir: String::new(),
            local_swarms_dir: String::new(),
            skills_dir: String::new(),
            default_agent_model: String::new(),
            a2a_http_enabled: false,
            memory_passphrase: String::new(),
            memory_db_path: String::new(),
            embedding_dim: 1024,
        }
    }
}

/// Training MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskTrainingSettings {
    /// Training host override: "nebius" or "runpod".
    /// When empty, auto-detects from available API keys.
    pub host: String,

    /// Cache directory for dataset pipeline. When empty, uses the
    /// agent adapters directory.
    pub cache_dir: String,
}

/// Kask-wide model configuration.
///
/// Provider-prefixed model names that override the kask built-in defaults.
/// When a field is empty, kask falls back to its default model selection
/// (typically the zed `agent.default_model`).
///
/// **Two-layer default design (intentional):** `default_model`, `embedding_model`,
/// and `classifier_model` default to empty strings in `Default`. When empty, the
/// `effective_*` methods fall back to the `DEFAULT_*_MODEL` constants, which are
/// themselves `const` references to the single source of truth in
/// `hkask_inference::model_constants`. This lets users override individual models
/// in settings.json while keeping the kask built-in defaults as the fallback.
/// Do not duplicate the model ids anywhere else — `model_constants` is canonical.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskModelsSettings {
    /// Default inference model (provider-prefixed, e.g. `"openrouter/z-ai/glm-5.2"`).
    /// When set, overrides the kask default for Curator, skill execution, and
    /// kask panel inference.
    pub default_model: String,

    /// Embedding model for corpus indexing and memory semantic recall
    /// (provider-prefixed). When empty, falls back to the corpus MCP server's
    /// `embedding_model` setting, then to the kask default.
    pub embedding_model: String,

    /// Classifier model for guard/regulation classification tasks
    /// (provider-prefixed). When empty, falls back to the kask default.
    pub classifier_model: String,

    /// OCR vision model for scanned document OCR (provider-prefixed).
    /// When empty, the corpus server falls back to the kask default
    /// (env `HKASK_OCR_MODEL` → `HkaskSettings::ocr_model` →
    /// `DEFAULT_OCR_MODEL` — resolved in hkask-mcp-corpus, not here).
    pub ocr_model: String,
}

impl KaskModelsSettings {
    /// The kask default inference model.
    ///
    /// Single source of truth: `hkask_inference::model_constants::DEFAULT_FALLBACK_MODEL`.
    /// Re-exported here so callers within kask_bridge don't need a direct dep on
    /// hkask-inference for this constant, but the value is not duplicated — it
    /// is a `const` reference to the canonical definition.
    pub const DEFAULT_INFERENCE_MODEL: &'static str =
        hkask_inference::model_constants::DEFAULT_FALLBACK_MODEL;

    /// Resolve the effective default inference model, falling back to the
    /// kask default when the setting is empty.
    #[must_use]
    pub fn effective_default_model(&self) -> &str {
        if self.default_model.trim().is_empty() {
            Self::DEFAULT_INFERENCE_MODEL
        } else {
            &self.default_model
        }
    }
}

/// Tool-router thresholds for narrowing the MCP tool set on complex or
/// tool-directed requests.
///
/// `Default` is the single source of truth — `From<Content>` reads from it
/// via `unwrap_or(default.field)`.
///
/// `complex_word_threshold` was lowered 40 -> 9 -> 6 (2026-08-12). At 40 the
/// router almost never activated: it is fail-open, so a sub-threshold message
/// retains **all** MCP tool schemas (~15,000 tokens across 331 tools), and few
/// real requests reach 40 words.
///
/// 6 rather than 9 because the 200-case eval set showed 82 of 215 graded
/// requests never activating at all -- ordinary asks like "list all the kanban
/// boards i own" sit at 6-8 words. Dropping to 6 cut mean retained tools from 170
/// to 93 of 252 with recall unchanged at 1.000 on both the tuned and held-out
/// splits. Going below 6 changed nothing further (the 4-word setting is identical
/// to 6), so 6 is the floor of the useful range rather than an aggressive value.
///
/// Short vague messages stay safe because the confidence gate, not the word
/// count, is what protects them: all 11 fail-open cases peak below 0.50 and fail
/// open regardless of length.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskToolRouterSettings {
    /// Score threshold for tool inclusion (0.0–1.0). Messages scoring above
    /// this threshold get the narrowed tool set.
    pub threshold: f64,

    /// Minimum word count for a message to be considered "complex" enough to
    /// trigger routing.
    pub complex_word_threshold: usize,
}

impl Default for KaskToolRouterSettings {
    fn default() -> Self {
        Self {
            threshold: 0.30,
            complex_word_threshold: 6,
        }
    }
}

impl From<KaskToolRouterSettingsContent> for KaskToolRouterSettings {
    fn from(c: KaskToolRouterSettingsContent) -> Self {
        let default = Self::default();
        Self {
            threshold: c.threshold.unwrap_or(default.threshold),
            complex_word_threshold: c
                .complex_word_threshold
                .unwrap_or(default.complex_word_threshold),
        }
    }
}

impl Settings for KaskSettings {
    fn from_settings(s: &settings_content::SettingsContent) -> Self {
        s.kask.clone().map(|c| c.into()).unwrap_or_default()
    }
}

impl KaskSettings {
    /// Effective embedding model, resolving the documented precedence:
    /// `models.embedding_model` (if non-empty) → `corpus.embedding_model`
    /// (if non-default) → the `DEFAULT_EMBEDDING_MODEL` constant.
    ///
    /// This is the single source of truth for the `HKASK_EMBEDDING_MODEL`
    /// env emission. Previously two separate `env.insert` blocks in
    /// `mcp_env()` targeted the same env var, with precedence enforced only
    /// by statement order — a silent reorder would flip which setting won.
    /// The precedence is now explicit and pinned by
    /// `mcp_env_models_embedding_model_overrides_corpus`.
    #[must_use]
    pub fn effective_embedding_model(&self) -> String {
        if !self.models.embedding_model.is_empty() {
            self.models.embedding_model.clone()
        } else if self.corpus.embedding_model != default_embedding_model() {
            self.corpus.embedding_model.clone()
        } else {
            default_embedding_model()
        }
    }

    /// Build the environment variable map for MCP server child processes.
    ///
    /// Translates all kask settings into the env vars that MCP servers read
    /// at startup. Only non-empty/non-default values are included — MCP
    /// servers have their own fallback defaults for unset env vars.
    ///
    /// This is the **config** half of the env map. The full env for a server
    /// child process — config + keychain credentials + the inference socket —
    /// is assembled by [`build_mcp_server_env`](crate::build_mcp_server_env)
    /// in `mcp_servers`, the single canonical path. It composes this crate's
    /// `mcp_env()` with the per-server credential/config allowlists. There
    /// is no other env-construction path; do not re-introduce one.
    pub fn mcp_env(&self) -> std::collections::HashMap<String, String> {
        let mut env = std::collections::HashMap::new();

        // Always inject HKASK_DATA_DIR so every MCP server can resolve
        // paths consistently. Priority: settings field → env var → resolved
        // platform default. Without this, servers that fall back to
        // `resolve_under_data_dir` may get an empty HKASK_DATA_DIR and resolve
        // to different paths depending on the launch context.
        let data_dir = if !self.data_dir.is_empty() {
            self.data_dir.clone()
        } else {
            std::env::var("HKASK_DATA_DIR").unwrap_or_else(|_| {
                hkask_types::agent_paths::resolve_data_dir()
                    .to_string_lossy()
                    .to_string()
            })
        };
        emit_data_dir_env(&data_dir, &mut env);
        emit_curator_webid_env(&mut env);
        emit_mcp_server_ids_env(&mut env);
        emit_condenser_env(&self.condenser, &mut env);
        emit_research_env(&self.research, &mut env);
        emit_companies_env(&self.companies, &mut env);
        let effective_embedding = self.effective_embedding_model();
        emit_corpus_embedding_env(&self.corpus, &effective_embedding, &mut env);
        emit_corpus_ocr_env(&self.corpus, &mut env);
        emit_corpus_template_root_env(&self.corpus, &data_dir, &mut env);
        emit_media_env(&self.media, &mut env);
        emit_scenarios_env(&self.scenarios, &mut env);
        emit_prediction_markets_env(&self.prediction_markets, &mut env);
        emit_swarm_env(&self.swarm, &mut env);
        emit_training_env(&self.training, &mut env);
        emit_models_env(&self.models, &mut env);
        emit_curator_email_env(&self.curator.email, &mut env);
        env
    }
}

// ── mcp_env translator functions ──────────────────────────────────────────
//
// Defaults are read from each subsection's `Default` impl so there's a
// single source of truth — changing `Default` automatically updates the
// comparison here. Do not inline magic numbers; they drift from
// `Default` (the same drift class that silently disabled all 10 kask
// MCP servers when `KaskMcpSettings::default()` disagreed with the
// serde default).

pub(crate) fn emit_data_dir_env(
    data_dir: &str,
    env: &mut std::collections::HashMap<String, String>,
) {
    env.insert("HKASK_DATA_DIR".to_string(), data_dir.to_string());
}

pub(crate) fn emit_curator_webid_env(env: &mut std::collections::HashMap<String, String>) {
    // Map the curator's WebID (stashed in `HKASK_CURATOR_WEBID` by the
    // deferred task) to `HKASK_WEBID` so the curator MCP server picks it
    // up as its identity. The `config_env` allowlist filters this to the
    // curator server only — other servers don't receive `HKASK_WEBID`
    // from this mapping and fall through to their own identity resolution.
    if let Ok(curator_webid) = std::env::var("HKASK_CURATOR_WEBID") {
        env.insert("HKASK_WEBID".to_string(), curator_webid);
    }
}

pub(crate) fn emit_mcp_server_ids_env(env: &mut std::collections::HashMap<String, String>) {
    // Pass the governed server id set to the swarm server so it can
    // filter cloned cards' declared `mcp_tools` to these servers (the
    // provenance boundary for third-party ABW cards). Only the swarm
    // server's `config_env` allowlist includes this var, so no other
    // child receives it.
    env.insert(
        "HKASK_MCP_SERVER_IDS".to_string(),
        crate::builtin_mcp_server_ids().join(","),
    );
}

pub(crate) fn emit_condenser_env(
    condenser: &KaskCondenserSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    let condenser_default = KaskCondenserSettings::default();
    if !condenser.persona_keywords.is_empty() {
        env.insert(
            "HKASK_CONDENSER_PERSONA_KEYWORDS".to_string(),
            condenser.persona_keywords.join(","),
        );
    }
    if condenser.saliency_window != condenser_default.saliency_window {
        env.insert(
            "HKASK_CONDENSE_SALIENCY_WINDOW".to_string(),
            condenser.saliency_window.to_string(),
        );
    }
}

pub(crate) fn emit_research_env(
    research: &KaskResearchSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !research.rss_db.is_empty() {
        env.insert("HKASK_RSS_DB".to_string(), research.rss_db.clone());
    }
}

pub(crate) fn emit_companies_env(
    companies: &KaskCompaniesSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if companies.chronic_staleness_days > 0 {
        env.insert(
            "HKASK_CHRONIC_STALENESS_DAYS".to_string(),
            companies.chronic_staleness_days.to_string(),
        );
    }
    if !companies.fermi_defaults.is_empty() {
        env.insert(
            "HKASK_FERMI_DEFAULTS".to_string(),
            companies.fermi_defaults.clone(),
        );
    }
    // D28 — Standardized Artifact Storage. Emit HKASK_TRANSACTIONS_DIR
    // so the portfolio server can auto-load transaction files. Default
    // is `mcp/portfolio/transactions/` under the kask data root.
    let transactions_dir = if !companies.transactions_dir.is_empty() {
        companies.transactions_dir.clone()
    } else {
        hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
            "mcp/portfolio/transactions",
        ))
        .to_string_lossy()
        .to_string()
    };
    env.insert("HKASK_TRANSACTIONS_DIR".to_string(), transactions_dir);
}

pub(crate) fn emit_corpus_embedding_env(
    corpus: &KaskCorpusSettings,
    effective_embedding_model: &str,
    env: &mut std::collections::HashMap<String, String>,
) {
    let corpus_default = KaskCorpusSettings::default();
    if corpus.embedding_dim != corpus_default.embedding_dim {
        env.insert(
            "HKASK_EMBEDDING_DIM".to_string(),
            corpus.embedding_dim.to_string(),
        );
    }
    // ── Embedding model ──
    // Precedence: models.embedding_model → corpus.embedding_model →
    // default. Resolved once by `effective_embedding_model` so the
    // emission cannot drift from the documented precedence. See its
    // doc comment and the `mcp_env_models_embedding_model_overrides_corpus` test.
    if effective_embedding_model != corpus_default.embedding_model {
        env.insert(
            "HKASK_EMBEDDING_MODEL".to_string(),
            effective_embedding_model.to_string(),
        );
    }
}

pub(crate) fn emit_corpus_ocr_env(
    corpus: &KaskCorpusSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    let corpus_default = KaskCorpusSettings::default();
    if corpus.ocr_concurrency != corpus_default.ocr_concurrency {
        env.insert(
            "HKASK_OCR_CONCURRENCY".to_string(),
            corpus.ocr_concurrency.to_string(),
        );
    }
    if (corpus.ocr_simple_max - corpus_default.ocr_simple_max).abs() > f64::EPSILON {
        env.insert(
            "HKASK_OCR_SIMPLE_MAX".to_string(),
            corpus.ocr_simple_max.to_string(),
        );
    }
    if (corpus.ocr_moderate_max - corpus_default.ocr_moderate_max).abs() > f64::EPSILON {
        env.insert(
            "HKASK_OCR_MODERATE_MAX".to_string(),
            corpus.ocr_moderate_max.to_string(),
        );
    }
    if (corpus.ocr_sample_rate - corpus_default.ocr_sample_rate).abs() > f64::EPSILON {
        env.insert(
            "HKASK_OCR_SAMPLE_RATE".to_string(),
            corpus.ocr_sample_rate.to_string(),
        );
    }
    if corpus.ocr_tuneable != corpus_default.ocr_tuneable {
        env.insert("HKASK_OCR_TUNEABLE".to_string(), "false".to_string());
    }
}

pub(crate) fn emit_corpus_template_root_env(
    corpus: &KaskCorpusSettings,
    data_dir: &str,
    env: &mut std::collections::HashMap<String, String>,
) {
    let corpus_default = KaskCorpusSettings::default();
    // Always emit HKASK_TEMPLATE_ROOT so MCP servers (corpus, training)
    // find templates in production where the CWD-relative default does not
    // exist. When the operator hasn't overridden the default, resolve to
    // `{data_dir}/skills/registry/` — the path where `seed_templates` writes.
    // When overridden, use the operator's value.
    let template_root = if corpus.template_root != corpus_default.template_root {
        corpus.template_root.clone()
    } else {
        std::path::Path::new(data_dir)
            .join("skills")
            .join("registry")
            .to_string_lossy()
            .to_string()
    };
    env.insert("HKASK_TEMPLATE_ROOT".to_string(), template_root);
}

pub(crate) fn emit_media_env(
    media: &KaskMediaSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !media.tts_model.is_empty() {
        env.insert("HKASK_MEDIA_TTS_MODEL".to_string(), media.tts_model.clone());
    }
    if !media.stt_model.is_empty() {
        env.insert("HKASK_MEDIA_STT_MODEL".to_string(), media.stt_model.clone());
    }
    if !media.vision_model.is_empty() {
        env.insert(
            "HKASK_MEDIA_VISION_MODEL".to_string(),
            media.vision_model.clone(),
        );
    }
    if !media.image_gen_model.is_empty() {
        env.insert(
            "HKASK_MEDIA_IMAGE_GEN_MODEL".to_string(),
            media.image_gen_model.clone(),
        );
    }
}

pub(crate) fn emit_scenarios_env(
    scenarios: &KaskScenariosSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !scenarios.data_dir.is_empty() {
        env.insert(
            "HKASK_SCENARIOS_DATA".to_string(),
            scenarios.data_dir.clone(),
        );
    }
}

pub(crate) fn emit_prediction_markets_env(
    prediction_markets: &KaskPredictionMarketsSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !prediction_markets.data_dir.is_empty() {
        env.insert(
            "HKASK_PREDICTION_MARKETS_DATA".to_string(),
            prediction_markets.data_dir.clone(),
        );
    }
    if prediction_markets.cache_ttl_secs > 0 {
        env.insert(
            "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS".to_string(),
            prediction_markets.cache_ttl_secs.to_string(),
        );
    }
    if !prediction_markets.base_events.is_empty() {
        env.insert(
            "HKASK_PREDICTION_MARKETS_BASE_EVENTS".to_string(),
            prediction_markets.base_events.clone(),
        );
    }
}

pub(crate) fn emit_swarm_env(
    swarm: &KaskSwarmSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    // ── Swarm (ABW + Local) ──
    // The API key is a credential (injected by `build_mcp_server_env`
    // from the keychain), not config — only non-secret fields are here.
    let swarm_default = KaskSwarmSettings::default();
    if swarm.mode != swarm_default.mode {
        env.insert("HKASK_SWARM_MODE".to_string(), swarm.mode.to_string());
    }
    if !swarm.api_url.is_empty() {
        env.insert("HKASK_ABW_API_URL".to_string(), swarm.api_url.clone());
    }
    if swarm.max_credits_per_dispatch != swarm_default.max_credits_per_dispatch {
        env.insert(
            "HKASK_ABW_MAX_CREDITS".to_string(),
            swarm.max_credits_per_dispatch.to_string(),
        );
    }
    if swarm.curator_consent_default != swarm_default.curator_consent_default {
        env.insert(
            "HKASK_ABW_CURATOR_CONSENT_DEFAULT".to_string(),
            swarm.curator_consent_default.to_string(),
        );
    }
    if !swarm.local_agents_dir.is_empty() {
        env.insert(
            "HKASK_LOCAL_AGENTS_DIR".to_string(),
            swarm.local_agents_dir.clone(),
        );
    }
    if !swarm.local_swarms_dir.is_empty() {
        env.insert(
            "HKASK_LOCAL_SWARMS_DIR".to_string(),
            swarm.local_swarms_dir.clone(),
        );
    }
    if !swarm.skills_dir.is_empty() {
        env.insert("HKASK_SKILLS_DIR".to_string(), swarm.skills_dir.clone());
    }
    if !swarm.default_agent_model.is_empty() {
        env.insert(
            "HKASK_ABW_DEFAULT_AGENT_MODEL".to_string(),
            swarm.default_agent_model.clone(),
        );
    }
    if swarm.a2a_http_enabled != swarm_default.a2a_http_enabled {
        env.insert(
            "HKASK_A2A_HTTP_ENABLE".to_string(),
            swarm.a2a_http_enabled.to_string(),
        );
    }
    if !swarm.memory_passphrase.is_empty() {
        env.insert(
            "HKASK_SWARM_MEMORY_PASSPHRASE".to_string(),
            swarm.memory_passphrase.clone(),
        );
    }
    if !swarm.memory_db_path.is_empty() {
        env.insert(
            "HKASK_SWARM_MEMORY_DB".to_string(),
            swarm.memory_db_path.clone(),
        );
    }
    if swarm.embedding_dim != swarm_default.embedding_dim {
        env.insert(
            "HKASK_SWARM_EMBEDDING_DIM".to_string(),
            swarm.embedding_dim.to_string(),
        );
    }
}

pub(crate) fn emit_training_env(
    training: &KaskTrainingSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    if !training.host.is_empty() {
        env.insert("HKASK_TRAINING_HOST".to_string(), training.host.clone());
    }
    if !training.cache_dir.is_empty() {
        env.insert(
            "HKASK_TRAINING_CACHE_DIR".to_string(),
            training.cache_dir.clone(),
        );
    }
}

pub(crate) fn emit_models_env(
    models: &KaskModelsSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    // ── Kask-wide model overrides ──
    // These take precedence over the per-server model settings above.
    if !models.default_model.is_empty() {
        env.insert(
            "HKASK_DEFAULT_MODEL".to_string(),
            models.default_model.clone(),
        );
    }
    // Embedding model already emitted above via `effective_embedding_model`,
    // which encodes the `models`-over-`corpus` precedence. Do not emit it
    // again here — a second insert would be a silent duplicate.
    if !models.classifier_model.is_empty() {
        env.insert(
            "HKASK_CLASSIFIER_MODEL".to_string(),
            models.classifier_model.clone(),
        );
    }
    if !models.ocr_model.is_empty() {
        env.insert("HKASK_OCR_MODEL".to_string(), models.ocr_model.clone());
    }
}

pub(crate) fn emit_curator_email_env(
    email: &KaskCuratorEmailSettings,
    env: &mut std::collections::HashMap<String, String>,
) {
    // ── Curator email (non-secret) ──
    // The SMTP password is injected separately by `build_mcp_server_env`
    // from the keychain entry `kask://credentials/hkask_smtp_password`.
    if !email.mxroute_server.is_empty() {
        env.insert(
            "HKASK_MXROUTE_SERVER".to_string(),
            email.mxroute_server.clone(),
        );
    }
    if !email.smtp_username.is_empty() {
        env.insert(
            "HKASK_SMTP_USERNAME".to_string(),
            email.smtp_username.clone(),
        );
        // `HKASK_CURATOR_EMAIL` defaults to `HKASK_SMTP_USERNAME` in the
        // email crate; only inject when explicitly set.
        if !email.curator_email.is_empty() {
            env.insert(
                "HKASK_CURATOR_EMAIL".to_string(),
                email.curator_email.clone(),
            );
        }
        // `HKASK_ALERT_EMAIL` defaults to `HKASK_SMTP_USERNAME` in the
        // email crate; only inject when explicitly set.
        if !email.alert_email.is_empty() {
            env.insert("HKASK_ALERT_EMAIL".to_string(), email.alert_email.clone());
        }
    }
    if !email.authorized_emails.is_empty() {
        env.insert(
            "HKASK_AUTHORIZED_EMAILS".to_string(),
            email.authorized_emails.join(","),
        );
    }
}

// ── Content → Settings conversions ─────────────────────────────────────────
//
// Each subsection has a `From<Content>` impl that reads defaults from `Default`.
// The top-level `From<KaskSettingsContent>` is then a one-liner per field.
// This makes `Default` the single source of truth — no inlined literals in the
// `From` impl, no dead `#[serde(default)]` attributes on `KaskSettings`.

impl From<KaskMcpSettingsContent> for KaskMcpSettings {
    fn from(c: KaskMcpSettingsContent) -> Self {
        let default = Self::default();
        Self {
            load_default: c.load_default.unwrap_or(default.load_default),
            overrides: c.overrides,
        }
    }
}

impl From<KaskGeneralSettingsContent> for KaskGeneralSettings {
    fn from(c: KaskGeneralSettingsContent) -> Self {
        let default = Self::default();
        // Treat 0 as "use default" — a user setting `max_concurrency: 0` would
        // construct a limiter that admits no permits, deadlocking every
        // inference call. Same for `concurrency_step: 0` (the ramp origin
        // and increment must be ≥ 1).
        Self {
            max_concurrency: c
                .max_concurrency
                .filter(|&v| v > 0)
                .unwrap_or(default.max_concurrency),
            concurrency_step: c
                .concurrency_step
                .filter(|&v| v > 0)
                .unwrap_or(default.concurrency_step),
        }
    }
}

impl From<KaskDataServiceSettingsContent> for KaskDataServiceSettings {
    fn from(c: KaskDataServiceSettingsContent) -> Self {
        let default = Self::default();
        Self {
            eodhd_enabled: c.eodhd_enabled.unwrap_or(default.eodhd_enabled),
            fmp_enabled: c.fmp_enabled.unwrap_or(default.fmp_enabled),
            exa_enabled: c.exa_enabled.unwrap_or(default.exa_enabled),
            tavily_enabled: c.tavily_enabled.unwrap_or(default.tavily_enabled),
            brave_enabled: c.brave_enabled.unwrap_or(default.brave_enabled),
            runpod_enabled: c.runpod_enabled.unwrap_or(default.runpod_enabled),
            nebius_enabled: c.nebius_enabled.unwrap_or(default.nebius_enabled),
        }
    }
}

impl From<KaskCuratorEmailSettingsContent> for KaskCuratorEmailSettings {
    fn from(c: KaskCuratorEmailSettingsContent) -> Self {
        let default = Self::default();
        Self {
            mxroute_server: c.mxroute_server.unwrap_or(default.mxroute_server),
            smtp_username: c.smtp_username.unwrap_or(default.smtp_username),
            curator_email: c.curator_email.unwrap_or(default.curator_email),
            alert_email: c.alert_email.unwrap_or(default.alert_email),
            authorized_emails: c.authorized_emails.unwrap_or(default.authorized_emails),
        }
    }
}

impl From<KaskCuratorSettingsContent> for KaskCuratorSettings {
    fn from(c: KaskCuratorSettingsContent) -> Self {
        let default = Self::default();
        Self {
            always_on: c.always_on.unwrap_or(default.always_on),
            algedonic_threshold: c.algedonic_threshold.unwrap_or(default.algedonic_threshold),
            email: c.email.map(Into::into).unwrap_or(default.email),
        }
    }
}

impl From<KaskMemorySettingsContent> for KaskMemorySettings {
    fn from(c: KaskMemorySettingsContent) -> Self {
        let default = Self::default();
        Self {
            consolidation_cadence_secs: c
                .consolidation_cadence_secs
                .unwrap_or(default.consolidation_cadence_secs),
            confidence_floor: c.confidence_floor.unwrap_or(default.confidence_floor),
            recall_limit: c.recall_limit.unwrap_or(default.recall_limit),
            recall_min_confidence: c
                .recall_min_confidence
                .unwrap_or(default.recall_min_confidence),
            auto_inject: c.auto_inject.unwrap_or(default.auto_inject),
            cascade_short_term_turns: c
                .cascade_short_term_turns
                .unwrap_or(default.cascade_short_term_turns),
            cascade_memory_saliency_floor: c
                .cascade_memory_saliency_floor
                .unwrap_or(default.cascade_memory_saliency_floor),
            cascade_memory_max_chunks: c
                .cascade_memory_max_chunks
                .unwrap_or(default.cascade_memory_max_chunks),
            cascade_turn_token_cap: c
                .cascade_turn_token_cap
                .unwrap_or(default.cascade_turn_token_cap),
        }
    }
}

impl From<KaskCondenserSettingsContent> for KaskCondenserSettings {
    fn from(c: KaskCondenserSettingsContent) -> Self {
        let default = Self::default();
        Self {
            profile: c.profile.unwrap_or(default.profile),
            auto_compress_tool_results: c
                .auto_compress_tool_results
                .unwrap_or(default.auto_compress_tool_results),
            persona_keywords: c.persona_keywords.unwrap_or(default.persona_keywords),
            saliency_window: c.saliency_window.unwrap_or(default.saliency_window),
        }
    }
}

impl From<KaskResearchSettingsContent> for KaskResearchSettings {
    fn from(c: KaskResearchSettingsContent) -> Self {
        let default = Self::default();
        Self {
            rss_db: c.rss_db.unwrap_or(default.rss_db),
        }
    }
}

impl From<KaskCompaniesSettingsContent> for KaskCompaniesSettings {
    fn from(c: KaskCompaniesSettingsContent) -> Self {
        let default = Self::default();
        Self {
            chronic_staleness_days: c
                .chronic_staleness_days
                .unwrap_or(default.chronic_staleness_days),
            fermi_defaults: c.fermi_defaults.unwrap_or(default.fermi_defaults),
            transactions_dir: c.transactions_dir.unwrap_or(default.transactions_dir),
        }
    }
}

impl From<KaskCorpusSettingsContent> for KaskCorpusSettings {
    fn from(c: KaskCorpusSettingsContent) -> Self {
        let default = Self::default();
        // Treat 0 as "use default" — a user setting `embedding_dim: 0` would
        // otherwise construct a zero-dimensional EmbeddingStore that silently
        // rejects every vector (DimensionMismatch { expected: 0, ... }),
        // disabling embedding-based recall with no startup signal.
        Self {
            embedding_dim: c
                .embedding_dim
                .filter(|&d| d > 0)
                .unwrap_or(default.embedding_dim),
            embedding_model: c.embedding_model.unwrap_or(default.embedding_model),
            // Treat 0 as "use default" — 0 concurrency would silently disable
            // OCR (no pages processed in parallel).
            ocr_concurrency: c
                .ocr_concurrency
                .filter(|&d| d > 0)
                .unwrap_or(default.ocr_concurrency),
            ocr_simple_max: c.ocr_simple_max.unwrap_or(default.ocr_simple_max),
            ocr_moderate_max: c.ocr_moderate_max.unwrap_or(default.ocr_moderate_max),
            ocr_sample_rate: c.ocr_sample_rate.unwrap_or(default.ocr_sample_rate),
            ocr_tuneable: c.ocr_tuneable.unwrap_or(default.ocr_tuneable),
            template_root: c.template_root.unwrap_or(default.template_root),
        }
    }
}

impl From<KaskMediaSettingsContent> for KaskMediaSettings {
    fn from(c: KaskMediaSettingsContent) -> Self {
        let default = Self::default();
        Self {
            tts_model: c.tts_model.unwrap_or(default.tts_model),
            stt_model: c.stt_model.unwrap_or(default.stt_model),
            vision_model: c.vision_model.unwrap_or(default.vision_model),
            image_gen_model: c.image_gen_model.unwrap_or(default.image_gen_model),
        }
    }
}

impl From<KaskPredictionMarketsSettingsContent> for KaskPredictionMarketsSettings {
    fn from(c: KaskPredictionMarketsSettingsContent) -> Self {
        let default = Self::default();
        Self {
            data_dir: c.data_dir.unwrap_or(default.data_dir),
            cache_ttl_secs: c.cache_ttl_secs.unwrap_or(default.cache_ttl_secs),
            base_events: c.base_events.unwrap_or(default.base_events),
        }
    }
}

impl From<KaskScenariosSettingsContent> for KaskScenariosSettings {
    fn from(c: KaskScenariosSettingsContent) -> Self {
        let default = Self::default();
        Self {
            data_dir: c.data_dir.unwrap_or(default.data_dir),
        }
    }
}

impl From<KaskSwarmSettingsContent> for KaskSwarmSettings {
    fn from(c: KaskSwarmSettingsContent) -> Self {
        let default = Self::default();
        Self {
            mode: c.mode.map(Into::into).unwrap_or(default.mode),
            api_url: c.api_url.unwrap_or(default.api_url),
            max_credits_per_dispatch: c
                .max_credits_per_dispatch
                .unwrap_or(default.max_credits_per_dispatch),
            curator_consent_default: c
                .curator_consent_default
                .unwrap_or(default.curator_consent_default),
            local_agents_dir: c.local_agents_dir.unwrap_or(default.local_agents_dir),
            local_swarms_dir: c.local_swarms_dir.unwrap_or(default.local_swarms_dir),
            skills_dir: c.skills_dir.unwrap_or(default.skills_dir),
            default_agent_model: c.default_agent_model.unwrap_or(default.default_agent_model),
            a2a_http_enabled: c.a2a_http_enabled.unwrap_or(default.a2a_http_enabled),
            memory_passphrase: c.memory_passphrase.unwrap_or(default.memory_passphrase),
            memory_db_path: c.memory_db_path.unwrap_or(default.memory_db_path),
            embedding_dim: c.embedding_dim.unwrap_or(default.embedding_dim),
        }
    }
}

impl From<KaskTrainingSettingsContent> for KaskTrainingSettings {
    fn from(c: KaskTrainingSettingsContent) -> Self {
        let default = Self::default();
        Self {
            host: c.host.unwrap_or(default.host),
            cache_dir: c.cache_dir.unwrap_or(default.cache_dir),
        }
    }
}

impl From<KaskModelsSettingsContent> for KaskModelsSettings {
    fn from(c: KaskModelsSettingsContent) -> Self {
        let default = Self::default();
        Self {
            default_model: c.default_model.unwrap_or(default.default_model),
            embedding_model: c.embedding_model.unwrap_or(default.embedding_model),
            classifier_model: c.classifier_model.unwrap_or(default.classifier_model),
            ocr_model: c.ocr_model.unwrap_or(default.ocr_model),
        }
    }
}


impl From<KaskSettingsContent> for KaskSettings {
    fn from(c: KaskSettingsContent) -> Self {
        Self {
            data_dir: c.data_dir.unwrap_or_default(),
            general: c.general.map(Into::into).unwrap_or_default(),
            mcp: c.mcp.map(Into::into).unwrap_or_default(),
            data_services: c.data_services.map(Into::into).unwrap_or_default(),
            curator: c.curator.map(Into::into).unwrap_or_default(),
            memory: c.memory.map(Into::into).unwrap_or_default(),
            condenser: c.condenser.map(Into::into).unwrap_or_default(),
            research: c.research.map(Into::into).unwrap_or_default(),
            companies: c.companies.map(Into::into).unwrap_or_default(),
            corpus: c.corpus.map(Into::into).unwrap_or_default(),
            media: c.media.map(Into::into).unwrap_or_default(),
            scenarios: c.scenarios.map(Into::into).unwrap_or_default(),
            prediction_markets: c.prediction_markets.map(Into::into).unwrap_or_default(),
            swarm: c.swarm.map(Into::into).unwrap_or_default(),
            training: c.training.map(Into::into).unwrap_or_default(),
            models: c.models.map(Into::into).unwrap_or_default(),
            tool_router: c.tool_router.map(Into::into).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the `kask_bridge` half of the tool-router default-sync contract.
    /// `LazyToolRouter::new()` in the `agent` crate hardcodes the same pair as a
    /// fallback for settings-free construction, and `agent` cannot depend on
    /// this crate (that would invert the D8 seam), so the invariant is pinned
    /// from both sides against literals. Change one, change the other:
    /// `crates/agent/src/tool_router.rs::default_thresholds_are_the_documented_values`.
    #[test]
    fn tool_router_defaults_match_agent_side_fallback() {
        let default = KaskToolRouterSettings::default();
        assert_eq!(
            default.complex_word_threshold, 6,
            "word threshold changed — update LazyToolRouter::new() in crates/agent/src/tool_router.rs"
        );
        assert!(
            (default.threshold - 0.30).abs() < f64::EPSILON,
            "score threshold changed — update LazyToolRouter::new() in crates/agent/src/tool_router.rs"
        );
    }

    // Regression test for the silent `embedding_dim == 0` bug. A user
    // setting `embedding_dim: 0` in their settings file would construct a
    // zero-dimensional EmbeddingStore that rejects every vector with
    // `DimensionMismatch { expected: 0, actual: N }`, silently disabling
    // embedding-based recall. The `unwrap_or(1024)` default only fires for
    // `None`, not for `Some(0)`. This test pins the fix: 0 is treated as
    // "use default".
    #[test]
    fn corpus_settings_treats_zero_embedding_dim_as_default() {
        let content = KaskSettingsContent {
            corpus: Some(KaskCorpusSettingsContent {
                embedding_dim: Some(0),
                embedding_model: None,
                ocr_concurrency: None,
                ocr_simple_max: None,
                ocr_moderate_max: None,
                ocr_sample_rate: None,
                ocr_tuneable: None,
                template_root: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert_eq!(
            settings.corpus.embedding_dim, 1024,
            "embedding_dim: 0 should fall back to the default (1024), \
             not construct a zero-dimensional store"
        );
    }

    #[test]
    fn corpus_settings_treats_zero_ocr_concurrency_as_default() {
        let content = KaskSettingsContent {
            corpus: Some(KaskCorpusSettingsContent {
                embedding_dim: None,
                embedding_model: None,
                ocr_concurrency: Some(0),
                ocr_simple_max: None,
                ocr_moderate_max: None,
                ocr_sample_rate: None,
                ocr_tuneable: None,
                template_root: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert_eq!(
            settings.corpus.ocr_concurrency, 4,
            "ocr_concurrency: 0 should fall back to the default (4), \
             not silently disable OCR"
        );
    }

    #[test]
    fn corpus_settings_preserves_explicit_nonzero_embedding_dim() {
        let content = KaskSettingsContent {
            corpus: Some(KaskCorpusSettingsContent {
                embedding_dim: Some(2560),
                embedding_model: None,
                ocr_concurrency: None,
                ocr_simple_max: None,
                ocr_moderate_max: None,
                ocr_sample_rate: None,
                ocr_tuneable: None,
                template_root: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert_eq!(settings.corpus.embedding_dim, 2560);
    }

    #[test]
    fn corpus_settings_defaults_embedding_dim_when_absent() {
        let content = KaskSettingsContent {
            corpus: Some(KaskCorpusSettingsContent {
                embedding_dim: None,
                embedding_model: None,
                ocr_concurrency: None,
                ocr_simple_max: None,
                ocr_moderate_max: None,
                ocr_sample_rate: None,
                ocr_tuneable: None,
                template_root: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert_eq!(settings.corpus.embedding_dim, 1024);
    }

    // Regression test for the Default-path crash (2026-07-28).
    // KaskCorpusSettings derived Default, but u32::default() == 0, not 1024.
    // The #[serde(default)] attribute only fires during deserialization, NOT
    // for Default::default(). When the user had no `kask.corpus` section,
    // from_settings fell back to KaskSettings::default() →
    // KaskCorpusSettings::default() → embedding_dim: 0, which panicked in
    // EmbeddingStore::from_driver (assert dim > 0). The manual Default impl
    // above returns 1024, matching the serde default.
    #[test]
    fn corpus_settings_default_embedding_dim_is_not_zero() {
        let default_corpus = KaskCorpusSettings::default();
        assert_eq!(
            default_corpus.embedding_dim, 1024,
            "KaskCorpusSettings::default() must return embedding_dim: 1024, not 0 — \
             a zero-dimensional store panics in EmbeddingStore::from_driver"
        );
    }

    // KaskSettings::default() (used when there is no kask section at all) must
    // also produce a non-zero embedding_dim, since it delegates to
    // KaskCorpusSettings::default() for the corpus field.
    #[test]
    fn kask_settings_default_corpus_embedding_dim_is_not_zero() {
        let settings = KaskSettings::default();
        assert_eq!(settings.corpus.embedding_dim, 1024);
    }

    // Regression test for the silent MCP-server-registration bug (2026-07-28).
    // KaskMcpSettings derived Default, but bool::default() == false, not true.
    // The #[serde(default = "default_true")] attribute only fires during
    // deserialization, NOT for Default::default(). When the user had a `kask`
    // section but no `kask.mcp` subsection, From<KaskSettingsContent> fell back
    // to KaskMcpSettings::default() → load_default: false, and sync_kask_mcp_servers
    // treated all 12 servers as disabled, registering nothing. The manual Default
    // impl above returns true, matching the serde default.
    #[test]
    fn mcp_settings_default_load_default_is_true() {
        let default_mcp = KaskMcpSettings::default();
        assert!(
            default_mcp.load_default,
            "KaskMcpSettings::default() must return load_default: true, not false — \
             a false default silently disables all kask MCP server registration"
        );
    }

    // The bug manifests when a user has a `kask` section but no `kask.mcp`
    // subsection: From<KaskSettingsContent> hits the `.unwrap_or_default()` path.
    // This test pins that path to load_default: true.
    #[test]
    fn kask_settings_from_content_without_mcp_section_loads_defaults() {
        let content = KaskSettingsContent {
            mcp: None,
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert!(
            settings.mcp.load_default,
            "a kask section with no mcp subsection must default to load_default: true"
        );
    }

    // ── Drift-class regression tests ─────────────────────────────────────────
    //
    // The triple-default drift class: a struct with `#[serde(default =
    // "default_true")]` on a bool field AND `#[derive(Default)]` would get
    // `Default::default() → false`, disagreeing with the serde spec (`true`).
    // When the user omitted that subsection, `From` hit `.unwrap_or_default()` and
    // silently used the wrong default. These tests pin every drifting subsection's
    // `Default` to match the intended (serde) default, so the drift can't recur.

    #[test]
    fn curator_settings_default_always_on_is_true() {
        assert!(
            KaskCuratorSettings::default().always_on,
            "KaskCuratorSettings::default() must return always_on: true"
        );
    }

    #[test]
    fn memory_settings_default_auto_inject_is_true() {
        assert!(
            KaskMemorySettings::default().auto_inject,
            "KaskMemorySettings::default() must return auto_inject: true"
        );
    }

    #[test]
    fn condenser_settings_default_auto_compress_is_false() {
        assert!(
            !KaskCondenserSettings::default().auto_compress_tool_results,
            "KaskCondenserSettings::default() must return auto_compress_tool_results: false"
        );
    }

    // The absent-subsection path: when a user has a `kask` section but omits a
    // subsection, `From` hits `.unwrap_or_default()`. This test verifies ALL
    // subsections produce their intended defaults through that path.
    #[test]
    fn kask_settings_from_empty_content_uses_all_defaults() {
        let settings = KaskSettings::from(KaskSettingsContent::default());
        assert!(settings.mcp.load_default);
        assert!(settings.curator.always_on);
        assert_eq!(settings.curator.algedonic_threshold, 0.8);
        assert!(settings.memory.auto_inject);
        assert_eq!(settings.memory.consolidation_cadence_secs, 300);
        assert!(!settings.condenser.auto_compress_tool_results);
        assert_eq!(settings.condenser.profile, "normal");
        assert_eq!(settings.corpus.embedding_dim, 1024);
    }

    // The present-but-null-field path: when a subsection is present but a field
    // is `None`, `From` hits `.unwrap_or(default.field)`. This test verifies the
    // field-level defaults also come from `Default`, not inlined literals.
    #[test]
    fn kask_settings_from_present_subsection_with_null_fields_uses_defaults() {
        let content = KaskSettingsContent {
            mcp: Some(KaskMcpSettingsContent {
                load_default: None,
                overrides: HashMap::default(),
            }),
            curator: Some(KaskCuratorSettingsContent {
                always_on: None,
                algedonic_threshold: None,
                email: None,
            }),
            memory: Some(KaskMemorySettingsContent {
                consolidation_cadence_secs: None,
                confidence_floor: None,
                recall_limit: None,
                recall_min_confidence: None,
                auto_inject: None,
                cascade_short_term_turns: None,
                cascade_memory_saliency_floor: None,
                cascade_memory_max_chunks: None,
                cascade_turn_token_cap: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert!(settings.mcp.load_default);
        assert!(settings.curator.always_on);
        assert_eq!(settings.curator.algedonic_threshold, 0.8);
        assert!(settings.memory.auto_inject);
        assert_eq!(settings.memory.consolidation_cadence_secs, 300);
    }

    // `mcp_env()` must not emit env vars for settings that match `Default`.
    // Previously `mcp_env()` compared against inlined magic numbers (1024, 4,
    // 0.05, 0.15, 0.10, "registry", 5) that duplicated `Default` values. If
    // `Default` changed, the comparison would drift and emit env vars for the
    // default case. Now `mcp_env()` reads from `Default::default()`, so changing
    // `Default` automatically updates the comparison. This test pins that: a
    // `KaskSettings::default()` (all defaults) does not emit per-server config
    // vars. `HKASK_DATA_DIR` is always emitted (it is a kask-wide critical
    // path, not a per-server toggle) — see `mcp_env_always_emits_data_dir`.
    #[test]
    fn mcp_env_emits_nothing_for_default_settings() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        // The only env vars that could appear for default settings are the
        // corpus/condenser numeric defaults — all should be suppressed because
        // they match `Default`.
        assert!(
            !env.contains_key("HKASK_EMBEDDING_DIM"),
            "default embedding_dim must not be emitted"
        );
        assert!(
            !env.contains_key("HKASK_OCR_CONCURRENCY"),
            "default ocr_concurrency must not be emitted"
        );
        assert!(
            !env.contains_key("HKASK_OCR_SIMPLE_MAX"),
            "default ocr_simple_max must not be emitted"
        );
        // HKASK_TEMPLATE_ROOT is now always emitted (resolved from data_dir),
        // so it's no longer suppressed for default settings — see
        // `mcp_env_always_emits_template_root`.
        assert!(
            !env.contains_key("HKASK_EMBEDDING_MODEL"),
            "default embedding_model must not be emitted — the `is_empty()` check was a drift bug; the default is non-empty"
        );
        assert!(
            !env.contains_key("HKASK_CONDENSE_SALIENCY_WINDOW"),
            "default saliency_window must not be emitted"
        );
    }

    // `HKASK_DATA_DIR` is a kask-wide critical path — it must ALWAYS be
    // injected by `mcp_env()` so every MCP server can resolve databases
    // consistently, even when the operator never set the env var or the
    // settings field. The resolved default comes from
    // `hkask_types::agent_paths::resolve_data_dir()`.
    #[test]
    fn mcp_env_always_emits_data_dir() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        assert!(
            env.contains_key("HKASK_DATA_DIR"),
            "HKASK_DATA_DIR must always be emitted — without it, MCP servers \
             cannot resolve database paths consistently"
        );
        let dir = env.get("HKASK_DATA_DIR").unwrap();
        assert!(
            !dir.is_empty(),
            "HKASK_DATA_DIR must resolve to a non-empty path even for default settings"
        );
    }

    // When the operator sets `data_dir` in settings, `mcp_env()` must use
    // that value instead of the env var or resolved default.
    #[test]
    fn mcp_env_data_dir_setting_overrides_env() {
        let mut settings = KaskSettings::default();
        settings.data_dir = "/custom/kask/data".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_DATA_DIR").map(String::as_str),
            Some("/custom/kask/data")
        );
    }

    // `HKASK_TEMPLATE_ROOT` must ALWAYS be emitted so MCP servers (corpus,
    // training) find templates in production where the CWD-relative default
    // ("kask/registry") does not exist. For default settings, the value is
    // resolved from `data_dir` as `{data_dir}/skills/registry/` — the path
    // where `seed_templates` writes at startup.
    #[test]
    fn mcp_env_always_emits_template_root() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        let template_root = env.get("HKASK_TEMPLATE_ROOT");
        assert!(
            template_root.is_some(),
            "HKASK_TEMPLATE_ROOT must always be emitted — without it, MCP servers \
             fall back to a CWD-relative path that does not exist in production"
        );
        let data_dir = env.get("HKASK_DATA_DIR").expect("data_dir must be emitted");
        let expected = std::path::Path::new(data_dir)
            .join("skills")
            .join("registry")
            .to_string_lossy()
            .to_string();
        assert_eq!(
            template_root.map(String::as_str),
            Some(expected.as_str()),
            "default template_root must resolve to `{{data_dir}}/skills/registry`"
        );
    }

    // When the operator overrides `corpus.template_root`, `mcp_env()` must use
    // that value instead of the data-dir-resolved default.
    #[test]
    fn mcp_env_template_root_override_emits_user_value() {
        let mut settings = KaskSettings::default();
        settings.corpus.template_root = "/custom/templates".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_TEMPLATE_ROOT").map(String::as_str),
            Some("/custom/templates")
        );
    }

    // When the operator sets `data_dir` in settings, the template_root must
    // resolve from the custom data_dir, not the platform default.
    #[test]
    fn mcp_env_template_root_follows_data_dir_override() {
        let mut settings = KaskSettings::default();
        settings.data_dir = "/custom/kask/data".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_TEMPLATE_ROOT").map(String::as_str),
            Some("/custom/kask/data/skills/registry")
        );
    }
    // This pins the other direction: the `Default`-based comparison still
    // detects non-default values.
    #[test]
    fn mcp_env_emits_for_non_default_settings() {
        let mut settings = KaskSettings::default();
        settings.corpus.embedding_dim = 2048;
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_EMBEDDING_DIM").map(String::as_str),
            Some("2048")
        );
    }

    // The `embedding_model` field has a non-empty `Default`
    // (`DEFAULT_EMBEDDING_MODEL`), so the comparison must be
    // against `Default`, not `is_empty()`. A user override must be emitted;
    // the default must not.
    #[test]
    fn mcp_env_emits_embedding_model_when_overridden() {
        let mut settings = KaskSettings::default();
        settings.corpus.embedding_model = "OpenAI/text-embedding-3-large".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_EMBEDDING_MODEL").map(String::as_str),
            Some("OpenAI/text-embedding-3-large")
        );
    }

    // Precedence pin: when BOTH `corpus.embedding_model` and
    // `models.embedding_model` are set, the kask-wide `models` override must
    // win. Previously this precedence existed only as statement order in
    // `mcp_env()` (the `models` block overwrote the `corpus` block). It is now
    // explicit in `effective_embedding_model`; this test locks the contract so a
    // reorder cannot silently flip which setting wins.
    #[test]
    fn mcp_env_models_embedding_model_overrides_corpus() {
        let mut settings = KaskSettings::default();
        settings.corpus.embedding_model = "OpenAI/text-embedding-3-large".to_string();
        settings.models.embedding_model = "voyage/voyage-3".to_string();
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_EMBEDDING_MODEL").map(String::as_str),
            Some("voyage/voyage-3")
        );
    }

    // When `models.embedding_model` is empty, the effective value falls back
    // to the corpus setting (if non-default), then to the constant.
    #[test]
    fn effective_embedding_model_falls_back_to_corpus_when_models_empty() {
        let mut settings = KaskSettings::default();
        assert_eq!(
            settings.effective_embedding_model(),
            default_embedding_model()
        );
        settings.corpus.embedding_model = "OpenAI/text-embedding-3-large".to_string();
        assert_eq!(
            settings.effective_embedding_model(),
            "OpenAI/text-embedding-3-large"
        );
        // The kask-wide override takes precedence over the corpus setting.
        settings.models.embedding_model = "voyage/voyage-3".to_string();
        assert_eq!(settings.effective_embedding_model(), "voyage/voyage-3");
    }

    // Swarm settings: `Default` is the single source of truth — default
    // settings emit no swarm env vars; a non-default credit ceiling emits
    // `HKASK_ABW_MAX_CREDITS`; the API key is never in `mcp_env()` (it is a
    // keychain credential, injected by `build_mcp_server_env`).

    #[test]
    fn swarm_settings_default_emits_no_env() {
        let settings = KaskSettings::default();
        let env = settings.mcp_env();
        assert!(!env.contains_key("HKASK_ABW_API_URL"));
        assert!(!env.contains_key("HKASK_ABW_MAX_CREDITS"));
        assert!(!env.contains_key("HKASK_ABW_CURATOR_CONSENT_DEFAULT"));
        assert!(!env.contains_key("HKASK_SWARM_MODE"));
        assert!(!env.contains_key("HKASK_LOCAL_AGENTS_DIR"));
        assert!(!env.contains_key("HKASK_LOCAL_SWARMS_DIR"));
        assert!(!env.contains_key("HKASK_SKILLS_DIR"));
        assert!(!env.contains_key("HKASK_ABW_DEFAULT_AGENT_MODEL"));
        assert!(!env.contains_key("HKASK_A2A_HTTP_ENABLE"));
        assert!(!env.contains_key("HKASK_SWARM_MEMORY_PASSPHRASE"));
        assert!(!env.contains_key("HKASK_SWARM_MEMORY_DB"));
        assert!(!env.contains_key("HKASK_SWARM_EMBEDDING_DIM"));
        assert!(
            !env.contains_key("HKASK_ABW_API_KEY"),
            "the ABW API key is a credential, not config — it must never appear in mcp_env()"
        );
        assert_eq!(settings.swarm.max_credits_per_dispatch, 50);
        assert!(!settings.swarm.curator_consent_default);
        assert_eq!(settings.swarm.mode, SwarmModeConfig::Abw);
        assert!(!settings.swarm.a2a_http_enabled);
        assert_eq!(settings.swarm.embedding_dim, 1024);
    }

    #[test]
    fn swarm_settings_non_default_emits_env() {
        let mut settings = KaskSettings::default();
        settings.swarm.mode = SwarmModeConfig::Local;
        settings.swarm.max_credits_per_dispatch = 100;
        settings.swarm.api_url = "https://staging.agent-bestiary.world".to_string();
        settings.swarm.curator_consent_default = true;
        settings.swarm.local_agents_dir = "/custom/agents/dir".to_string();
        settings.swarm.local_swarms_dir = "/custom/swarms/dir".to_string();
        settings.swarm.skills_dir = "/custom/skills/dir".to_string();
        settings.swarm.default_agent_model = "claude-sonnet-4-6".to_string();
        settings.swarm.a2a_http_enabled = true;
        settings.swarm.memory_passphrase = "real-secret".to_string();
        settings.swarm.memory_db_path = "/custom/memory.db".to_string();
        settings.swarm.embedding_dim = 2048;
        let env = settings.mcp_env();
        assert_eq!(
            env.get("HKASK_SWARM_MODE").map(String::as_str),
            Some("local")
        );
        assert_eq!(
            env.get("HKASK_ABW_MAX_CREDITS").map(String::as_str),
            Some("100")
        );
        assert_eq!(
            env.get("HKASK_ABW_API_URL").map(String::as_str),
            Some("https://staging.agent-bestiary.world")
        );
        assert_eq!(
            env.get("HKASK_ABW_CURATOR_CONSENT_DEFAULT")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            env.get("HKASK_LOCAL_AGENTS_DIR").map(String::as_str),
            Some("/custom/agents/dir")
        );
        assert_eq!(
            env.get("HKASK_LOCAL_SWARMS_DIR").map(String::as_str),
            Some("/custom/swarms/dir")
        );
        assert_eq!(
            env.get("HKASK_SKILLS_DIR").map(String::as_str),
            Some("/custom/skills/dir")
        );
        assert_eq!(
            env.get("HKASK_ABW_DEFAULT_AGENT_MODEL").map(String::as_str),
            Some("claude-sonnet-4-6")
        );
        assert_eq!(
            env.get("HKASK_A2A_HTTP_ENABLE").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            env.get("HKASK_SWARM_MEMORY_PASSPHRASE").map(String::as_str),
            Some("real-secret")
        );
        assert_eq!(
            env.get("HKASK_SWARM_MEMORY_DB").map(String::as_str),
            Some("/custom/memory.db")
        );
        assert_eq!(
            env.get("HKASK_SWARM_EMBEDDING_DIM").map(String::as_str),
            Some("2048")
        );
    }

}
