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

// The credential namespace constant lives in the `credentials` module and is
// re-exported from the crate root. Importing from the defining module here
// documents the true location and avoids routing through the root re-export.
use crate::credentials::KASK_CREDENTIAL_NAMESPACE;

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
    /// Whether `credential_urls_for_mcp` injects this provider's key for MCP
    /// servers. `false` when `DATA_SERVICES` handles injection (RunPod — the
    /// same credential_key appears in both registries; DATA_SERVICES is the
    /// single injector to avoid double-injection).
    pub inject_for_mcp: bool,
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
        inject_for_mcp: true,
    },
    // RunPod has a dedicated `LanguageModelProvider` (D29), not an
    // `openai_compatible` entry. It's listed here so
    // `mirror_kask_credentials_to_providers` writes the key to the Zed
    // keychain under `api_url` (where the RunPod provider's `ApiKeyState`
    // reads it), in addition to the `kask://credentials/runpod` write
    // handled by `DATA_SERVICES`. Skipped in
    // `credential_urls_for_mcp`'s `INFERENCE_PROVIDERS` loop (MCP injection
    // is handled by the `DATA_SERVICES` loop — the key's presence is the toggle).
    InferenceProviderDescriptor {
        id: "RunPod",
        name: "RunPod",
        api_url: "https://api.runpod.io",
        env_var: "RUNPOD_API_KEY",
        credential_key: "runpod",
        dashboard_url: "https://www.runpod.io/",
        inject_for_mcp: false,
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
        inject_for_mcp: true,
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
        inject_for_mcp: true,
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
    /// Used as the UI's row key.
    pub credential_key: &'static str,
    /// Human-readable label shown in the settings UI.
    pub label: &'static str,
    /// Dashboard URL where the user can obtain or manage the credential.
    pub dashboard_url: &'static str,
    /// Whether this credential should appear as a row in the Data Services
    /// settings UI. Credentials managed elsewhere (e.g. `HKASK_SMTP_PASSWORD`
    /// in the Curator page, `HKASK_DB_PASSPHRASE`) set this to `false`.
    pub shows_in_ui: bool,
}

