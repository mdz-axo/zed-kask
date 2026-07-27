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
use settings_content::KaskSettingsContent;

use collections::HashMap;

/// Kask-specific settings (the `"kask"` section in settings.json).
///
/// Non-secret configuration for hKask features: MCP server load set,
/// data-service toggles, curator/regulation/guard/memory/condenser settings.
/// API keys are stored in the keychain via `CredentialsProvider` (D9b).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default, RegisterSetting)]
pub struct KaskSettings {
    /// MCP server configuration — which of the 11 built-in servers to load.
    #[serde(default)]
    pub mcp: KaskMcpSettings,

    /// Data service toggles (non-secret — API keys are in the keychain).
    #[serde(default)]
    pub data_services: KaskDataServiceSettings,

    /// Curator configuration.
    #[serde(default)]
    pub curator: KaskCuratorSettings,

    /// Guard / regulation configuration.
    #[serde(default)]
    pub guard: KaskGuardSettings,

    /// Memory consolidation and recall configuration.
    #[serde(default)]
    pub memory: KaskMemorySettings,

    /// Condenser configuration for context management in inference threads.
    #[serde(default)]
    pub condenser: KaskCondenserSettings,

    /// Codegraph MCP server configuration.
    #[serde(default)]
    pub codegraph: KaskCodegraphSettings,

    /// Companies MCP server configuration.
    #[serde(default)]
    pub companies: KaskCompaniesSettings,

    /// Corpus MCP server configuration.
    #[serde(default)]
    pub corpus: KaskCorpusSettings,

    /// Media MCP server configuration.
    #[serde(default)]
    pub media: KaskMediaSettings,

    /// Scenarios MCP server configuration.
    #[serde(default)]
    pub scenarios: KaskScenariosSettings,

    /// Training MCP server configuration.
    #[serde(default)]
    pub training: KaskTrainingSettings,

    /// Multi-model fusion inference configuration.
    #[serde(default)]
    pub fusion: KaskFusionSettings,

    /// Inference provider toggles (non-secret — API keys are in the keychain).
    #[serde(default)]
    pub inference_providers: KaskInferenceProvidersSettings,
}

/// MCP server load configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskMcpSettings {
    /// Whether to load the default MCP server set (11 servers).
    /// Set to `false` to disable all kask MCP servers.
    #[serde(default = "default_true")]
    pub load_default: bool,

    /// Per-server overrides (e.g., `"curator": false` to unload the curator MCP).
    #[serde(default)]
    pub overrides: HashMap<String, bool>,
}

fn default_true() -> bool {
    true
}

/// Data service toggles. API keys are in the keychain, not here.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskDataServiceSettings {
    /// Enable EODHD (historical price data).
    #[serde(default)]
    pub eodhd_enabled: bool,

    /// Enable FMP (Financial Modeling Prep).
    #[serde(default)]
    pub fmp_enabled: bool,

    /// Enable Exa (research search).
    #[serde(default)]
    pub exa_enabled: bool,

    /// Enable Tavily (research search).
    #[serde(default)]
    pub tavily_enabled: bool,

    /// Enable Brave Search.
    #[serde(default)]
    pub brave_enabled: bool,

    /// Enable RunPod (GPU cloud for training).
    #[serde(default)]
    pub runpod_enabled: bool,

    /// Enable Nebius (GPU cloud for training).
    #[serde(default)]
    pub nebius_enabled: bool,
}

/// Inference provider toggles. API keys are in the keychain, not here.
///
/// When a provider is enabled, the composition root writes an
/// `openai_compatible.<provider_id>` entry to settings.json so zed's
/// OpenAI-compatible provider machinery registers it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskInferenceProvidersSettings {
    /// Enable DeepInfra (OpenAI-compatible inference).
    #[serde(default)]
    pub deepinfra_enabled: bool,

    /// Enable fal.ai (OpenAI-compatible inference + media).
    #[serde(default)]
    pub fal_enabled: bool,

    /// Enable Together AI (OpenAI-compatible inference).
    #[serde(default)]
    pub together_enabled: bool,

    /// Enable OpenRouter (unified API for 200+ models).
    #[serde(default)]
    pub openrouter_enabled: bool,

    /// Enable KiloCode (unified API for 200+ models + tools).
    #[serde(default)]
    pub kilocode_enabled: bool,

    /// Enable Cline (open source unified API for models and tools).
    #[serde(default)]
    pub cline_enabled: bool,
}

