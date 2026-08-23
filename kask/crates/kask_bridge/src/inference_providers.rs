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
//! - `mirror_env_keys_to_keychain` (mirrors `.env` values into the keychain)
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

/// The inference providers surfaced in kask settings.
///
/// OpenRouter is included for API-key mirroring to MCP servers, not for
/// `openai_compatible` registration (see `ensure_openai_compatible_entries`).
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
    // `openai_compatible` entry. It's listed here so `mirror_env_keys_to_keychain`
    // writes the key to the Zed keychain under `api_url` (where the RunPod
    // provider's `ApiKeyState` reads it), in addition to the
    // `kask://credentials/runpod` write handled by `DATA_SERVICES`. Skipped in
    // `ensure_openai_compatible_entries` (no `openai_compatible` entry) and
    // in `credential_urls_for_mcp`'s `INFERENCE_PROVIDERS` loop (MCP injection
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
    // inject) and `collect_env_keys_for_mirror` (empty env_var filters out).
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
/// - `mirror_env_keys_to_keychain` (mirrors `.env` values to the keychain)
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
    // Not shown in the Data Services UI (no toggle; managed via `.env`
    // or the hkask keystore chain).
    DataServiceDescriptor {
        env_var: "HKASK_DB_PASSPHRASE",
        credential_key: "hkask_db_passphrase",
        label: "DB Passphrase",
        dashboard_url: "",
        ui_toggle: None,
    },
    // Swarm memory SQLCipher passphrase — read by the swarm server at
    // hkask-mcp-swarm/src/config.rs via `HKASK_SWARM_MEMORY_PASSPHRASE`.
    // Registered here so the value is mirrored from `.env` and injected from the
    // keychain by `credential_urls_for_mcp`; without a descriptor the allowlist
    // entry alone would name a credential that nothing ever sources (RR-0061).
    // Distinct from HKASK_DB_PASSPHRASE: the swarm memory store is a separate DB
    // with its own key. No UI toggle — managed via `.env` or the keystore chain.
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
    // registry so the mirror writes them to the keychain if set in `.env`,
    // preserving the option for a future consumer.
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
    // enter; set via `.env` or the training server's config).
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
    // but an operator who sets it in `.env` expects it to reach the server.
    // Shown in the Data Services UI as an always-on row (no enable toggle —
    // enabled when the key is present, mirroring `hf_token`/`serpapi`); also
    // autoloaded from `.env` into the keychain by `mirror_env_keys_to_keychain`.
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

    // Inference providers — inject the API key as the env var the MCP servers
    // and hKask's InferenceConfig expect. RunPod is skipped here: it's in
    // `INFERENCE_PROVIDERS` for the keychain `api_url` mirror (so the RunPod
    // `LanguageModelProvider` finds the key), but MCP env injection is handled
    // by the `DATA_SERVICES` loop above via `runpod_enabled`.
    for provider in INFERENCE_PROVIDERS {
        if provider.credential_key == "runpod" || provider.env_var.is_empty() {
            continue;
        }
        let enabled = match provider.credential_key {
            "openrouter" => settings.inference_providers.openrouter_enabled,
            "deepinfra" => settings.inference_providers.deepinfra_enabled,
            _ => false,
        };
        if enabled {
            urls.push((provider.env_var.to_string(), provider.credential_url()));
        }
    }

    // Note: HKASK_SMTP_PASSWORD is in DATA_SERVICES as a Secret (unconditional
    // injection). The consumer (curator server) gates on smtp_username being
    // non-empty, and `build_mcp_server_env` skips injection when the
    // keychain entry is absent — so emitting the URL unconditionally is
    // harmless when email is not configured.

    urls
}