impl DataServiceDescriptor {
    /// The keychain URL for this credential in the kask namespace.
    pub fn credential_url(&self) -> String {
        format!("{KASK_CREDENTIAL_NAMESPACE}/{}", self.credential_key)
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
        shows_in_ui: true,
    },
    DataServiceDescriptor {
        env_var: "HKASK_FMP_API_KEY",
        credential_key: "fmp",
        label: "FMP (Financial Modeling Prep)",
        dashboard_url: "https://site.financialmodelingprep.com/developer/docs",
        shows_in_ui: true,
    },
    DataServiceDescriptor {
        env_var: "HKASK_EXA_API_KEY",
        credential_key: "exa",
        label: "Exa",
        dashboard_url: "https://dashboard.exa.ai/api-keys",
        shows_in_ui: true,
    },
    DataServiceDescriptor {
        env_var: "HKASK_TAVILY_API_KEY",
        credential_key: "tavily",
        label: "Tavily",
        dashboard_url: "https://app.tavily.com/api-key",
        shows_in_ui: true,
    },
    DataServiceDescriptor {
        env_var: "HKASK_BRAVE_API_KEY",
        credential_key: "brave",
        label: "Brave Search",
        dashboard_url: "https://api.search.brave.com/app/subscriptions",
        shows_in_ui: true,
    },
    DataServiceDescriptor {
        env_var: "HKASK_SERPAPI_API_KEY",
        credential_key: "serpapi",
        label: "SerpAPI (Google Search)",
        dashboard_url: "https://serpapi.com/dashboard",
        shows_in_ui: true,
    },
    DataServiceDescriptor {
        env_var: "HKASK_FIRECRAWL_API_KEY",
        credential_key: "firecrawl",
        label: "Firecrawl (web scraping)",
        dashboard_url: "https://firecrawl.dev/",
        shows_in_ui: true,
    },
    // ABW API key — not shown in the Data Services UI (no toggle, managed
    // via the keychain by the swarm server's governed launch path).
    DataServiceDescriptor {
        env_var: "HKASK_ABW_API_KEY",
        credential_key: "hkask_abw_api_key",
        label: "ABW API Key",
        dashboard_url: "",
        shows_in_ui: false,
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
        shows_in_ui: false,
    },
    // Curator SMTP password — managed in the Curator Email settings page,
    // not in the Data Services page (avoids duplicate reset surfaces).
    DataServiceDescriptor {
        env_var: "HKASK_SMTP_PASSWORD",
        credential_key: "hkask_smtp_password",
        label: "SMTP Password",
        dashboard_url: "",
        shows_in_ui: false,
    },
    DataServiceDescriptor {
        env_var: "RUNPOD_API_KEY",
        credential_key: "runpod",
        label: "RunPod (GPU cloud for training)",
        dashboard_url: "https://runpod.io/",
        shows_in_ui: true,
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
        shows_in_ui: false,
    },
    DataServiceDescriptor {
        env_var: "RUNPOD_S3_SECRET",
        credential_key: "runpod_s3_secret",
        label: "RunPod S3 Secret (adapter storage)",
        dashboard_url: "https://runpod.io/",
        shows_in_ui: false,
    },
    // RUNPOD_TEMPLATE_ID was here as a Secret, but it is architecturally a
    // non-secret config value (a RunPod template ID). It has been moved to
    // the training server's `config_env` allowlist and is read via
    // `std::env::var` in hkask_mcp_training.rs. The runpod provider has a
    // real fallback when empty (DEFAULT_RUNPOD_DOCKER_IMAGE). Keeping it in
    // DATA_SERVICES caused a spurious "not set or empty" warning on every
    // launch because `build_mcp_server_env` looked for a keychain entry
    // that was never written — the child had a working default path the
    // bridge couldn't see.
    // Nebius project ID — same note as RUNPOD_TEMPLATE_ID above. Shown in
    // the UI because the old UI listed it (operator convenience for the
    // training host config).
    DataServiceDescriptor {
        env_var: "NEBIUS_PROJECT_ID",
        credential_key: "nebius_project_id",
        label: "Nebius Project ID (GPU cloud for training)",
        dashboard_url: "https://nebius.com/",
        shows_in_ui: true,
    },
    // Nebius subnet ID — same note as NEBIUS_PROJECT_ID above.
    DataServiceDescriptor {
        env_var: "NEBIUS_SUBNET_ID",
        credential_key: "nebius_subnet_id",
        label: "Nebius Subnet ID",
        dashboard_url: "https://nebius.com/",
        shows_in_ui: true,
    },
    DataServiceDescriptor {
        env_var: "HF_TOKEN",
        credential_key: "hf_token",
        label: "HuggingFace Token",
        dashboard_url: "https://huggingface.co/settings/tokens",
        shows_in_ui: true,
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
        shows_in_ui: true,
    },
];