/// Curator configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskCuratorSettings {
    /// Whether the Curator agent is always-on (runs regulation loops in background).
    #[serde(default = "default_true")]
    pub always_on: bool,

    /// Algedonic signal threshold (0.0–1.0).
    #[serde(default = "default_algedonic_threshold")]
    pub algedonic_threshold: f64,
}

fn default_algedonic_threshold() -> f64 {
    0.8
}

/// Guard / regulation configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskGuardSettings {
    /// Direct-chat guard strategy: "buffer", "incremental", or "cascade_only".
    #[serde(default = "default_guard_strategy")]
    pub direct_chat_strategy: String,
}

fn default_guard_strategy() -> String {
    "cascade_only".to_string()
}

/// Memory consolidation and recall configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskMemorySettings {
    /// Consolidation cadence in seconds (0 = disabled).
    #[serde(default = "default_consolidation_cadence")]
    pub consolidation_cadence_secs: u64,

    /// Confidence floor for memory retention (0.0–1.0).
    #[serde(default = "default_confidence_floor")]
    pub confidence_floor: f64,

    /// Maximum number of memory snippets to retrieve for context injection.
    #[serde(default = "default_recall_limit")]
    pub recall_limit: u32,

    /// Minimum confidence for a memory to be injected into context (0.0–1.0).
    #[serde(default = "default_recall_min_confidence")]
    pub recall_min_confidence: f64,

    /// Whether to automatically inject recalled memories into prompts.
    #[serde(default = "default_true")]
    pub auto_inject: bool,
}

fn default_consolidation_cadence() -> u64 {
    300
}

fn default_confidence_floor() -> f64 {
    0.3
}

fn default_recall_limit() -> u32 {
    5
}

fn default_recall_min_confidence() -> f64 {
    0.3
}

/// Condenser configuration for context management in inference threads.
///
/// Controls how tool results are compressed before entering the message
/// history, and what compression profile to use.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskCondenserSettings {
    /// Compression profile: "heavy", "normal", "soft", or "light".
    /// - Heavy: 10% retention, 30 max lines — aggressive compression
    /// - Normal: 20% retention, 80 max lines — balanced
    /// - Soft: 60% retention, 200 max lines — light touch
    /// - Light: 95% retention, no max — near-passthrough
    #[serde(default = "default_condenser_profile")]
    pub profile: String,

    /// Whether to automatically compress tool results before they enter
    /// the message history. When false, tool results are stored verbatim.
    #[serde(default = "default_true")]
    pub auto_compress_tool_results: bool,

    /// Persona keywords for saliency scoring (comma-separated in settings.json).
    /// Used by the condenser's word_rank algorithm to prioritize lines
    /// relevant to the user's domain.
    #[serde(default)]
    pub persona_keywords: Vec<String>,

    /// Saliency window multiplier for thread summarization.
    /// Controls the max_tokens budget: saliency_window * 100, clamped [150, 2000].
    #[serde(default = "default_saliency_window")]
    pub saliency_window: u32,
}

fn default_condenser_profile() -> String {
    "normal".to_string()
}

fn default_saliency_window() -> u32 {
    5
}

/// Codegraph MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskCodegraphSettings {
    /// Database path for the codegraph store. When empty, uses in-memory.
    #[serde(default)]
    pub db_path: String,
}

