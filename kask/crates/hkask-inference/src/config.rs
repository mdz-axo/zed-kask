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
//! - `HKASK_DEFAULT_MODEL` — default model (default: `OpenRouter/z-ai/glm-5.2`)
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
    /// Parse a full-name provider prefix from a model name.
    ///
    /// Returns `None` if the model name has no recognized prefix.
    /// Returns `Some((provider, stripped_model))` if a prefix is found.
    ///
    /// expect: "The system normalizes provider responses for monitoring"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — model-name routing to provider boundary
    /// pre:  model is non-empty
    /// post: returns Some((ProviderId, stripped_model)) for RunPod/, OpenRouter/, ollama/ prefixes
    /// post: returns None for unrecognized or missing prefix
    #[must_use]
    pub fn parse_from_model(model: &str) -> Option<(Self, &str)> {
        // Full-name prefixes. Each entry is (prefix, provider, prefix_len).
        // `strip_prefix` handles the matching; the match assigns the variant.
        const PREFIXES: &[(&str, ProviderId)] = &[
            ("RunPod/", ProviderId::Runpod),
            ("OpenRouter/", ProviderId::OpenRouter),
            ("ollama/", ProviderId::Ollama),
        ];
        for (prefix, provider) in PREFIXES {
            if let Some(rest) = model.strip_prefix(prefix) {
                if rest.is_empty() {
                    return None;
                }
                return Some((*provider, rest));
            }
        }
        None
    }

    /// Classify a provider prefix segment (the text before the first `/` in a
    /// model name) to a `ProviderId`, case-insensitively, recognizing short
    /// aliases. Unrecognized segments fall back to `OpenRouter`.
    ///
    /// This is the lenient counterpart to [`ProviderId::parse_from_model`]:
    /// `parse_from_model` does strict case-sensitive full-prefix stripping and
    /// returns the rest of the model name; `from_prefix_segment` classifies an
    /// already-split segment and accepts aliases (`"or"`, …).
    /// Centralizing the alias table here keeps provider knowledge in one place,
    /// so adding or removing a variant updates one match instead of several.
    ///
    /// expect: "The system classifies a provider prefix segment to a ProviderId"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — provider classification from model-name prefix
    /// pre:  segment may be any string (caller splits on `/`)
    /// post: returns the matching ProviderId, or OpenRouter for unrecognized segments
    #[must_use]
    pub fn from_prefix_segment(segment: &str) -> Self {
        match segment.to_lowercase().as_str() {
            "runpod" | "rp" => ProviderId::Runpod,
            "openrouter" | "or" => ProviderId::OpenRouter,
            "ollama" | "om" => ProviderId::Ollama,
            _ => ProviderId::OpenRouter,
        }
    }

    /// Format a model name with this provider's prefix.
    ///
    /// expect: "The system normalizes provider responses for monitoring"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — canonical provider-prefixed model naming
    /// pre:  model is non-empty
    /// post: returns "{prefix}/{model}" string
    #[must_use]
    pub fn prefix_model(&self, model: &str) -> String {
        format!("{}/{}", self.as_str(), model)
    }

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
            ollama_base_url: "http://localhost:11434".to_string(),
            ollama_api_key: String::new(),
            default_model: crate::model_constants::DEFAULT_FALLBACK_MODEL.to_string(),
        }
    }
}

impl InferenceConfig {
    /// Resolve from environment variables only (no keychain fallback).
    ///
    /// API keys resolve keychain-first, then fall back to environment variables.
    ///
    /// expect: "The system resolves inference configuration from the environment"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — inference configuration resolved from environment
    /// post: returns InferenceConfig resolved from env vars and keychain
    /// post: defaults to OpenRouter cloud if env vars unset
    pub fn from_env() -> Self {
        let or = ProviderConfig::from_env("OpenRouter", "https://openrouter.ai/api");
        let om = ProviderConfig::from_env("ollama", "http://localhost:11434");

        Self {
            default_provider: resolve_default_provider(),
            openrouter_base_url: or.base_url,
            openrouter_api_key: or.api_key,
            ollama_base_url: om.base_url,
            ollama_api_key: om.api_key,
            default_model: resolve_config_str("HKASK_DEFAULT_MODEL")
                .unwrap_or_else(|| crate::model_constants::DEFAULT_FALLBACK_MODEL.to_string()),
        }
    }
}

// ── Private resolution helpers ──────────────────────────────────────────────

/// Parse an `f64` from an environment variable, falling back to `default` on
/// absence or parse failure. Used for tunable thresholds (price caps, etc.).
/// Resolve a provider API key from the process environment.
///
/// In zed-kask, inference API keys are injected into MCP server child
/// processes as environment variables by the parent zed process (via
/// `kask_bridge::build_mcp_server_env`, which reads from zed's
/// `CredentialsProvider` keychain under `kask://credentials/<key>`).
/// Standalone MCP servers set the same env vars in their shell.
///
/// This function reads **only** the environment variable. It does **not**
/// fall back to the `hkask` keychain namespace — that namespace is reserved
/// for sovereignty keys (db_passphrase) per the
/// `hkask_keystore` module contract. Reading inference keys from the `hkask`
/// namespace was a spec violation: the settings UI writes to zed's
/// `CredentialsProvider` (`kask://credentials/<key>`), not the `hkask`
/// keyring, so the fallback read a namespace that was always empty in
/// zed-kask, producing silent "API key not configured" errors.
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

/// Resolve the default provider from env var or keychain.
///
/// Reads `HKASK_DEFAULT_PROVIDER` via [`resolve_api_key`] (env var only — no
/// keychain fallback). Accepted values: RunPod, OpenRouter,
/// ollama. Defaults to OpenRouter.
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
pub struct ProviderConfig {
    pub base_url: String,
    pub api_key: String,
}

impl ProviderConfig {
    /// Resolve base URL and API key from environment using a full provider name.
    ///
    /// Reads `{prefix}_BASE_URL` (falls back to `default_base_url` if unset)
    /// and `{prefix}_API_KEY` (keychain-first, then env).
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

    pub fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }
}
