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
//!
//! ONE key, ONE location: an inference provider's API key lives in the
//! keychain at the provider's `api_url` — the same slot zed's
//! `LanguageModelProvider`'s `ApiKeyState` reads. Every consumer
//! (`ApiKeyState`, `build_mcp_server_env` via `credential_urls_for_mcp`,
//! `resolve_embedding_credentials`, the IPC batch/rerank credential reads)
//! resolves that one slot, so the key cannot exist in two places that
//! diverge. The former `kask://credentials/<credential_key>` duplicate was
//! the 2026-08-31 split-brain: a stale copy there fed MCP servers a dead key
//! while the user's fresh key sat unread at `api_url` (DeepInfra 401). Those
//! legacy slots are dead data nothing reads — there is no migration and no
//! fallback; a missing key surfaces as `permission_denied` naming the env var.
//! Operators set provider keys via Settings → AI → LLM Providers.

use credentials_provider::CredentialsProvider;

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
    /// Also the keychain URL the provider's API key lives under — the same
    /// slot zed's `ApiKeyState` reads (`credential_url_for_key` maps the
    /// provider's `credential_key` here).
    pub api_url: &'static str,
    /// The env var name that MCP servers and hKask read for this provider's key.
    pub env_var: &'static str,
    /// The credential key that identifies this provider's key for lookups
    /// (`credential_url_for_key` maps it to the provider's `api_url`
    /// keychain slot — the ONE location for the key).
    pub credential_key: &'static str,
    /// Dashboard URL where the user can obtain an API key.
    pub dashboard_url: &'static str,
}

/// The inference providers used for credential injection and embedding
/// model resolution. Providers are registered in zed's native Settings →
/// AI → LLM Providers, not via kask settings.
///
/// OpenRouter is included for API-key injection into MCP servers.
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
    // `openai_compatible` entry. Its key lives at the provider `api_url`
    // (`https://api.runpod.io`) — the same slot the RunPod provider's
    // `ApiKeyState` reads — and `credential_urls_for_mcp` injects it into
    // MCP server env from there (`RUNPOD_API_KEY`). RunPod also has a
    // `DATA_SERVICES` row (the Data Services settings UI entry); its env var
    // matches this one, so the injection loop emits the pair exactly once.
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
    // signals "no key needed" to `resolve_embedding_credentials` and skips
    // it in `credential_urls_for_mcp` (no key to inject). Listed so the
    // default embedding model resolves without a warning.
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
    // embedding model routes through this provider.
    // Operators must set `DEEPINFRA_API_KEY` (via Settings → AI → LLM
    // Providers, which writes the keychain slot at the provider `api_url`, or
    // via the env var).
    InferenceProviderDescriptor {
        id: "DeepInfra",
        name: "DeepInfra",
        api_url: "https://api.deepinfra.com/v1/openai",
        env_var: "DEEPINFRA_API_KEY",
        credential_key: "deepinfra",
        dashboard_url: "https://deepinfra.com/",
    },
];

/// A typed descriptor for a data service credential — the single source of
/// truth for data service env vars, credential keys, and display metadata.
/// Replaces the former `DATA_SERVICE_CREDENTIALS` `&[(&str, &str)]` 2-tuple
/// and the settings UI's parallel `DATA_SERVICES` `&[(&str, &str, &str, &str)]`
/// 4-tuple, which had overlapping fields in different positions.
pub struct DataServiceDescriptor {
    /// The env var name that MCP servers read for this credential.
    pub env_var: &'static str,
    /// The credential key — `credential_url_for_key` resolves it to the
    /// keychain URL. Used as the UI's row key.
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

/// Find the inference-provider descriptor for a credential key. Providers
/// with an empty `env_var` (ollama) have no key and are excluded — a caller
/// needing a credential for them is a table-divergence bug.
pub fn provider_by_credential_key(
    credential_key: &str,
) -> Option<&'static InferenceProviderDescriptor> {
    INFERENCE_PROVIDERS
        .iter()
        .find(|p| p.credential_key == credential_key && !p.env_var.is_empty())
}

/// The canonical keychain URL for a credential, keyed by `credential_key`:
/// inference-provider-backed credentials (openrouter, deepinfra, runpod) live
/// at the provider's `api_url` — the same slot zed's `ApiKeyState` reads —
/// while pure data-service credentials live at
/// `kask://credentials/<credential_key>`. One key, one location: both the
/// settings UI and `credential_urls_for_mcp` resolve their URLs through this
/// function, so no consumer can read a slot the writer didn't target.
pub fn credential_url_for_key(credential_key: &str) -> String {
    match provider_by_credential_key(credential_key) {
        Some(provider) => provider.api_url.to_string(),
        None => format!("{KASK_CREDENTIAL_NAMESPACE}/{credential_key}"),
    }
}