/// Companies MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskCompaniesSettings {
    /// Chronic staleness threshold in days for superforecasting learning state.
    #[serde(default)]
    pub chronic_staleness_days: u32,

    /// Fermi decomposition defaults as JSON (growth + margin question arrays).
    /// When empty, uses hardcoded defaults.
    #[serde(default)]
    pub fermi_defaults: String,

    /// Directory for portfolio transaction files (CSV/JSON). The portfolio
    /// dashboard auto-loads any new files from this directory. When empty,
    /// defaults to `<kask_data_dir>/transactions/`.
    #[serde(default)]
    pub transactions_dir: String,
}

/// Corpus MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskCorpusSettings {
    /// Embedding dimensionality (must match the embedding model's output).
    #[serde(default = "default_embedding_dim")]
    pub embedding_dim: u32,

    /// Embedding model override. When empty, defers to the kask router default
    /// (`hkask_inference::model_constants::DEFAULT_EMBEDDING_MODEL`).
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,

    /// OCR concurrency — number of pages sent to the vision model in parallel.
    #[serde(default = "default_ocr_concurrency")]
    pub ocr_concurrency: u32,

    /// OCR simple threshold (0.0–1.0). Pages below this are processed simply.
    #[serde(default = "default_ocr_simple_max")]
    pub ocr_simple_max: f64,

    /// OCR moderate threshold (0.0–1.0). Pages above simple but below this
    /// are processed with moderate pipeline.
    #[serde(default = "default_ocr_moderate_max")]
    pub ocr_moderate_max: f64,

    /// OCR moderate sample rate (0.0–1.0). Fraction of moderate pages sampled.
    #[serde(default = "default_ocr_sample_rate")]
    pub ocr_sample_rate: f64,

    /// Whether OCR tuneable mode is enabled.
    #[serde(default = "default_true")]
    pub ocr_tuneable: bool,

    /// Template root directory for Jinja2 templates.
    #[serde(default = "default_template_root")]
    pub template_root: String,
}

fn default_embedding_dim() -> u32 {
    1024
}

fn default_embedding_model() -> String {
    hkask_inference::model_constants::DEFAULT_EMBEDDING_MODEL.to_string()
}

fn default_ocr_concurrency() -> u32 {
    4
}

fn default_ocr_simple_max() -> f64 {
    0.05
}

fn default_ocr_moderate_max() -> f64 {
    0.15
}

fn default_ocr_sample_rate() -> f64 {
    0.10
}

fn default_template_root() -> String {
    "registry".to_string()
}

/// Media MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskMediaSettings {
    /// TTS model override (e.g., "fal.ai/qwen-3-tts").
    #[serde(default)]
    pub tts_model: String,

    /// STT model override (e.g., "fal.ai/wizper").
    #[serde(default)]
    pub stt_model: String,

    /// Vision model override (e.g., "KiloCode/qwen/qwen3-vl-235b-a22b-instruct").
    #[serde(default)]
    pub vision_model: String,

    /// Image generation model override (e.g., "fal.ai/flux-2").
    #[serde(default)]
    pub image_gen_model: String,
}

/// Scenarios MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskScenariosSettings {
    /// Data directory for scenario persistence. When empty, uses in-memory.
    #[serde(default)]
    pub data_dir: String,
}

/// Training MCP server configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskTrainingSettings {
    /// Training host override: "deepinfra", "nebius", or "runpod".
    /// When empty, auto-detects from available API keys.
    #[serde(default)]
    pub host: String,

    /// Cache directory for dataset pipeline. When empty, uses the
    /// agent adapters directory.
    #[serde(default)]
    pub cache_dir: String,
}

