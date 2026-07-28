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
    KaskDataServiceSettingsContent, KaskFusionSettingsContent, KaskGuardSettingsContent,
    KaskInferenceProvidersSettingsContent, KaskMcpSettingsContent, KaskMediaSettingsContent,
    KaskMemorySettingsContent, KaskModelsSettingsContent, KaskScenariosSettingsContent,
    KaskSettingsContent, KaskTrainingSettingsContent,
};

use collections::HashMap;

/// Kask-specific settings (the `"kask"` section in settings.json).
///
/// Non-secret configuration for hKask features: MCP server load set,
/// data-service toggles, curator/regulation/guard/memory/condenser settings.
/// API keys are stored in the keychain via `CredentialsProvider` (D9b).
///
/// `Default` is the single source of truth for every subsection's defaults.
/// `From<KaskSettingsContent>` delegates to each subsection's `Default` via
/// `unwrap_or(default.field)`. Do not add `#[serde(default = ...)]` attributes
/// here — `KaskSettings` is never deserialized directly (the settings system
/// deserializes `SettingsContent` and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default, RegisterSetting)]
pub struct KaskSettings {
    /// MCP server configuration — which of the 10 built-in servers to load.
    pub mcp: KaskMcpSettings,

    /// Data service toggles (non-secret — API keys are in the keychain).
    pub data_services: KaskDataServiceSettings,

    /// Curator configuration.
    pub curator: KaskCuratorSettings,

    /// Guard / regulation configuration.
    pub guard: KaskGuardSettings,

    /// Memory consolidation and recall configuration.
    pub memory: KaskMemorySettings,

    /// Condenser configuration for context management in inference threads.
    pub condenser: KaskCondenserSettings,

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

    /// Training MCP server configuration.
    pub training: KaskTrainingSettings,

    /// Multi-model fusion inference configuration.
    pub fusion: KaskFusionSettings,

    /// Kask-wide model configuration: default, embedding, and classifier models.
    pub models: KaskModelsSettings,

    /// Inference provider toggles (non-secret — API keys are in the keychain).
    pub inference_providers: KaskInferenceProvidersSettings,
}

/// MCP server load configuration.
///
/// `Default` is the single source of truth for defaults — `From<Content>` reads
/// from it via `unwrap_or(default.field)`. Do not add `#[serde(default = ...)]`
/// attributes here; `KaskSettings` is never deserialized directly (the settings
/// system deserializes `SettingsContent` and converts via `From`).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskMcpSettings {
    /// Whether to load the default MCP server set (10 servers).
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

    /// Enable fal.ai (OpenAI-compatible inference + media).
    pub fal_enabled: bool,

    /// Enable Together AI (OpenAI-compatible inference).
    pub together_enabled: bool,

    /// Enable OpenRouter (unified API for 200+ models).
    pub openrouter_enabled: bool,

    /// Enable KiloCode (unified API for 200+ models + tools).
    pub kilocode_enabled: bool,

    /// Enable Cline (open source unified API for models and tools).
    pub cline_enabled: bool,
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
            fal_enabled: std::env::var("FALAI_API_KEY").is_ok(),
            together_enabled: std::env::var("TOGETHERAI_API_KEY").is_ok(),
            openrouter_enabled: std::env::var("OPENROUTER_API_KEY").is_ok(),
            kilocode_enabled: std::env::var("KILOCODE_API_KEY").is_ok(),
            cline_enabled: std::env::var("CLINE_API_KEY").is_ok(),
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

/// Curator email configuration (non-secret fields).
/// Guard / regulation configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskGuardSettings {
    /// Direct-chat guard strategy: "buffer", "incremental", or "cascade_only".
    pub direct_chat_strategy: String,
}

impl Default for KaskGuardSettings {
    fn default() -> Self {
        Self {
            direct_chat_strategy: "cascade_only".to_string(),
        }
    }
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
            auto_compress_tool_results: true,
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
            template_root: "registry".to_string(),
        }
    }
}

fn default_embedding_model() -> String {
    hkask_inference::model_constants::DEFAULT_EMBEDDING_MODEL.to_string()
}

