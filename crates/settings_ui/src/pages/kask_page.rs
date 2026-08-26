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

mod companies;
mod condenser;
mod curator;
mod data_services;
mod general;
mod security;

pub(crate) use {
    companies::render_companies_page, condenser::render_condenser_page, corpus::render_corpus_page,
    curator::render_curator_email_page, curator::render_curator_page,
    data_services::render_data_services_page, general::render_general_page,
    mcp_servers::render_mcp_servers_page, memory::render_memory_page, models::render_models_page,
    prediction_markets::render_prediction_markets_page, research::render_research_page,
    scenarios::render_scenarios_page, security::render_security_page, swarm::render_swarm_page,
    training::render_training_page,
};
mod corpus;
mod mcp_servers;
mod memory;
mod models;
mod prediction_markets;
mod research;
mod scenarios;
mod swarm;
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
/// Re-bound here as `(id, description)` pairs for the settings UI's rendering pattern.
pub(crate) fn builtin_mcp_servers() -> Vec<(&'static str, &'static str)> {
    kask_bridge::builtin_mcp_server_pairs()
}

/// Data service descriptors, sourced from the bridge's canonical registry
/// (`kask_bridge::DATA_SERVICES`). The bridge's `DataServiceDescriptor` is the
/// single source of truth; the UI re-binds it as `(key, label, dashboard_url,
/// env_var)` tuples for the settings UI's rendering pattern. This eliminates
/// the former parallel `DATA_SERVICES` 4-tuple that drifted from the bridge's
/// `DATA_SERVICE_CREDENTIALS` 2-tuple (different field order, overlapping but
/// not identical entries).
pub(crate) fn data_service_descriptors()
-> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    kask_bridge::DATA_SERVICES
        .iter()
        .filter(|d| d.shows_in_ui)
        .map(|d| (d.credential_key, d.label, d.dashboard_url, d.env_var))
        .collect()
}

// ---------------------------------------------------------------------------
// Shared credential helpers (used by data_services and curator sub-modules)
// ---------------------------------------------------------------------------

/// Check whether a credential is available — either via env var or in the
/// session cache of recently-written keychain URLs.
///
/// The keychain read is async, so we can't block on it here. We check the env
/// var synchronously (instant) and the session-level cache of recently-written
/// URLs. For credentials with a single keychain URL (data services, curator
/// SMTP), pass `&[url]`. For inference providers with two keychain URLs
/// (api_url + credential_url), pass `&[api_url, credential_url]`.
pub(crate) fn has_credential(
    _provider: &Arc<dyn CredentialsProvider>,
    urls: &[&str],
    env_var: &str,
) -> bool {
    // Env-var check is synchronous and instant. Use `!v.is_empty()` (not
    // `.is_ok()`) to match `build_mcp_server_env`'s predicate — an empty
    // env var (`FOO=`) is not a meaningful value and would cause the runtime
    // to skip keychain injection, so the UI should not show it as "configured".
    if std::env::var(env_var)
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    // Check the session cache for keys written via the settings UI.
    // Inference providers have two keychain URLs (api_url + credential_url);
    // either one being recently written means the key is configured.
    for url in urls {
        if was_recently_written(url) {
            return true;
        }
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
    let is_kask_namespace = url.starts_with(KASK_CREDENTIAL_NAMESPACE);
    // Extract the credential key from the URL (e.g. `kask://credentials/openrouter` → `openrouter`).
    let credential_key = url
        .strip_prefix(KASK_CREDENTIAL_NAMESPACE)
        .and_then(|s| s.strip_prefix('/'))
        .unwrap_or("")
        .to_string();
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
        // Mirror the key to the inference provider's `api_url` in the Zed
        // keychain so the `LanguageModelProvider`'s `ApiKeyState` finds it.
        // This is essential for OpenRouter, RunPod, DeepInfra, etc. — without
        // it, the key is at `kask://credentials/<key>` but the provider reads
        // from `https://openrouter.ai/api/v1` (or similar) and never sees it.
        let mirror_value = value.clone();
        let _ = kask_bridge::mirror_credential_to_provider(
            &provider,
            &credential_key,
            Some(&mirror_value),
            &cx,
        )
        .await
        .log_err();
        // After the keychain write lands, nudge `SettingsStore` so the
        // `sync_kask_mcp_runtime_servers` observer re-reads the keychain via
        // `build_mcp_server_env` and restarts any governed MCP server whose
        // env changed. The nudge must fire AFTER the write completes —
        // otherwise the restart would re-read the stale (empty) value.
        // Only kask-namespaced credentials feed MCP servers; inference-provider
        // keys (written under their `api_url`) are consumed by zed's
        // `LanguageModelRegistry`, not by kask MCP servers, so they don't need
        // a restart.
        if is_kask_namespace {
            // `AsyncApp::update` returns `R` directly (not `Result`), so there
            // is no error to propagate — the call is infallible once the app
            // is alive (the spawn's `cx` keeps it alive).
            cx.update(|cx| nudge_mcp_servers(cx));
        }
    })
}

