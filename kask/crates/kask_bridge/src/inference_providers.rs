//! Inference provider descriptors for MCP server credential injection.
//!
//! Kask uses zed's native inference provider infrastructure — providers are
//! registered in Settings → AI → LLM Providers, not via kask settings. This
//! module only maintains the `INFERENCE_PROVIDERS` descriptor table used by:
//! - `resolve_embedding_credentials` (maps a provider-prefixed model string
//!   to `(api_url, api_key)` for MCP servers that can't access zed's
//!   `LanguageModelRegistry`)
//! - `credential_urls_for_mcp` (builds keychain URLs so MCP server child
//!   processes receive API keys via `build_mcp_server_env`)
//! - `mirror_kask_credentials_to_providers` (mirrors keys from the kask
//!   credential store to each provider's `api_url` so the
//!   `LanguageModelProvider`'s `ApiKeyState` finds them)
//!
//! API keys are stored in the keychain under the provider's `api_url` (the
//! same URL zed's OpenAI-compatible provider reads) and mirrored to
//! `kask://credentials/<credential_key>` for MCP server env injection.

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::{App, Task};

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

/// The inference providers used for credential injection and embedding
/// model resolution. Providers are registered in zed's native Settings →
/// AI → LLM Providers, not via kask settings.
///
/// OpenRouter is included for API-key mirroring to MCP servers.
pub static INFERENCE_PROVIDERS: &[InferenceProviderDescriptor] = &[
    InferenceProviderDescriptor {
        id: "OpenRouter",
        name: "OpenRouter",
        api_url: "https://openrouter.ai/api/v1",
        env_var: "OPENROUTER_API_KEY",
        credential_key: "openrouter",
        dashboard_url: "https://openrouter.ai/",
    },
    // RunPod has a dedicated `LanguageModelProvider` (D29), not an
    // `openai_compatible` entry. It's listed here so
    // `mirror_kask_credentials_to_providers` writes the key to the Zed
    // keychain under `api_url` (where the RunPod provider's `ApiKeyState`
    // reads it), in addition to the `kask://credentials/runpod` write
    // handled by `DATA_SERVICES`. Skipped in
    // `credential_urls_for_mcp`'s `INFERENCE_PROVIDERS` loop (MCP injection
    // is handled by the `DATA_SERVICES` loop via `runpod_enabled`).
    InferenceProviderDescriptor {
        id: "RunPod",
        name: "RunPod",
        api_url: "https://api.runpod.io",
        env_var: "RUNPOD_API_KEY",
        credential_key: "runpod",
        dashboard_url: "https://www.runpod.io/",
    },
    // Ollama is a local LLM/embedding service (default port 11434). It's
    // OpenAI-compatible at `/v1` and requires no API key — an empty `env_var`
    // signals "no key needed" to `resolve_embedding_credentials`. Listed here
    // so the default embedding model resolves
    // without a warning. Skipped in `credential_urls_for_mcp` (no key to
    // inject) and in `mirror_kask_credentials_to_providers` (empty env_var
    // filters out).
    InferenceProviderDescriptor {
        id: "ollama",
        name: "Ollama",
        api_url: "http://localhost:11434/v1",
        env_var: "",
        credential_key: "ollama",
        dashboard_url: "https://ollama.com/",
    },
    // DeepInfra is a cloud inference platform serving Qwen embedding models
    // via an OpenAI-compatible `/v1/embeddings` endpoint. The default
    // embedding model (`DEFAULT_EMBEDDING_MODEL`) routes through this provider.
    // Operators must set `DEEPINFRA_API_KEY` (via the settings UI or env var).
    InferenceProviderDescriptor {
        id: "DeepInfra",
        name: "DeepInfra",
        api_url: "https://api.deepinfra.com/v1/openai",
        env_var: "DEEPINFRA_API_KEY",
        credential_key: "deepinfra",
        dashboard_url: "https://deepinfra.com/",
    },
];

impl InferenceProviderDescriptor {
    /// The keychain URL for this provider's API key in the kask namespace.
    pub fn credential_url(&self) -> String {
        format!("{KASK_CREDENTIAL_NAMESPACE}/{}", self.credential_key)
    }
}