/// Multi-model fusion inference configuration (the `"kask.fusion"` section).
///
/// When `enabled` is true, the Curator and the kask panel route inference
/// through a panel of models judged by `judge_model` according to `mode`.
/// Mirrors `hkask_types::FusionConfig` but lives in the non-secret settings
/// layer so users can edit it in the settings UI.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskFusionSettings {
    /// Master toggle. When false, fusion is disabled.
    #[serde(default)]
    pub enabled: bool,

    /// Judge/fuser model (provider-prefixed, e.g. `"OpenRouter/z-ai/glm-5.2"`).
    /// When empty, defers to `FusionConfig::kask_default()`.
    #[serde(default)]
    pub judge_model: String,

    /// Comma-separated panel models (provider-prefixed). When empty, defers
    /// to `FusionConfig::kask_default()` or auto-discovery (Slice 4).
    #[serde(default)]
    pub panel_models: String,

    /// Judge deliberation mode: `"synthesis"` | `"best-of-n"` | `"critique"` |
    /// `"deliberation"` | `"pi"` | `"algo"`. When empty, defaults to `"synthesis"`.
    #[serde(default = "default_fusion_mode")]
    pub mode: String,

    /// Algo merge strategy when `mode == "algo"`: `"merge"` | `"vote"`.
    /// When empty, defaults to `"merge"`.
    #[serde(default = "default_algo_method")]
    pub algo_method: String,

    /// Comma-separated skill anchors (e.g. `"pragmatic-semantics,coding-guidelines"`).
    /// Each must match a `FusionSkill` serde rename.
    #[serde(default)]
    pub skills: String,

    /// Max rounds for `deliberation` mode. Default 5.
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u32,

    /// OpenRouter auto-discovery max prompt price per million tokens (USD).
    /// Default 1.0. Used by Slice 4 to filter candidate panel models.
    #[serde(default = "default_openrouter_max_price")]
    pub openrouter_max_price: f64,

    /// OpenRouter auto-discovery minimum intelligence index.
    /// Default 40.0. Used by Slice 4 to filter candidate panel models.
    #[serde(default = "default_openrouter_min_intelligence")]
    pub openrouter_min_intelligence: f64,

    /// Coherence threshold (0.0–1.0) for measured convergence in deliberation
    /// mode. When set, the orchestrator computes epistemic tension ξ and
    /// coherence Γ from panel response embeddings; if Γ exceeds this threshold,
    /// an advisory "measured convergence" signal is emitted. Empty/disabled
    /// by default — requires an embedding API key (`DEEPINFRA_API_KEY` or `OPENROUTER_API_KEY`).
    #[serde(default)]
    pub coherence_threshold: Option<f64>,

    /// Enable query-complexity-based panel sizing. When `true`, simple queries
    /// dispatch fewer panel models (1 for Simple, 2 for Medium, all for Complex).
    /// Default: `false`.
    #[serde(default)]
    pub panel_sizing_enabled: bool,

    /// Enable substrate-aware degradation. When `true`, panel size is reduced
    /// under high latency pressure. Default: `false`.
    #[serde(default)]
    pub pressure_adaptive_enabled: bool,
}

fn default_fusion_mode() -> String {
    "synthesis".to_string()
}

fn default_algo_method() -> String {
    "merge".to_string()
}

fn default_max_rounds() -> u32 {
    5
}

fn default_openrouter_max_price() -> f64 {
    1.0
}

