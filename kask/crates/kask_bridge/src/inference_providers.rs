//! Inference provider descriptors and `openai_compatible` settings sync.
//!
//! Each inference provider (DeepInfra, fal.ai, Together, OpenRouter, KiloCode,
//! Cline) is exposed as a zed OpenAI-compatible provider. When the user enables
//! a provider in the kask settings UI, the composition root calls
//! `ensure_openai_compatible_entries` to write the corresponding
//! `openai_compatible.<provider_id>` entry into settings.json. The existing
//! `register_compatible_providers` machinery then registers the provider in the
//! `LanguageModelRegistry`, making it appear in Settings → AI → LLM Providers
//! and in the agent model picker.
//!
//! API keys are stored in the keychain under the provider's `api_url` (the same
//! URL zed's OpenAI-compatible provider reads from), and mirrored to
//! `kask://credentials/<env_var>` for MCP server env injection.

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::{App, ReadGlobal as _, Task};
use settings::SettingsStore;
use settings_content::OpenAiCompatibleSettingsContent;
use util::ResultExt as _;

/// The URL prefix for kask-namespaced credentials in the keychain.
/// Must match `kask_bridge::KASK_CREDENTIAL_NAMESPACE`.
const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";

/// A descriptor for an inference provider that exposes an OpenAI-compatible API.
pub struct InferenceProviderDescriptor {
    /// The provider ID used as the `openai_compatible` HashMap key.
    pub id: &'static str,
    /// Human-readable name for logging.
    pub name: &'static str,
    /// The OpenAI-compatible API base URL (zed appends `/chat/completions`).
    pub api_url: &'static str,
    /// The env var name that MCP servers and hKask read for this provider's key.
    pub env_var: &'static str,
    /// The credential key in the keychain (`kask://credentials/<key>`).
    pub credential_key: &'static str,
    /// Dashboard URL where the user can obtain an API key.
    pub dashboard_url: &'static str,
}

/// The 6 inference providers surfaced in kask settings.
pub static INFERENCE_PROVIDERS: &[InferenceProviderDescriptor] = &[
    InferenceProviderDescriptor {
        id: "DeepInfra",
        name: "DeepInfra",
        api_url: "https://api.deepinfra.com/v1/openai",
        env_var: "DEEPINFRA_API_KEY",
        credential_key: "deepinfra",
        dashboard_url: "https://deepinfra.com/",
    },
    InferenceProviderDescriptor {
        id: "fal.ai",
        name: "fal.ai",
        api_url: "https://api.fal.ai/v1",
        env_var: "FALAI_API_KEY",
        credential_key: "fal",
        dashboard_url: "https://fal.ai/",
    },
    InferenceProviderDescriptor {
        id: "Together AI",
        name: "Together AI",
        api_url: "https://api.together.xyz/v1",
        env_var: "TOGETHERAI_API_KEY",
        credential_key: "together",
        dashboard_url: "https://together.ai/",
    },
    InferenceProviderDescriptor {
        id: "OpenRouter",
        name: "OpenRouter",
        api_url: "https://openrouter.ai/api/v1",
        env_var: "OPENROUTER_API_KEY",
        credential_key: "openrouter",
        dashboard_url: "https://openrouter.ai/",
    },
    InferenceProviderDescriptor {
        id: "KiloCode",
        name: "KiloCode",
        api_url: "https://api.kilo.ai/api/gateway",
        env_var: "KILOCODE_API_KEY",
        credential_key: "kilocode",
        dashboard_url: "https://kilo.ai/",
    },
    InferenceProviderDescriptor {
        id: "Cline",
        name: "Cline",
        api_url: "https://api.cline.bot/api/v1",
        env_var: "CLINE_API_KEY",
        credential_key: "cline",
        dashboard_url: "https://cline.bot/",
    },
];

impl InferenceProviderDescriptor {
    /// The keychain URL for this provider's API key in the kask namespace.
    pub fn credential_url(&self) -> String {
        format!("{KASK_CREDENTIAL_NAMESPACE}/{}", self.credential_key)
    }
}

