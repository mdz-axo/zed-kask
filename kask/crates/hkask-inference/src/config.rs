//! Inference configuration — multi-provider routing for 4 chat providers: DeepInfra, RunPod (vision/OCR only), OpenRouter, Ollama (local).
//!
//! # Environment Variables
//!
//! - `DEEPINFRA_BASE_URL` / `DEEPINFRA_API_KEY` — DeepInfra (cloud, required)
//! - `ATLASCLOUD_BASE_URL` / `ATLASCLOUD_API_KEY` — AtlasCloud (cloud media + LLM)
//! - `OPENROUTER_BASE_URL` / `OPENROUTER_API_KEY` — OpenRouter (cloud, required)
//! - `OLLAMA_BASE_URL` / `OLLAMA_API_KEY` — Ollama (local; key optional, header ignored)
//! - `RUNPOD_API_KEY` / `RUNPOD_BASE_URL` or `RUNPOD_TEMPLATE_ID` — RunPod (vision/OCR only)
//! - `HKASK_DEFAULT_PROVIDER` — default provider for unprefixed models (DeepInfra, RunPod, OpenRouter, ollama; default: DeepInfra)
//! - `HKASK_DEFAULT_MODEL` — default model (default: `OpenRouter/z-ai/glm-5.2`)
//!
//! # API Key Resolution
//!
//! Provider API keys resolve through a 2-tier chain (env-first):
//! 1. Environment variable (fast path — set via shell or keychain resolution)
//! 2. OS keychain (encrypted at rest; guarded against concurrent-access SIGABRT from libdbus)
//!
//! # Model Naming Convention
//!
//! Models use a full-name provider prefix:
//! - `DeepInfra/meta-llama/Llama-3.3-70B-Instruct` → DeepInfra (cloud)
//! - `OpenRouter/openai/gpt-4o` → OpenRouter (cloud)
//! - `ollama/qwen3:8b` → Ollama (local)
//! - `RunPod/kask-ocr` → RunPod (OLMOCR-2 vision/OCR via serverless vLLM endpoint, D29)
//! - No prefix → default provider (configurable, default: DeepInfra)

use serde::{Deserialize, Serialize};

/// Provider identifier for inference routing. Used as the model-string
/// prefix (e.g. `DeepInfra/model`, `OpenRouter/model`) and in log messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    /// DeepInfra (cloud) — prefix `DeepInfra/`
    #[serde(rename = "DI")]
    DeepInfra,
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
    /// post: returns Some((ProviderId, stripped_model)) for DeepInfra/, RunPod/, OpenRouter/, ollama/ prefixes
    /// post: returns None for unrecognized or missing prefix
    #[must_use]
    pub fn parse_from_model(model: &str) -> Option<(Self, &str)> {
        // Full-name prefixes. Each entry is (prefix, provider, prefix_len).
        // `strip_prefix` handles the matching; the match assigns the variant.
        const PREFIXES: &[(&str, ProviderId)] = &[
            ("DeepInfra/", ProviderId::DeepInfra),
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
    /// already-split segment and accepts aliases (`"di"`, `"or"`, …).
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
            "deepinfra" | "di" => ProviderId::DeepInfra,
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
    /// post: returns "DeepInfra", "RunPod", "OpenRouter", or "ollama"
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::DeepInfra => "DeepInfra",
            ProviderId::Runpod => "RunPod",
            ProviderId::OpenRouter => "OpenRouter",
            ProviderId::Ollama => "ollama",
        }
    }
}