fn default_openrouter_min_intelligence() -> f64 {
    40.0
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

        // ── Condenser ──
        if !self.condenser.persona_keywords.is_empty() {
            env.insert(
                "HKASK_CONDENSER_PERSONA_KEYWORDS".to_string(),
                self.condenser.persona_keywords.join(","),
            );
        }
        if self.condenser.saliency_window != 5 {
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
        if self.corpus.embedding_dim != 1024 {
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
        if self.corpus.ocr_concurrency != 4 {
            env.insert(
                "HKASK_OCR_CONCURRENCY".to_string(),
                self.corpus.ocr_concurrency.to_string(),
            );
        }
        if (self.corpus.ocr_simple_max - 0.05).abs() > f64::EPSILON {
            env.insert(
                "HKASK_OCR_SIMPLE_MAX".to_string(),
                self.corpus.ocr_simple_max.to_string(),
            );
        }
        if (self.corpus.ocr_moderate_max - 0.15).abs() > f64::EPSILON {
            env.insert(
                "HKASK_OCR_MODERATE_MAX".to_string(),
                self.corpus.ocr_moderate_max.to_string(),
            );
        }
        if (self.corpus.ocr_sample_rate - 0.10).abs() > f64::EPSILON {
            env.insert(
                "HKASK_OCR_SAMPLE_RATE".to_string(),
                self.corpus.ocr_sample_rate.to_string(),
            );
        }
        if !self.corpus.ocr_tuneable {
            env.insert("HKASK_OCR_TUNEABLE".to_string(), "false".to_string());
        }
        if self.corpus.template_root != "registry" {
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

impl From<KaskSettingsContent> for KaskSettings {
    fn from(c: KaskSettingsContent) -> Self {
        Self {
            mcp: c
                .mcp
                .map(|m| KaskMcpSettings {
                    load_default: m.load_default.unwrap_or(true),
                    overrides: m.overrides,
                })
                .unwrap_or_default(),
            data_services: c
                .data_services
                .map(|d| KaskDataServiceSettings {
                    eodhd_enabled: d.eodhd_enabled.unwrap_or(false),
                    fmp_enabled: d.fmp_enabled.unwrap_or(false),
                    exa_enabled: d.exa_enabled.unwrap_or(false),
                    tavily_enabled: d.tavily_enabled.unwrap_or(false),
                    brave_enabled: d.brave_enabled.unwrap_or(false),
                    runpod_enabled: d.runpod_enabled.unwrap_or(false),
                    nebius_enabled: d.nebius_enabled.unwrap_or(false),
                })
                .unwrap_or_default(),
            curator: c
                .curator
                .map(|c| KaskCuratorSettings {
                    always_on: c.always_on.unwrap_or(true),
                    algedonic_threshold: c.algedonic_threshold.unwrap_or(0.8),
                })
                .unwrap_or_default(),
            guard: c
                .guard
                .map(|g| KaskGuardSettings {
                    direct_chat_strategy: g
                        .direct_chat_strategy
                        .unwrap_or_else(|| "cascade_only".to_string()),
                })
                .unwrap_or_default(),
            memory: c
                .memory
                .map(|m| KaskMemorySettings {
                    consolidation_cadence_secs: m.consolidation_cadence_secs.unwrap_or(300),
                    confidence_floor: m.confidence_floor.unwrap_or(0.3),
                    recall_limit: m.recall_limit.unwrap_or(5),
                    recall_min_confidence: m.recall_min_confidence.unwrap_or(0.3),
                    auto_inject: m.auto_inject.unwrap_or(true),
                })
                .unwrap_or_default(),
            condenser: c
                .condenser
                .map(|c| KaskCondenserSettings {
                    profile: c.profile.unwrap_or_else(|| "normal".to_string()),
                    auto_compress_tool_results: c.auto_compress_tool_results.unwrap_or(true),
                    persona_keywords: c.persona_keywords.unwrap_or_default(),
                    saliency_window: c.saliency_window.unwrap_or(5),
                })
                .unwrap_or_default(),
            codegraph: c
                .codegraph
                .map(|cg| KaskCodegraphSettings {
                    db_path: cg.db_path.unwrap_or_default(),
                })
                .unwrap_or_default(),
            companies: c
                .companies
                .map(|cm| KaskCompaniesSettings {
                    chronic_staleness_days: cm.chronic_staleness_days.unwrap_or(0),
                    fermi_defaults: cm.fermi_defaults.unwrap_or_default(),
                    transactions_dir: cm.transactions_dir.unwrap_or_default(),
                })
                .unwrap_or_default(),
            corpus: c
                .corpus
                .map(|cp| KaskCorpusSettings {
                    embedding_dim: cp.embedding_dim.unwrap_or(1024),
                    embedding_model: cp.embedding_model.unwrap_or_else(default_embedding_model),
                    ocr_concurrency: cp.ocr_concurrency.unwrap_or(4),
                    ocr_simple_max: cp.ocr_simple_max.unwrap_or(0.05),
                    ocr_moderate_max: cp.ocr_moderate_max.unwrap_or(0.15),
                    ocr_sample_rate: cp.ocr_sample_rate.unwrap_or(0.10),
                    ocr_tuneable: cp.ocr_tuneable.unwrap_or(true),
                    template_root: cp.template_root.unwrap_or_else(|| "registry".to_string()),
                })
                .unwrap_or_default(),
            media: c
                .media
                .map(|m| KaskMediaSettings {
                    tts_model: m.tts_model.unwrap_or_default(),
                    stt_model: m.stt_model.unwrap_or_default(),
                    vision_model: m.vision_model.unwrap_or_default(),
                    image_gen_model: m.image_gen_model.unwrap_or_default(),
                })
                .unwrap_or_default(),
            scenarios: c
                .scenarios
                .map(|s| KaskScenariosSettings {
                    data_dir: s.data_dir.unwrap_or_default(),
                })
                .unwrap_or_default(),
            training: c
                .training
                .map(|t| KaskTrainingSettings {
                    host: t.host.unwrap_or_default(),
                    cache_dir: t.cache_dir.unwrap_or_default(),
                })
                .unwrap_or_default(),
            fusion: c
                .fusion
                .map(|f| KaskFusionSettings {
                    enabled: f.enabled.unwrap_or(false),
                    judge_model: f.judge_model.unwrap_or_default(),
                    panel_models: f.panel_models.unwrap_or_default(),
                    mode: f.mode.unwrap_or_else(|| "synthesis".to_string()),
                    algo_method: f.algo_method.unwrap_or_else(|| "merge".to_string()),
                    skills: f.skills.unwrap_or_default(),
                    max_rounds: f.max_rounds.unwrap_or(5),
                    openrouter_max_price: f.openrouter_max_price.unwrap_or(1.0),
                    openrouter_min_intelligence: f.openrouter_min_intelligence.unwrap_or(40.0),
                    coherence_threshold: f.coherence_threshold,
                    panel_sizing_enabled: f.panel_sizing_enabled.unwrap_or(false),
                    pressure_adaptive_enabled: f.pressure_adaptive_enabled.unwrap_or(false),
                })
                .unwrap_or_default(),
            inference_providers: c
                .inference_providers
                .map(|ip| KaskInferenceProvidersSettings {
                    deepinfra_enabled: ip
                        .deepinfra_enabled
                        .unwrap_or_else(|| std::env::var("DEEPINFRA_API_KEY").is_ok()),
                    fal_enabled: ip
                        .fal_enabled
                        .unwrap_or_else(|| std::env::var("FALAI_API_KEY").is_ok()),
                    together_enabled: ip
                        .together_enabled
                        .unwrap_or_else(|| std::env::var("TOGETHERAI_API_KEY").is_ok()),
                    openrouter_enabled: ip
                        .openrouter_enabled
                        .unwrap_or_else(|| std::env::var("OPENROUTER_API_KEY").is_ok()),
                    kilocode_enabled: ip
                        .kilocode_enabled
                        .unwrap_or_else(|| std::env::var("KILOCODE_API_KEY").is_ok()),
                    cline_enabled: ip
                        .cline_enabled
                        .unwrap_or_else(|| std::env::var("CLINE_API_KEY").is_ok()),
                })
                .unwrap_or_else(|| KaskInferenceProvidersSettings {
                    deepinfra_enabled: std::env::var("DEEPINFRA_API_KEY").is_ok(),
                    fal_enabled: std::env::var("FALAI_API_KEY").is_ok(),
                    together_enabled: std::env::var("TOGETHERAI_API_KEY").is_ok(),
                    openrouter_enabled: std::env::var("OPENROUTER_API_KEY").is_ok(),
                    kilocode_enabled: std::env::var("KILOCODE_API_KEY").is_ok(),
                    cline_enabled: std::env::var("CLINE_API_KEY").is_ok(),
                }),
        }
    }
}