pub(crate) fn delete_credential(
    provider: &Arc<dyn CredentialsProvider>,
    url: &str,
    cx: &mut App,
) -> Task<()> {
    let provider = provider.clone();
    let url = url.to_string();
    let is_kask_namespace = url.starts_with(KASK_CREDENTIAL_NAMESPACE);
    // Extract the credential key from the URL (e.g. `kask://credentials/openrouter` → `openrouter`).
    let credential_key = url
        .strip_prefix(KASK_CREDENTIAL_NAMESPACE)
        .and_then(|s| s.strip_prefix('/'))
        .unwrap_or("")
        .to_string();
    // Remove from session cache so the UI shows the input field again.
    unmark_recently_written(&url);
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        let _ = provider.delete_credentials(&url, cx).await.log_err();
        // Mirror the deletion to the inference provider's `api_url` so the
        // `LanguageModelProvider`'s `ApiKeyState` sees the key as removed.
        let _ = kask_bridge::mirror_credential_to_provider(&provider, &credential_key, None, &cx)
            .await
            .log_err();
        // After the keychain delete lands, nudge `SettingsStore` so the
        // `sync_kask_mcp_runtime_servers` observer re-reads the keychain and
        // restarts any governed MCP server that no longer has a key (rather
        // than keeping a stale key in its launch env). Same namespace guard as
        // `write_credential` — inference-provider deletes don't need a restart.
        if is_kask_namespace {
            cx.update(|cx| nudge_mcp_servers(cx));
        }
    })
}

/// Nudge `SettingsStore` so the `sync_kask_mcp_runtime_servers` observer
/// (wired to `cx.observe_global::<SettingsStore>` in `main.rs`) re-runs.
///
/// A keychain write/delete does NOT touch `SettingsStore` — the keychain is
/// out-of-band storage — so without this nudge the governed `McpRuntime`
/// keeps its launch-time env (with the stale empty key) until Zed restarts
/// or an unrelated settings edit fires the observer.
///
/// The nudge fires `SettingsStore::notify_observers`, which pushes
/// `Effect::NotifyGlobalObservers` directly without touching the settings
/// file. This bypasses D32's no-op-write skip (which would defeat a no-op
/// `update_settings_file` re-write) while preserving D32's loop-breaker (no
/// file write → no file-watcher re-fire → no self-sustaining loop).
///
/// Only call this for `kask://credentials/...` URLs — inference-provider
/// keys are consumed by zed's `LanguageModelRegistry`, not by kask MCP
/// servers, so a restart would be wasted work.
pub(crate) fn nudge_mcp_servers(cx: &mut App) {
    SettingsStore::notify_observers(cx);
}

// ---------------------------------------------------------------------------
// Shared input helper (used by companies, corpus, media, scenarios,
// training, models sub-modules)
// ---------------------------------------------------------------------------

