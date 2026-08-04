//! Kask settings page — the `"kask"` section's UI surface (D9a UI).
//!
//! Top-level "Kask" page with sub-page links to:
//! - Data Services (API key entry → keychain via `CredentialsProvider` + enable toggles)
//! - MCP Servers (10 built-in servers + load toggles + `load_default` master toggle)
//! - Curator (`always_on` toggle + `algedonic_threshold`)
//! - Curator Email (MXroute SMTP config + keychain-backed password)
//! - Memory (`consolidation_cadence_secs` + `confidence_floor`)
//!
//! API keys are stored in the OS keychain under the `kask://credentials/<key>`
//! namespace (see `kask_bridge::secrets::KASK_CREDENTIAL_NAMESPACE`), not in
//! settings.json. The non-secret toggles and numeric config live in the `"kask"`
//! section of settings.json via `KaskSettingsContent`.
//!
//! This file is the module root: it holds the top-level `kask_page()` function,
//! the shared credential-cache helpers, shared constants, and `mod` declarations
//! for the per-sub-page render modules under `kask_page/`.

mod codegraph;
mod collab;
mod companies;
mod condenser;
mod curator;
mod data_services;
mod inference_providers;
pub(crate) use {
    codegraph::render_codegraph_page, collab::render_collab_page, companies::render_companies_page,
    condenser::render_condenser_page, corpus::render_corpus_page,
    curator::render_curator_email_page, curator::render_curator_page,
    data_services::render_data_services_page, inference_providers::render_inference_providers_page,
    mcp_servers::render_mcp_servers_page, media::render_media_page, memory::render_memory_page,
    models::render_models_page, scenarios::render_scenarios_page, training::render_training_page,
};
mod corpus;
mod mcp_servers;
mod media;
mod memory;
mod models;
mod scenarios;
mod training;

use std::sync::{Arc, Mutex};

use collections::HashSet;
use credentials_provider::CredentialsProvider;
use gpui::{ReadGlobal as _, ScrollHandle, Task, prelude::*};
use settings::SettingsStore;
use ui::{
    Button, ButtonLink, ButtonStyle, ConfiguredApiCard, Divider, SwitchField, ToggleState,
    prelude::*,
};
use util::ResultExt as _;
use zed_credentials_provider as zed_credentials;

use crate::SettingsWindow;
use crate::components::{SettingsInputField, SettingsSectionHeader};
use crate::{SettingsPage, SettingsPageItem, SubPageLink, USER};

/// The URL prefix for kask-namespaced credentials in the keychain.
/// Re-exported from `kask_bridge` so there's a single source of truth.
pub(crate) use kask_bridge::KASK_CREDENTIAL_NAMESPACE;

/// Session-level cache of credential URLs written during this session.
/// The keychain read is async, so we can't check it synchronously on render.
/// Instead, we track URLs we've written and treat them as "configured" until
/// the process exits. This avoids the input field reappearing after the user
/// enters a key.
pub(crate) static RECENTLY_WRITTEN_CREDENTIALS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Check if a credential URL was written during this session.
pub(crate) fn was_recently_written(url: &str) -> bool {
    RECENTLY_WRITTEN_CREDENTIALS
        .lock()
        .map(|opt| opt.as_ref().is_some_and(|set| set.contains(url)))
        .unwrap_or(false)
}

/// Mark a credential URL as written during this session.
pub(crate) fn mark_recently_written(url: &str) {
    if let Ok(mut guard) = RECENTLY_WRITTEN_CREDENTIALS.lock() {
        guard
            .get_or_insert_with(HashSet::default)
            .insert(url.to_string());
    }
}

/// Remove a credential URL from the session cache (after deletion).
pub(crate) fn unmark_recently_written(url: &str) {
    if let Ok(mut guard) = RECENTLY_WRITTEN_CREDENTIALS.lock() {
        if let Some(set) = guard.as_mut() {
            set.remove(url);
        }
    }
}