/// Configuration for the inference router.
///
/// Holds connection settings for DeepInfra, OpenRouter,
/// Ollama, and AtlasCloud. The router uses this config to construct
/// backends and decide the default provider for unprefixed model names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// Default provider for model names without a prefix.
    /// Default: DeepInfra (cloud-first).
    pub default_provider: ProviderId,

    pub deepinfra_base_url: String,
    pub deepinfra_api_key: String,
    pub openrouter_base_url: String,
    pub openrouter_api_key: String,
    /// Ollama local inference — defaults to `http://localhost:11434`. The API key
    /// is optional (Ollama ignores it) but kept as `String` for consistency with the
    /// other backends and to support remote Ollama instances that require auth.
    pub ollama_base_url: String,
    pub ollama_api_key: String,
    /// AtlasCloud — task-based media API (image/video/3D/audio/ASR) + OpenAI-compatible LLM.
    /// Env: `ATLASCLOUD_API_KEY`, `ATLASCLOUD_BASE_URL` (default `https://api.atlascloud.ai/api/v1`).
    pub atlascloud_base_url: String,
    pub atlascloud_api_key: String,
    pub timeout_secs: u64,
    pub pool_max_idle: usize,
    pub default_model: String,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            default_provider: ProviderId::DeepInfra,
            deepinfra_base_url: "https://api.deepinfra.com".to_string(),
            deepinfra_api_key: String::new(),
            openrouter_base_url: "https://openrouter.ai/api".to_string(),
            openrouter_api_key: String::new(),
            ollama_base_url: "http://localhost:11434".to_string(),
            ollama_api_key: String::new(),
            atlascloud_base_url: "https://api.atlascloud.ai/api/v1".to_string(),
            atlascloud_api_key: String::new(),
            timeout_secs: 120,
            pool_max_idle: 5,
            default_model: crate::model_constants::DEFAULT_FALLBACK_MODEL.to_string(),
        }
    }
}

impl InferenceConfig {
    /// Resolve from environment variables and OS keychain.
    ///
    /// API keys resolve keychain-first, then fall back to environment variables.
    ///
    /// expect: "The system resolves inference configuration from the environment"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — inference configuration resolved from environment
    /// post: returns InferenceConfig resolved from env vars and keychain
    /// post: defaults to DeepInfra cloud if env vars unset
    pub fn from_env() -> Self {
        let di = ProviderConfig::from_env("DeepInfra", "https://api.deepinfra.com");
        let or = ProviderConfig::from_env("OpenRouter", "https://openrouter.ai/api");
        let om = ProviderConfig::from_env("ollama", "http://localhost:11434");

        let atlascloud_base_url = std::env::var("ATLASCLOUD_BASE_URL")
            .unwrap_or_else(|_| "https://api.atlascloud.ai/api/v1".to_string());
        let atlascloud_api_key = resolve_api_key("ATLASCLOUD_API_KEY");

        Self {
            default_provider: resolve_default_provider(),
            deepinfra_base_url: di.base_url,
            deepinfra_api_key: di.api_key,
            openrouter_base_url: or.base_url,
            openrouter_api_key: or.api_key,
            ollama_base_url: om.base_url,
            ollama_api_key: om.api_key,
            atlascloud_base_url,
            atlascloud_api_key,
            timeout_secs: parse_env_numeric(
                "HKASK_HTTP_TIMEOUT_SECS",
                resolve_config_str("HKASK_HTTP_TIMEOUT_SECS"),
                Self::default().timeout_secs,
            ),
            pool_max_idle: parse_env_numeric(
                "HKASK_HTTP_POOL_MAX_IDLE",
                resolve_config_str("HKASK_HTTP_POOL_MAX_IDLE"),
                Self::default().pool_max_idle,
            ),
            default_model: resolve_config_str("HKASK_DEFAULT_MODEL")
                .unwrap_or_else(|| crate::model_constants::DEFAULT_FALLBACK_MODEL.to_string()),
        }
    }

    /// Build a reqwest HTTP client with the configured timeout and pool settings.
    ///
    /// expect: "The system resolves inference configuration from the environment"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — bounded HTTP client for regulated requests
    /// post: returns reqwest::Client with timeout and pool settings from config
    #[must_use = "result must be used"]
    pub fn build_client(&self) -> anyhow::Result<reqwest::Client> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .pool_max_idle_per_host(self.pool_max_idle)
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))
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
/// Reads `HKASK_DEFAULT_PROVIDER` via [`resolve_api_key`] (env var first, then
/// OS keychain). Accepted values: DeepInfra, RunPod, OpenRouter,
/// ollama. Defaults to DeepInfra.
fn resolve_default_provider() -> ProviderId {
    let raw = resolve_api_key("HKASK_DEFAULT_PROVIDER");
    parse_provider_code(&raw)
}