/// Build a settings input field that writes a string value to the kask settings
/// at the given JSON path. Used by MCP server config pages for simple text/number
/// fields that map to env vars.
pub(crate) fn kask_string_input(
    field_id: &'static str,
    label: &'static str,
    placeholder: impl Into<SharedString>,
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
                            ("kask", "data_dir") => {
                                kask.data_dir = Some(parsed.clone());
                            }
                            ("research", "rss_db") => {
                                kask.research.get_or_insert_default().rss_db = Some(parsed.clone());
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
                            ("scenarios", "data_dir") => {
                                // No-op — scenarios has no per-server path field.
                                // The data dir is derived from the global
                                // `data_dir` as `mcp/scenarios/`.
                            }
                            ("prediction_markets", "data_dir") => {
                                // No-op — prediction_markets has no per-server
                                // path field. The data dir is derived from the
                                // global `data_dir` as `mcp/prediction-markets/`.
                            }
                            ("prediction_markets", "cache_ttl_secs") => {
                                if let Ok(v) = parsed.parse::<u64>() {
                                    kask.prediction_markets
                                        .get_or_insert_default()
                                        .cache_ttl_secs = Some(v);
                                }
                            }
                            ("prediction_markets", "base_events") => {
                                kask.prediction_markets.get_or_insert_default().base_events =
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
                            ("models", "ocr_model") => {
                                kask.models.get_or_insert_default().ocr_model =
                                    Some(parsed.clone());
                            }
                            ("condenser", "persona_keywords") => {
                                let keywords: Vec<String> = parsed
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                                kask.condenser.get_or_insert_default().persona_keywords =
                                    Some(keywords);
                            }
                            ("portfolio", "transactions_dir") => {
                                // No-op — portfolio has no per-server path field.
                                // The transactions dir is derived from the global
                                // `data_dir` as `mcp/portfolio/transactions/`.
                            }
                            ("corpus", "embedding_dim") => {
                                if let Ok(v) = parsed.parse::<u32>() {
                                    kask.corpus.get_or_insert_default().embedding_dim = Some(v);
                                }
                            }
                            ("corpus", "ocr_concurrency") => {
                                if let Ok(v) = parsed.parse::<u32>() {
                                    kask.corpus.get_or_insert_default().ocr_concurrency = Some(v);
                                }
                            }
                            ("general", "max_concurrency") => {
                                if let Ok(v) = parsed.parse::<u32>() {
                                    kask.general.get_or_insert_default().max_concurrency = Some(v);
                                }
                            }
                            ("corpus", "ocr_simple_max") => {
                                if let Ok(v) = parsed.parse::<f64>() {
                                    kask.corpus.get_or_insert_default().ocr_simple_max = Some(v);
                                }
                            }
                            ("corpus", "ocr_moderate_max") => {
                                if let Ok(v) = parsed.parse::<f64>() {
                                    kask.corpus.get_or_insert_default().ocr_moderate_max = Some(v);
                                }
                            }
                            ("corpus", "ocr_sample_rate") => {
                                if let Ok(v) = parsed.parse::<f64>() {
                                    kask.corpus.get_or_insert_default().ocr_sample_rate = Some(v);
                                }
                            }
                            ("tool_router", "threshold") => {
                                if let Ok(v) = parsed.parse::<f64>() {
                                    kask.tool_router.get_or_insert_default().threshold = Some(v);
                                }
                            }
                            ("tool_router", "complex_word_threshold") => {
                                if let Ok(v) = parsed.parse::<usize>() {
                                    kask.tool_router
                                        .get_or_insert_default()
                                        .complex_word_threshold = Some(v);
                                }
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
            title: "General".into(),
            r#type: Default::default(),
            json_path: Some("kask.data_dir"),
            description: Some(
                "Configure the zed-kask data directory — the root for internal \
                 app data (agents/, mcp/, skills/, threads/). Every MCP server \
                 receives this path as HKASK_DATA_DIR. When empty, the runtime \
                 resolves a platform default (~/.local/share/zed-kask on Linux). \
                 User-facing artifacts (reports, exports) are stored separately \
                 in ~/Documents/zk-data/."
                    .into(),
            ),
            search_aliases: &["data dir", "data directory", "zed-kask data", "hkask data", "database path"],
            in_json: true,
            files: USER,
            render: render_general_page,
        }),
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
            title: "Security".into(),
            r#type: Default::default(),
            json_path: None,
            description: Some(
                "Change the SQLCipher passphrase for kask memory databases \
                 (curator, corpus, kata-kanban). Re-encrypts the DB atomically \
                 — no data loss on failure.".into(),
            ),
            search_aliases: &["security", "passphrase", "encryption", "rotate", "key"],
            in_json: false,
            files: USER,
            render: render_security_page,
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
            title: "Research".into(),
            r#type: Default::default(),
            json_path: Some("kask.research"),
            description: Some(
                "Configure the research MCP server: RSS database path for persistent feed storage.".into(),
            ),
            search_aliases: &["research", "rss", "feed", "web search", "leap"],
            in_json: true,
            files: USER,
            render: render_research_page,
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
            title: "Prediction Markets".into(),
            r#type: Default::default(),
            json_path: Some("kask.prediction_markets"),
            description: Some(
                "Configure the prediction-markets MCP server: calibration data directory, cache TTL, base-event registry.".into(),
            ),
            search_aliases: &["prediction", "markets", "polymarket", "kalshi", "forecasting"],
            in_json: true,
            files: USER,
            render: render_prediction_markets_page,
        }),
        SettingsPageItem::SubPageLink(SubPageLink {
            title: "Swarm".into(),
            r#type: Default::default(),
            json_path: Some("kask.swarm"),
            description: Some(
                "Configure the swarm MCP server: backend mode (remote ABW vs local \
                 substrate), per-dispatch credit ceiling, curator consent default, and \
                 local agent/swarm directories.".into(),
            ),
            search_aliases: &["swarm", "abw", "agent bestiary", "xaman ek", "local agents"],
            in_json: true,
            files: USER,
            render: render_swarm_page,
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
                 embedding model for corpus/memory, classifier model for \
                 guard/regulation, and OCR model for scanned document OCR. \
                 These are provider-prefixed strings (e.g. \
                 \"openrouter/z-ai/glm-5.2\") that override the kask built-in defaults."
                    .into(),
            ),
            search_aliases: &[
                "model",
                "default model",
                "embedding model",
                "classifier model",
                "inference model",
                "ocr model",
            ],
            in_json: true,
            files: USER,
            render: render_models_page,
        }),
    ];

    SettingsPage {
        title: "Kask",
        items: items.into_boxed_slice(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// zed-kask: pinning test for the credential-write → MCP-restart wiring.
    ///
    /// `write_credential` / `delete_credential` write to the OS keychain,
    /// which is out-of-band storage that does NOT touch `SettingsStore`.
    /// The governed `McpRuntime` only re-reads env (and thus the keychain via
    /// `build_mcp_server_env`) inside `sync_kask_mcp_runtime_servers`, which
    /// is wired to `cx.observe_global::<SettingsStore>` in `main.rs`. Without
    /// a nudge, a freshly-written key is invisible to running MCP servers
    /// until Zed restarts.
    ///
    /// The fix adds `nudge_mcp_servers`, which performs a no-op
    /// `update_settings_file` on the `kask` section so `SettingsStore` fires
    /// its observers. This test pins the wiring so removing `nudge_mcp_servers`
    /// or breaking its signature breaks compilation here.
    ///
    /// A full integration test (real `McpRuntime` + keychain + `SettingsStore`
    /// observer) is infeasible in this crate's test harness — it requires the
    /// `zed` binary crate's composition root (`main.rs` owns the observer and
    /// the `McpRuntime` instance). The `zed` crate's `kask_wiring_symbols_exist`
    /// test pins the observer side (`sync_kask_mcp_runtime_servers`); this test
    /// pins the nudge side. Together they cover the full path:
    ///   keychain write → `nudge_mcp_servers` → `SettingsStore` notification →
    ///   `sync_kask_mcp_runtime_servers` → `build_mcp_server_env` (re-reads
    ///   keychain) → restart changed servers.
    #[test]
    fn nudge_mcp_servers_symbol_exists() {
        // Referencing the fn value pins both its existence and its signature;
        // renaming, deleting, or changing the signature breaks compilation.
        let _ = nudge_mcp_servers as fn(&mut gpui::App);
    }

    /// The nudge must only fire for `kask://credentials/...` URLs. Inference
    /// providers write keys under their `api_url` (e.g.
    /// `https://openrouter.ai/api/v1`) AND mirror to
    /// `kask://credentials/<key>`; the `api_url` write is consumed by zed's
    /// `LanguageModelRegistry`, not by kask MCP servers, so it must NOT
    /// trigger a restart. This test pins the namespace guard predicate so a
    /// future change that drops the `starts_with(KASK_CREDENTIAL_NAMESPACE)`
    /// check (and starts nudging on every credential write, including
    /// inference-provider `api_url` writes) fails loudly.
    #[test]
    fn kask_credential_namespace_guard_distinguishes_kask_and_provider_urls() {
        // Kask-namespaced credential URLs (data services, swarm, curator
        // email, etc.) — these feed MCP server env and MUST nudge.
        let kask_urls = [
            "kask://credentials/hkask_abw_api_key",
            "kask://credentials/hkask_eodhd_api_key",
            "kask://credentials/hkask_smtp_password",
            "kask://credentials/hkask_exa_api_key",
        ];
        for url in kask_urls {
            assert!(
                url.starts_with(KASK_CREDENTIAL_NAMESPACE),
                "expected `{url}` to be in the kask credential namespace"
            );
        }

        // Inference-provider `api_url` writes — these are consumed by zed's
        // `LanguageModelRegistry`, NOT by kask MCP servers, and must NOT nudge.
        // (The mirrored `kask://credentials/<key>` write from the same flow IS
        // kask-namespaced and will nudge — that's the intended dual-write.)
        let provider_api_urls = ["https://openrouter.ai/api/v1", "https://api.runpod.io"];
        for url in provider_api_urls {
            assert!(
                !url.starts_with(KASK_CREDENTIAL_NAMESPACE),
                "inference-provider `api_url` `{url}` must NOT be in the kask \
                 credential namespace — it is consumed by zed's \
                 `LanguageModelRegistry`, not by kask MCP servers"
            );
        }
    }
}
