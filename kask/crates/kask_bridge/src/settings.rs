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

impl Settings for KaskSettings {
    fn from_settings(s: &settings_content::SettingsContent) -> Self {
        s.kask.clone().map(|c| c.into()).unwrap_or_default()
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
        }
    }
}