/// A typed descriptor for a data service credential — the single source of
/// truth for data service env vars, credential keys, and display metadata.
/// Replaces the former `DATA_SERVICE_CREDENTIALS` `&[(&str, &str)]` 2-tuple
/// and the settings UI's parallel `DATA_SERVICES` `&[(&str, &str, &str, &str)]`
/// 4-tuple, which had overlapping fields in different positions.
pub struct DataServiceDescriptor {
    /// The env var name that MCP servers read for this credential.
    pub env_var: &'static str,
    /// The credential key in the keychain (`kask://credentials/<key>`).
    /// Used as the UI's row key and the settings toggle matcher.
    pub credential_key: &'static str,
    /// Human-readable label shown in the settings UI.
    pub label: &'static str,
    /// Dashboard URL where the user can obtain or manage the credential.
    pub dashboard_url: &'static str,
    /// The settings toggle key for this credential in the Data Services UI,
    /// or `None` if the credential should not appear in the UI (managed
    /// elsewhere, e.g. `HKASK_SMTP_PASSWORD` in the Curator page, or
    /// `HKASK_DB_PASSPHRASE` which has no toggle). When `Some`, the value
    /// matches the `key` arm in `set_data_service_enabled` and the `match
    /// key` in `render_data_services_page`.
    pub ui_toggle: Option<&'static str>,
}

impl DataServiceDescriptor {
    /// The keychain URL for this credential in the kask namespace.
    pub fn credential_url(&self) -> String {
        format!("{KASK_CREDENTIAL_NAMESPACE}/{}", self.credential_key)
    }

    /// Whether this credential should appear as a row in the Data Services
    /// settings UI. Credentials with `ui_toggle: None` are managed elsewhere
    /// (e.g. SMTP password in the Curator page) or have no toggle.
    pub fn shows_in_ui(&self) -> bool {
        self.ui_toggle.is_some()
    }

    /// Whether this service has a functional enable/disable toggle backed by a
    /// `KaskDataServiceSettings` field. Key-only services (SerpAPI, Firecrawl,
    /// Browserbase, HF Token, FRED) have `ui_toggle: Some(...)` so they appear
    /// in the UI for API key entry, but they have no settings.json toggle —
    /// they're enabled unconditionally when the key is present. The UI renders
    /// these without a SwitchField, always showing the key input.
    pub fn has_toggle(&self) -> bool {
        self.ui_toggle.is_some()
            && !matches!(
                self.credential_key,
                "serpapi" | "firecrawl" | "browserbase" | "hf_token" | "fred"
            )
    }
}

