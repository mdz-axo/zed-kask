//! Inference configuration for the IPC bridge facade — base URLs, default
//! model, and provider-prefix routing. The direct-HTTP provider backends were
//! removed when inference routing moved to the zed IPC bridge; these settings
//! now feed model-listing metadata only.
//!
//! # Environment Variables
//!
//! - `OPENROUTER_BASE_URL` / `OPENROUTER_API_KEY` — OpenRouter (cloud, required)
//! - `OLLAMA_BASE_URL` / `OLLAMA_API_KEY` — Ollama (local; key optional, header ignored)
//! - `RUNPOD_API_KEY` / `RUNPOD_BASE_URL` or `RUNPOD_TEMPLATE_ID` — RunPod (vision/OCR only)
//! - `HKASK_DEFAULT_PROVIDER` — default provider for unprefixed models (RunPod, OpenRouter, ollama; default: OpenRouter)
//! - `HKASK_DEFAULT_MODEL` — default model (injected from the visible
//!   `kask.models.default_model` setting; NO code-constant fallback — unset
//!   is a typed error at the call site, per the operator's no-hidden-models
//!   spec)
//!
//! # API Key Resolution
//!
//! Provider API keys are read **only** from the environment (no keychain
//! fallback). In zed-kask the parent zed process injects them into MCP server
//! child processes via `kask_bridge::build_mcp_server_env`; standalone, the
//! operator's shell sets them. See `resolve_api_key` for the precise contract.
//!
//! # Model Naming Convention
//!
//! Models use a full-name provider prefix:
//! - `OpenRouter/openai/gpt-4o` → OpenRouter (cloud)
//! - `ollama/qwen3:8b` → Ollama (local)
//! - `RunPod/kask-ocr` → RunPod (OLMOCR-2 vision/OCR via serverless vLLM endpoint, D29)
//! - No prefix → default provider (configurable, default: OpenRouter)

use serde::{Deserialize, Serialize};

/// Provider identifier for inference routing. Used as the model-string
/// prefix (e.g. `OpenRouter/model`, `ollama/model`) and in log messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    /// Runpod (cloud) — prefix `RunPod/`
    #[serde(rename = "RP")]
    Runpod,
    /// OpenRouter (cloud) — prefix `OpenRouter/`
    #[serde(rename = "OR")]
    OpenRouter,
    /// Ollama (local) — prefix `ollama/`. No API key required; the OpenAI-compatible
    /// endpoint at `/v1/chat/completions` ignores the `Authorization` header.
    #[serde(rename = "OM")]
    Ollama,
}

impl ProviderId {
    /// Full provider name used as the model-string prefix.
    ///
    /// expect: "The system normalizes provider responses for monitoring"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — stable provider name for routing
    /// post: returns "RunPod", "OpenRouter", or "ollama"
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::Runpod => "RunPod",
            ProviderId::OpenRouter => "OpenRouter",
            ProviderId::Ollama => "ollama",
        }
    }
}

/// Configuration for the inference router.
///
/// Holds connection settings for OpenRouter and
/// Ollama. The router uses this config to construct
/// backends and decide the default provider for unprefixed model names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// Default provider for model names without a prefix.
    /// Default: OpenRouter (cloud-first).
    pub default_provider: ProviderId,

    pub openrouter_base_url: String,
    pub openrouter_api_key: String,
    /// DeepInfra — media generation (image, TTS, STT, background removal).
    /// Base URL defaults to `https://api.deepinfra.com/v1/openai`.
    pub deepinfra_base_url: String,
    pub deepinfra_api_key: String,
    /// Ollama local inference — defaults to `http://localhost:11434`. The API key
    /// is optional (Ollama ignores it) but kept as `String` for consistency with the
    /// other backends and to support remote Ollama instances that require auth.
    pub ollama_base_url: String,
    pub ollama_api_key: String,
    pub default_model: String,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            default_provider: ProviderId::OpenRouter,
            openrouter_base_url: "https://openrouter.ai/api".to_string(),
            openrouter_api_key: String::new(),
            deepinfra_base_url: "https://api.deepinfra.com".to_string(),
            deepinfra_api_key: String::new(),
            ollama_base_url: "http://localhost:11434".to_string(),
            ollama_api_key: String::new(),
            // Empty = not configured. The operator's spec: no hidden code
            // constant may be the effective inference model — unset is a
            // typed error at the call site, never a silent fallback.
            default_model: String::new(),
        }
    }
}

