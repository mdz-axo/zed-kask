//! Inference configuration — multi-provider routing for 9 providers: DeepInfra, fal.ai, Together AI, OpenRouter, KiloCode, Ollama (local), Cline (cloud gateway), RunPod (vision/OCR only), Z.ai.
//!
//! # Environment Variables
//!
//! - `DEEPINFRA_BASE_URL` / `DEEPINFRA_API_KEY` — DeepInfra (cloud, required)
//! - `FALAI_BASE_URL` / `FALAI_API_KEY` — fal.ai (cloud, required)
//! - `TOGETHERAI_BASE_URL` / `TOGETHERAI_API_KEY` — Together AI (cloud, required)
//! - `OPENROUTER_BASE_URL` / `OPENROUTER_API_KEY` — OpenRouter (cloud, required)
//! - `KILOCODE_BASE_URL` / `KILOCODE_API_KEY` — KiloCode (cloud, required)
//! - `OLLAMA_BASE_URL` / `OLLAMA_API_KEY` — Ollama (local; key optional, header ignored)
//! - `CLINE_BASE_URL` / `CLINE_API_KEY` — Cline cloud gateway (required)
//! - `ZAI_BASE_URL` / `ZAI_API_KEY` — Z.ai (cloud, required). OpenAI-compatible platform at `api.z.ai`
//!   hosting GLM models (e.g. `glm-5.2`). Base URL default: `https://api.z.ai/api/paas/v4`.
//! - `RUNPOD_API_KEY` / `RUNPOD_BASE_URL` or `RUNPOD_TEMPLATE_ID` — RunPod (vision/OCR only)
//! - `HKASK_DEFAULT_PROVIDER` — default provider for unprefixed models (DeepInfra, fal.ai, Together AI, RunPod, OpenRouter, KiloCode, ollama, Cline, Z.ai; default: DeepInfra)
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
//! - `fal.ai/paddleocr` → fal.ai (cloud)
//! - `Together AI/Qwen/Qwen2.5-7B-Instruct-Turbo` → Together AI (cloud)
//! - `OpenRouter/openai/gpt-4o` → OpenRouter (cloud)
//! - `KiloCode/anthropic/claude-sonnet-4.5` → KiloCode (cloud)
//! - `ollama/qwen3:8b` → Ollama (local)
//! - `Cline/anthropic/claude-sonnet-4-6` → Cline (cloud gateway)
//! - `Z.ai/glm-5.2` → Z.ai (cloud)
//! - `RunPod/kask-ocr` → RunPod (vision/OCR only — not available for chat)
//! - No prefix → default provider (configurable, default: DeepInfra)

use serde::{Deserialize, Serialize};