/// The canonical registry of data service credentials. The single source of
/// truth consumed by:
/// - `credential_urls_for_mcp` (builds keychain URLs for MCP env injection)
/// - `mirror_kask_credentials_to_providers` (mirrors keys from the kask
///   credential store to each provider's `api_url`)
/// - the settings UI (`data_services.rs` renders rows from this registry)
/// - the coverage governance test (asserts MCP server allowlists align)
pub static DATA_SERVICES: &[DataServiceDescriptor] = &[
    DataServiceDescriptor {
        env_var: "HKASK_EODHD_API_KEY",
        credential_key: "eodhd",
        label: "EODHD",
        dashboard_url: "https://eodhd.com/dashboard",
        ui_toggle: Some("eodhd"),
    },
    DataServiceDescriptor {
        env_var: "HKASK_FMP_API_KEY",
        credential_key: "fmp",
        label: "FMP (Financial Modeling Prep)",
        dashboard_url: "https://site.financialmodelingprep.com/developer/docs",
        ui_toggle: Some("fmp"),
    },
    DataServiceDescriptor {
        env_var: "HKASK_EXA_API_KEY",
        credential_key: "exa",
        label: "Exa",
        dashboard_url: "https://dashboard.exa.ai/api-keys",
        ui_toggle: Some("exa"),
    },
    DataServiceDescriptor {
        env_var: "HKASK_TAVILY_API_KEY",
        credential_key: "tavily",
        label: "Tavily",
        dashboard_url: "https://app.tavily.com/api-key",
        ui_toggle: Some("tavily"),
    },
    DataServiceDescriptor {
        env_var: "HKASK_BRAVE_API_KEY",
        credential_key: "brave",
        label: "Brave Search",
        dashboard_url: "https://api.search.brave.com/app/subscriptions",
        ui_toggle: Some("brave"),
    },
    DataServiceDescriptor {
        env_var: "HKASK_SERPAPI_API_KEY",
        credential_key: "serpapi",
        label: "SerpAPI (Google Search)",
        dashboard_url: "https://serpapi.com/dashboard",
        ui_toggle: Some("serpapi"),
    },
    DataServiceDescriptor {
        env_var: "HKASK_FIRECRAWL_API_KEY",
        credential_key: "firecrawl",
        label: "Firecrawl (web scraping)",
        dashboard_url: "https://firecrawl.dev/",
        ui_toggle: Some("firecrawl"),
    },
    DataServiceDescriptor {
        env_var: "HKASK_BROWSERBASE_API_KEY",
        credential_key: "browserbase",
        label: "Browserbase (headless browser)",
        dashboard_url: "https://browserbase.com/",
        ui_toggle: Some("browserbase"),
    },
    // ABW API key — not shown in the Data Services UI (no toggle, managed
    // via the keychain by the swarm server's governed launch path).
    DataServiceDescriptor {
        env_var: "HKASK_ABW_API_KEY",
        credential_key: "hkask_abw_api_key",
        label: "ABW API Key",
        dashboard_url: "",
        ui_toggle: None,
    },
    // curator, corpus, training, kata-kanban, research) via
    // `ctx.credentials.get("HKASK_DB_PASSPHRASE")` for SQLCipher stores.
    // Not shown in the Data Services UI (no toggle; managed via the
    // hkask keystore chain).
    DataServiceDescriptor {
        env_var: "HKASK_DB_PASSPHRASE",
        credential_key: "hkask_db_passphrase",
        label: "DB Passphrase",
        dashboard_url: "",
        ui_toggle: None,
    },
    // Swarm memory SQLCipher passphrase — read by the swarm server at
    // hkask-mcp-swarm/src/config.rs via `HKASK_SWARM_MEMORY_PASSPHRASE`.
    // Registered here so the value is injected from the keychain by
    // `credential_urls_for_mcp`; without a descriptor the allowlist
    // entry alone would name a credential that nothing ever sources (RR-0061).
    // Distinct from HKASK_DB_PASSPHRASE: the swarm memory store is a separate DB
    // with its own key. No UI toggle — managed via the keystore chain.
    DataServiceDescriptor {
        env_var: "HKASK_SWARM_MEMORY_PASSPHRASE",
        credential_key: "hkask_swarm_memory_passphrase",
        label: "Swarm Memory Passphrase",
        dashboard_url: "",
        ui_toggle: None,
    },
    // Curator SMTP password — managed in the Curator Email settings page,
    // not in the Data Services page (avoids duplicate reset surfaces).
    DataServiceDescriptor {
        env_var: "HKASK_SMTP_PASSWORD",
        credential_key: "hkask_smtp_password",
        label: "SMTP Password",
        dashboard_url: "",
        ui_toggle: None,
    },
    DataServiceDescriptor {
        env_var: "RUNPOD_API_KEY",
        credential_key: "runpod",
        label: "RunPod (GPU cloud for training)",
        dashboard_url: "https://runpod.io/",
        ui_toggle: Some("runpod"),
    },
    // RunPod S3 credentials — not read by any MCP server (no allowlist
    // references them). Not shown in the UI (dead surface); kept in the
    // registry so `credential_urls_for_mcp` can inject them if set via
    // the keychain, preserving the option for a future consumer.
    DataServiceDescriptor {
        env_var: "RUNPOD_S3_ACCESS_KEY",
        credential_key: "runpod_s3_access_key",
        label: "RunPod S3 Access Key (adapter storage)",
        dashboard_url: "https://runpod.io/",
        ui_toggle: None,
    },
    DataServiceDescriptor {
        env_var: "RUNPOD_S3_SECRET",
        credential_key: "runpod_s3_secret",
        label: "RunPod S3 Secret (adapter storage)",
        dashboard_url: "https://runpod.io/",
        ui_toggle: None,
    },
    // RunPod template ID — architecturally a non-secret config value, but
    // currently read by the training server via `ctx.credentials.get` (the
    // keychain injection path). Classified as `Secret` to preserve the
    // current injection path; a future refactor could move it to `config_env`
    // and read via `std::env::var` (matching `HKASK_KANBAN_DB`'s pattern).
    // Not shown in the Data Services UI (it's a config value, not a key to
    // enter; set via the training server's config).
    DataServiceDescriptor {
        env_var: "RUNPOD_TEMPLATE_ID",
        credential_key: "runpod_template_id",
        label: "RunPod Template ID",
        dashboard_url: "https://runpod.io/",
        ui_toggle: None,
    },
    // Nebius project ID — same note as RUNPOD_TEMPLATE_ID above. Shown in
    // the UI because the old UI listed it (operator convenience for the
    // training host config).
    DataServiceDescriptor {
        env_var: "NEBIUS_PROJECT_ID",
        credential_key: "nebius_project_id",
        label: "Nebius Project ID (GPU cloud for training)",
        dashboard_url: "https://nebius.com/",
        ui_toggle: Some("nebius_project_id"),
    },
    // Nebius subnet ID — same note as NEBIUS_PROJECT_ID above.
    DataServiceDescriptor {
        env_var: "NEBIUS_SUBNET_ID",
        credential_key: "nebius_subnet_id",
        label: "Nebius Subnet ID",
        dashboard_url: "https://nebius.com/",
        ui_toggle: Some("nebius_subnet_id"),
    },
    DataServiceDescriptor {
        env_var: "HF_TOKEN",
        credential_key: "hf_token",
        label: "HuggingFace Token",
        dashboard_url: "https://huggingface.co/settings/tokens",
        ui_toggle: Some("hf_token"),
    },
    // FRED (Federal Reserve Economic Data) — read by the prediction-markets
    // MCP server via `ctx.credentials.get("HKASK_FRED_API_KEY")` for live
    // reference-level fetches. Optional (curated static fallback when absent),
    // but an operator who sets it via the settings UI expects it to reach the server.
    // Shown in the Data Services UI as an always-on row (no enable toggle —
    // enabled when the key is present, mirroring `hf_token`/`serpapi`).
    DataServiceDescriptor {
        env_var: "HKASK_FRED_API_KEY",
        credential_key: "fred",
        label: "FRED API Key",
        dashboard_url: "https://fred.stlouisfed.org/docs/api/api_key.html",
        ui_toggle: Some("fred"),
    },
];