/// Media MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskMediaSettings {
    /// TTS model override (e.g., "fal.ai/qwen-3-tts").
    pub tts_model: String,

    /// STT model override (e.g., "fal.ai/wizper").
    pub stt_model: String,

    /// Vision model override (e.g., "KiloCode/qwen/qwen3-vl-235b-a22b-instruct").
    pub vision_model: String,

    /// Image generation model override (e.g., "fal.ai/flux-2").
    pub image_gen_model: String,
}

/// Scenarios MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskScenariosSettings {
    /// Data directory for scenario persistence. When empty, uses in-memory.
    pub data_dir: String,
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

/// Multi-model fusion inference configuration (the `"kask.fusion"` section).
///
/// When `enabled` is true, the Curator and the kask panel route inference
/// through a panel of models judged by `judge_model` according to `mode`.
/// Mirrors `hkask_types::FusionConfig` but lives in the non-secret settings
/// layer so users can edit it in the settings UI.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct KaskFusionSettings {
    /// Master toggle. When false, fusion is disabled.
    pub enabled: bool,

    /// Judge/fuser model (provider-prefixed, e.g. `"OpenRouter/z-ai/glm-5.2"`).
    /// When empty, defers to `FusionConfig::kask_default()`.
    pub judge_model: String,

    /// Comma-separated panel models (provider-prefixed). When empty, defers
    /// to `FusionConfig::kask_default()` or auto-discovery (Slice 4).
    pub panel_models: String,

    /// Judge deliberation mode: `"synthesis"` | `"best-of-n"` | `"critique"` |
    /// `"deliberation"` | `"pi"` | `"algo"`. When empty, defaults to `"synthesis"`.
    pub mode: String,

    /// Algo merge strategy when `mode == "algo"`: `"merge"` | `"vote"`.
    /// When empty, defaults to `"merge"`.
    pub algo_method: String,

    /// Comma-separated skill anchors (e.g. `"pragmatic-semantics,coding-guidelines"`).
    /// Each must match a `FusionSkill` serde rename.
    pub skills: String,

    /// Max rounds for `deliberation` mode. Default 5.
    pub max_rounds: u32,

    /// OpenRouter auto-discovery max prompt price per million tokens (USD).
    /// Default 1.0. Used by Slice 4 to filter candidate panel models.
    pub openrouter_max_price: f64,

    /// OpenRouter auto-discovery minimum intelligence index.
    /// Default 40.0. Used by Slice 4 to filter candidate panel models.
    pub openrouter_min_intelligence: f64,

    /// Coherence threshold (0.0–1.0) for measured convergence in deliberation
    /// mode. When set, the orchestrator computes epistemic tension ξ and
    /// coherence Γ from panel response embeddings; if Γ exceeds this threshold,
    /// an advisory "measured convergence" signal is emitted. Empty/disabled
    /// by default — requires an embedding API key (`DEEPINFRA_API_KEY` or `OPENROUTER_API_KEY`).
    pub coherence_threshold: Option<f64>,

    /// Enable query-complexity-based panel sizing. When `true`, simple queries
    /// dispatch fewer panel models (1 for Simple, 2 for Medium, all for Complex).
    /// Default: `false`.
    pub panel_sizing_enabled: bool,

    /// Enable substrate-aware degradation. When `true`, panel size is reduced
    /// under high latency pressure. Default: `false`.
    pub pressure_adaptive_enabled: bool,
}

impl Default for KaskFusionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            judge_model: String::default(),
            panel_models: String::default(),
            mode: "synthesis".to_string(),
            algo_method: "merge".to_string(),
            skills: String::default(),
            max_rounds: 5,
            openrouter_max_price: 1.0,
            openrouter_min_intelligence: 40.0,
            coherence_threshold: None,
            panel_sizing_enabled: false,
            pressure_adaptive_enabled: false,
        }
    }
}