/// Data service credential descriptors: `(env_var, credential_key)`.
///
/// These are the env vars that MCP servers read for data service API keys.
/// The composition root passes these to `mcp_env_with_credentials` so keys
/// entered in the kask settings UI are injected into the MCP server child
/// process environment.
pub static DATA_SERVICE_CREDENTIALS: &[(&str, &str)] = &[
    ("HKASK_EODHD_API_KEY", "eodhd"),
    ("HKASK_FMP_API_KEY", "fmp"),
    ("HKASK_EXA_API_KEY", "exa"),
    ("HKASK_TAVILY_API_KEY", "tavily"),
    ("HKASK_BRAVE_API_KEY", "brave"),
    ("HKASK_SERPAPI_API_KEY", "serpapi"),
    ("HKASK_FIRECRAWL_API_KEY", "firecrawl"),
    ("HKASK_BROWSERBASE_API_KEY", "browserbase"),
    ("RUNPOD_API_KEY", "runpod"),
    ("RUNPOD_TEMPLATE_ID", "runpod_template_id"),
    ("RUNPOD_S3_ACCESS_KEY", "runpod_s3_access_key"),
    ("RUNPOD_S3_SECRET", "runpod_s3_secret"),
    ("NEBIUS_PROJECT_ID", "nebius_project_id"),
    ("NEBIUS_SUBNET_ID", "nebius_subnet_id"),
    ("HF_TOKEN", "hf_token"),
    ("TINKER_API_KEY", "tinker"),
];

/// Build the `(env_var, credential_url)` pairs for all credentials that
/// should be injected into MCP server child processes.
///
/// Reads the `KaskSettings` to determine which data services and inference
/// providers are enabled, and returns the credential URLs for each.
pub fn credential_urls_for_mcp(settings: &super::KaskSettings) -> Vec<(String, String)> {
    let mut urls = Vec::new();

    // Data services — always inject if the key exists in the keychain.
    // The `mcp_env_with_credentials` function skips env vars already set
    // in the process environment, so there's no harm in listing all of them.
    for (env_var, key) in DATA_SERVICE_CREDENTIALS {
        urls.push((
            env_var.to_string(),
            format!("{KASK_CREDENTIAL_NAMESPACE}/{key}"),
        ));
    }

    // Inference providers — inject the API key as the env var the MCP servers
    // and hKask's InferenceRouter expect.
    for provider in INFERENCE_PROVIDERS {
        let enabled = match provider.credential_key {
            "deepinfra" => settings.inference_providers.deepinfra_enabled,
            "fal" => settings.inference_providers.fal_enabled,
            "together" => settings.inference_providers.together_enabled,
            "openrouter" => settings.inference_providers.openrouter_enabled,
            "kilocode" => settings.inference_providers.kilocode_enabled,
            "cline" => settings.inference_providers.cline_enabled,
            _ => false,
        };
        if enabled {
            urls.push((provider.env_var.to_string(), provider.credential_url()));
        }
    }

    urls
}