/// Build the `(env_var, credential_url)` pairs for all credentials that
/// should be injected into MCP server child processes.
///
/// Reads the `KaskSettings` to determine which data services and inference
/// providers are enabled, and returns the credential URLs for each.
pub fn credential_urls_for_mcp(settings: &super::KaskSettings) -> Vec<(String, String)> {
    let mut urls = Vec::new();

    // Data services — inject secrets (keychain-backed). Services with a
    // settings toggle (`*_enabled` on `KaskDataServiceSettings`) are gated on
    // the toggle; services without a toggle (ABW, DB passphrase, SMTP, RunPod
    // S3/template, FRED, and the no-field services like SerpAPI/Firecrawl/
    // Browserbase/HF token) are injected unconditionally when the keychain
    // entry exists — they have no enable/disable control. The per-MCP-server
    // `credentials` allowlist is the final filter, so listing a key here does
    // not reach a server that doesn't declare it. `build_mcp_server_env`
    // also skips env vars already set in the process environment.
    for desc in DATA_SERVICES {
        let enabled = match desc.credential_key {
            "eodhd" => settings.data_services.eodhd_enabled,
            "fmp" => settings.data_services.fmp_enabled,
            "exa" => settings.data_services.exa_enabled,
            "tavily" => settings.data_services.tavily_enabled,
            "brave" => settings.data_services.brave_enabled,
            "runpod" => settings.data_services.runpod_enabled,
            "nebius_project_id" | "nebius_subnet_id" => settings.data_services.nebius_enabled,
            // No toggle for this service — inject when the keychain entry exists.
            _ => true,
        };
        if enabled {
            urls.push((desc.env_var.to_string(), desc.credential_url()));
        }
    }

    // Inference providers — inject API keys for all providers that have an
    // env var. There are no kask-level toggles for inference providers; zed's
    // native provider infrastructure (Settings → AI → LLM Providers) handles
    // registration. The keychain read in `build_mcp_server_env` is the final
    // filter: if the key isn't in the keychain, it won't be injected.
    // RunPod is skipped (handled by DATA_SERVICES above via `runpod_enabled`).
    // Ollama is skipped (empty env_var — local, no key needed).
    for provider in INFERENCE_PROVIDERS {
        if provider.credential_key == "runpod" || provider.env_var.is_empty() {
            continue;
        }
        urls.push((provider.env_var.to_string(), provider.credential_url()));
    }

    // Note: HKASK_SMTP_PASSWORD is in DATA_SERVICES as a Secret (unconditional
    // injection). The consumer (curator server) gates on smtp_username being
    // non-empty, and `build_mcp_server_env` skips injection when the
    // keychain entry is absent — so emitting the URL unconditionally is
    // harmless when email is not configured.

    urls
}

