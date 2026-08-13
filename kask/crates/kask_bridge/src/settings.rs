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
    KaskCodegraphSettingsContent, KaskCompaniesSettingsContent, KaskCondenserSettingsContent,
    KaskCorpusSettingsContent, KaskCuratorEmailSettingsContent, KaskCuratorSettingsContent,
    KaskDataServiceSettingsContent, KaskInferenceProvidersSettingsContent, KaskMcpSettingsContent,
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

    /// Codegraph MCP server configuration.
    pub codegraph: KaskCodegraphSettings,

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
    pub inference_providers: KaskInferenceProvidersSettings,

    /// Local collab server configuration. When enabled, zed-kask launches a
    /// local collab server at startup so the kask extensions panel can fetch
    /// `/api/kask-skills` without depending on the deployed `zed.dev` server.
    pub collab: KaskCollabSettings,
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
/// When a provider is enabled, the composition root writes an
/// `openai_compatible.<provider_id>` entry to settings.json so zed's
/// OpenAI-compatible provider machinery registers it.
///
/// `Default` returns all-false (pure, no side effects). The env-var-based
/// auto-enable logic lives in `From<KaskInferenceProvidersSettingsContent>`,
/// which is the only production path. This keeps `Default` deterministic for
/// tests and `KaskSettings::default()`.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskInferenceProvidersSettings {
    /// Enable DeepInfra (OpenAI-compatible inference).
    pub deepinfra_enabled: bool,

    /// Enable OpenRouter (unified API for 200+ models).
    pub openrouter_enabled: bool,

    /// Enable AtlasCloud (task-based media + OpenAI-compatible LLM).
    pub atlascloud_enabled: bool,
}

impl KaskInferenceProvidersSettings {
    /// Construct from the process environment — auto-enables providers whose
    /// API key env var is set. This is the same logic `From<Content>` uses
    /// when the user hasn't explicitly set a toggle. Exposed as a public
    /// method so the settings UI (which doesn't depend on `settings_content`)
    /// can resolve the same defaults without constructing a `Content` struct.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            deepinfra_enabled: std::env::var("DEEPINFRA_API_KEY").is_ok(),
            openrouter_enabled: std::env::var("OPENROUTER_API_KEY").is_ok(),
            atlascloud_enabled: std::env::var("ATLASCLOUD_API_KEY").is_ok(),
        }
    }
}

/// Local collab server configuration.
///
/// When `enabled` is true, zed-kask launches a local `collab serve api`
/// process at startup so the kask extensions panel can fetch
/// `/api/kask-skills` without depending on the deployed `zed.dev` server
/// having the kask route. The server uses SQLite for local dev; S3 is
/// only required for publish/download/vote.
///
/// `Default` is the single source of truth — `From<Content>` reads from it
/// via `unwrap_or(default.field)`. The defaults launch a server on
/// `localhost:3000` with a `sqlite:./kask_marketplace.db` database.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskCollabSettings {
    /// Whether to auto-launch the local collab server at startup.
    pub enabled: bool,

    /// SQLite connection string (e.g. `sqlite:./kask_marketplace.db`).
    pub database_url: String,

    /// HTTP port the collab server listens on.
    pub http_port: u16,

    /// Zed environment (`development`, `staging`, `production`).
    pub zed_environment: String,

    /// Marketplace base URL the extensions panel uses. When set and non-empty,
    /// overrides the `server_url`-based resolution in `kask_marketplace_base_url`.
    pub marketplace_url: String,
}

impl Default for KaskCollabSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            database_url: "sqlite:kask_marketplace.db?mode=rwc".into(),
            http_port: 3000,
            zed_environment: "development".into(),
            marketplace_url: "http://localhost:3000".into(),
        }
    }
}

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

    /// Inbox poll interval in seconds (0 = disabled). Reserved for a future
    /// inbound IMAP path; currently unused by the outbound-only sink.
    pub inbox_poll_interval_secs: u64,

    /// Digest interval in seconds (0 = disabled). Reserved for a future
    /// periodic digest sender; currently unused by the outbound-only sink.
    pub digest_interval_secs: u64,
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
}