/// Provider identifier for inference routing. Used as the model-string
/// prefix (e.g. `DeepInfra/model`, `fal.ai/model`) and in log messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    /// DeepInfra (cloud) — prefix `DeepInfra/`
    #[serde(rename = "DI")]
    DeepInfra,
    /// fal.ai (cloud) — prefix `fal.ai/`
    #[serde(rename = "FA")]
    Fal,
    /// Together AI (cloud) — prefix `Together AI/`
    #[serde(rename = "TG")]
    Together,
    /// Runpod (cloud) — prefix `RunPod/`
    #[serde(rename = "RP")]
    Runpod,
    /// OpenRouter (cloud) — prefix `OpenRouter/`
    #[serde(rename = "OR")]
    OpenRouter,
    /// KiloCode (cloud) — prefix `KiloCode/`
    #[serde(rename = "KC")]
    KiloCode,
    /// Ollama (local) — prefix `ollama/`. No API key required; the OpenAI-compatible
    /// endpoint at `/v1/chat/completions` ignores the `Authorization` header.
    #[serde(rename = "OM")]
    Ollama,
    /// Cline (cloud) — prefix `Cline/`. OpenAI-compatible gateway at `api.cline.bot`
    /// routing to Anthropic/OpenAI/Google/DeepSeek/xAI models behind one key.
    /// Env: `CLINE_API_KEY`, `CLINE_BASE_URL` (default `https://api.cline.bot/api`).
    #[serde(rename = "CL")]
    Cline,
    /// Z.ai (cloud) — prefix `Z.ai/`. OpenAI-compatible platform at `api.z.ai`
    /// hosting GLM models (e.g. `glm-5.2`, `glm-5v-turbo`).
    /// Env: `ZAI_API_KEY`, `ZAI_BASE_URL` (default `https://api.z.ai/api/paas/v4`).
    #[serde(rename = "ZA")]
    Zai,
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
    /// post: returns Some((ProviderId, stripped_model)) for DeepInfra/, fal.ai/, Together AI/, RunPod/, OpenRouter/, KiloCode/, ollama/, Cline/, Z.ai/ prefixes
    /// post: returns None for unrecognized or missing prefix
    #[must_use]
    pub fn parse_from_model(model: &str) -> Option<(Self, &str)> {
        // Full-name prefixes. Each entry is (prefix, provider, prefix_len).
        // `strip_prefix` handles the matching; the match assigns the variant.
        const PREFIXES: &[(&str, ProviderId)] = &[
            ("DeepInfra/", ProviderId::DeepInfra),
            ("fal.ai/", ProviderId::Fal),
            ("Together AI/", ProviderId::Together),
            ("RunPod/", ProviderId::Runpod),
            ("OpenRouter/", ProviderId::OpenRouter),
            ("KiloCode/", ProviderId::KiloCode),
            ("ollama/", ProviderId::Ollama),
            ("Cline/", ProviderId::Cline),
            ("Z.ai/", ProviderId::Zai),
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

    /// Returns true if `model` has a provider-prefix shape — i.e. it
    /// *looks* like a provider-prefixed name even when the prefix is not
    /// recognized.
    ///
    /// `InferenceRouter::parse_provider` uses this to reject unknown prefixes
    /// with a clear error rather than silently routing them to the default
    /// provider as a garbage model name.
    ///
    /// A model name has the prefix shape if it contains a `/` and the segment
    /// before the first `/` is non-empty. This catches both full-name prefixes
    /// ("DeepInfra/...", "fal.ai/...") and any future prefix format.
    ///
    /// expect: "The system rejects unrecognized provider prefixes explicitly"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — fail fast on unknown prefix
    /// pre:  model may be any string
    /// post: returns true iff model contains a non-empty segment before the first `/`
    /// post: recognized prefixes return false here (handled by `parse_from_model`)
    #[must_use]
    pub fn looks_like_prefix(model: &str) -> bool {
        match model.find('/') {
            Some(slash_idx) if slash_idx > 0 => !model[slash_idx + 1..].is_empty(),
            _ => false,
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
    /// post: returns "DeepInfra", "fal.ai", "Together AI", "RunPod", "OpenRouter", "KiloCode", "ollama", "Cline", or "Z.ai"
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::DeepInfra => "DeepInfra",
            ProviderId::Fal => "fal.ai",
            ProviderId::Together => "Together AI",
            ProviderId::Runpod => "RunPod",
            ProviderId::OpenRouter => "OpenRouter",
            ProviderId::KiloCode => "KiloCode",
            ProviderId::Ollama => "ollama",
            ProviderId::Cline => "Cline",
            ProviderId::Zai => "Z.ai",
        }
    }
}

/// Configuration for the inference router.
///
/// Holds connection settings for DeepInfra, fal.ai, Together AI, and OpenRouter.
/// The router uses this config to construct backends and decide
/// the default provider for unprefixed model names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// Default provider for model names without a prefix.
    /// Default: DeepInfra (cloud-first).
    pub default_provider: ProviderId,

    pub deepinfra_base_url: String,
    pub deepinfra_api_key: String,
    pub fal_base_url: String,
    pub fal_media_base_url: String,
    pub fal_queue_base_url: String,
    pub fal_api_key: String,
    pub together_base_url: String,
    pub together_api_key: String,
    pub openrouter_base_url: String,
    pub openrouter_api_key: String,
    pub kilocode_base_url: String,
    pub kilocode_api_key: String,
    /// Ollama local inference — defaults to `http://localhost:11434`. The API key
    /// is optional (Ollama ignores it) but kept as `String` for consistency with the
    /// other backends and to support remote Ollama instances that require auth.
    pub ollama_base_url: String,
    pub ollama_api_key: String,
    /// Cline cloud gateway — OpenAI-compatible router at `api.cline.bot`.
    /// Env: `CLINE_API_KEY`, `CLINE_BASE_URL` (default `https://api.cline.bot/api`).
    pub cline_base_url: String,
    pub cline_api_key: String,
    /// Z.ai cloud — OpenAI-compatible platform at `api.z.ai` hosting GLM models.
    /// Env: `ZAI_API_KEY`, `ZAI_BASE_URL` (default `https://api.z.ai/api/paas/v4`).
    pub zai_base_url: String,
    pub zai_api_key: String,
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
            fal_base_url: "https://api.fal.ai".to_string(),
            fal_media_base_url: "https://fal.run".to_string(),
            fal_queue_base_url: "https://queue.fal.run".to_string(),
            fal_api_key: String::new(),
            together_base_url: "https://api.together.xyz".to_string(),
            together_api_key: String::new(),
            openrouter_base_url: "https://openrouter.ai/api".to_string(),
            openrouter_api_key: String::new(),
            kilocode_base_url: "https://api.kilo.ai/api/gateway".to_string(),
            kilocode_api_key: String::new(),
            ollama_base_url: "http://localhost:11434".to_string(),
            ollama_api_key: String::new(),
            cline_base_url: "https://api.cline.bot/api".to_string(),
            cline_api_key: String::new(),
            zai_base_url: "https://api.z.ai/api/paas/v4".to_string(),
            zai_api_key: String::new(),
            timeout_secs: 120,
            pool_max_idle: 5,
            default_model: "OpenRouter/z-ai/glm-5.2".to_string(),
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
        let tg = ProviderConfig::from_env("Together AI", "https://api.together.xyz");
        let or = ProviderConfig::from_env("OpenRouter", "https://openrouter.ai/api");
        let kc = ProviderConfig::from_env("KiloCode", "https://api.kilo.ai/api/gateway");
        let om = ProviderConfig::from_env("ollama", "http://localhost:11434");
        // Cline uses the sanitized prefix convention (CLINE_API_KEY).
        let cline_base_url = std::env::var("CLINE_BASE_URL")
            .unwrap_or_else(|_| "https://api.cline.bot/api".to_string());
        let cline_api_key = resolve_api_key("CLINE_API_KEY");
        // Z.ai — sanitized prefix "Z.ai" → ZAI, reading ZAI_BASE_URL / ZAI_API_KEY.
        let za = ProviderConfig::from_env("Z.ai", "https://api.z.ai/api/paas/v4");

        let fal_base_url =
            std::env::var("FALAI_BASE_URL").unwrap_or_else(|_| "https://api.fal.ai".to_string());

        let fal_media_base_url =
            std::env::var("FALAI_MEDIA_BASE_URL").unwrap_or_else(|_| "https://fal.run".to_string());

        let fal_queue_base_url = std::env::var("FALAI_QUEUE_BASE_URL")
            .unwrap_or_else(|_| "https://queue.fal.run".to_string());

        let fal_api_key = resolve_api_key("FALAI_API_KEY");

        Self {
            default_provider: resolve_default_provider(),
            deepinfra_base_url: di.base_url,
            deepinfra_api_key: di.api_key,
            fal_base_url,
            fal_media_base_url,
            fal_queue_base_url,
            fal_api_key,
            together_base_url: tg.base_url,
            together_api_key: tg.api_key,
            openrouter_base_url: or.base_url,
            openrouter_api_key: or.api_key,
            kilocode_base_url: kc.base_url,
            kilocode_api_key: kc.api_key,
            ollama_base_url: om.base_url,
            ollama_api_key: om.api_key,
            cline_base_url,
            cline_api_key,
            zai_base_url: za.base_url,
            zai_api_key: za.api_key,
            timeout_secs: resolve_config_str("HKASK_HTTP_TIMEOUT_SECS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            pool_max_idle: resolve_config_str("HKASK_HTTP_POOL_MAX_IDLE")
                .and_then(|v| v.parse().ok())
                .unwrap_or(256),
            default_model: resolve_config_str("HKASK_DEFAULT_MODEL")
                .unwrap_or_else(|| "OpenRouter/z-ai/glm-5.2".to_string()),
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
/// `kask_bridge::KaskSettings::mcp_env_with_credentials`, which reads from
/// zed's `CredentialsProvider` keychain under `kask://credentials/<key>`).
/// Standalone MCP servers set the same env vars in their shell.
///
/// This function reads **only** the environment variable. It does **not**
/// fall back to the `hkask` keychain namespace — that namespace is reserved
/// for sovereignty keys (a2a_secret, db_passphrase, ocap_secret) per the
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
/// OS keychain). Accepted values: DeepInfra, fal.ai, Together AI, RunPod,
/// OpenRouter, KiloCode, ollama, Cline, Z.ai. Defaults to DeepInfra.
fn resolve_default_provider() -> ProviderId {
    let raw = resolve_api_key("HKASK_DEFAULT_PROVIDER");
    parse_provider_code(&raw)
}

/// Parse a provider code string to a ProviderId.
///
/// Accepted values: full provider names (DeepInfra, fal.ai, Together AI,
/// RunPod, OpenRouter, KiloCode, ollama, Cline, Z.ai). Anything else (including
/// empty) → DeepInfra.
fn parse_provider_code(raw: &str) -> ProviderId {
    match raw {
        "DeepInfra" => ProviderId::DeepInfra,
        "fal.ai" => ProviderId::Fal,
        "Together AI" => ProviderId::Together,
        "RunPod" => ProviderId::Runpod,
        "OpenRouter" => ProviderId::OpenRouter,
        "KiloCode" => ProviderId::KiloCode,
        "ollama" => ProviderId::Ollama,
        "Cline" => ProviderId::Cline,
        "Z.ai" => ProviderId::Zai,
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
        // and dots. e.g. "DeepInfra" → "DEEPINFRA", "Together AI" → "TOGETHERAI",
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
            ProviderId::parse_from_model("Together AI/Qwen/Qwen2.5-7B-Instruct-Turbo"),
            Some((ProviderId::Together, "Qwen/Qwen2.5-7B-Instruct-Turbo"))
        );
        assert_eq!(
            ProviderId::parse_from_model("DeepInfra/meta-llama/Llama-3.3-70B-Instruct"),
            Some((ProviderId::DeepInfra, "meta-llama/Llama-3.3-70B-Instruct"))
        );
        assert_eq!(
            ProviderId::parse_from_model("RunPod/my-model"),
            Some((ProviderId::Runpod, "my-model"))
        );
        assert_eq!(
            ProviderId::parse_from_model("Z.ai/glm-5.2"),
            Some((ProviderId::Zai, "glm-5.2"))
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
            ProviderId::Together.prefix_model("Qwen/Qwen2.5-7B"),
            "Together AI/Qwen/Qwen2.5-7B"
        );
        assert_eq!(
            ProviderId::DeepInfra.prefix_model("meta-llama/Llama-3.3-70B"),
            "DeepInfra/meta-llama/Llama-3.3-70B"
        );
        assert_eq!(
            ProviderId::Fal.prefix_model("paddleocr"),
            "fal.ai/paddleocr"
        );
        assert_eq!(
            ProviderId::Runpod.prefix_model("my-model"),
            "RunPod/my-model"
        );
        assert_eq!(ProviderId::Zai.prefix_model("glm-5.2"), "Z.ai/glm-5.2");
    }

    /// expect: "Inference fal.ai prefix parsing works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates fal.ai routing
    #[test]
    fn parse_fal_prefix() {
        assert_eq!(
            ProviderId::parse_from_model("fal.ai/paddleocr"),
            Some((ProviderId::Fal, "paddleocr"))
        );
        assert_eq!(
            ProviderId::parse_from_model("fal.ai/nemotron-parse"),
            Some((ProviderId::Fal, "nemotron-parse"))
        );
    }

    // ── parse_provider_code ────────────────────────────────────────────

    /// expect: "Inference provider code parsing works correctly under test conditions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — validates provider code parser
    #[test]
    fn parse_provider_code_all_codes() {
        assert_eq!(parse_provider_code("DeepInfra"), ProviderId::DeepInfra);
        assert_eq!(parse_provider_code("fal.ai"), ProviderId::Fal);
        assert_eq!(parse_provider_code("Together AI"), ProviderId::Together);
        assert_eq!(parse_provider_code("RunPod"), ProviderId::Runpod);
        assert_eq!(parse_provider_code("OpenRouter"), ProviderId::OpenRouter);
        assert_eq!(parse_provider_code("KiloCode"), ProviderId::KiloCode);
        assert_eq!(parse_provider_code("ollama"), ProviderId::Ollama);
        assert_eq!(parse_provider_code("Cline"), ProviderId::Cline);
        assert_eq!(parse_provider_code("Z.ai"), ProviderId::Zai);
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
    /// keys (a2a_secret, db_passphrase, ocap_secret) per the `hkask_keystore`
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

    // ── looks_like_prefix ──────────────────────────────────────────────────

    /// expect: "Prefix-shape detection distinguishes unrecognized XX/ prefixes from unprefixed names" [P9]
    #[test]
    fn looks_like_prefix_detects_shape() {
        // Any non-empty segment before a `/` = prefix-shaped.
        assert!(ProviderId::looks_like_prefix("BT/foo"));
        assert!(ProviderId::looks_like_prefix("XX/model"));
        assert!(ProviderId::looks_like_prefix("DeepInfra/foo"));
        assert!(ProviderId::looks_like_prefix("fal.ai/bar"));
        assert!(ProviderId::looks_like_prefix("SomeUnknown/model"));
        // No slash, too short, or empty prefix segment = not prefix-shaped.
        // No slash, too short, or empty prefix segment = not prefix-shaped.
        assert!(!ProviderId::looks_like_prefix("qwen3:8b"));
        assert!(!ProviderId::looks_like_prefix("deepseek-v4-pro"));
        assert!(!ProviderId::looks_like_prefix("DI"));
        assert!(!ProviderId::looks_like_prefix("DeepInfra/"));
        assert!(!ProviderId::looks_like_prefix("/model"));
        // `ab/c` has the prefix shape (non-empty segment before `/`) —
        // `looks_like_prefix` only checks shape, not whether the prefix is
        // recognized. `parse_from_model` handles recognition.
        assert!(ProviderId::looks_like_prefix("ab/c"));
    }
}