/// The canonical registry of data service credentials. The single source of
/// truth consumed by:
/// - `credential_urls_for_mcp` (builds keychain URLs for MCP env injection)
/// - the settings UI (`data_services.rs` renders rows from this registry)
/// - the coverage governance test (asserts MCP server allowlists align)
///
/// Inference-provider-backed entries (RunPod) resolve their keychain URL
/// through `credential_url_for_key` to the provider's `api_url` slot — see
/// the module docs for the one-key-one-location invariant.
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
    // RunPod — the ONE entry whose key belongs to an inference provider: it
    // lives at the provider `api_url` slot (`https://api.runpod.io`, where the
    // RunPod `ApiKeyState` reads it), not at `kask://credentials/runpod`.
    // `credential_url_for_key("runpod")` maps this row to that slot, so the
    // settings UI and `credential_urls_for_mcp` both read/write the same
    // location the provider does. Shown in the UI as the training/GPU row.
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
    // URLs resolve through `credential_url_for_key`, so inference-provider-
    // backed rows (RunPod) read the provider's `api_url` slot — the same
    // slot zed's `ApiKeyState` and the settings UI use.
    for desc in DATA_SERVICES {
        urls.push((
            desc.env_var.to_string(),
            credential_url_for_key(desc.credential_key),
        ));
    }

    // Inference providers — inject every provider's key from its `api_url`
    // keychain slot, the same slot zed's `ApiKeyState` reads, so the child
    // process and zed's provider infrastructure always see the same key.
    // This is the 2026-08-31 401 fix: the child previously read a legacy
    // `kask://credentials/<key>` slot that no update path ever refreshed,
    // so a key rotated via Settings → AI → LLM Providers left MCP servers
    // authenticating with the dead key. Providers whose env var a
    // DATA_SERVICES entry already carries (RunPod) are skipped — the loop
    // above emits that pair exactly once, at the same `api_url`. Ollama is
    // skipped (empty env_var — local, no key needed).
    for provider in INFERENCE_PROVIDERS {
        if provider.env_var.is_empty()
            || DATA_SERVICES
                .iter()
                .any(|desc| desc.env_var == provider.env_var)
        {
            continue;
        }
        urls.push((provider.env_var.to_string(), provider.api_url.to_string()));
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
    // so the embedding port uses the same key the provider uses. It is the
    // ONE location for the key — a key entered via Settings → AI → LLM
    // Providers is immediately visible here (and to MCP server children via
    // `credential_urls_for_mcp`).
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
                 via Settings → AI → LLM Providers.",
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

    // zed-kask: pins the inference-provider credential consolidation (the
    // 2026-08-31 split-brain fix — successor to the D29 mirror contract):
    // (a) every inference-provider key — RunPod included — resolves to the
    //     provider `api_url` slot via `credential_url_for_key`, so the
    //     settings UI, MCP env injection, and zed's `ApiKeyState` all read
    //     ONE location;
    // (b) `credential_urls_for_mcp` injects each provider env var exactly
    //     once, at the `api_url` slot — the legacy
    //     `kask://credentials/<key>` slots feed nothing.
    //
    // NOT covered here: "RunPod is absent from the openai_compatible
    // registration set" — no such set exists in kask_bridge (providers are
    // registered via zed's native Settings → AI → LLM Providers). RunPod's
    // non-registration as openai_compatible is enforced by the dedicated
    // `LanguageModelProvider` in `crates/language_models/src/provider/runpod.rs`,
    // outside this crate.
    #[test]
    fn inference_provider_keys_resolve_to_the_provider_slot() {
        // (a) one key, one location: the provider api_url is the canonical slot.
        for provider in INFERENCE_PROVIDERS {
            if provider.env_var.is_empty() {
                continue; // ollama — no key exists at all
            }
            assert_eq!(
                super::credential_url_for_key(provider.credential_key),
                provider.api_url,
                "credential key '{}' must resolve to the provider api_url slot",
                provider.credential_key
            );
        }
        // Pure data-service keys keep the kask namespace.
        assert_eq!(
            super::credential_url_for_key("exa"),
            "kask://credentials/exa",
            "non-provider credential keys must stay in the kask namespace"
        );
        // RunPod's S3 credentials are data-service keys, NOT the provider key —
        // they must not be swept into the provider slot.
        assert_eq!(
            super::credential_url_for_key("runpod_s3_access_key"),
            "kask://credentials/runpod_s3_access_key"
        );
    }

    #[test]
    fn credential_urls_for_mcp_injects_provider_keys_from_their_api_url_slot() {
        let urls = super::credential_urls_for_mcp();

        // Every inference-provider env var appears exactly once, resolved to
        // the provider api_url slot (the same slot ApiKeyState reads).
        let expected = [
            ("OPENROUTER_API_KEY", "https://openrouter.ai/api/v1"),
            ("DEEPINFRA_API_KEY", "https://api.deepinfra.com/v1/openai"),
            ("RUNPOD_API_KEY", "https://api.runpod.io"),
        ];
        for (env_var, api_url) in expected {
            let matches: Vec<&(String, String)> =
                urls.iter().filter(|(var, _)| var == env_var).collect();
            assert_eq!(
                matches.len(),
                1,
                "{env_var} must be injected exactly once (RunPod appears in both \
                 registries — double-injection would leave the child with an \
                 arbitrary one of the two slots)"
            );
            assert_eq!(
                matches[0].1, api_url,
                "{env_var} must inject from the provider api_url slot"
            );
        }

        // The legacy kask slots must feed NOTHING — a consumer reintroducing
        // them re-creates the split-brain (stale key → 401).
        for legacy in [
            "kask://credentials/openrouter",
            "kask://credentials/deepinfra",
            "kask://credentials/runpod",
        ] {
            assert!(
                !urls.iter().any(|(_, url)| url == legacy),
                "legacy slot {legacy} must not appear in the injection set"
            );
        }

        // Data services keep injecting from the kask namespace.
        assert!(
            urls.iter()
                .any(|(var, url)| var == "HKASK_EXA_API_KEY" && url == "kask://credentials/exa")
        );
    }
}