impl Default for KaskMemorySettings {
    fn default() -> Self {
        Self {
            consolidation_cadence_secs: 300,
            confidence_floor: 0.3,
            recall_limit: 5,
            recall_min_confidence: 0.3,
            auto_inject: true,
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

/// Codegraph MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskCodegraphSettings {
    /// Database path for the codegraph store. When empty, uses in-memory.
    pub db_path: String,
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
    /// TTS model override (e.g., "DeepInfra/hexgrad/Kokoro-82M").
    pub tts_model: String,

    /// STT model override (e.g., "DeepInfra/whisper-large-v3").
    pub stt_model: String,

    /// Vision model override (e.g., "OpenRouter/qwen/qwen3-vl-235b-a22b-instruct").
    pub vision_model: String,

    /// Image generation model override (e.g., "DeepInfra/black-forest-labs/FLUX-2-klein-4b").
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
        // `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs`. The bridge
        // emits env vars (`HKASK_ABW_*` / `HKASK_SWARM_*`) from this `Default` via `mcp_env()`;
        // the server reads them in `SwarmConfig::from_env`. The two `Default` impls
        // are deliberately separate (the server crate does not depend on the
        // bridge crate) to avoid a circular dependency — the duplication is
        // the seam between them. If you change a default here, change it there
        // too, and update the `swarm_settings_default_emits_no_env` test below.
        // Note: `default_agent_model` is server-only (operator env var, not
        // settings-file) — it has no counterpart here.
        Self {
            mode: SwarmModeConfig::default(),
            api_url: String::new(),
            max_credits_per_dispatch: 50,
            curator_consent_default: false,
            local_agents_dir: String::new(),
            local_swarms_dir: String::new(),
            skills_dir: String::new(),
        }
    }
}

/// Training MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskTrainingSettings {
    /// Training host override: "deepinfra", "nebius", or "runpod".
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
    /// When set, overrides the kask default for Curator, skill cascade, and
    /// kask panel inference.
    pub default_model: String,

    /// Embedding model for corpus indexing and memory semantic recall
    /// (provider-prefixed). When empty, falls back to the corpus MCP server's
    /// `embedding_model` setting, then to the kask default.
    pub embedding_model: String,

    /// Classifier model for guard/regulation classification tasks
    /// (provider-prefixed). When empty, falls back to the kask default.
    pub classifier_model: String,
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
        env.insert("HKASK_DATA_DIR".to_string(), data_dir);

        // Map the curator's WebID (stashed in `HKASK_CURATOR_WEBID` by the
        // deferred task) to `HKASK_WEBID` so the curator MCP server picks it
        // up as its identity. The `config_env` allowlist filters this to the
        // curator server only — other servers don't receive `HKASK_WEBID`
        // from this mapping and fall through to their own identity resolution.
        if let Ok(curator_webid) = std::env::var("HKASK_CURATOR_WEBID") {
            env.insert("HKASK_WEBID".to_string(), curator_webid);
        }

        // Pass the governed server id set to the swarm server so it can
        // filter cloned cards' declared `mcp_tools` to these servers (the
        // provenance boundary for third-party ABW cards). Only the swarm
        // server's `config_env` allowlist includes this var, so no other
        // child receives it.
        env.insert(
            "HKASK_MCP_SERVER_IDS".to_string(),
            crate::BUILT_IN_MCP_SERVERS_IDS.join(","),
        );

        // Defaults are read from each subsection's `Default` impl so there's a
        // single source of truth — changing `Default` automatically updates
        // the comparison here. Do not inline magic numbers; they drift from
        // `Default` (the same drift class that silently disabled all 10 kask
        // MCP servers when `KaskMcpSettings::default()` disagreed with the
        // serde default).
        let condenser_default = KaskCondenserSettings::default();
        let corpus_default = KaskCorpusSettings::default();