/// Resolve `(api_url, api_key)` for an embedding model string by reading
/// the API key from the Zed keychain at the provider's `api_url`.
///
/// This is the direct path: parse the provider prefix from the model string
/// (e.g. `DEFAULT_EMBEDDING_MODEL` → the provider prefix), look up the
/// descriptor in `INFERENCE_PROVIDERS`, and read the API key from the
/// keychain at the provider's `api_url` (the same URL the
/// `LanguageModelProvider`'s `ApiKeyState` reads). No `LanguageModelRegistry`
/// lookup, no case-sensitivity traps.
///
/// Returns `None` (after logging a warn) if:
/// - The model string has no recognized provider prefix.
/// - The provider is not in `INFERENCE_PROVIDERS`.
/// - The keychain has no key at the provider's `api_url`.
///
/// Providers with an empty `env_var` (e.g. ollama) are local services that
/// require no API key — the function returns an empty key for them.
pub async fn resolve_embedding_credentials(
    embedding_model: &str,
    credentials_provider: &dyn CredentialsProvider,
    cx: &gpui::AsyncApp,
) -> Option<(String, String)> {
    let provider = embedding_provider_descriptor(embedding_model).or_else(|| {
        tracing::warn!(
            "Embedding model '{}' has no recognized provider prefix \
             (expected e.g. 'OpenRouter/...'). \
             Set kask.corpus.embedding_model to a provider-prefixed name, \
             or set HKASK_EMBEDDING_MODEL.",
            embedding_model
        );
        None
    })?;

    // Local providers (e.g. ollama) don't require an API key — an empty
    // `env_var` signals "no key needed." Return an empty key so the embedding
    // port can connect without authentication.
    if provider.env_var.is_empty() {
        return Some((provider.api_url.to_string(), String::new()));
    }

    // Read the API key from the Zed keychain at the provider's `api_url`.
    // This is the same URL the `LanguageModelProvider`'s `ApiKeyState` reads,
    // so the embedding port uses the same key the provider uses. The
    // `mirror_kask_credentials_to_providers` call at startup ensures a key
    // set via the kask settings UI (`kask://credentials/<key>`) is mirrored
    // to the provider's `api_url`.
    let api_key = match credentials_provider
        .read_credentials(provider.api_url, cx)
        .await
    {
        Ok(Some((_, bytes))) => match String::from_utf8(bytes) {
            Ok(key) if !key.is_empty() => key,
            _ => {
                tracing::warn!(
                    "Embedding provider '{}' — keychain entry at {} is empty or invalid. \
                     Embedding-based recall will not work until the key is set.",
                    provider.id,
                    provider.api_url
                );
                return None;
            }
        },
        Ok(None) => {
            tracing::warn!(
                "Embedding provider '{}' — no key found in keychain at {}. \
                 Embedding-based recall will not work until the key is set \
                 via Settings → Kask → Data Services.",
                provider.id,
                provider.api_url
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(
                "Embedding provider '{}' — failed to read key from keychain at {}: {error}. \
                 Embedding-based recall will not work until the key is set.",
                provider.id,
                provider.api_url
            );
            return None;
        }
    };

    Some((provider.api_url.to_string(), api_key))
}

/// Find the `InferenceProviderDescriptor` for an embedding model string by
/// matching its provider prefix (case-insensitive) against `INFERENCE_PROVIDERS`.
fn embedding_provider_descriptor(
    embedding_model: &str,
) -> Option<&'static InferenceProviderDescriptor> {
    for provider in INFERENCE_PROVIDERS {
        let prefix = format!("{}/", provider.id);
        if embedding_model.len() >= prefix.len()
            && embedding_model[..prefix.len()].eq_ignore_ascii_case(&prefix)
        {
            return Some(provider);
        }
    }
    None
}