/// Parse a provider code string to a ProviderId.
///
/// Accepted values: full provider names (DeepInfra,
/// RunPod, OpenRouter, ollama). Anything else (including
/// empty) → DeepInfra.
fn parse_provider_code(raw: &str) -> ProviderId {
    match raw {
        "DeepInfra" => ProviderId::DeepInfra,
        "RunPod" => ProviderId::Runpod,
        "OpenRouter" => ProviderId::OpenRouter,
        "ollama" => ProviderId::Ollama,
        _ => ProviderId::DeepInfra,
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

/// Parse an env-var string into a numeric type, logging a warning naming the
/// env var and the malformed value before falling back to `default`. Without
/// the warning, an operator cannot distinguish "not configured" from
/// "configured but broken" (a malformed numeric silently degrades to the
/// default, masking a broken feedback loop).
fn parse_env_numeric<T>(name: &str, raw: Option<String>, default: T) -> T
where
    T: std::str::FromStr + std::fmt::Display,
{
    match raw {
        Some(value) => match value.parse::<T>() {
            Ok(parsed) => parsed,
            Err(_) => {
                tracing::warn!(
                    target: "hkask.inference",
                    env = name,
                    value = %value,
                    "malformed numeric value; falling back to default ({default})"
                );
                default
            }
        },
        None => default,
    }
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
        // and dots. e.g. "DeepInfra" → "DEEPINFRA",
        // "fal.ai" → "FALAI", "ollama" → "OLLAMA".
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

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "Inference provider prefix parsing works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates provider routing parser
    #[test]
    fn parse_provider_prefix() {
        assert_eq!(
            ProviderId::parse_from_model("DeepInfra/meta-llama/Llama-3.3-70B-Instruct"),
            Some((ProviderId::DeepInfra, "meta-llama/Llama-3.3-70B-Instruct"))
        );
        assert_eq!(
            ProviderId::parse_from_model("RunPod/my-model"),
            Some((ProviderId::Runpod, "my-model"))
        );
    }

    /// expect: "Inference model prefix fallback works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates default-provider fallback
    #[test]
    fn parse_no_prefix_returns_none() {
        assert_eq!(ProviderId::parse_from_model("deepseek-v4-pro"), None);
        assert_eq!(ProviderId::parse_from_model("qwen3:8b"), None);
    }

    /// expect: "Inference malformed model rejection works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates malformed model rejection
    #[test]
    fn parse_empty_model_returns_none() {
        assert_eq!(ProviderId::parse_from_model("DeepInfra/"), None);
        assert_eq!(ProviderId::parse_from_model("fal.ai/"), None);
    }

    /// expect: "Inference malformed model rejection works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates malformed model rejection
    #[test]
    fn parse_too_short_returns_none() {
        // No prefix — too short to contain a recognized provider prefix.
        assert_eq!(ProviderId::parse_from_model("DI"), None);
        assert_eq!(ProviderId::parse_from_model("FA"), None);
        assert_eq!(ProviderId::parse_from_model("X"), None);
    }

    /// expect: "Inference unknown provider rejection works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates unknown provider rejection
    #[test]
    fn parse_unknown_prefix_returns_none() {
        assert_eq!(ProviderId::parse_from_model("XX/model"), None);
        assert_eq!(ProviderId::parse_from_model("AB/test"), None);
        assert_eq!(ProviderId::parse_from_model("UnknownProvider/model"), None);
    }

    /// expect: "Inference model name formatting works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates canonical model naming
    #[test]
    fn prefix_model_format() {
        assert_eq!(
            ProviderId::DeepInfra.prefix_model("meta-llama/Llama-3.3-70B"),
            "DeepInfra/meta-llama/Llama-3.3-70B"
        );
        assert_eq!(
            ProviderId::Runpod.prefix_model("my-model"),
            "RunPod/my-model"
        );
    }

    // ── parse_provider_code ────────────────────────────────────────────

    /// expect: "Inference provider code parsing works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates provider code parser
    #[test]
    fn parse_provider_code_all_codes() {
        assert_eq!(parse_provider_code("DeepInfra"), ProviderId::DeepInfra);
        assert_eq!(parse_provider_code("RunPod"), ProviderId::Runpod);
        assert_eq!(parse_provider_code("OpenRouter"), ProviderId::OpenRouter);
        assert_eq!(parse_provider_code("ollama"), ProviderId::Ollama);
    }

    /// expect: "Inference provider code default works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates safe default provider
    #[test]
    fn parse_provider_code_unknown_defaults_to_deepinfra() {
        assert_eq!(parse_provider_code("XX"), ProviderId::DeepInfra);
        assert_eq!(parse_provider_code(""), ProviderId::DeepInfra);
        assert_eq!(parse_provider_code("unknown"), ProviderId::DeepInfra);
        // Wrong case ("Ollama" vs canonical "ollama") is not recognized.
        assert_eq!(parse_provider_code("Ollama"), ProviderId::DeepInfra);
    }

    // ── resolve_api_key ──────────────────────────────────────────────────

    /// expect: "Inference API key resolution works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates API key resolution
    #[test]
    fn resolve_api_key_primary_env() {
        // SAFETY: Setting/removing test environment variables in test code is safe in a single-threaded test context (Rust runs tests serially by default).
        unsafe { std::env::set_var("HKASK_TEST_KEY_010", "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX") };
        assert_eq!(
            resolve_api_key("HKASK_TEST_KEY_010"),
            "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX"
        );
        // SAFETY: Test cleanup — see above.
        unsafe { std::env::remove_var("HKASK_TEST_KEY_010") };
    }

    /// expect: "Inference API key missing handling works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates missing key handling
    #[test]
    fn resolve_api_key_empty_when_missing() {
        // SAFETY: Test cleanup — removing environment variables is safe in single-threaded test context.
        unsafe {
            std::env::remove_var("HKASK_TEST_KEY_012");
        }
        assert_eq!(resolve_api_key("HKASK_TEST_KEY_012"), "");
    }

    /// Regression test for the two-namespace split. `resolve_api_key` must
    /// read **only** the env var — it must not fall back to the `hkask`
    /// keychain namespace. The `hkask` namespace is reserved for sovereignty
    /// keys (db_passphrase) per the `hkask_keystore`
    /// module contract. Inference keys live in zed's `CredentialsProvider`
    /// under `kask://credentials/<key>` and are injected as env vars by the
    /// parent process. This test pins that contract: with the env var unset,
    /// the result is empty regardless of any keychain state.
    #[test]
    fn resolve_api_key_no_keychain_fallback() {
        // SAFETY: Test cleanup — removing environment variables is safe in
        // single-threaded test context.
        unsafe {
            std::env::remove_var("HKASK_TEST_KEY_NO_FALLBACK");
        }
        // With no env var and no keychain fallback, the result must be empty.
        // If a keychain fallback were re-introduced, this would read the
        // `hkask` keychain entry for "HKASK_TEST_KEY_NO_FALLBACK" — which is
        // empty in the test environment — and still return "". The test
        // cannot distinguish env-only from keychain-fallback-on-empty by
        // result alone, but it pins the contract that the function returns
        // empty when the env var is unset, which is the observable behavior
        // the rest of the system depends on.
        assert_eq!(resolve_api_key("HKASK_TEST_KEY_NO_FALLBACK"), "");
    }

    // ── from_prefix_segment ────────────────────────────────────────────────

    /// expect: "Provider prefix segment classification matches aliases case-insensitively" [P9]
    #[test]
    fn from_prefix_segment_classifies_aliases_case_insensitively() {
        // Full names, case-insensitive.
        assert_eq!(
            ProviderId::from_prefix_segment("DeepInfra"),
            ProviderId::DeepInfra
        );
        assert_eq!(
            ProviderId::from_prefix_segment("openrouter"),
            ProviderId::OpenRouter
        );
        assert_eq!(
            ProviderId::from_prefix_segment("RUNPOD"),
            ProviderId::Runpod
        );
        assert_eq!(
            ProviderId::from_prefix_segment("ollama"),
            ProviderId::Ollama
        );
        // Short aliases.
        assert_eq!(ProviderId::from_prefix_segment("di"), ProviderId::DeepInfra);
        assert_eq!(
            ProviderId::from_prefix_segment("or"),
            ProviderId::OpenRouter
        );
        assert_eq!(ProviderId::from_prefix_segment("rp"), ProviderId::Runpod);
        assert_eq!(ProviderId::from_prefix_segment("om"), ProviderId::Ollama);
        // Unrecognized → OpenRouter fallback.
        assert_eq!(
            ProviderId::from_prefix_segment("unknown"),
            ProviderId::OpenRouter
        );
        assert_eq!(ProviderId::from_prefix_segment(""), ProviderId::OpenRouter);
    }
}