/// The built-in kask MCP servers (canonical source: `kask_bridge::BUILT_IN_MCP_SERVERS`).
/// Re-bound here as `(&str, &str)` for the settings UI's `(id, description)` pattern.
pub(crate) const BUILT_IN_MCP_SERVERS: &[(&str, &str)] = kask_bridge::BUILT_IN_MCP_SERVERS_PAIRS;

/// Data service descriptors: (key, label, dashboard_url, env_var).
/// The `key` is the credential key in the keychain (`kask://credentials/<key>`).
/// The `env_var` is what MCP servers read (checked synchronously for "configured" status).
pub(crate) const DATA_SERVICES: &[(&str, &str, &str, &str)] = &[
    (
        "eodhd",
        "EODHD",
        "https://eodhd.com/dashboard",
        "HKASK_EODHD_API_KEY",
    ),
    (
        "fmp",
        "FMP (Financial Modeling Prep)",
        "https://site.financialmodelingprep.com/developer/docs",
        "HKASK_FMP_API_KEY",
    ),
    (
        "exa",
        "Exa",
        "https://dashboard.exa.ai/api-keys",
        "HKASK_EXA_API_KEY",
    ),
    (
        "tavily",
        "Tavily",
        "https://app.tavily.com/api-key",
        "HKASK_TAVILY_API_KEY",
    ),
    (
        "brave",
        "Brave Search",
        "https://api.search.brave.com/app/subscriptions",
        "HKASK_BRAVE_API_KEY",
    ),
    (
        "serpapi",
        "SerpAPI (Google Search)",
        "https://serpapi.com/dashboard",
        "HKASK_SERPAPI_API_KEY",
    ),
    (
        "firecrawl",
        "Firecrawl (web scraping)",
        "https://firecrawl.dev/",
        "HKASK_FIRECRAWL_API_KEY",
    ),
    (
        "browserbase",
        "Browserbase (headless browser)",
        "https://browserbase.com/",
        "HKASK_BROWSERBASE_API_KEY",
    ),
    (
        "runpod",
        "RunPod (GPU cloud for training)",
        "https://runpod.io/",
        "RUNPOD_API_KEY",
    ),
    (
        "runpod_s3_access_key",
        "RunPod S3 Access Key (adapter storage)",
        "https://runpod.io/",
        "RUNPOD_S3_ACCESS_KEY",
    ),
    (
        "runpod_s3_secret",
        "RunPod S3 Secret (adapter storage)",
        "https://runpod.io/",
        "RUNPOD_S3_SECRET",
    ),
    (
        "nebius_project_id",
        "Nebius Project ID (GPU cloud for training)",
        "https://nebius.com/",
        "NEBIUS_PROJECT_ID",
    ),
    (
        "nebius_subnet_id",
        "Nebius Subnet ID",
        "https://nebius.com/",
        "NEBIUS_SUBNET_ID",
    ),
    (
        "hf_token",
        "HuggingFace Token",
        "https://huggingface.co/settings/tokens",
        "HF_TOKEN",
    ),
];

// ---------------------------------------------------------------------------
// Shared credential helpers (used by data_services, inference_providers,
// and curator sub-modules)
// ---------------------------------------------------------------------------

/// Check whether a credential is available — either in the keychain or via env var.
///
/// The keychain read is async, so we can't block on it here. We check the env var
/// synchronously (instant) and the session-level cache of recently-written URLs.
pub(crate) fn has_credential(
    _provider: &Arc<dyn CredentialsProvider>,
    url: &str,
    env_var: &str,
) -> bool {
    // Env-var check is synchronous and instant.
    if std::env::var(env_var).is_ok() {
        return true;
    }
    // Check the session cache for keys written via the settings UI.
    if was_recently_written(url) {
        return true;
    }
    false
}

