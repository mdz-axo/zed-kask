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

/// Kask-specific settings (the `"kask"` section in settings.json).
///
/// Non-secret configuration for hKask features: MCP server load set,
/// data-service toggles, curator/regulation/guard/memory settings.
/// API keys are stored in the keychain via `CredentialsProvider` (D9b).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default, RegisterSetting)]
#[setting_section = "kask"]
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

    /// Memory consolidation configuration.
    #[serde(default)]
    pub memory: KaskMemorySettings,
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
    pub overrides: std::collections::HashMap<String, bool>,
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

/// Memory consolidation configuration.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct KaskMemorySettings {
    /// Consolidation cadence in seconds (0 = disabled).
    #[serde(default = "default_consolidation_cadence")]
    pub consolidation_cadence_secs: u64,

    /// Confidence floor for memory retention (0.0–1.0).
    #[serde(default = "default_confidence_floor")]
    pub confidence_floor: f64,
}

fn default_consolidation_cadence() -> u64 {
    300
}

fn default_confidence_floor() -> f64 {
    0.3
}