/// Mirror inference-provider API keys from the kask credential store
/// (`kask://credentials/<key>`) to each provider's `api_url` in the Zed
/// keychain so the `LanguageModelProvider`'s `ApiKeyState` finds them.
///
/// The kask settings UI writes keys to `kask://credentials/<credential_key>`,
/// but each provider's `ApiKeyState` reads from the provider's `api_url`
/// (e.g. `https://openrouter.ai/api/v1`). Without this mirror, a key set via
/// the kask settings UI is invisible to the provider — models never load.
///
/// For each provider in `INFERENCE_PROVIDERS` that has a non-empty `env_var`
/// (i.e. requires a key) and a key present at `kask://credentials/<key>`,
/// this writes the key to the provider's `api_url` in the Zed keychain —
/// but only if no key is already present at `api_url` (doesn't clobber a
/// key entered via the provider's own settings UI).
///
/// Per the `.rules` trap "Process-global hooks set at runtime need a
/// startup-failure signal": `tracing::info!` on success, `tracing::warn!`
/// on failure. Runs in the deferred task because it needs the
/// `CredentialsProvider` (app-global, available post-init).
pub fn mirror_kask_credentials_to_providers(
    credentials_provider: &Arc<dyn CredentialsProvider>,
    cx: &mut App,
) -> Task<()> {
    let credentials_provider = credentials_provider.clone();
    cx.spawn(async move |cx| {
        for provider in INFERENCE_PROVIDERS {
            // Skip providers that don't require a key (e.g. ollama).
            if provider.env_var.is_empty() {
                continue;
            }

            let kask_url = provider.credential_url();

            // Read the key from the kask credential store.
            let key = match credentials_provider.read_credentials(&kask_url, cx).await {
                Ok(Some((_, bytes))) => match String::from_utf8(bytes) {
                    Ok(key) if !key.is_empty() => key,
                    _ => continue, // Empty or invalid — skip.
                },
                Ok(None) => continue, // No key in the kask store — skip.
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.kask_bridge",
                        %error,
                        credential_url = %kask_url,
                        provider = %provider.name,
                        "Failed to read API key from kask credential store for mirror"
                    );
                    continue;
                }
            };

            // Write to the Zed keychain under the provider's api_url, but
            // only if no key is already present — don't clobber a key entered
            // via the provider's own settings UI.
            let api_url = provider.api_url;
            match credentials_provider.read_credentials(api_url, cx).await {
                Ok(Some(_)) => {
                    // Zed keychain already has a key at api_url — preserve it.
                    continue;
                }
                Ok(None) => {} // No key — proceed to write.
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.kask_bridge",
                        %error,
                        api_url = %api_url,
                        provider = %provider.name,
                        "Failed to check Zed keychain for existing key — skipping mirror to avoid clobbering"
                    );
                    continue;
                }
            }

            match credentials_provider
                .write_credentials(api_url, "Bearer", key.as_bytes(), cx)
                .await
            {
                Ok(()) => tracing::info!(
                    target: "hkask.kask_bridge",
                    api_url = %api_url,
                    provider = %provider.name,
                    "Mirrored API key from kask credential store to Zed keychain"
                ),
                Err(error) => tracing::warn!(
                    target: "hkask.kask_bridge",
                    %error,
                    api_url = %api_url,
                    provider = %provider.name,
                    "Failed to mirror API key to Zed keychain — \
                     the provider will not find the key via ApiKeyState. \
                     Remediation: enter the key in Settings → AI → LLM Providers."
                ),
            }
        }
    })
}