pub(crate) fn write_credential(
    provider: &Arc<dyn CredentialsProvider>,
    url: &str,
    value: &str,
    cx: &mut App,
) -> Task<()> {
    let provider = provider.clone();
    let url = url.to_string();
    let value = value.to_string();
    // Mark as written immediately so the UI shows "Configured" on next render.
    // The keychain write is async; the session cache bridges the gap.
    // `refresh_windows` triggers a re-render so the "Configured" card appears.
    mark_recently_written(&url);
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        let _ = provider
            .write_credentials(&url, "kask", value.as_bytes(), cx)
            .await
            .log_err();
    })
}

pub(crate) fn delete_credential(
    provider: &Arc<dyn CredentialsProvider>,
    url: &str,
    cx: &mut App,
) -> Task<()> {
    let provider = provider.clone();
    let url = url.to_string();
    // Remove from session cache so the UI shows the input field again.
    unmark_recently_written(&url);
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        let _ = provider.delete_credentials(&url, cx).await.log_err();
    })
}

// ---------------------------------------------------------------------------
// Shared input helper (used by codegraph, companies, corpus, media, scenarios,
// training, models sub-modules)
// ---------------------------------------------------------------------------

/// Build a settings input field that writes a string value to the kask settings
/// at the given JSON path. Used by MCP server config pages for simple text/number
/// fields that map to env vars.
pub(crate) fn kask_string_input(
    field_id: &'static str,
    label: &'static str,
    placeholder: &'static str,
    initial: String,
    struct_name: &'static str,
    field_name: &'static str,
) -> SettingsInputField {
    SettingsInputField::new(field_id)
        .tab_index(0)
        .with_initial_text(initial)
        .with_placeholder(placeholder)
        .aria_label(label)
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                let parsed = text.trim().to_string();
                SettingsStore::global(cx).update_settings_file(
                    <dyn fs::Fs>::global(cx),
                    move |settings, _| {
                        let kask = settings.kask.get_or_insert_default();
                        match (struct_name, field_name) {
                            ("codegraph", "db_path") => {
                                kask.codegraph.get_or_insert_default().db_path =
                                    Some(parsed.clone());
                            }
                            ("companies", "chronic_staleness_days") => {
                                if let Ok(v) = parsed.parse::<u32>() {
                                    kask.companies
                                        .get_or_insert_default()
                                        .chronic_staleness_days = Some(v);
                                }
                            }
                            ("companies", "fermi_defaults") => {
                                kask.companies.get_or_insert_default().fermi_defaults =
                                    Some(parsed.clone());
                            }
                            ("corpus", "embedding_model") => {
                                kask.corpus.get_or_insert_default().embedding_model =
                                    Some(parsed.clone());
                            }
                            ("corpus", "template_root") => {
                                kask.corpus.get_or_insert_default().template_root =
                                    Some(parsed.clone());
                            }
                            ("media", "tts_model") => {
                                kask.media.get_or_insert_default().tts_model = Some(parsed.clone());
                            }
                            ("media", "stt_model") => {
                                kask.media.get_or_insert_default().stt_model = Some(parsed.clone());
                            }
                            ("media", "vision_model") => {
                                kask.media.get_or_insert_default().vision_model =
                                    Some(parsed.clone());
                            }
                            ("media", "image_gen_model") => {
                                kask.media.get_or_insert_default().image_gen_model =
                                    Some(parsed.clone());
                            }
                            ("scenarios", "data_dir") => {
                                kask.scenarios.get_or_insert_default().data_dir =
                                    Some(parsed.clone());
                            }
                            ("training", "host") => {
                                kask.training.get_or_insert_default().host = Some(parsed.clone());
                            }
                            ("training", "cache_dir") => {
                                kask.training.get_or_insert_default().cache_dir =
                                    Some(parsed.clone());
                            }
                            ("models", "default_model") => {
                                kask.models.get_or_insert_default().default_model =
                                    Some(parsed.clone());
                            }
                            ("models", "embedding_model") => {
                                kask.models.get_or_insert_default().embedding_model =
                                    Some(parsed.clone());
                            }
                            ("models", "classifier_model") => {
                                kask.models.get_or_insert_default().classifier_model =
                                    Some(parsed.clone());
                            }
                            ("collab", "database_url") => {
                                kask.collab.get_or_insert_default().database_url =
                                    Some(parsed.clone());
                            }
                            ("collab", "zed_environment") => {
                                kask.collab.get_or_insert_default().zed_environment =
                                    Some(parsed.clone());
                            }
                            ("collab", "marketplace_url") => {
                                kask.collab.get_or_insert_default().marketplace_url =
                                    Some(parsed.clone());
                            }
                            _ => {}
                        }
                    },
                );
            }
        })
}