        // ── Condenser ──
        if !self.condenser.persona_keywords.is_empty() {
            env.insert(
                "HKASK_CONDENSER_PERSONA_KEYWORDS".to_string(),
                self.condenser.persona_keywords.join(","),
            );
        }
        if self.condenser.saliency_window != condenser_default.saliency_window {
            env.insert(
                "HKASK_CONDENSE_SALIENCY_WINDOW".to_string(),
                self.condenser.saliency_window.to_string(),
            );
        }

        // ── Codegraph ──
        if !self.codegraph.db_path.is_empty() {
            env.insert(
                "HKASK_CODEGRAPH_DB".to_string(),
                self.codegraph.db_path.clone(),
            );
        }

        // ── Research ──
        if !self.research.rss_db.is_empty() {
            env.insert("HKASK_RSS_DB".to_string(), self.research.rss_db.clone());
        }

        // ── Companies ──
        if self.companies.chronic_staleness_days > 0 {
            env.insert(
                "HKASK_CHRONIC_STALENESS_DAYS".to_string(),
                self.companies.chronic_staleness_days.to_string(),
            );
        }
        if !self.companies.fermi_defaults.is_empty() {
            env.insert(
                "HKASK_FERMI_DEFAULTS".to_string(),
                self.companies.fermi_defaults.clone(),
            );
        }
        // D28 — Standardized Artifact Storage. Emit HKASK_TRANSACTIONS_DIR
        // so the portfolio server can auto-load transaction files. Default
        // is `mcp/portfolio/transactions/` under the kask data root.
        let transactions_dir = if !self.companies.transactions_dir.is_empty() {
            self.companies.transactions_dir.clone()
        } else {
            hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
                "mcp/portfolio/transactions",
            ))
            .to_string_lossy()
            .to_string()
        };
        env.insert("HKASK_TRANSACTIONS_DIR".to_string(), transactions_dir);

        // ── Corpus ──
        if self.corpus.embedding_dim != corpus_default.embedding_dim {
            env.insert(
                "HKASK_EMBEDDING_DIM".to_string(),
                self.corpus.embedding_dim.to_string(),
            );
        }
        if self.corpus.embedding_model != corpus_default.embedding_model {
            env.insert(
                "HKASK_EMBEDDING_MODEL".to_string(),
                self.corpus.embedding_model.clone(),
            );
        }
        if self.corpus.ocr_concurrency != corpus_default.ocr_concurrency {
            env.insert(
                "HKASK_OCR_CONCURRENCY".to_string(),
                self.corpus.ocr_concurrency.to_string(),
            );
        }
        if (self.corpus.ocr_simple_max - corpus_default.ocr_simple_max).abs() > f64::EPSILON {
            env.insert(
                "HKASK_OCR_SIMPLE_MAX".to_string(),
                self.corpus.ocr_simple_max.to_string(),
            );
        }
        if (self.corpus.ocr_moderate_max - corpus_default.ocr_moderate_max).abs() > f64::EPSILON {
            env.insert(
                "HKASK_OCR_MODERATE_MAX".to_string(),
                self.corpus.ocr_moderate_max.to_string(),
            );
        }
        if (self.corpus.ocr_sample_rate - corpus_default.ocr_sample_rate).abs() > f64::EPSILON {
            env.insert(
                "HKASK_OCR_SAMPLE_RATE".to_string(),
                self.corpus.ocr_sample_rate.to_string(),
            );
        }
        if self.corpus.ocr_tuneable != corpus_default.ocr_tuneable {
            env.insert("HKASK_OCR_TUNEABLE".to_string(), "false".to_string());
        }
        if self.corpus.template_root != corpus_default.template_root {
            env.insert(
                "HKASK_TEMPLATE_ROOT".to_string(),
                self.corpus.template_root.clone(),
            );
        }

        // ── Media ──
        if !self.media.tts_model.is_empty() {
            env.insert(
                "HKASK_MEDIA_TTS_MODEL".to_string(),
                self.media.tts_model.clone(),
            );
        }
        if !self.media.stt_model.is_empty() {
            env.insert(
                "HKASK_MEDIA_STT_MODEL".to_string(),
                self.media.stt_model.clone(),
            );
        }
        if !self.media.vision_model.is_empty() {
            env.insert(
                "HKASK_MEDIA_VISION_MODEL".to_string(),
                self.media.vision_model.clone(),
            );
        }
        if !self.media.image_gen_model.is_empty() {
            env.insert(
                "HKASK_MEDIA_IMAGE_GEN_MODEL".to_string(),
                self.media.image_gen_model.clone(),
            );
        }

        // ── Scenarios ──
        if !self.scenarios.data_dir.is_empty() {
            env.insert(
                "HKASK_SCENARIOS_DATA".to_string(),
                self.scenarios.data_dir.clone(),
            );
        }

        // ── Prediction markets ──
        if !self.prediction_markets.data_dir.is_empty() {
            env.insert(
                "HKASK_PREDICTION_MARKETS_DATA".to_string(),
                self.prediction_markets.data_dir.clone(),
            );
        }
        if self.prediction_markets.cache_ttl_secs > 0 {
            env.insert(
                "HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS".to_string(),
                self.prediction_markets.cache_ttl_secs.to_string(),
            );
        }
        if !self.prediction_markets.base_events.is_empty() {
            env.insert(
                "HKASK_PREDICTION_MARKETS_BASE_EVENTS".to_string(),
                self.prediction_markets.base_events.clone(),
            );
        }

        // ── Swarm (ABW + Local) ──
        // The API key is a credential (injected by `build_mcp_server_env`
        // from the keychain), not config — only non-secret fields are here.
        let swarm_default = KaskSwarmSettings::default();
        if self.swarm.mode != swarm_default.mode {
            env.insert("HKASK_SWARM_MODE".to_string(), self.swarm.mode.to_string());
        }
        if !self.swarm.api_url.is_empty() {
            env.insert("HKASK_ABW_API_URL".to_string(), self.swarm.api_url.clone());
        }
        if self.swarm.max_credits_per_dispatch != swarm_default.max_credits_per_dispatch {
            env.insert(
                "HKASK_ABW_MAX_CREDITS".to_string(),
                self.swarm.max_credits_per_dispatch.to_string(),
            );
        }
        if self.swarm.curator_consent_default != swarm_default.curator_consent_default {
            env.insert(
                "HKASK_ABW_CURATOR_CONSENT_DEFAULT".to_string(),
                self.swarm.curator_consent_default.to_string(),
            );
        }
        if !self.swarm.local_agents_dir.is_empty() {
            env.insert(
                "HKASK_LOCAL_AGENTS_DIR".to_string(),
                self.swarm.local_agents_dir.clone(),
            );
        }
        if !self.swarm.local_swarms_dir.is_empty() {
            env.insert(
                "HKASK_LOCAL_SWARMS_DIR".to_string(),
                self.swarm.local_swarms_dir.clone(),
            );
        }
        if !self.swarm.skills_dir.is_empty() {
            env.insert(
                "HKASK_SKILLS_DIR".to_string(),
                self.swarm.skills_dir.clone(),
            );
        }

        // ── Training ──
        if !self.training.host.is_empty() {
            env.insert(
                "HKASK_TRAINING_HOST".to_string(),
                self.training.host.clone(),
            );
        }
        if !self.training.cache_dir.is_empty() {
            env.insert(
                "HKASK_TRAINING_CACHE_DIR".to_string(),
                self.training.cache_dir.clone(),
            );
        }

        // ── Kask-wide model overrides ──
        // These take precedence over the per-server model settings above.
        if !self.models.default_model.is_empty() {
            env.insert(
                "HKASK_DEFAULT_MODEL".to_string(),
                self.models.default_model.clone(),
            );
        }
        if !self.models.embedding_model.is_empty() {
            env.insert(
                "HKASK_EMBEDDING_MODEL".to_string(),
                self.models.embedding_model.clone(),
            );
        }
        if !self.models.classifier_model.is_empty() {
            env.insert(
                "HKASK_CLASSIFIER_MODEL".to_string(),
                self.models.classifier_model.clone(),
            );
        }

        // ── Curator email (non-secret) ──
        // The SMTP password is injected separately by `build_mcp_server_env`
        // from the keychain entry `kask://credentials/hkask_smtp_password`.
        if !self.curator.email.mxroute_server.is_empty() {
            env.insert(
                "HKASK_MXROUTE_SERVER".to_string(),
                self.curator.email.mxroute_server.clone(),
            );
        }
        if !self.curator.email.smtp_username.is_empty() {
            env.insert(
                "HKASK_SMTP_USERNAME".to_string(),
                self.curator.email.smtp_username.clone(),
            );
            // `HKASK_CURATOR_EMAIL` defaults to `HKASK_SMTP_USERNAME` in the
            // email crate; only inject when explicitly set.
            if !self.curator.email.curator_email.is_empty() {
                env.insert(
                    "HKASK_CURATOR_EMAIL".to_string(),
                    self.curator.email.curator_email.clone(),
                );
            }
            // `HKASK_ALERT_EMAIL` defaults to `HKASK_SMTP_USERNAME` in the
            // email crate; only inject when explicitly set.
            if !self.curator.email.alert_email.is_empty() {
                env.insert(
                    "HKASK_ALERT_EMAIL".to_string(),
                    self.curator.email.alert_email.clone(),
                );
            }
        }
        if !self.curator.email.authorized_emails.is_empty() {
            env.insert(
                "HKASK_AUTHORIZED_EMAILS".to_string(),
                self.curator.email.authorized_emails.join(","),
            );
        }
        if self.curator.email.inbox_poll_interval_secs > 0 {
            env.insert(
                "HKASK_INBOX_POLL_INTERVAL_SECS".to_string(),
                self.curator.email.inbox_poll_interval_secs.to_string(),
            );
        }
        if self.curator.email.digest_interval_secs > 0 {
            env.insert(
                "HKASK_DIGEST_INTERVAL_SECS".to_string(),
                self.curator.email.digest_interval_secs.to_string(),
            );
        }

        env
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
            inbox_poll_interval_secs: c
                .inbox_poll_interval_secs
                .unwrap_or(default.inbox_poll_interval_secs),
            digest_interval_secs: c
                .digest_interval_secs
                .unwrap_or(default.digest_interval_secs),
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