/// Build the `(env_var, credential_url)` pairs for all credentials that
/// should be injected into MCP server child processes.
///
/// Returns credential URLs for all data services and inference providers.
/// The key's presence in env or keychain is the toggle — no settings bool.
pub fn credential_urls_for_mcp() -> Vec<(String, String)> {
    let mut urls = Vec::new();

    // Data services — inject unconditionally. The key's presence is the
    // toggle: if it's not in the parent env or keychain, `build_mcp_server_env`
    // skips it and the server surfaces `permission_denied`. A separate
    // `*_enabled` bool on settings is a spandrel — it can only ever block a
    // key that is already configured, which is pure negative value.
    for desc in DATA_SERVICES {
        urls.push((desc.env_var.to_string(), desc.credential_url()));
    }

    // Inference providers — inject API keys for all providers that have an
    // env var. There are no kask-level toggles for inference providers; zed's
    // native provider infrastructure (Settings → AI → LLM Providers) handles
    // registration. The keychain read in `build_mcp_server_env` is the final
    // filter: if the key isn't in the keychain, it won't be injected.
    // RunPod is skipped (handled by DATA_SERVICES above).
    // Ollama is skipped (empty env_var — local, no key needed).
    for provider in INFERENCE_PROVIDERS {
        if !provider.inject_for_mcp || provider.env_var.is_empty() {
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

/// Mirror a single credential from `kask://credentials/<credential_key>` to
/// the corresponding inference provider's `api_url` in the Zed keychain.
/// Called by the settings UI after a user writes or deletes a key, so the
/// `LanguageModelProvider`'s `ApiKeyState` sees the change without requiring
/// a restart.
///
/// When `api_key` is `Some`, writes the key to the provider's `api_url`
/// (overwriting any existing key — the user explicitly set it via the UI).
/// When `api_key` is `None`, deletes the key at the provider's `api_url`.
///
/// Returns `Ok(true)` if a provider was found and the mirror ran, `Ok(false)`
/// if no inference provider matches the `credential_key` (e.g. it's a data
/// service key, not an inference provider key), or `Err` on keychain failure.
pub async fn mirror_credential_to_provider(
    credentials_provider: &Arc<dyn CredentialsProvider>,
    credential_key: &str,
    api_key: Option<&str>,
    cx: &gpui::AsyncApp,
) -> anyhow::Result<bool> {
    let Some(provider) = INFERENCE_PROVIDERS
        .iter()
        .find(|p| p.credential_key == credential_key && !p.env_var.is_empty())
    else {
        return Ok(false);
    };

    let api_url = provider.api_url;
    match api_key {
        Some(key) if !key.is_empty() => {
            credentials_provider
                .write_credentials(api_url, "Bearer", key.as_bytes(), cx)
                .await?;
            tracing::info!(
                target: "hkask.kask_bridge",
                api_url = %api_url,
                provider = %provider.name,
                "Mirrored API key from settings UI to Zed keychain"
            );
        }
        _ => {
            // None or empty — delete the key at api_url so the provider
            // sees it as removed.
            match credentials_provider.delete_credentials(api_url, cx).await {
                Ok(()) => tracing::info!(
                    target: "hkask.kask_bridge",
                    api_url = %api_url,
                    provider = %provider.name,
                    "Deleted API key from Zed keychain (settings UI reset)"
                ),
                Err(e) => tracing::warn!(
                    target: "hkask.kask_bridge",
                    error = %e,
                    api_url = %api_url,
                    provider = %provider.name,
                    "Failed to delete API key from Zed keychain — may be stale"
                ),
            }
        }
    }
    Ok(true)
}

/// Strip the provider prefix from a model string, case-insensitive.
///
/// Accepts long-form prefixes (`OpenRouter/`,
/// `RunPod/`, `ollama/`, `DeepInfra/`).
/// Returns the bare model id. If no prefix is recognized, returns the
/// string unchanged (the API will reject it, which surfaces a clear error).
///
/// The recognized prefixes are driven by the bridge's `INFERENCE_PROVIDERS`
/// table — adding a provider there automatically extends stripping here.
/// This is the single source of truth: a previous hardcoded `LONG_FORM`
/// table diverged from `INFERENCE_PROVIDERS` and omitted `DeepInfra/`,
/// causing the default embedding model to reach the DeepInfra API unstripped
/// and 404 with `model_not_found`.
pub(crate) fn strip_provider_prefix(model: &str) -> String {
    for provider in INFERENCE_PROVIDERS {
        let prefix = provider.id;
        if model.len() > prefix.len() + 1
            && model.as_bytes()[prefix.len()] == b'/'
            && model[..prefix.len()].eq_ignore_ascii_case(prefix)
        {
            return model[prefix.len() + 1..].to_string();
        }
    }

    // No recognized prefix — return as-is. The API will reject an unknown
    // model, which surfaces a clear error to the operator.
    model.to_string()
}

#[cfg(test)]
mod tests {
    use super::{INFERENCE_PROVIDERS, strip_provider_prefix};

    // Regression for the embedding 404: `DEFAULT_EMBEDDING_MODEL` is
    // `DeepInfra/Qwen/Qwen3-Embedding-0.6B`, but the old hardcoded
    // `LONG_FORM` table omitted `DeepInfra/`, so the full prefixed string
    // reached the DeepInfra `/embeddings` endpoint and 404'd with
    // `model_not_found`. The fix drives the prefix set from
    // `INFERENCE_PROVIDERS`, which registers `DeepInfra`.
    #[test]
    fn strip_provider_prefix_handles_deepinfra() {
        assert_eq!(
            strip_provider_prefix("DeepInfra/Qwen/Qwen3-Embedding-0.6B"),
            "Qwen/Qwen3-Embedding-0.6B"
        );
    }

    // Case-insensitivity is load-bearing: operators may write `deepinfra/...`
    // or `OPENROUTER/...` in settings. The `eq_ignore_ascii_case` match
    // must cover all casings.
    #[test]
    fn strip_provider_prefix_is_case_insensitive() {
        assert_eq!(
            strip_provider_prefix("deepinfra/Qwen/Qwen3-Embedding-0.6B"),
            "Qwen/Qwen3-Embedding-0.6B"
        );
        assert_eq!(
            strip_provider_prefix("OPENROUTER/z-ai/glm-5.2"),
            "z-ai/glm-5.2"
        );
    }

    // Every provider registered in `INFERENCE_PROVIDERS` must be strippable.
    // This is the single-source-of-truth contract: if a provider is added to
    // the table, stripping comes for free. If this test breaks, either the
    // table or the stripper diverged.
    #[test]
    fn strip_provider_prefix_handles_every_registered_provider() {
        for provider in INFERENCE_PROVIDERS {
            let prefixed = format!("{}/some-model", provider.id);
            assert_eq!(
                strip_provider_prefix(&prefixed),
                "some-model",
                "provider {} not strippable",
                provider.id
            );
        }
    }

    // Unrecognized prefixes pass through unchanged — the API rejects them
    // with a clear error. This is the documented fallback behavior.
    #[test]
    fn strip_provider_prefix_passes_through_unknown_prefix() {
        assert_eq!(strip_provider_prefix("no-slash"), "no-slash");
        assert_eq!(
            strip_provider_prefix("UnknownProvider/some-model"),
            "UnknownProvider/some-model"
        );
    }

    // ── D29: RunPod credential mirror ─────────────────────────────────────
    //
    // A recording `CredentialsProvider` stand-in: reads consult a seeded
    // secrets map (plus anything previously written), writes are recorded
    // so tests can assert exactly which URLs the mirror touched. Modeled on
    // `MockCredentialsProvider` in `mcp_servers.rs` tests, extended with
    // write recording + read-back so the round-trip is observable.
    struct RecordingCredentialsProvider {
        secrets: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
        writes: std::sync::Mutex<Vec<(String, String, Vec<u8>)>>,
    }

    impl RecordingCredentialsProvider {
        fn new(secrets: std::collections::HashMap<String, Vec<u8>>) -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                secrets: std::sync::Mutex::new(secrets),
                writes: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn written_urls(&self) -> Vec<String> {
            self.writes
                .lock()
                .expect("writes lock poisoned")
                .iter()
                .map(|(url, _, _)| url.clone())
                .collect()
        }
    }

    impl credentials_provider::CredentialsProvider for RecordingCredentialsProvider {
        fn read_credentials<'a>(
            &'a self,
            url: &'a str,
            _cx: &'a gpui::AsyncApp,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<Option<(String, Vec<u8>)>>> + 'a>,
        > {
            let result = self
                .secrets
                .lock()
                .expect("secrets lock poisoned")
                .get(url)
                .cloned()
                .map(|pw| ("user".to_string(), pw));
            Box::pin(async move { Ok(result) })
        }

        fn write_credentials<'a>(
            &'a self,
            url: &'a str,
            username: &'a str,
            password: &'a [u8],
            _cx: &'a gpui::AsyncApp,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + 'a>> {
            self.writes.lock().expect("writes lock poisoned").push((
                url.to_string(),
                username.to_string(),
                password.to_vec(),
            ));
            self.secrets
                .lock()
                .expect("secrets lock poisoned")
                .insert(url.to_string(), password.to_vec());
            Box::pin(async move { Ok(()) })
        }

        fn delete_credentials<'a>(
            &'a self,
            _url: &'a str,
            _cx: &'a gpui::AsyncApp,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + 'a>> {
            Box::pin(async { Ok(()) })
        }
    }

    // zed-kask: D29 — pins the RunPod credential mirror contract:
    // (a) a key at `kask://credentials/runpod` is mirrored to
    //     `https://api.runpod.io` (round-trip: the written value reads back
    //     through the provider),
    // (b) an existing key at `https://api.runpod.io` is NOT clobbered,
    // (c) RunPod is in `INFERENCE_PROVIDERS` for the mirror but with
    //     `inject_for_mcp == false` — `DATA_SERVICES` is the single MCP
    //     injector (same credential_key in both registries; no
    //     double-injection).
    //
    // NOT covered here: "RunPod is absent from the openai_compatible
    // registration set" — no such set exists in kask_bridge (providers are
    // registered via zed's native Settings → AI → LLM Providers). RunPod's
    // non-registration as openai_compatible is enforced by the dedicated
    // `LanguageModelProvider` in `crates/language_models/src/provider/runpod.rs`,
    // outside this crate.
    #[gpui::test]
    async fn runpod_is_mirrored_to_keychain_but_not_registered_as_openai_compatible(
        cx: &mut gpui::TestAppContext,
    ) {
        let runpod = INFERENCE_PROVIDERS
            .iter()
            .find(|p| p.id == "RunPod")
            .expect("RunPod must be in INFERENCE_PROVIDERS so the mirror covers it");
        assert_eq!(
            runpod.credential_url(),
            "kask://credentials/runpod",
            "RunPod's kask credential URL must be the canonical namespace entry"
        );
        assert_eq!(
            runpod.api_url, "https://api.runpod.io",
            "RunPod's mirror target must be the GraphQL/API domain its ApiKeyState reads"
        );
        assert!(
            !runpod.env_var.is_empty(),
            "RunPod must require a key or the mirror loop skips it"
        );
        assert!(
            !runpod.inject_for_mcp,
            "RunPod must not be MCP-injected from INFERENCE_PROVIDERS — DATA_SERVICES \
             is the single injector (same credential_key in both registries)"
        );
        assert!(
            super::DATA_SERVICES
                .iter()
                .any(|d| d.credential_key == runpod.credential_key),
            "RunPod's credential_key must also appear in DATA_SERVICES (the single \
             MCP injector) or its key would never reach MCP servers"
        );

        // (a) kask key present, no existing key at the target URL → mirror writes.
        let mut secrets = std::collections::HashMap::new();
        secrets.insert(
            "kask://credentials/runpod".to_string(),
            b"kask-runpod-secret".to_vec(),
        );
        let provider = RecordingCredentialsProvider::new(secrets);
        let credentials: std::sync::Arc<dyn credentials_provider::CredentialsProvider> =
            provider.clone();
        let task = cx.update(|cx| super::mirror_kask_credentials_to_providers(&credentials, cx));
        cx.run_until_parked();
        drop(task);

        let written_urls = provider.written_urls();
        assert_eq!(
            written_urls,
            vec!["https://api.runpod.io".to_string()],
            "with only the runpod kask key set, the mirror must write exactly RunPod's \
             api_url and nothing else"
        );
        // Round-trip: the mirrored key reads back through the provider at the
        // target URL — not just "a write happened".
        let cx_async = cx.to_async();
        let read_back = credentials
            .read_credentials("https://api.runpod.io", &cx_async)
            .await
            .expect("read after mirror must succeed")
            .expect("mirrored key must be readable at https://api.runpod.io");
        assert_eq!(
            read_back.1,
            b"kask-runpod-secret".to_vec(),
            "the mirrored value must equal the kask-store key"
        );

        // (b) existing key at the target URL → mirror must NOT clobber it.
        let mut secrets = std::collections::HashMap::new();
        secrets.insert(
            "kask://credentials/runpod".to_string(),
            b"kask-runpod-secret".to_vec(),
        );
        secrets.insert(
            "https://api.runpod.io".to_string(),
            b"ui-entered-key".to_vec(),
        );
        let provider = RecordingCredentialsProvider::new(secrets);
        let credentials: std::sync::Arc<dyn credentials_provider::CredentialsProvider> =
            provider.clone();
        let task = cx.update(|cx| super::mirror_kask_credentials_to_providers(&credentials, cx));
        cx.run_until_parked();
        drop(task);

        assert!(
            !provider
                .written_urls()
                .contains(&"https://api.runpod.io".to_string()),
            "the mirror must not write to https://api.runpod.io when a key already \
             exists there (UI-entered keys are preserved)"
        );
        let read_back = credentials
            .read_credentials("https://api.runpod.io", &cx.to_async())
            .await
            .expect("read after mirror must succeed")
            .expect("existing key must still be present");
        assert_eq!(
            read_back.1,
            b"ui-entered-key".to_vec(),
            "the pre-existing key at the target URL must be unchanged"
        );
    }
}