impl InferenceConfig {
    /// Resolve from environment variables only (no keychain fallback).
    ///
    /// API keys are injected as env vars by `build_mcp_server_env`, which reads
    /// each provider's key from its `api_url` keychain slot (the slot `ApiKeyState` reads).
    ///
    /// expect: "The system resolves inference configuration from the environment"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — inference configuration resolved from environment
    /// post: returns InferenceConfig resolved from env vars
    /// post: defaults to OpenRouter cloud if env vars unset
    pub fn from_env() -> Self {
        let or = ProviderConfig::from_env("OpenRouter", "https://openrouter.ai/api");
        let om = ProviderConfig::from_env("ollama", "http://localhost:11434");
        let di = ProviderConfig::from_env("DeepInfra", "https://api.deepinfra.com");

        Self {
            default_provider: resolve_default_provider(),
            openrouter_base_url: or.base_url,
            openrouter_api_key: or.api_key,
            deepinfra_base_url: di.base_url,
            deepinfra_api_key: di.api_key,
            ollama_base_url: om.base_url,
            ollama_api_key: om.api_key,
            default_model: resolve_config_str("HKASK_DEFAULT_MODEL").unwrap_or_default(),
        }
    }
}

// ── Private resolution helpers ──────────────────────────────────────────────

/// Resolve a provider API key from the process environment.
///
/// In zed-kask, inference API keys are injected into MCP server child
/// processes as environment variables by the parent zed process (via
/// `kask_bridge::build_mcp_server_env`, which reads each provider's key
/// from its `api_url` keychain slot — the same slot zed's `ApiKeyState`
/// reads and Settings → AI → LLM Providers writes). Standalone MCP servers
/// set the same env vars in their shell.
///
/// This function reads **only** the environment variable. It does **not**
/// fall back to the `hkask` keychain namespace — that namespace is reserved
/// for sovereignty keys (db_passphrase) per the
/// `hkask_keystore` module contract. Reading inference keys from the `hkask`
/// namespace was a spec violation: the key lives at the provider's
/// `api_url` keychain slot, never in the `hkask` keyring, so the fallback
/// read a namespace that was always empty in zed-kask, producing silent
/// "API key not configured" errors.
///
/// Returns an empty string if the env var is unset or empty — the backend
/// will be unavailable.
fn resolve_api_key(env_name: &str) -> String {
    // The env var is the sole source. In zed-kask it is injected by the
    // parent process; standalone, by the operator's shell.
    if let Ok(val) = std::env::var(env_name)
        && !val.is_empty()
    {
        return val;
    }
    String::new()
}

/// Resolve the default provider from env var.
///
/// Reads `HKASK_DEFAULT_PROVIDER` (env var only — injected by
/// `build_mcp_server_env` from zed's keychain). Accepted values: RunPod,
/// OpenRouter, ollama. Defaults to OpenRouter.
fn resolve_default_provider() -> ProviderId {
    let raw = resolve_api_key("HKASK_DEFAULT_PROVIDER");
    parse_provider_code(&raw)
}

/// Parse a provider code string to a ProviderId.
///
/// Accepted values: full provider names (RunPod,
/// OpenRouter, ollama). Anything else (including
/// empty) → OpenRouter.
fn parse_provider_code(raw: &str) -> ProviderId {
    match raw {
        "RunPod" => ProviderId::Runpod,
        "OpenRouter" => ProviderId::OpenRouter,
        "ollama" => ProviderId::Ollama,
        _ => ProviderId::OpenRouter,
    }
}

/// Resolve a configuration string from the process environment.
///
/// Reads only the env var. Does not fall back to the `hkask` keychain —
/// that namespace is reserved for sovereignty keys per the `hkask_keystore`
/// module contract. Config values (`HKASK_DEFAULT_MODEL`, etc.) are injected
/// by the parent zed process via `mcp_env()` or set in the operator's shell.
fn resolve_config_str(key: &str) -> Option<String> {
    if let Ok(val) = std::env::var(key)
        && !val.is_empty()
    {
        return Some(val);
    }
    None
}

// ── Provider configuration ───────────────────────────────────────────────────

/// Per-provider connection config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProviderConfig {
    pub base_url: String,
    pub api_key: String,
}

impl ProviderConfig {
    /// Resolve base URL and API key from environment using a full provider name.
    ///
    /// Reads `{prefix}_BASE_URL` (falls back to `default_base_url` if unset)
    /// and `{prefix}_API_KEY` (env only — `resolve_api_key`, no keychain fallback).
    pub fn from_env(prefix: &str, default_base_url: &str) -> Self {
        // Sanitize the prefix for env var names: uppercase, remove spaces
        // and dots. e.g. "fal.ai" → "FALAI", "ollama" → "OLLAMA".
        // This keeps env var names valid (no spaces/dots) while the provider
        // ID (used for routing) retains its zed-format display name.
        let env_prefix = prefix.to_uppercase().replace([' ', '.'], "");
        Self {
            base_url: std::env::var(format!("{env_prefix}_BASE_URL"))
                .unwrap_or_else(|_| default_base_url.to_string()),
            api_key: resolve_api_key(&format!("{env_prefix}_API_KEY")),
        }
    }
}