impl From<KaskCodegraphSettingsContent> for KaskCodegraphSettings {
    fn from(c: KaskCodegraphSettingsContent) -> Self {
        let default = Self::default();
        Self {
            db_path: c.db_path.unwrap_or(default.db_path),
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
        // Mirrors the `dim > 0` guard in codegraph's `resolve_embedding_dim`.
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
        }
    }
}

impl From<KaskInferenceProvidersSettingsContent> for KaskInferenceProvidersSettings {
    fn from(c: KaskInferenceProvidersSettingsContent) -> Self {
        // When the user hasn't explicitly set a toggle (field is `None`),
        // auto-enable the provider if its API key is present in the process
        // environment. `from_env()` is the single source of truth for this
        // logic — `Default` returns all-false so that `KaskSettings::default()`
        // and tests remain deterministic and side-effect-free.
        let from_env = Self::from_env();
        Self {
            deepinfra_enabled: c.deepinfra_enabled.unwrap_or(from_env.deepinfra_enabled),
            openrouter_enabled: c.openrouter_enabled.unwrap_or(from_env.openrouter_enabled),
            atlascloud_enabled: c.atlascloud_enabled.unwrap_or(from_env.atlascloud_enabled),
        }
    }
}

impl From<settings_content::KaskCollabSettingsContent> for KaskCollabSettings {
    fn from(c: settings_content::KaskCollabSettingsContent) -> Self {
        let default = Self::default();
        Self {
            enabled: c.enabled.unwrap_or(default.enabled),
            database_url: c.database_url.unwrap_or(default.database_url),
            http_port: c.http_port.unwrap_or(default.http_port),
            zed_environment: c.zed_environment.unwrap_or(default.zed_environment),
            marketplace_url: c.marketplace_url.unwrap_or(default.marketplace_url),
        }
    }
}