/// Ensure that `openai_compatible.<provider_id>` entries exist in settings.json
/// for every enabled inference provider, and remove entries for disabled ones.
///
/// This is called by the composition root after `KaskSettings` are loaded.
/// The existing `register_compatible_providers` machinery in `language_models`
/// watches the `openai_compatible` settings section and registers/unregisters
/// providers in the `LanguageModelRegistry` automatically.
///
/// Each entry is written with an empty `available_models` list — the user
/// adds models via the LLM Providers settings page, which writes to the same
/// `openai_compatible.<provider_id>` key.
pub fn ensure_openai_compatible_entries(settings: &super::KaskSettings, cx: &mut App) {
    // Extract the enabled states before the closure so we don't borrow
    // `settings` inside the `move` closure.
    let enabled_states: [(&'static str, bool); 6] = [
        ("DeepInfra", settings.inference_providers.deepinfra_enabled),
        ("fal.ai", settings.inference_providers.fal_enabled),
        ("Together AI", settings.inference_providers.together_enabled),
        (
            "OpenRouter",
            settings.inference_providers.openrouter_enabled,
        ),
        ("KiloCode", settings.inference_providers.kilocode_enabled),
        ("Cline", settings.inference_providers.cline_enabled),
    ];

    let fs = <dyn fs::Fs>::global(cx);
    SettingsStore::global(cx).update_settings_file(fs, move |content, _| {
        let openai_compatible = content
            .language_models
            .get_or_insert_default()
            .openai_compatible
            .get_or_insert_default();

        for provider in INFERENCE_PROVIDERS {
            let enabled = enabled_states
                .iter()
                .find(|(id, _)| *id == provider.id)
                .map(|(_, e)| *e)
                .unwrap_or(false);

            let provider_id: std::sync::Arc<str> = std::sync::Arc::from(provider.id);
            if enabled {
                // Only insert if not already present — don't overwrite
                // user-configured available_models or custom headers.
                openai_compatible.entry(provider_id).or_insert_with(|| {
                    OpenAiCompatibleSettingsContent {
                        api_url: provider.api_url.to_string(),
                        available_models: Vec::new(),
                        custom_headers: None,
                    }
                });
            } else {
                // Remove the entry if the provider was disabled.
                // We only remove if the api_url matches our known URL
                // (to avoid removing a user's custom provider that happens
                // to share the ID).
                if let Some(existing) = openai_compatible.get(&provider_id)
                    && existing.api_url == provider.api_url
                {
                    openai_compatible.remove(&provider_id);
                }
            }
        }
    });
}

/// Read an inference provider's API key from zed's keychain.
///
/// The key is stored under the provider's `api_url` (the same URL zed's
/// OpenAI-compatible provider reads from). This function is used by the
/// settings UI to display "Configured" / "Not configured" status.
pub fn provider_credential_url(provider: &InferenceProviderDescriptor) -> String {
    provider.api_url.to_string()
}

/// Write an inference provider's API key to zed's keychain under both:
/// 1. The provider's `api_url` (so zed's OpenAI-compatible provider finds it).
/// 2. `kask://credentials/<credential_key>` (for MCP server env injection).
pub fn write_provider_api_key(
    provider: &InferenceProviderDescriptor,
    api_key: &str,
    credentials_provider: &Arc<dyn CredentialsProvider>,
    cx: &mut App,
) -> Task<()> {
    let api_url = provider.api_url.to_string();
    let credential_url = provider.credential_url();
    let api_key = api_key.to_string();
    let credentials_provider = credentials_provider.clone();
    let provider_clone = credentials_provider.clone();
    cx.spawn(async move |cx| {
        // Write under the api_url (for zed's OpenAI-compatible provider).
        let _ = credentials_provider
            .write_credentials(&api_url, "Bearer", api_key.as_bytes(), cx)
            .await
            .log_err();
        // Write under the kask credential URL (for MCP env injection).
        let _ = provider_clone
            .write_credentials(&credential_url, "kask", api_key.as_bytes(), cx)
            .await
            .log_err();
    })
}

/// Delete an inference provider's API key from both keychain locations.
pub fn delete_provider_api_key(
    provider: &InferenceProviderDescriptor,
    credentials_provider: &Arc<dyn CredentialsProvider>,
    cx: &mut App,
) -> Task<()> {
    let api_url = provider.api_url.to_string();
    let credential_url = provider.credential_url();
    let credentials_provider = credentials_provider.clone();
    let provider_clone = credentials_provider.clone();
    cx.spawn(async move |cx| {
        let _ = credentials_provider
            .delete_credentials(&api_url, cx)
            .await
            .log_err();
        let _ = provider_clone
            .delete_credentials(&credential_url, cx)
            .await
            .log_err();
    })
}

/// Check whether an inference provider's API key is available.
///
/// Checks the env var synchronously (instant). The keychain read is async
/// and can't block on the foreground thread, so we optimistically report
/// false for the keychain and let the user enter the key.
pub fn has_provider_api_key(provider: &InferenceProviderDescriptor) -> bool {
    std::env::var(provider.env_var).is_ok()
}

/// Write a data service API key to zed's keychain under
/// `kask://credentials/<key>`.
pub fn write_data_service_api_key(
    credential_key: &str,
    api_key: &str,
    credentials_provider: &Arc<dyn CredentialsProvider>,
    cx: &mut App,
) -> Task<()> {
    let url = format!("{KASK_CREDENTIAL_NAMESPACE}/{credential_key}");
    let api_key = api_key.to_string();
    let credentials_provider = credentials_provider.clone();
    cx.spawn(async move |cx| {
        let _ = credentials_provider
            .write_credentials(&url, "kask", api_key.as_bytes(), cx)
            .await
            .log_err();
    })
}

/// Delete a data service API key from zed's keychain.
pub fn delete_data_service_api_key(
    credential_key: &str,
    credentials_provider: &Arc<dyn CredentialsProvider>,
    cx: &mut App,
) -> Task<()> {
    let url = format!("{KASK_CREDENTIAL_NAMESPACE}/{credential_key}");
    let credentials_provider = credentials_provider.clone();
    cx.spawn(async move |cx| {
        let _ = credentials_provider
            .delete_credentials(&url, cx)
            .await
            .log_err();
    })
}

/// Check whether a data service API key is available (env var only).
pub fn has_data_service_api_key(env_var: &str) -> bool {
    std::env::var(env_var).is_ok()
}