/// Kask-wide model configuration.
///
/// Provider-prefixed model names that override the kask built-in defaults.
/// When a field is empty, kask falls back to its default model selection
/// (typically the zed `agent.default_model` or the fusion judge model).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskModelsSettings {
    /// Default inference model (provider-prefixed, e.g. `"openrouter/z-ai/glm-5.2"`).
    /// When set, overrides the kask default for Curator, skill cascade, and
    /// kask panel inference (unless fusion is enabled, which takes precedence).
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
    pub const DEFAULT_INFERENCE_MODEL: &'static str = "openrouter/z-ai/glm-5.2";

    /// The kask default embedding model.
    pub const DEFAULT_EMBEDDING_MODEL: &'static str = "openrouter/z-ai/glm-5.2";

    /// The kask default classifier model.
    pub const DEFAULT_CLASSIFIER_MODEL: &'static str = "openrouter/z-ai/glm-5.2";

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

    /// Resolve the effective embedding model, falling back to the kask default.
    #[must_use]
    pub fn effective_embedding_model(&self) -> &str {
        if self.embedding_model.trim().is_empty() {
            Self::DEFAULT_EMBEDDING_MODEL
        } else {
            &self.embedding_model
        }
    }

    /// Resolve the effective classifier model, falling back to the kask default.
    #[must_use]
    pub fn effective_classifier_model(&self) -> &str {
        if self.classifier_model.trim().is_empty() {
            Self::DEFAULT_CLASSIFIER_MODEL
        } else {
            &self.classifier_model
        }
    }
}

impl Settings for KaskSettings {
    fn from_settings(s: &settings_content::SettingsContent) -> Self {
        s.kask.clone().map(|c| c.into()).unwrap_or_default()
    }
}

impl KaskFusionSettings {
    /// Convert the settings-layer representation into the runtime `FusionConfig`.
    ///
    /// Returns `None` when fusion is disabled (`enabled == false`) or when the
    /// panel models string fails to parse into a non-empty panel.
    ///
    /// When `judge_model` or `panel_models` are empty, falls back to
    /// `FusionConfig::kask_default()` so users can enable fusion with just the
    /// master toggle and get sensible defaults.
    #[must_use]
    pub fn to_fusion_config(&self) -> Option<hkask_types::fusion::FusionConfig> {
        if !self.enabled {
            return None;
        }

        // Parse mode (fall back to synthesis on unknown values).
        let mode = self
            .mode
            .parse::<hkask_types::fusion::FusionMode>()
            .unwrap_or_default();

        // Parse algo method (fall back to merge on unknown values).
        let algo_method = self
            .algo_method
            .parse::<hkask_types::fusion::AlgoMethod>()
            .unwrap_or_default();

        // Parse skills — silently drop unknown anchors.
        let skills: Vec<hkask_types::fusion::FusionSkill> = self
            .skills
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();

        // Start from kask_default so empty judge/panel fields get sensible
        // defaults rather than producing an invalid config.
        let mut config = hkask_types::fusion::FusionConfig::kask_default();
        if !self.judge_model.trim().is_empty() {
            config.judge = self.judge_model.trim().to_string();
        }
        if !self.panel_models.trim().is_empty() {
            let panel: Vec<String> = self
                .panel_models
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if let Some(non_empty) = hkask_types::fusion::NonEmptyVec::from_vec(panel) {
                config.panel = non_empty;
            }
        }
        config.mode = mode;
        config.algo_method = algo_method;
        config.skills = skills;
        config.max_rounds = self.max_rounds;
        config.coherence_threshold = self.coherence_threshold;
        config.panel_sizing_enabled = self.panel_sizing_enabled;
        config.pressure_adaptive_enabled = self.pressure_adaptive_enabled;
        Some(config)
    }
}