impl From<KaskSettingsContent> for KaskSettings {
    fn from(c: KaskSettingsContent) -> Self {
        Self {
            data_dir: c.data_dir.unwrap_or_default(),
            mcp: c.mcp.map(Into::into).unwrap_or_default(),
            data_services: c.data_services.map(Into::into).unwrap_or_default(),
            curator: c.curator.map(Into::into).unwrap_or_default(),
            memory: c.memory.map(Into::into).unwrap_or_default(),
            condenser: c.condenser.map(Into::into).unwrap_or_default(),
            codegraph: c.codegraph.map(Into::into).unwrap_or_default(),
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
            inference_providers: c
                .inference_providers
                .map(Into::into)
                .unwrap_or_else(KaskInferenceProvidersSettings::from_env),
            collab: c.collab.map(Into::into).unwrap_or_default(),
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
    // "use default" (mirroring codegraph's `resolve_embedding_dim` guard).
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

    // Collab server defaults: the local collab server is enabled by default
    // with SQLite on port 3000. This pins the zero-config behavior — without
    // these defaults, the kask extensions panel hits 404 on zed.dev because
    // the deployed server doesn't have the /api/kask-skills route.
    #[test]
    fn collab_settings_default_is_enabled_with_sqlite() {
        let default = KaskCollabSettings::default();
        assert!(
            default.enabled,
            "KaskCollabSettings::default() must be enabled"
        );
        assert_eq!(default.database_url, "sqlite:kask_marketplace.db?mode=rwc");
        assert_eq!(default.http_port, 3000);
        assert_eq!(default.zed_environment, "development");
        assert_eq!(default.marketplace_url, "http://localhost:3000");
    }

    #[test]
    fn kask_settings_from_empty_content_uses_collab_defaults() {
        let settings = KaskSettings::from(KaskSettingsContent::default());
        assert!(settings.collab.enabled);
        assert_eq!(settings.collab.http_port, 3000);
        assert_eq!(settings.collab.marketplace_url, "http://localhost:3000");
    }

    #[test]
    fn kask_settings_from_present_collab_subsection_with_null_fields_uses_defaults() {
        let content = KaskSettingsContent {
            collab: Some(settings_content::KaskCollabSettingsContent {
                enabled: None,
                database_url: None,
                http_port: None,
                zed_environment: None,
                marketplace_url: None,
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert!(settings.collab.enabled);
        assert_eq!(settings.collab.http_port, 3000);
        assert_eq!(
            settings.collab.database_url,
            "sqlite:kask_marketplace.db?mode=rwc"
        );
    }

    #[test]
    fn kask_settings_from_present_collab_subsection_preserves_explicit_overrides() {
        let content = KaskSettingsContent {
            collab: Some(settings_content::KaskCollabSettingsContent {
                enabled: Some(false),
                database_url: Some("sqlite:./custom.db".into()),
                http_port: Some(4000),
                zed_environment: Some("staging".into()),
                marketplace_url: Some("https://market.example.com".into()),
            }),
            ..Default::default()
        };
        let settings = KaskSettings::from(content);
        assert!(!settings.collab.enabled);
        assert_eq!(settings.collab.database_url, "sqlite:./custom.db");
        assert_eq!(settings.collab.http_port, 4000);
        assert_eq!(settings.collab.zed_environment, "staging");
        assert_eq!(
            settings.collab.marketplace_url,
            "https://market.example.com"
        );
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
        assert!(
            !env.contains_key("HKASK_TEMPLATE_ROOT"),
            "default template_root must not be emitted"
        );
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

    // `mcp_env()` must emit env vars when a setting differs from `Default`.
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
    // (`DeepInfra/Qwen/Qwen3-Embedding-0.6B`), so the comparison must be
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
        assert!(
            !env.contains_key("HKASK_ABW_API_KEY"),
            "the ABW API key is a credential, not config — it must never appear in mcp_env()"
        );
        assert_eq!(settings.swarm.max_credits_per_dispatch, 50);
        assert!(!settings.swarm.curator_consent_default);
        assert_eq!(settings.swarm.mode, SwarmModeConfig::Abw);
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
    }

    // `KaskInferenceProvidersSettings::default()` must be pure (all-false) —
    // no env-var reads. This keeps `KaskSettings::default()` and tests
    // deterministic. The env-var auto-enable logic lives in `from_env()` and
    // `From<Content>`, not `Default`.
    #[test]
    fn inference_providers_default_is_all_false() {
        let default = KaskInferenceProvidersSettings::default();
        assert!(!default.deepinfra_enabled);
        assert!(!default.openrouter_enabled);
        assert!(!default.atlascloud_enabled);
    }

    // `KaskSettings::default()` must also have all-false inference providers,
    // since it delegates to `KaskInferenceProvidersSettings::default()`.
    #[test]
    fn kask_settings_default_inference_providers_all_false() {
        let settings = KaskSettings::default();
        assert!(!settings.inference_providers.deepinfra_enabled);
        assert!(!settings.inference_providers.openrouter_enabled);
    }

    // `from_env()` reads env vars — this test verifies it doesn't panic and
    // returns a valid struct. We can't assert specific values because the
    // test environment may or may not have API keys set.
    #[test]
    fn inference_providers_from_env_does_not_panic() {
        let _ = KaskInferenceProvidersSettings::from_env();
    }

    // Regression test for the polarity inversion in the credential-injection
    // path (now `build_mcp_server_env`). The skip check `std::env::var(env_var).is_ok()`
    // treated an empty env var (`FOO=`) as "present" and suppressed keychain
    // injection, leaving the child process with no key. The fix skips only
    // non-empty parent env vars. This test pins the `From<Content>` resolution
    // path that feeds `credential_urls_for_mcp`: when a toggle is explicitly
    // `false`, no credential URL is produced for that provider, so the polarity
    // bug cannot suppress a key that should never be injected in the first
    // place. The toggle→credential-URL gate is the upstream guard.
    #[test]
    fn credential_urls_for_mcp_omits_disabled_inference_providers() {
        let settings = KaskSettings::default();
        // All inference toggles default to false → no inference credential URLs.
        let urls = crate::inference_providers::credential_urls_for_mcp(&settings);
        let has_inference_key = urls.iter().any(|(env_var, _)| {
            matches!(
                env_var.as_str(),
                "DEEPINFRA_API_KEY" | "OPENROUTER_API_KEY" | "ATLASCLOUD_API_KEY"
            )
        });
        assert!(
            !has_inference_key,
            "disabled inference providers must not produce credential URLs — \
             this is the gate that prevents the polarity bug from suppressing \
             keys that should be injected"
        );
    }

    // When an inference provider is explicitly enabled, its credential URL
    // must appear in the MCP credential list. This is the cascade root: the
    // UI toggle writes `inference_providers.<provider>_enabled = true` →
    // `credential_urls_for_mcp` includes the URL → `build_mcp_server_env`
    // injects the keychain value as an env var → MCP server `resolve_api_key`
    // Tier 1 finds it. If any link breaks, inference fails with "API key not
    // configured".
    #[test]
    fn credential_urls_for_mcp_includes_enabled_inference_providers() {
        let mut settings = KaskSettings::default();
        settings.inference_providers.openrouter_enabled = true;
        let urls = crate::inference_providers::credential_urls_for_mcp(&settings);
        let openrouter_url = urls
            .iter()
            .find(|(env_var, _)| env_var == "OPENROUTER_API_KEY")
            .map(|(_, url)| url.clone());
        assert_eq!(
            openrouter_url.as_deref(),
            Some("kask://credentials/openrouter"),
            "enabled OpenRouter must produce its credential URL so the bridge \
             injects the keychain value into MCP server env"
        );
    }
}