// ---------------------------------------------------------------------------
// Shared settings reader (used by all render_* sub-modules)
// ---------------------------------------------------------------------------

/// Read the raw `KaskSettingsContent` from the user settings file.
pub(crate) fn raw_kask_settings(cx: &App) -> Option<settings::KaskSettingsContent> {
    SettingsStore::global(cx)
        .raw_user_settings()
        .and_then(|user| user.content.kask.clone())
}

// ---------------------------------------------------------------------------
// Top-level Kask page
// ---------------------------------------------------------------------------

pub(crate) fn kask_page() -> SettingsPage {
    let items: Vec<SettingsPageItem> = vec![
        SettingsPageItem::SectionHeader("Kask"),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Data Services".into(),
            r#type: Default::default(),
            json_path: Some("kask.data_services"),
            description: Some(
                "Configure API keys for data services (EODHD, FMP, Exa, Tavily, Brave, \
                 RunPod, Nebius, HuggingFace). Keys are stored in the system keychain, \
                 not in settings.json."
                    .into(),
            ),
            search_aliases: &[
                "api key",
                "brave",
                "browserbase",
                "data service",
                "eodhd",
                "exa",
                "firecrawl",
                "fmp",
                "financial modeling prep",
                "huggingface",
                "keychain",
                "nebius",
                "runpod",
                "serpapi",
                "tavily",
            ],
            in_json: false,
            files: USER,
            render: render_data_services_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Inference Providers".into(),
            r#type: Default::default(),
            json_path: Some("kask.inference_providers"),
            description: Some(
                "Configure API keys for OpenAI-compatible inference providers \
                 (DeepInfra, fal.ai, OpenRouter, KiloCode, Cline). \
                 When enabled, each provider appears in Settings → AI → LLM Providers \
                 and in the agent model picker."
                    .into(),
            ),
            search_aliases: &[
                "inference",
                "provider",
                "deepinfra",
                "fal",
                "openrouter",
                "kilocode",
                "cline",
                "glm",
                "llm",
                "model",
            ],
            in_json: true,
            files: USER,
            render: render_inference_providers_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "MCP Servers".into(),
            r#type: Default::default(),
            json_path: Some("kask.mcp"),
            description: Some(
                "Toggle which of the 10 built-in kask MCP servers are loaded.".into(),
            ),
            search_aliases: &["mcp", "model context protocol", "server", "tool"],
            in_json: true,
            files: USER,
            render: render_mcp_servers_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Curator".into(),
            r#type: Default::default(),
            json_path: Some("kask.curator"),
            description: Some(
                "Configure the Curator agent: always-on regulation and algedonic threshold.".into(),
            ),
            search_aliases: &["algedonic", "curator", "regulation"],
            in_json: true,
            files: USER,
            render: render_curator_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Curator Email".into(),
            r#type: Default::default(),
            json_path: Some("kask.curator.email"),
            description: Some(
                "Configure outbound algedonic alert email via MXroute. The SMTP \
                 password is stored in the system keychain; non-secret fields live \
                 in settings.json. When unconfigured, the alert sink falls back \
                 to log-only."
                    .into(),
            ),
            search_aliases: &[
                "alert",
                "curator",
                "email",
                "imap",
                "mxroute",
                "smtp",
            ],
            in_json: true,
            files: USER,
            render: render_curator_email_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Memory".into(),
            r#type: Default::default(),
            json_path: Some("kask.memory"),
            description: Some(
                "Configure memory consolidation, recall, and context injection.".into(),
            ),
            search_aliases: &["consolidation", "confidence", "memory", "recall", "inject"],
            in_json: true,
            files: USER,
            render: render_memory_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Condenser".into(),
            r#type: Default::default(),
            json_path: Some("kask.condenser"),
            description: Some(
                "Configure context condensation: compression profile, tool result compression, and saliency.".into(),
            ),
            search_aliases: &["condenser", "compress", "profile", "saliency"],
            in_json: true,
            files: USER,
            render: render_condenser_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Codegraph".into(),
            r#type: Default::default(),
            json_path: Some("kask.codegraph"),
            description: Some(
                "Configure the codegraph MCP server: database path for code structure storage.".into(),
            ),
            search_aliases: &["codegraph", "graph", "code structure"],
            in_json: true,
            files: USER,
            render: render_codegraph_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Companies".into(),
            r#type: Default::default(),
            json_path: Some("kask.companies"),
            description: Some(
                "Configure the companies MCP server: superforecasting staleness and Fermi defaults.".into(),
            ),
            search_aliases: &["companies", "fermi", "staleness", "superforecasting"],
            in_json: true,
            files: USER,
            render: render_companies_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Corpus".into(),
            r#type: Default::default(),
            json_path: Some("kask.corpus"),
            description: Some(
                "Configure the corpus MCP server: embedding model, OCR pipeline, and template root.".into(),
            ),
            search_aliases: &["corpus", "embedding", "ocr", "template"],
            in_json: true,
            files: USER,
            render: render_corpus_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Media".into(),
            r#type: Default::default(),
            json_path: Some("kask.media"),
            description: Some(
                "Configure the media MCP server: TTS, STT, vision, and image generation models.".into(),
            ),
            search_aliases: &["media", "tts", "stt", "vision", "image generation"],
            in_json: true,
            files: USER,
            render: render_media_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Scenarios".into(),
            r#type: Default::default(),
            json_path: Some("kask.scenarios"),
            description: Some(
                "Configure the scenarios MCP server: data directory for scenario persistence.".into(),
            ),
            search_aliases: &["scenarios", "wardley", "planning"],
            in_json: true,
            files: USER,
            render: render_scenarios_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Training".into(),
            r#type: Default::default(),
            json_path: Some("kask.training"),
            description: Some(
                "Configure the training MCP server: host selection and cache directory.".into(),
            ),
            search_aliases: &["training", "lora", "host", "cache"],
            in_json: true,
            files: USER,
            render: render_training_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Models".into(),
            r#type: Default::default(),
            json_path: Some("kask.models"),
            description: Some(
                "Configure kask-wide model defaults: default inference model, \
                 embedding model for corpus/memory, and classifier model for \
                 guard/regulation. These are provider-prefixed strings (e.g. \
                 \"openrouter/z-ai/glm-5.2\") that override the kask built-in defaults."
                    .into(),
            ),
            search_aliases: &[
                "model",
                "default model",
                "embedding model",
                "classifier model",
                "inference model",
            ],
            in_json: true,
            files: USER,
            render: render_models_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Collab Server".into(),
            r#type: Default::default(),
            json_path: Some("kask.collab"),
            description: Some(
                "Configure the local kask marketplace server. When enabled, \
                 zed-kask launches a local collab server at startup so the kask \
                 extensions panel can fetch skills without depending on the \
                 deployed zed.dev server. Uses SQLite — no Postgres or S3 \
                 required for browsing."
                    .into(),
            ),
            search_aliases: &[
                "collab",
                "marketplace",
                "server",
                "sqlite",
                "local",
                "kask-skills",
            ],
            in_json: true,
            files: USER,
            render: render_collab_page,
        }),
    ];

    SettingsPage {
        title: "Kask",
        items: items.into_boxed_slice(),
    }
}