/// Write `openai_compatible.<provider_id>` entries for enabled providers and
/// remove entries for disabled or removed providers.
///
/// Called by the composition root after `KaskSettings` are loaded. zed's
/// `register_compatible_providers` watches the `openai_compatible` settings
/// section and registers/unregisters providers in `LanguageModelRegistry`
/// automatically.
///
/// OpenRouter is skipped: zed's built-in `OpenRouterLanguageModelProvider`
/// already registers it, so a kask `openai_compatible.OpenRouter` entry would
/// duplicate it in the LLM picker. OpenRouter's kask toggle still mirrors its
/// key to MCP servers via `credential_urls_for_mcp` (which iterates
/// `INFERENCE_PROVIDERS` directly, not this function).
///
/// DeepInfra is registered here as an `openai_compatible.DeepInfra` entry
/// with `auto_discover: true` so zed fetches its full model list (chat,
/// embeddings, vision) via `/v1/models`. The API key is stored in the
/// keychain under the provider's `api_url`, same pattern as any other
/// OpenAI-compatible provider.
pub fn ensure_openai_compatible_entries(cx: &mut App) {
    // Scrub stale entries from removed providers, then write the DeepInfra
    // entry if enabled.

    // Stale `openai_compatible` entries to scrub. The api_url guard avoids
    // removing a user's custom provider that happens to share an id.
    // OpenRouter is included to clean up entries written by prior versions.
    // The last two entries scrub inference providers removed in prior
    // versions — the id + api_url literals are required to match stale
    // user settings.json entries written by prior versions (they are scrub
    // data, not provider support). Together AI was removed 2026-08-22;
    // AtlasCloud was removed 2026-08-20.
    let removed_providers: [(&'static str, &str); 6] = [
        ("fal.ai", "https://api.fal.ai/v1"),
        ("Cline", "https://api.cline.bot/api/v1"),
        ("KiloCode", "https://api.kilo.ai/api/gateway"),
        ("OpenRouter", "https://openrouter.ai/api/v1"),
        ("AtlasCloud", "https://api.atlascloud.ai/v1"),
        ("Together", "https://api.together.xyz/v1"),
    ];

    // Check if DeepInfra is enabled in kask settings.
    let kask_settings = KaskSettings::get_global(cx).clone();
    let deepinfra_enabled = kask_settings.inference_providers.deepinfra_enabled;

    let fs = <dyn fs::Fs>::global(cx);
    SettingsStore::global(cx).update_settings_file(fs, move |content, _| {
        let openai_compatible = content
            .language_models
            .get_or_insert_default()
            .openai_compatible
            .get_or_insert_default();

        // Scrub stale entries for removed providers.
        for (id, known_api_url) in removed_providers {
            let id: std::sync::Arc<str> = std::sync::Arc::from(id);
            if let Some(existing) = openai_compatible.get(&id)
                && existing.api_url == known_api_url
            {
                openai_compatible.remove(&id);
            }
        }

        // Write or remove the DeepInfra entry based on the toggle.
        let deepinfra_id: std::sync::Arc<str> = std::sync::Arc::from("DeepInfra");
        if deepinfra_enabled {
            openai_compatible.insert(
                deepinfra_id,
                settings_content::OpenAiCompatibleSettingsContent {
                    api_url: "https://api.deepinfra.com/v1/openai".to_string(),
                    available_models: Vec::new(),
                    custom_headers: None,
                    auto_discover: true,
                },
            );
        } else {
            // Remove the entry if the toggle is off (and it matches our api_url
            // guard, so we don't remove a user's custom provider).
            if let Some(existing) = openai_compatible.get(&deepinfra_id)
                && existing.api_url == "https://api.deepinfra.com/v1/openai"
            {
                openai_compatible.remove(&deepinfra_id);
            }
        }
    });
}

/// Resolve `(api_url, api_key)` for an embedding model string directly from
/// the `INFERENCE_PROVIDERS` table + env var.
///
/// This is the direct path: parse the provider prefix from the model string
/// (e.g. `DEFAULT_EMBEDDING_MODEL` → the provider prefix), look up the descriptor in
/// `INFERENCE_PROVIDERS`, and read the API key from the env var named in the
/// descriptor (`OPENROUTER_API_KEY`, etc.). No `LanguageModelRegistry` lookup,
/// no GPUI access, no case-sensitivity traps.
///
/// Returns `None` (after logging a warn) if:
/// - The model string has no recognized provider prefix.
/// - The provider is not in `INFERENCE_PROVIDERS`.
/// - The env var is not set (key not loaded).
///
/// Providers with an empty `env_var` (e.g. ollama) are local services that
/// require no API key — the function returns an empty key for them.
pub fn resolve_embedding_credentials(embedding_model: &str) -> Option<(String, String)> {
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

    let api_key = std::env::var(provider.env_var).ok().or_else(|| {
        tracing::warn!(
            "Embedding provider '{}' — env var {} is not set. \
             Embedding-based recall will not work until the key is loaded.",
            provider.id,
            provider.env_var
        );
        None
    })?;

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

/// A credential to mirror from the process environment into the OS keychain.
///
/// Replaces the former `(env_var, api_url, credential_url, key)` 4-tuple
/// that used an empty-string `api_url` sentinel to distinguish data services
/// from inference providers. The enum makes the two-kind distinction
/// type-level: `InferenceProvider` writes two keychain entries (api_url +
/// credential_url); `DataService` writes one (credential_url only).
#[derive(Debug)]
enum MirrorTarget {
    InferenceProvider {
        env_var: String,
        api_url: String,
        credential_url: String,
        key: String,
    },
    DataService {
        env_var: String,
        credential_url: String,
        key: String,
    },
}

impl MirrorTarget {
    fn env_var(&self) -> &str {
        match self {
            Self::InferenceProvider { env_var, .. } => env_var,
            Self::DataService { env_var, .. } => env_var,
        }
    }

    fn credential_url(&self) -> &str {
        match self {
            Self::InferenceProvider { credential_url, .. } => credential_url,
            Self::DataService { credential_url, .. } => credential_url,
        }
    }

    fn key(&self) -> &str {
        match self {
            Self::InferenceProvider { key, .. } => key,
            Self::DataService { key, .. } => key,
        }
    }

    /// The settings-UI remediation path for this credential category.
    fn remediation_path(&self) -> &'static str {
        match self {
            Self::InferenceProvider { .. } => "Settings → Kask → Inference Providers",
            Self::DataService { .. } => "Settings → Kask → Data Services",
        }
    }
}

/// Mirror inference-provider API keys from the process environment into zed's
/// keychain so MCP server child processes (media, corpus, etc.) can read them.
///
/// The main process reads inference keys from `std::env::var` (populated by
/// the `.env` load at startup or by the shell). MCP servers, however, receive
/// their credentials via `build_mcp_server_env`, which reads from the
/// keychain (`kask://credentials/<key>`) — not from the parent process env.
/// Without this mirror, an operator who sets a provider key (e.g.
/// `OPENROUTER_API_KEY`) only in `.env` gets a working main process but
/// MCP servers silently fail with "API key not configured".
///
/// For each provider in `INFERENCE_PROVIDERS` whose env var is set and
/// non-empty, this writes the key to both keychain locations (the provider's
/// `api_url` and `kask://credentials/<credential_key>`). For each secret entry
/// in `DATA_SERVICES` whose env var is set and non-empty, this writes the key
/// only to `kask://credentials/<credential_key>` (data services have no
/// OpenAI-compatible `api_url`).
/// `Config` entries in `DATA_SERVICES` are skipped (non-secret, not keychain-backed).
/// It always writes (overwrites any existing keychain entry with the env
/// value). The env var takes precedence on the next restart because the
/// main process reads `std::env::var` first; the keychain write ensures MCP
/// servers (which read the keychain via `build_mcp_server_env`) see the
/// key even when the env var is later removed from `.env`. When the env var
/// is absent, no mirror happens, so a key the operator set via the settings
/// UI is preserved — matching the main-process precedence rule in
/// `ApiKeyState::load_if_needed` (shell env > .env file > keychain).
///
/// Per the `.rules` trap "Process-global hooks set at runtime need a
/// startup-failure signal":
/// - No inference env vars set → silent no-op (the `.env`-not-found warn at
///   `main.rs` already covers the "not configured" case; a second warn here
///   would be redundant noise).
/// - Env var present, keychain write succeeds → `tracing::info!` naming the env
///   var and the credential URL (confirms the mirror ran).
/// - Env var present, keychain write fails → `tracing::warn!` naming the env
///   var, the error, and the remediation (the main process will use the env
///   var, but MCP servers reading from the keychain will not see this key).
pub fn mirror_env_keys_to_keychain(
    credentials_provider: &Arc<dyn CredentialsProvider>,
    cx: &mut App,
) -> Task<()> {
    let to_mirror = collect_env_keys_for_mirror();
    if to_mirror.is_empty() {
        // Silent no-op: either no `.env` loaded or no inference keys in it.
        // The `.env`-not-found warn in `main.rs` already covers the first
        // case; the second case is a legitimate "user configured some
        // providers via the settings UI only" state.
        return Task::ready(());
    }

    let credentials_provider = credentials_provider.clone();
    cx.spawn(async move |cx| {
        for target in to_mirror {
            // Inference providers write under the api_url (for zed's
            // OpenAI-compatible provider). Data services have no api_url.
            if let MirrorTarget::InferenceProvider { api_url, .. } = &target {
                match credentials_provider
                    .write_credentials(api_url, "Bearer", target.key().as_bytes(), cx)
                    .await
                {
                    Ok(()) => {}
                    Err(e) => {
                        tracing::warn!(
                            target: "hkask.kask_bridge",
                            env_var = %target.env_var(),
                            api_url = %api_url,
                            error = %e,
                            "Failed to mirror env key to keychain at api_url. \
                             The main process will use the env var, but zed's \
                             OpenAI-compatible provider (which reads the keychain \
                             when the env var is absent on a future restart) will \
                             not see this key. Falling through to the \
                             credential_url write so MCP servers still receive it."
                        );
                        // Do NOT `continue` — the credential_url write is an
                        // independent keychain entry (different URL) and MCP
                        // servers read from it, not from api_url. Skipping it
                        // would suppress the MCP-server key on an unrelated
                        // OpenAI-compatible-provider write failure.
                    }
                }
            }
            // Write under the kask credential URL (for MCP env injection).
            // Both inference providers and data services write this entry.
            let credential_url = target.credential_url();
            match credentials_provider
                .write_credentials(credential_url, "kask", target.key().as_bytes(), cx)
                .await
            {
                Ok(()) => {
                    tracing::info!(
                        target: "hkask.kask_bridge",
                        env_var = %target.env_var(),
                        credential_url = %credential_url,
                        "Mirrored env key to keychain for MCP server env injection"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.kask_bridge",
                        env_var = %target.env_var(),
                        credential_url = %credential_url,
                        error = %e,
                        "Failed to mirror env key to keychain at credential_url. \
                         The main process will use the env var, but MCP servers \
                         that read from the keychain will not see this key. \
                         Remediation: re-enter the key in {}."
                        , target.remediation_path()
                    );
                }
            }
        }
    })
}

/// Collect `MirrorTarget` entries for every inference provider and data
/// service secret whose env var is set and non-empty. Extracted from
/// `mirror_env_keys_to_keychain` for testability (the collection logic is
/// synchronous and doesn't need a GPUI executor).
///
/// `Config` entries in `DATA_SERVICES` are skipped — they're non-secret and
/// not keychain-backed. Without this mirror, operators who set
/// `RUNPOD_API_KEY` / `HF_TOKEN` / `HKASK_EODHD_API_KEY` etc. only in `.env`
/// get a working main process (the env var is read directly), but MCP server
/// child processes that read from the keychain via `build_mcp_server_env`
/// silently fail with "API key not configured" — the same failure mode the
/// mirror exists to prevent for inference providers.
fn collect_env_keys_for_mirror() -> Vec<MirrorTarget> {
    let mut out: Vec<MirrorTarget> = INFERENCE_PROVIDERS
        .iter()
        .filter_map(|provider| {
            std::env::var(provider.env_var)
                .ok()
                .filter(|key| !key.is_empty())
                .map(|key| MirrorTarget::InferenceProvider {
                    env_var: provider.env_var.to_string(),
                    api_url: provider.api_url.to_string(),
                    credential_url: provider.credential_url(),
                    key,
                })
        })
        .collect();

    // Data service secrets: no OpenAI-compatible api_url, so only the
    // credential_url is written. RunPod is skipped here because it's in
    // `INFERENCE_PROVIDERS` (which writes both `api_url` and
    // `credential_url`), so the `DataService` duplicate would only re-write
    // `credential_url`.
    for desc in DATA_SERVICES {
        if desc.credential_key == "runpod" {
            continue;
        }
        if let Some(key) = std::env::var(desc.env_var).ok().filter(|k| !k.is_empty()) {
            out.push(MirrorTarget::DataService {
                env_var: desc.env_var.to_string(),
                credential_url: desc.credential_url(),
                key,
            });
        }
    }

    out
}