impl KaskSettings {
    /// Build the environment variable map for MCP server child processes.
    ///
    /// Translates all kask settings into the env vars that MCP servers read
    /// at startup. Called by the composition root before `start_server_with_env`.
    /// Only non-empty/non-default values are included — MCP servers have their
    /// own fallback defaults for unset env vars.
    pub fn mcp_env(&self) -> std::collections::HashMap<String, String> {
        let mut env = std::collections::HashMap::new();

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
        if !self.companies.transactions_dir.is_empty() {
            env.insert(
                "HKASK_TRANSACTIONS_DIR".to_string(),
                self.companies.transactions_dir.clone(),
            );
        }

        // ── Corpus ──
        if self.corpus.embedding_dim != corpus_default.embedding_dim {
            env.insert(
                "HKASK_EMBEDDING_DIM".to_string(),
                self.corpus.embedding_dim.to_string(),
            );
        }
        if !self.corpus.embedding_model.is_empty() {
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
        // The SMTP password is injected separately by `mcp_env_with_credentials`
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

    /// Build the environment variable map for MCP server child processes,
    /// including API keys resolved from zed's `CredentialsProvider` keychain.
    ///
    /// This bridges the two keychain namespaces: the kask settings UI writes
    /// keys via zed's `CredentialsProvider` (under `kask://credentials/<key>`),
    /// while MCP servers read env vars / hKask's `Keychain` (service "hkask").
    /// This function reads from zed's keychain and injects the values as env
    /// vars so MCP servers find them via `std::env::var`.
    ///
    /// `credential_urls` is a list of `(env_var_name, keychain_url)` pairs to read.
    /// The composition root builds this from the enabled data services and
    /// inference providers via `credential_urls_for_mcp`.
    pub async fn mcp_env_with_credentials(
        &self,
        credential_urls: &[(String, String)],
        credentials_provider: &dyn credentials_provider::CredentialsProvider,
        cx: &gpui::AsyncApp,
    ) -> std::collections::HashMap<String, String> {
        let mut env = self.mcp_env();
        for (env_var, url) in credential_urls {
            // Don't override env vars already set in the process environment —
            // the operator's shell takes precedence.
            if std::env::var(env_var).is_ok() {
                continue;
            }
            if let Ok(Some((_username, password))) =
                credentials_provider.read_credentials(url, cx).await
                && let Ok(value) = String::from_utf8(password)
                && !value.is_empty()
            {
                env.insert(env_var.clone(), value);
            }
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

impl From<KaskGuardSettingsContent> for KaskGuardSettings {
    fn from(c: KaskGuardSettingsContent) -> Self {
        let default = Self::default();
        Self {
            direct_chat_strategy: c
                .direct_chat_strategy
                .unwrap_or(default.direct_chat_strategy),
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
            ocr_concurrency: c.ocr_concurrency.unwrap_or(default.ocr_concurrency),
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

impl From<KaskScenariosSettingsContent> for KaskScenariosSettings {
    fn from(c: KaskScenariosSettingsContent) -> Self {
        let default = Self::default();
        Self {
            data_dir: c.data_dir.unwrap_or(default.data_dir),
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

impl From<KaskFusionSettingsContent> for KaskFusionSettings {
    fn from(c: KaskFusionSettingsContent) -> Self {
        let default = Self::default();
        Self {
            enabled: c.enabled.unwrap_or(default.enabled),
            judge_model: c.judge_model.unwrap_or(default.judge_model),
            panel_models: c.panel_models.unwrap_or(default.panel_models),
            mode: c.mode.unwrap_or(default.mode),
            algo_method: c.algo_method.unwrap_or(default.algo_method),
            skills: c.skills.unwrap_or(default.skills),
            max_rounds: c.max_rounds.unwrap_or(default.max_rounds),
            openrouter_max_price: c
                .openrouter_max_price
                .unwrap_or(default.openrouter_max_price),
            openrouter_min_intelligence: c
                .openrouter_min_intelligence
                .unwrap_or(default.openrouter_min_intelligence),
            coherence_threshold: c.coherence_threshold,
            panel_sizing_enabled: c
                .panel_sizing_enabled
                .unwrap_or(default.panel_sizing_enabled),
            pressure_adaptive_enabled: c
                .pressure_adaptive_enabled
                .unwrap_or(default.pressure_adaptive_enabled),
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
            fal_enabled: c.fal_enabled.unwrap_or(from_env.fal_enabled),
            together_enabled: c.together_enabled.unwrap_or(from_env.together_enabled),
            openrouter_enabled: c.openrouter_enabled.unwrap_or(from_env.openrouter_enabled),
            kilocode_enabled: c.kilocode_enabled.unwrap_or(from_env.kilocode_enabled),
            cline_enabled: c.cline_enabled.unwrap_or(from_env.cline_enabled),
        }
    }
}

impl From<KaskSettingsContent> for KaskSettings {
    fn from(c: KaskSettingsContent) -> Self {
        Self {
            mcp: c.mcp.map(Into::into).unwrap_or_default(),
            data_services: c.data_services.map(Into::into).unwrap_or_default(),
            curator: c.curator.map(Into::into).unwrap_or_default(),
            guard: c.guard.map(Into::into).unwrap_or_default(),
            memory: c.memory.map(Into::into).unwrap_or_default(),
            condenser: c.condenser.map(Into::into).unwrap_or_default(),
            codegraph: c.codegraph.map(Into::into).unwrap_or_default(),
            companies: c.companies.map(Into::into).unwrap_or_default(),
            corpus: c.corpus.map(Into::into).unwrap_or_default(),
            media: c.media.map(Into::into).unwrap_or_default(),
            scenarios: c.scenarios.map(Into::into).unwrap_or_default(),
            training: c.training.map(Into::into).unwrap_or_default(),
            fusion: c.fusion.map(Into::into).unwrap_or_default(),
            models: c.models.map(Into::into).unwrap_or_default(),
            inference_providers: c
                .inference_providers
                .map(Into::into)
                .unwrap_or_else(KaskInferenceProvidersSettings::from_env),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    // treated all 10 servers as disabled, registering nothing. The manual Default
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
    fn condenser_settings_default_auto_compress_is_true() {
        assert!(
            KaskCondenserSettings::default().auto_compress_tool_results,
            "KaskCondenserSettings::default() must return auto_compress_tool_results: true"
        );
    }

    #[test]
    fn fusion_settings_default_mode_and_algo_method() {
        let default = KaskFusionSettings::default();
        assert_eq!(default.mode, "synthesis");
        assert_eq!(default.algo_method, "merge");
        assert_eq!(default.max_rounds, 5);
        assert_eq!(default.openrouter_max_price, 1.0);
        assert_eq!(default.openrouter_min_intelligence, 40.0);
    }

    #[test]
    fn guard_settings_default_strategy_is_cascade_only() {
        assert_eq!(
            KaskGuardSettings::default().direct_chat_strategy,
            "cascade_only"
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
        assert_eq!(settings.guard.direct_chat_strategy, "cascade_only");
        assert!(settings.memory.auto_inject);
        assert_eq!(settings.memory.consolidation_cadence_secs, 300);
        assert!(settings.condenser.auto_compress_tool_results);
        assert_eq!(settings.condenser.profile, "normal");
        assert_eq!(settings.corpus.embedding_dim, 1024);
        assert_eq!(settings.fusion.mode, "synthesis");
        assert_eq!(settings.fusion.max_rounds, 5);
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

    // `mcp_env()` must not emit env vars for settings that match `Default`.
    // Previously `mcp_env()` compared against inlined magic numbers (1024, 4,
    // 0.05, 0.15, 0.10, "registry", 5) that duplicated `Default` values. If
    // `Default` changed, the comparison would drift and emit env vars for the
    // default case. Now `mcp_env()` reads from `Default::default()`, so changing
    // `Default` automatically updates the comparison. This test pins that: a
    // `KaskSettings::default()` (all defaults) produces an empty `mcp_env()`.
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
            !env.contains_key("HKASK_CONDENSE_SALIENCY_WINDOW"),
            "default saliency_window must not be emitted"
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

    // `KaskInferenceProvidersSettings::default()` must be pure (all-false) —
    // no env-var reads. This keeps `KaskSettings::default()` and tests
    // deterministic. The env-var auto-enable logic lives in `from_env()` and
    // `From<Content>`, not `Default`.
    #[test]
    fn inference_providers_default_is_all_false() {
        let default = KaskInferenceProvidersSettings::default();
        assert!(!default.deepinfra_enabled);
        assert!(!default.fal_enabled);
        assert!(!default.together_enabled);
        assert!(!default.openrouter_enabled);
        assert!(!default.kilocode_enabled);
        assert!(!default.cline_enabled);
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
}
