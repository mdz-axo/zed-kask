//! Kask settings page — the `"kask"` section's UI surface (D9a UI).
//!
//! Top-level "Kask" page with sub-page links to:
//! - Data Services (API key entry → keychain via `CredentialsProvider` + enable toggles)
//! - MCP Servers (10 built-in servers + load toggles + `load_default` master toggle)
//! - Curator (`always_on` toggle + `algedonic_threshold`)
//! - Curator Email (MXroute SMTP config + keychain-backed password)
//! - Guard / Regulation (`direct_chat_strategy`)
//! - Memory (`consolidation_cadence_secs` + `confidence_floor`)
//!
//! API keys are stored in the OS keychain under the `kask://credentials/<key>`
//! namespace (see `kask_bridge::secrets::KASK_CREDENTIAL_NAMESPACE`), not in
//! settings.json. The non-secret toggles and numeric config live in the `"kask"`
//! section of settings.json via `KaskSettingsContent`.

use std::sync::{Arc, Mutex};

use collections::HashSet;

/// Session-level cache of credential URLs written during this session.
/// The keychain read is async, so we can't check it synchronously on render.
/// Instead, we track URLs we've written and treat them as "configured" until
/// the process exits. This avoids the input field reappearing after the user
/// enters a key.
static RECENTLY_WRITTEN_CREDENTIALS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Check if a credential URL was written during this session.
fn was_recently_written(url: &str) -> bool {
    RECENTLY_WRITTEN_CREDENTIALS
        .lock()
        .map(|opt| opt.as_ref().is_some_and(|set| set.contains(url)))
        .unwrap_or(false)
}

/// Mark a credential URL as written during this session.
fn mark_recently_written(url: &str) {
    if let Ok(mut guard) = RECENTLY_WRITTEN_CREDENTIALS.lock() {
        guard
            .get_or_insert_with(HashSet::default)
            .insert(url.to_string());
    }
}

/// Remove a credential URL from the session cache (after deletion).
fn unmark_recently_written(url: &str) {
    if let Ok(mut guard) = RECENTLY_WRITTEN_CREDENTIALS.lock() {
        if let Some(set) = guard.as_mut() {
            set.remove(url);
        }
    }
}

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
/// Must match `kask_bridge::secrets::KASK_CREDENTIAL_NAMESPACE`.
const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";

/// The built-in kask MCP servers (canonical source: `kask_bridge::BUILT_IN_MCP_SERVERS`).
/// Re-bound here as `(&str, &str)` for the settings UI's `(id, description)` pattern.
const BUILT_IN_MCP_SERVERS: &[(&str, &str)] = kask_bridge::BUILT_IN_MCP_SERVERS_PAIRS;

/// Data service descriptors: (key, label, dashboard_url, env_var).
/// The `key` is the credential key in the keychain (`kask://credentials/<key>`).
/// The `env_var` is what MCP servers read (checked synchronously for "configured" status).
const DATA_SERVICES: &[(&str, &str, &str, &str)] = &[
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
                 (DeepInfra, fal.ai, Together, OpenRouter, KiloCode, Cline). \
                 When enabled, each provider appears in Settings → AI → LLM Providers \
                 and in the agent model picker."
                    .into(),
            ),
            search_aliases: &[
                "inference",
                "provider",
                "deepinfra",
                "fal",
                "together",
                "openrouter",
                "kilocode",
                "cline",
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
            title: "Guard / Regulation".into(),
            r#type: Default::default(),
            json_path: Some("kask.guard"),
            description: Some("Configure the guard layer's direct-chat streaming strategy.".into()),
            search_aliases: &["guard", "regulation", "strategy", "cascade"],
            in_json: true,
            files: USER,
            render: render_guard_page,
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
            title: "Fusion".into(),
            r#type: Default::default(),
            json_path: Some("kask.fusion"),
            description: Some(
                "Configure multi-model fusion inference: judge, panel, deliberation mode, \
                 and skill anchors. When enabled, the Curator and kask panel route inference \
                 through a panel of models judged by the configured judge model."
                    .into(),
            ),
            search_aliases: &[
                "fusion",
                "judge",
                "panel",
                "multi-model",
                "deliberation",
                "synthesis",
                "best-of-n",
                "critique",
            ],
            in_json: true,
            files: USER,
            render: render_fusion_page,
        }),
    ];

    SettingsPage {
        title: "Kask",
        items: items.into_boxed_slice(),
    }
}

// ---------------------------------------------------------------------------
// Data Services sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_data_services_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let provider = zed_credentials::global(cx);
    let raw = raw_kask_settings(cx);
    let data_services = raw.and_then(|c| c.data_services).unwrap_or_default();

    let mut rows: Vec<AnyElement> = Vec::new();
    for (key, label, dashboard_url, env_var) in DATA_SERVICES {
        let enabled = match *key {
            "eodhd" => data_services.eodhd_enabled.unwrap_or(false),
            "fmp" => data_services.fmp_enabled.unwrap_or(false),
            "exa" => data_services.exa_enabled.unwrap_or(false),
            "tavily" => data_services.tavily_enabled.unwrap_or(false),
            "brave" => data_services.brave_enabled.unwrap_or(false),
            "runpod" => data_services.runpod_enabled.unwrap_or(false),
            "runpod_s3_access_key" | "runpod_s3_secret" => {
                data_services.runpod_enabled.unwrap_or(false)
            }
            "nebius_project_id" | "nebius_subnet_id" => {
                data_services.nebius_enabled.unwrap_or(false)
            }
            // These services don't have individual toggles — they're enabled
            // when their API key is present. We show them as enabled if the
            // key is in the keychain (checked via env var for display).
            "serpapi" | "firecrawl" | "browserbase" | "hf_token" => std::env::var(env_var).is_ok(),
            _ => false,
        };
        rows.push(render_data_service_row(
            key,
            label,
            dashboard_url,
            env_var,
            enabled,
            provider.clone(),
            cx,
        ));
    }

    v_flex()
        .id("kask-data-services-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Data Services"))
                .child(
                    Label::new(
                        "API keys are stored in the system keychain (kask://credentials/<key>). \
                         Toggle a service to enable it, then enter its API key.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(v_flex().gap_6().children(rows))
        .into_any_element()
}

fn render_data_service_row(
    key: &'static str,
    label: &'static str,
    dashboard_url: &'static str,
    env_var: &'static str,
    enabled: bool,
    provider: Arc<dyn CredentialsProvider>,
    _cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let credential_url = format!("{KASK_CREDENTIAL_NAMESPACE}/{key}");
    let has_key = has_credential(&provider, &credential_url, env_var);

    let toggle_id = format!("kask-{key}-enabled");
    let enable_toggle = SwitchField::new(
        toggle_id,
        Some(label),
        Some(
            format!(
                "Enable {label}. API key is stored in the keychain under \
             kask://credentials/{key}. Or set the {env_var} environment variable."
            )
            .into(),
        ),
        if enabled {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let enabled = *state == ToggleState::Selected;
            set_data_service_enabled(key, enabled, cx);
        },
    )
    .tab_index(0);

    let key_input = if has_key {
        let reset_id = format!("kask-{key}-reset");
        ConfiguredApiCard::new(reset_id, "API Key Configured")
            .button_label("Reset Key")
            .button_tab_index(0)
            .on_click({
                let provider = provider.clone();
                move |_, _, cx| {
                    delete_credential(&provider, &credential_url, cx).detach();
                }
            })
            .into_any_element()
    } else {
        let input_id = format!("kask-{key}-api-key-input");
        let aria_label = format!("{label} API Key");
        let provider = provider.clone();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .pt_2p5()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .justify_between()
                    .child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .max_w_1_2()
                            .gap_0p5()
                            .child(Label::new("API Key"))
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .flex_wrap()
                                    .gap_0p5()
                                    .child(
                                        Label::new("Visit the")
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    )
                                    .child(
                                        ButtonLink::new(
                                            format!("{label} dashboard"),
                                            dashboard_url,
                                        )
                                        .no_icon(true)
                                        .label_size(LabelSize::Small)
                                        .label_color(Color::Muted),
                                    )
                                    .child(
                                        Label::new("to generate an API key.")
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    ),
                            )
                            .child(
                                Label::new(format!(
                                    "Or set the {env_var} env var and restart Zed for it to take effect."
                                ))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                            ),
                    )
                    .child(
                        SettingsInputField::new(input_id)
                            .tab_index(0)
                            .with_placeholder("xxxxxxxxxxxxxxxxxxxx")
                            .aria_label(aria_label)
                            .on_confirm(move |api_key, _window, cx| {
                                if let Some(key_value) =
                                    api_key.filter(|key_value| !key_value.is_empty())
                                {
                                    write_credential(
                                        &provider,
                                        &credential_url,
                                        &key_value,
                                        cx,
                                    )
                                    .detach();
                                }
                            }),
                    ),
            )
            .into_any_element()
    };

    v_flex()
        .gap_2()
        .child(enable_toggle)
        .when(enabled, |this| this.child(key_input))
        .into_any_element()
}

fn set_data_service_enabled(key: &str, enabled: bool, cx: &mut App) {
    let key = key.to_string();
    SettingsStore::global(cx).update_settings_file(<dyn fs::Fs>::global(cx), move |settings, _| {
        let kask = settings.kask.get_or_insert_default();
        let data_services = kask.data_services.get_or_insert_default();
        match key.as_str() {
            "eodhd" => data_services.eodhd_enabled = Some(enabled),
            "fmp" => data_services.fmp_enabled = Some(enabled),
            "exa" => data_services.exa_enabled = Some(enabled),
            "tavily" => data_services.tavily_enabled = Some(enabled),
            "brave" => data_services.brave_enabled = Some(enabled),
            "runpod" => data_services.runpod_enabled = Some(enabled),
            "runpod_s3_access_key" | "runpod_s3_secret" => {
                data_services.runpod_enabled = Some(enabled);
            }
            "nebius_project_id" | "nebius_subnet_id" => {
                data_services.nebius_enabled = Some(enabled);
            }
            // These services don't have individual toggles — no-op.
            _ => {}
        }
    });
}

/// Check whether a credential is available — either in the keychain or via env var.
///
/// The keychain read is async, so we can't block on it here. We check the env var
/// synchronously (instant) and the session-level cache of recently-written URLs.
fn has_credential(_provider: &Arc<dyn CredentialsProvider>, url: &str, env_var: &str) -> bool {
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

fn write_credential(
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

fn delete_credential(provider: &Arc<dyn CredentialsProvider>, url: &str, cx: &mut App) -> Task<()> {
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
// Inference Providers sub-page
// ---------------------------------------------------------------------------

/// Render the Inference Providers sub-page.
///
/// Each provider has an enable toggle and an API key input. When enabled,
/// an `openai_compatible.<provider_id>` entry is written to settings.json so
/// the provider appears in Settings → AI → LLM Providers. The API key is
/// stored in the keychain under the provider's `api_url` (so zed's
/// OpenAI-compatible provider finds it) and mirrored to
/// `kask://credentials/<key>` for MCP server env injection.
pub(crate) fn render_inference_providers_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let provider = zed_credentials::global(cx);
    let raw = raw_kask_settings(cx);
    let inference = raw.and_then(|c| c.inference_providers).unwrap_or_default();

    let mut rows: Vec<AnyElement> = Vec::new();
    for desc in kask_bridge::INFERENCE_PROVIDERS {
        let enabled = match desc.id {
            "deepinfra" => inference.deepinfra_enabled.unwrap_or(false),
            "fal" => inference.fal_enabled.unwrap_or(false),
            "together" => inference.together_enabled.unwrap_or(false),
            "openrouter" => inference.openrouter_enabled.unwrap_or(false),
            "kilocode" => inference.kilocode_enabled.unwrap_or(false),
            "cline" => inference.cline_enabled.unwrap_or(false),
            _ => false,
        };
        rows.push(render_inference_provider_row(
            desc,
            enabled,
            provider.clone(),
            cx,
        ));
    }

    v_flex()
        .id("kask-inference-providers-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Inference Providers"))
                .child(
                    Label::new(
                        "API keys for OpenAI-compatible inference providers. \
                         Toggle a provider to register it as an LLM provider in zed \
                         (appears in Settings → AI → LLM Providers and the agent model picker). \
                         Keys are stored in the system keychain.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(v_flex().gap_6().children(rows))
        .into_any_element()
}

fn render_inference_provider_row(
    desc: &kask_bridge::InferenceProviderDescriptor,
    enabled: bool,
    credentials_provider: Arc<dyn CredentialsProvider>,
    _cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let has_key = kask_bridge::has_provider_api_key(desc)
        || was_recently_written(&desc.credential_url())
        || was_recently_written(desc.api_url);
    let provider_id = desc.id;
    let provider_name = desc.name;
    let dashboard_url = desc.dashboard_url;
    let env_var = desc.env_var;
    let api_url = desc.api_url.to_string();
    let credential_url = desc.credential_url();

    let toggle_id = format!("kask-inference-{provider_id}-enabled");
    let enable_toggle = SwitchField::new(
        toggle_id,
        Some(provider_name),
        Some(
            format!(
                "Enable {provider_name} as an OpenAI-compatible LLM provider. \
                 Writes an `openai_compatible.{provider_id}` entry to settings.json \
                 with api_url `{api_url}`. The API key is stored in the keychain."
            )
            .into(),
        ),
        if enabled {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let is_enabled = *state == ToggleState::Selected;
            set_inference_provider_enabled(provider_id, is_enabled, cx);
        },
    )
    .tab_index(0);

    let key_input = if has_key {
        let reset_id = format!("kask-inference-{provider_id}-reset");
        ConfiguredApiCard::new(reset_id, "API Key Configured")
            .button_label("Reset Key")
            .button_tab_index(0)
            .on_click({
                let provider = credentials_provider.clone();
                let desc_credential_url = credential_url;
                let desc_api_url = api_url;
                move |_, _, cx| {
                    // Delete from both keychain locations.
                    let provider = provider.clone();
                    let url1 = desc_api_url.clone();
                    let url2 = desc_credential_url.clone();
                    // Remove from session cache so the UI shows the input field.
                    unmark_recently_written(&url1);
                    unmark_recently_written(&url2);
                    cx.refresh_windows();
                    cx.spawn(async move |cx| {
                        let _ = provider.delete_credentials(&url1, cx).await.log_err();
                        let _ = provider.delete_credentials(&url2, cx).await.log_err();
                    })
                    .detach();
                }
            })
            .into_any_element()
    } else {
        let input_id = format!("kask-inference-{provider_id}-api-key-input");
        let aria_label = format!("{provider_name} API Key");
        let credentials_provider = credentials_provider.clone();
        let desc_credential_url = credential_url;
        let desc_api_url = api_url;
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .pt_2p5()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .justify_between()
                    .child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .max_w_1_2()
                            .gap_0p5()
                            .child(Label::new("API Key"))
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .flex_wrap()
                                    .gap_0p5()
                                    .child(
                                        Label::new("Visit the")
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    )
                                    .child(
                                        ButtonLink::new(
                                            format!("{provider_name} dashboard"),
                                            dashboard_url,
                                        )
                                        .no_icon(true)
                                        .label_size(LabelSize::Small)
                                        .label_color(Color::Muted),
                                    )
                                    .child(
                                        Label::new("to generate an API key.")
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    ),
                            )
                            .child(
                                Label::new(format!(
                                    "Or set the {env_var} env var and restart Zed for it to take effect. \
                                     The API URL is {desc_api_url}."
                                ))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                            ),
                    )
                    .child(
                        SettingsInputField::new(input_id)
                            .tab_index(0)
                            .with_placeholder("xxxxxxxxxxxxxxxxxxxx")
                            .aria_label(aria_label)
                            .on_confirm(move |api_key, _window, cx| {
                                if let Some(key_value) =
                                    api_key.filter(|key_value| !key_value.is_empty())
                                {
                                    let provider = credentials_provider.clone();
                                    let url1 = desc_api_url.clone();
                                    let url2 = desc_credential_url.clone();
                                    // Mark both URLs as written so the UI shows "Configured".
                                    mark_recently_written(&url1);
                                    mark_recently_written(&url2);
                                    cx.refresh_windows();
                                    cx.spawn(async move |cx| {
                                        // Write under the api_url (for zed's OpenAI-compatible provider).
                                        let _ = provider
                                            .write_credentials(&url1, "Bearer", key_value.as_bytes(), cx)
                                            .await
                                            .log_err();
                                        // Write under the kask credential URL (for MCP env injection).
                                        let _ = provider
                                            .write_credentials(&url2, "kask", key_value.as_bytes(), cx)
                                            .await
                                            .log_err();
                                    })
                                    .detach();
                                }
                            }),
                    ),
            )
            .into_any_element()
    };

    v_flex()
        .gap_2()
        .child(enable_toggle)
        .when(enabled, |this| this.child(key_input))
        .into_any_element()
}

fn set_inference_provider_enabled(provider_id: &str, enabled: bool, cx: &mut App) {
    let provider_id = provider_id.to_string();
    SettingsStore::global(cx).update_settings_file(<dyn fs::Fs>::global(cx), move |settings, _| {
        let kask = settings.kask.get_or_insert_default();
        let inference = kask.inference_providers.get_or_insert_default();
        match provider_id.as_str() {
            "deepinfra" => inference.deepinfra_enabled = Some(enabled),
            "fal" => inference.fal_enabled = Some(enabled),
            "together" => inference.together_enabled = Some(enabled),
            "openrouter" => inference.openrouter_enabled = Some(enabled),
            "kilocode" => inference.kilocode_enabled = Some(enabled),
            "cline" => inference.cline_enabled = Some(enabled),
            _ => {}
        }
    });
}

// ---------------------------------------------------------------------------
// MCP Servers sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_mcp_servers_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let mcp = raw.and_then(|c| c.mcp).unwrap_or_default();
    let load_default = mcp.load_default.unwrap_or(true);
    let overrides = &mcp.overrides;

    let master_toggle = SwitchField::new(
        "kask-mcp-load-default",
        Some("Load Default MCP Servers"),
        Some(
            "When enabled, all 10 built-in kask MCP servers are loaded unless \
             individually overridden below. Disable to load no kask MCP servers."
                .into(),
        ),
        if load_default {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let enabled = *state == ToggleState::Selected;
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .mcp
                        .get_or_insert_default()
                        .load_default = Some(enabled);
                },
            );
        },
    )
    .tab_index(0);

    let mut server_rows: Vec<AnyElement> = Vec::new();
    for (server_id, description) in BUILT_IN_MCP_SERVERS {
        let loaded = load_default && *overrides.get(*server_id).unwrap_or(&true);
        server_rows.push(render_mcp_server_toggle(
            server_id,
            description,
            loaded,
            load_default,
        ));
    }

    v_flex()
        .id("kask-mcp-servers-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("MCP Servers"))
                .child(
                    Label::new(
                        "Toggle which of the 10 built-in kask MCP servers are loaded. \
                         Individual overrides take precedence over the master toggle.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(master_toggle)
        .child(Divider::horizontal())
        .child(v_flex().gap_4().children(server_rows))
        .into_any_element()
}

fn render_mcp_server_toggle(
    server_id: &'static str,
    description: &'static str,
    loaded: bool,
    load_default: bool,
) -> AnyElement {
    let toggle_id = format!("kask-mcp-{server_id}");
    let server_id_for_write = server_id.to_string();
    SwitchField::new(
        toggle_id,
        Some(server_id),
        Some((*description).into()),
        if loaded {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let enabled = *state == ToggleState::Selected;
            let server_id = server_id_for_write.clone();
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .mcp
                        .get_or_insert_default()
                        .overrides
                        .insert(server_id, enabled);
                },
            );
        },
    )
    .when(!load_default, |this| {
        this.tooltip(ui::Tooltip::text(
            "The master \"Load Default MCP Servers\" toggle is off — \
             enable it for this override to take effect.",
        ))
    })
    .tab_index(0)
    .into_any_element()
}

// ---------------------------------------------------------------------------
// Curator sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_curator_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let curator = raw.and_then(|c| c.curator).unwrap_or_default();
    let always_on = curator.always_on.unwrap_or(true);
    let algedonic_threshold = curator
        .algedonic_threshold
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "0.8".to_string());

    let always_on_toggle = SwitchField::new(
        "kask-curator-always-on",
        Some("Always On"),
        Some(
            "Whether the Curator agent is always-on (runs regulation loops in background).".into(),
        ),
        if always_on {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let enabled = *state == ToggleState::Selected;
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .curator
                        .get_or_insert_default()
                        .always_on = Some(enabled);
                },
            );
        },
    )
    .tab_index(0);

    let threshold_input = SettingsInputField::new("kask-curator-algedonic-threshold")
        .tab_index(0)
        .with_initial_text(algedonic_threshold)
        .with_placeholder("0.8")
        .aria_label("Algedonic Threshold")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                if let Ok(parsed) = text.parse::<f64>() {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .curator
                                .get_or_insert_default()
                                .algedonic_threshold = Some(parsed);
                        },
                    );
                }
            }
        });

    v_flex()
        .id("kask-curator-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Curator"))
                .child(
                    Label::new(
                        "The Curator agent runs regulation loops and monitors algedonic signals.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(always_on_toggle)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Algedonic Threshold"))
                .child(
                    Label::new(
                        "Algedonic signal threshold (0.0–1.0). Signals above this trigger the Curator.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(threshold_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Curator Email sub-page
// ---------------------------------------------------------------------------

/// Render the Curator Email sub-page.
///
/// Non-secret email fields (MXroute server, SMTP username, From address,
/// alert recipient, authorized senders, poll/digest intervals) live in
/// settings.json under `kask.curator.email`. The SMTP password is stored in
/// the OS keychain under `kask://credentials/hkask_smtp_password` and
/// injected into MCP server child processes as `HKASK_SMTP_PASSWORD`.
pub(crate) fn render_curator_email_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let provider = zed_credentials::global(cx);
    let raw = raw_kask_settings(cx);
    let email = raw
        .and_then(|c| c.curator)
        .and_then(|c| c.email)
        .unwrap_or_default();

    let mxroute_server = email.mxroute_server.unwrap_or_default();
    let smtp_username = email.smtp_username.unwrap_or_default();
    let curator_email = email.curator_email.unwrap_or_default();
    let alert_email = email.alert_email.unwrap_or_default();
    let authorized_emails = email
        .authorized_emails
        .map(|v| v.join(", "))
        .unwrap_or_default();
    let inbox_poll_interval = email
        .inbox_poll_interval_secs
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "0".to_string());
    let digest_interval = email
        .digest_interval_secs
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "0".to_string());

    // SMTP password — keychain-backed, mirrors the data-service API key pattern.
    let smtp_password_url = format!("{KASK_CREDENTIAL_NAMESPACE}/hkask_smtp_password");
    let has_password = has_credential(&provider, &smtp_password_url, "HKASK_SMTP_PASSWORD");
    let password_card = if has_password {
        ConfiguredApiCard::new(
            "kask-curator-email-smtp-password-reset",
            "SMTP Password Configured",
        )
        .button_label("Reset Password")
        .button_tab_index(0)
        .on_click({
            let provider = provider.clone();
            let url = smtp_password_url;
            move |_, _, cx| {
                delete_credential(&provider, &url, cx).detach();
            }
        })
        .into_any_element()
    } else {
        let provider = provider.clone();
        let url = smtp_password_url;
        v_flex()
            .gap_2()
            .child(
                v_flex().gap_0p5().child(Label::new("SMTP Password")).child(
                    Label::new(
                        "The mailbox password for HKASK_SMTP_USERNAME. Stored in the \
                             keychain under kask://credentials/hkask_smtp_password, or set \
                             the HKASK_SMTP_PASSWORD env var and restart Zed.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
            )
            .child(
                SettingsInputField::new("kask-curator-email-smtp-password-input")
                    .tab_index(0)
                    .with_placeholder("••••••••••••")
                    .aria_label("SMTP Password")
                    .confirm_on_focus_out()
                    .on_confirm(move |value, _window, cx| {
                        if let Some(pw) = value.filter(|v| !v.is_empty()) {
                            write_credential(&provider, &url, &pw, cx).detach();
                        }
                    }),
            )
            .into_any_element()
    };

    // Helper to build a labeled text input bound to a settings.json field.
    let make_text_input = |id: &'static str,
                           label: &'static str,
                           help: &'static str,
                           initial: String,
                           placeholder: &'static str| {
        let input = SettingsInputField::new(id)
            .tab_index(0)
            .with_initial_text(initial)
            .with_placeholder(placeholder)
            .aria_label(label)
            .confirm_on_focus_out()
            .on_confirm(move |value, _window, cx| {
                if let Some(text) = value {
                    // Compute the final field value up front so the
                    // `update_settings_file` closure only needs to move
                    // already-owned values into place (it requires `'static`).
                    //
                    // For string fields: `None` when empty, else `Some(text)`.
                    // For numeric fields: parsed `Option<u64>`.
                    // For authorized-emails: split + trimmed `Vec<String>`.
                    let string_value: Option<String> = if text.is_empty() {
                        None
                    } else {
                        Some(text.clone())
                    };
                    let authorized_emails: Option<Vec<String>> =
                        if id == "kask-curator-email-authorized-emails" && !text.is_empty() {
                            Some(
                                text.split(',')
                                    .map(|p| p.trim().to_string())
                                    .filter(|p| !p.is_empty())
                                    .collect(),
                            )
                        } else {
                            None
                        };
                    let inbox_poll: Option<u64> = if id == "kask-curator-email-inbox-poll-interval"
                    {
                        text.parse::<u64>().ok()
                    } else {
                        None
                    };
                    let digest: Option<u64> = if id == "kask-curator-email-digest-interval" {
                        text.parse::<u64>().ok()
                    } else {
                        None
                    };
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            let email = settings
                                .kask
                                .get_or_insert_default()
                                .curator
                                .get_or_insert_default()
                                .email
                                .get_or_insert_default();
                            // Dispatch on `id` to set the right field.
                            match id {
                                "kask-curator-email-mxroute-server" => {
                                    email.mxroute_server = string_value;
                                }
                                "kask-curator-email-smtp-username" => {
                                    email.smtp_username = string_value;
                                }
                                "kask-curator-email-curator-email" => {
                                    email.curator_email = string_value;
                                }
                                "kask-curator-email-alert-email" => {
                                    email.alert_email = string_value;
                                }
                                "kask-curator-email-authorized-emails" => {
                                    email.authorized_emails = authorized_emails;
                                }
                                "kask-curator-email-inbox-poll-interval" => {
                                    email.inbox_poll_interval_secs = inbox_poll;
                                }
                                "kask-curator-email-digest-interval" => {
                                    email.digest_interval_secs = digest;
                                }
                                _ => {}
                            }
                        },
                    );
                }
            });
        v_flex()
            .gap_1()
            .child(Label::new(label))
            .child(Label::new(help).size(LabelSize::Small).color(Color::Muted))
            .child(input)
    };

    // Compute the test email recipient up front, before the strings are
    // moved into `make_text_input` calls.
    let test_email_recipient = if !alert_email.is_empty() {
        alert_email.clone()
    } else {
        smtp_username.clone()
    };
    let test_email_enabled = !test_email_recipient.is_empty();

    let mxroute_input = make_text_input(
        "kask-curator-email-mxroute-server",
        "MXroute Server",
        "MXroute server hostname (e.g. \"tuesday.mxrouting.net\"). Or set HKASK_MXROUTE_SERVER.",
        mxroute_server,
        "tuesday.mxrouting.net",
    );
    let smtp_username_input = make_text_input(
        "kask-curator-email-smtp-username",
        "SMTP Username",
        "Full email address used for SMTP auth and the From header. Or set HKASK_SMTP_USERNAME.",
        smtp_username,
        "curator@example.com",
    );
    let curator_email_input = make_text_input(
        "kask-curator-email-curator-email",
        "From Address",
        "From address (defaults to SMTP Username when empty). Or set HKASK_CURATOR_EMAIL.",
        curator_email,
        "curator@example.com",
    );
    let alert_email_input = make_text_input(
        "kask-curator-email-alert-email",
        "Alert Recipient",
        "Where algedonic alert emails are sent (defaults to SMTP Username when empty). Or set HKASK_ALERT_EMAIL.",
        alert_email,
        "ops@example.com",
    );
    let authorized_input = make_text_input(
        "kask-curator-email-authorized-emails",
        "Authorized Senders",
        "Comma-separated allowlist of senders who may reply with curator commands (P12). Empty means inbound replies are rejected. Or set HKASK_AUTHORIZED_EMAILS.",
        authorized_emails,
        "ops@example.com, alice@example.com",
    );
    let inbox_poll_input = make_text_input(
        "kask-curator-email-inbox-poll-interval",
        "Inbox Poll Interval (secs)",
        "IMAP inbox poll interval for inbound command replies. 0 = disabled. Default 60. Or set HKASK_INBOX_POLL_INTERVAL_SECS.",
        inbox_poll_interval,
        "0",
    );
    let digest_input = make_text_input(
        "kask-curator-email-digest-interval",
        "Digest Interval (secs)",
        "Periodic escalation digest email interval. 0 = disabled. Default 86400 (daily). Or set HKASK_DIGEST_INTERVAL_SECS.",
        digest_interval,
        "0",
    );

    // Test Email button — sends a test email to the alert recipient to verify
    // MXroute credentials. Uses the alert recipient (or SMTP username) as the
    // destination. The send runs on the kask tokio runtime via
    // `kask_bridge::spawn_test_email`; success/failure surfaces in the logs.
    let test_email_button = Button::new("kask-curator-email-test", "Send Test Email")
        .style(ButtonStyle::Outlined)
        .label_size(LabelSize::Small)
        .tab_index(0isize)
        .disabled(!test_email_enabled)
        .on_click(move |_, _, cx| {
            kask_bridge::spawn_test_email(test_email_recipient.clone(), cx);
        });

    v_flex()
        .id("kask-curator-email-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Curator Email"))
                .child(
                    Label::new(
                        "Outbound algedonic alert emails via MXroute. The SMTP password is \
                         stored in the system keychain; non-secret fields live in settings.json. \
                         When unconfigured, the alert sink falls back to log-only.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(mxroute_input)
        .child(Divider::horizontal())
        .child(smtp_username_input)
        .child(Divider::horizontal())
        .child(password_card)
        .child(Divider::horizontal())
        .child(curator_email_input)
        .child(Divider::horizontal())
        .child(alert_email_input)
        .child(Divider::horizontal())
        .child(authorized_input)
        .child(Divider::horizontal())
        .child(inbox_poll_input)
        .child(Divider::horizontal())
        .child(digest_input)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Test Configuration"))
                .child(
                    Label::new(
                        "Send a test email to the alert recipient to verify MXroute \
                         credentials. Check the logs (reg.email.sent) for the result. \
                         Requires SMTP Username and SMTP Password to be configured.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(test_email_button),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Guard / Regulation sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_guard_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let guard = raw.and_then(|c| c.guard).unwrap_or_default();
    let strategy = guard
        .direct_chat_strategy
        .unwrap_or_else(|| "cascade_only".to_string());

    let strategy_input = SettingsInputField::new("kask-guard-direct-chat-strategy")
        .tab_index(0)
        .with_initial_text(strategy)
        .with_placeholder("cascade_only")
        .aria_label("Direct-Chat Strategy")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                let trimmed = text.trim().to_string();
                if matches!(trimmed.as_str(), "buffer" | "incremental" | "cascade_only") {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .guard
                                .get_or_insert_default()
                                .direct_chat_strategy = Some(trimmed);
                        },
                    );
                }
            }
        });

    v_flex()
        .id("kask-guard-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Guard / Regulation"))
                .child(
                    Label::new(
                        "The guard layer wraps the inference path. The direct-chat \
                         strategy controls how guard output is streamed: \"buffer\" \
                         (wait for full guard output), \"incremental\" (stream guard \
                         and inference interleaved), or \"cascade_only\" (no guard on \
                         direct chat — only the Curator cascade runs).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Direct-Chat Strategy"))
                .child(
                    Label::new("One of: \"buffer\", \"incremental\", or \"cascade_only\".")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(strategy_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Memory sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_memory_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let memory = raw.and_then(|c| c.memory).unwrap_or_default();
    let cadence = memory
        .consolidation_cadence_secs
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "300".to_string());
    let confidence_floor = memory
        .confidence_floor
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "0.3".to_string());
    let recall_limit = memory
        .recall_limit
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "5".to_string());
    let recall_min_confidence = memory
        .recall_min_confidence
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "0.3".to_string());
    let auto_inject = memory.auto_inject.unwrap_or(true);

    let cadence_input = SettingsInputField::new("kask-memory-consolidation-cadence")
        .tab_index(0)
        .with_initial_text(cadence)
        .with_placeholder("300")
        .aria_label("Consolidation Cadence (seconds)")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                if let Ok(parsed) = text.parse::<u64>() {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .memory
                                .get_or_insert_default()
                                .consolidation_cadence_secs = Some(parsed);
                        },
                    );
                }
            }
        });

    let confidence_input = SettingsInputField::new("kask-memory-confidence-floor")
        .tab_index(0)
        .with_initial_text(confidence_floor)
        .with_placeholder("0.3")
        .aria_label("Confidence Floor")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                if let Ok(parsed) = text.parse::<f64>() {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .memory
                                .get_or_insert_default()
                                .confidence_floor = Some(parsed);
                        },
                    );
                }
            }
        });

    v_flex()
        .id("kask-memory-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Memory"))
                .child(
                    Label::new(
                        "Memory consolidation ingests completed threads into episodic \
                         and semantic memory. Set the cadence to 0 to disable.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Consolidation Cadence (seconds)"))
                .child(
                    Label::new("Memory consolidation cadence in seconds (0 = disabled).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(cadence_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Confidence Floor"))
                .child(
                    Label::new("Confidence floor for memory retention (0.0–1.0).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(confidence_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Recall Limit"))
                .child(
                    Label::new(
                        "Maximum number of memory snippets to retrieve for context injection.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(
                    SettingsInputField::new("kask-memory-recall-limit")
                        .tab_index(0)
                        .with_initial_text(recall_limit)
                        .with_placeholder("5")
                        .aria_label("Recall Limit")
                        .confirm_on_focus_out()
                        .on_confirm(move |value, _window, cx| {
                            if let Some(text) = value {
                                if let Ok(parsed) = text.parse::<u32>() {
                                    SettingsStore::global(cx).update_settings_file(
                                        <dyn fs::Fs>::global(cx),
                                        move |settings, _| {
                                            settings
                                                .kask
                                                .get_or_insert_default()
                                                .memory
                                                .get_or_insert_default()
                                                .recall_limit = Some(parsed);
                                        },
                                    );
                                }
                            }
                        }),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Recall Minimum Confidence"))
                .child(
                    Label::new(
                        "Minimum confidence for a memory to be injected into context (0.0–1.0).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(
                    SettingsInputField::new("kask-memory-recall-min-confidence")
                        .tab_index(0)
                        .with_initial_text(recall_min_confidence)
                        .with_placeholder("0.3")
                        .aria_label("Recall Minimum Confidence")
                        .confirm_on_focus_out()
                        .on_confirm(move |value, _window, cx| {
                            if let Some(text) = value {
                                if let Ok(parsed) = text.parse::<f64>() {
                                    SettingsStore::global(cx).update_settings_file(
                                        <dyn fs::Fs>::global(cx),
                                        move |settings, _| {
                                            settings
                                                .kask
                                                .get_or_insert_default()
                                                .memory
                                                .get_or_insert_default()
                                                .recall_min_confidence = Some(parsed);
                                        },
                                    );
                                }
                            }
                        }),
                ),
        )
        .child(Divider::horizontal())
        .child(
            SwitchField::new(
                "kask-memory-auto-inject",
                Some("Auto-Inject Memories"),
                Some("Whether to automatically inject recalled memories into prompts.".into()),
                auto_inject,
                move |state, _window, cx| {
                    let value = *state == ToggleState::Selected;
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .memory
                                .get_or_insert_default()
                                .auto_inject = Some(value);
                        },
                    );
                },
            )
            .tab_index(0),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Condenser sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_condenser_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let condenser = raw.and_then(|c| c.condenser).unwrap_or_default();
    let profile = condenser.profile.as_deref().unwrap_or("normal");
    let auto_compress = condenser.auto_compress_tool_results.unwrap_or(true);
    let saliency_window = condenser
        .saliency_window
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "5".to_string());

    let profile_input = SettingsInputField::new("kask-condenser-profile")
        .tab_index(0)
        .with_initial_text(profile.to_string())
        .with_placeholder("normal")
        .aria_label("Compression Profile")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                let parsed = text.trim().to_string();
                SettingsStore::global(cx).update_settings_file(
                    <dyn fs::Fs>::global(cx),
                    move |settings, _| {
                        settings
                            .kask
                            .get_or_insert_default()
                            .condenser
                            .get_or_insert_default()
                            .profile = Some(parsed);
                    },
                );
            }
        });

    let saliency_input = SettingsInputField::new("kask-condenser-saliency-window")
        .tab_index(0)
        .with_initial_text(saliency_window)
        .with_placeholder("5")
        .aria_label("Saliency Window")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                if let Ok(parsed) = text.parse::<u32>() {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .condenser
                                .get_or_insert_default()
                                .saliency_window = Some(parsed);
                        },
                    );
                }
            }
        });

    v_flex()
        .id("kask-condenser-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Condenser"))
                .child(
                    Label::new(
                        "The condenser compresses tool output and manages context \
                         in inference threads. Configure the compression profile \
                         and saliency settings."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Compression Profile"))
                .child(
                    Label::new("Profile: heavy (10% retention), normal (20%), soft (60%), or light (95%).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(profile_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Auto-Compress Tool Results"))
                .child(
                    Label::new("Whether to automatically compress tool results before they enter the message history.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    SwitchField::new(
                        "kask-condenser-auto-compress",
                        Some("Auto-Compress Tool Results"),
                        Some("Whether to automatically compress tool results before they enter the message history.".into()),
                        auto_compress,
                        move |state, _window, cx| {
                            let value = *state == ToggleState::Selected;
                            SettingsStore::global(cx).update_settings_file(
                                <dyn fs::Fs>::global(cx),
                                move |settings, _| {
                                    settings
                                        .kask
                                        .get_or_insert_default()
                                        .condenser
                                        .get_or_insert_default()
                                        .auto_compress_tool_results = Some(value);
                                },
                            );
                        },
                    )
                    .tab_index(0),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Saliency Window"))
                .child(
                    Label::new("Saliency window multiplier for thread summarization (max_tokens = window * 100, clamped [150, 2000]).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(saliency_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a settings input field that writes a string value to the kask settings
/// at the given JSON path. Used by MCP server config pages for simple text/number
/// fields that map to env vars.
fn kask_string_input(
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
                            ("fusion", "judge_model") => {
                                kask.fusion.get_or_insert_default().judge_model =
                                    Some(parsed.clone());
                            }
                            ("fusion", "panel_models") => {
                                kask.fusion.get_or_insert_default().panel_models =
                                    Some(parsed.clone());
                            }
                            ("fusion", "mode") => {
                                kask.fusion.get_or_insert_default().mode = Some(parsed.clone());
                            }
                            ("fusion", "algo_method") => {
                                kask.fusion.get_or_insert_default().algo_method =
                                    Some(parsed.clone());
                            }
                            ("fusion", "skills") => {
                                kask.fusion.get_or_insert_default().skills = Some(parsed.clone());
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
                            _ => {}
                        }
                    },
                );
            }
        })
}

// ---------------------------------------------------------------------------
// Codegraph sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_codegraph_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let codegraph = raw.and_then(|c| c.codegraph).unwrap_or_default();
    let db_path = codegraph.db_path.unwrap_or_default();

    let db_input = kask_string_input(
        "kask-codegraph-db-path",
        "Database Path",
        "(in-memory)",
        db_path,
        "codegraph",
        "db_path",
    );

    v_flex()
        .id("kask-codegraph-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Codegraph"))
                .child(
                    Label::new(
                        "The codegraph server provides code structure query and traversal. \
                         Configure the database path for persistent graph storage."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Database Path"))
                .child(
                    Label::new("SQLite database path for persistent code graph storage. Leave empty for in-memory.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(db_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Companies sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_companies_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let companies = raw.and_then(|c| c.companies).unwrap_or_default();
    let staleness_days = companies
        .chronic_staleness_days
        .map(|v| format!("{v}"))
        .unwrap_or_default();
    let fermi_defaults = companies.fermi_defaults.unwrap_or_default();

    let staleness_input = kask_string_input(
        "kask-companies-staleness-days",
        "Chronic Staleness Days",
        "0",
        staleness_days,
        "companies",
        "chronic_staleness_days",
    );
    let fermi_input = kask_string_input(
        "kask-companies-fermi-defaults",
        "Fermi Defaults (JSON)",
        "{\"growth\": [...], \"margin\": [...]}",
        fermi_defaults,
        "companies",
        "fermi_defaults",
    );

    v_flex()
        .id("kask-companies-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Companies"))
                .child(
                    Label::new(
                        "The companies server provides company research and filings. \
                         Configure superforecasting parameters."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Chronic Staleness Days"))
                .child(
                    Label::new("Staleness threshold in days for the superforecasting learning state. 0 uses the default.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(staleness_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Fermi Defaults"))
                .child(
                    Label::new("JSON with growth and margin question arrays for Fermi decomposition. Leave empty for hardcoded defaults.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(fermi_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Corpus sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_corpus_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let corpus = raw.and_then(|c| c.corpus).unwrap_or_default();
    let embedding_model = corpus.embedding_model.unwrap_or_default();
    let template_root = corpus
        .template_root
        .unwrap_or_else(|| "registry".to_string());

    let embedding_model_input = kask_string_input(
        "kask-corpus-embedding-model",
        "Embedding Model",
        "DeepInfra/Qwen/Qwen3-Embedding-0.6B",
        embedding_model,
        "corpus",
        "embedding_model",
    );
    let template_root_input = kask_string_input(
        "kask-corpus-template-root",
        "Template Root",
        "registry",
        template_root,
        "corpus",
        "template_root",
    );

    v_flex()
        .id("kask-corpus-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Corpus"))
                .child(
                    Label::new(
                        "The corpus server provides document corpus management, \
                         OCR, and QA generation. Configure embedding and OCR settings."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Embedding Model"))
                .child(
                    Label::new("Override the embedding model (e.g., DI/Qwen/Qwen3-Embedding-0.6B). Leave empty for default.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(embedding_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Template Root"))
                .child(
                    Label::new("Root directory for Jinja2 templates. Default: registry.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(template_root_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Media sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_media_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let media = raw.and_then(|c| c.media).unwrap_or_default();
    let tts_model = media.tts_model.unwrap_or_default();
    let stt_model = media.stt_model.unwrap_or_default();
    let vision_model = media.vision_model.unwrap_or_default();
    let image_gen_model = media.image_gen_model.unwrap_or_default();

    let tts_input = kask_string_input(
        "kask-media-tts-model",
        "TTS Model",
        "fal.ai/Qwen3-TTS",
        tts_model,
        "media",
        "tts_model",
    );
    let stt_input = kask_string_input(
        "kask-media-stt-model",
        "STT Model",
        "fal.ai/wizper",
        stt_model,
        "media",
        "stt_model",
    );
    let vision_input = kask_string_input(
        "kask-media-vision-model",
        "Vision Model",
        "KiloCode/Qwen/Qwen3-VL-235B-A22B-Instruct",
        vision_model,
        "media",
        "vision_model",
    );
    let image_gen_input = kask_string_input(
        "kask-media-image-gen-model",
        "Image Generation Model",
        "fal.ai/flux-2",
        image_gen_model,
        "media",
        "image_gen_model",
    );

    v_flex()
        .id("kask-media-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Media"))
                .child(
                    Label::new(
                        "The media server provides image generation, OCR, TTS, and STT. \
                         Configure model overrides for each capability."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("TTS Model"))
                .child(
                    Label::new("Text-to-speech model override. Leave empty for default (FA/qwen-3-tts).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(tts_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("STT Model"))
                .child(
                    Label::new("Speech-to-text model override. Leave empty for default (FA/wizper).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(stt_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Vision Model"))
                .child(
                    Label::new("Vision model override for OCR and image analysis. Leave empty for default.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(vision_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Image Generation Model"))
                .child(
                    Label::new("Image generation model override. Leave empty for default (FA/flux-2).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(image_gen_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Scenarios sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_scenarios_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let scenarios = raw.and_then(|c| c.scenarios).unwrap_or_default();
    let data_dir = scenarios.data_dir.unwrap_or_default();

    let data_dir_input = kask_string_input(
        "kask-scenarios-data-dir",
        "Data Directory",
        "(in-memory)",
        data_dir,
        "scenarios",
        "data_dir",
    );

    v_flex()
        .id("kask-scenarios-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Scenarios"))
                .child(
                    Label::new(
                        "The scenarios server provides scenario planning and Wardley mapping. \
                         Configure the data directory for scenario persistence.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Data Directory"))
                .child(
                    Label::new(
                        "Directory for scenario data persistence. Leave empty for in-memory.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(data_dir_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Training sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_training_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let training = raw.and_then(|c| c.training).unwrap_or_default();
    let host = training.host.unwrap_or_default();
    let cache_dir = training.cache_dir.unwrap_or_default();

    let host_input = kask_string_input(
        "kask-training-host",
        "Training Host",
        "deepinfra | nebius | runpod",
        host,
        "training",
        "host",
    );
    let cache_dir_input = kask_string_input(
        "kask-training-cache-dir",
        "Cache Directory",
        "(agent adapters dir)",
        cache_dir,
        "training",
        "cache_dir",
    );

    v_flex()
        .id("kask-training-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Training"))
                .child(
                    Label::new(
                        "The training server provides LoRA training configuration and audit. \
                         Configure host selection and cache directory."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Training Host"))
                .child(
                    Label::new("Host override: deepinfra, nebius, or runpod. Leave empty for auto-detect from API keys.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(host_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Cache Directory"))
                .child(
                    Label::new("Cache directory for dataset pipeline. Leave empty for the agent adapters directory.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(cache_dir_input),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Fusion sub-page
// ---------------------------------------------------------------------------

/// The fusion modes offered in the UI. Kept in sync with
/// `hkask_types::fusion::FusionMode`'s serde renames.
const FUSION_MODES: &[(&str, &str)] = &[
    (
        "synthesis",
        "Synthesis — compose a unified response from all panelists",
    ),
    (
        "best-of-n",
        "Best-of-N — pick the single best panel response",
    ),
    (
        "critique",
        "Critique — 2-round: draft → panel critique → revised final",
    ),
    (
        "deliberation",
        "Deliberation — multi-round with convergence check",
    ),
    (
        "pi",
        "Plan-Implement — 2-phase: strategy plan → implementation plan",
    ),
    ("algo", "Algo — deterministic JSON merge, no LLM judge call"),
];

/// The algo merge strategies. Only meaningful when `mode == "algo"`.
const ALGO_METHODS: &[(&str, &str)] = &[
    ("merge", "Merge — recursive JSON union (2 panelists)"),
    ("vote", "Vote — majority vote (scales beyond 2 panelists)"),
];

// ---------------------------------------------------------------------------
// Models sub-page
// ---------------------------------------------------------------------------

pub(crate) fn render_models_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let models = raw.and_then(|c| c.models).unwrap_or_default();
    let default_model = models.default_model.unwrap_or_default();
    let embedding_model = models.embedding_model.unwrap_or_default();
    let classifier_model = models.classifier_model.unwrap_or_default();

    let default_model_input = kask_string_input(
        "kask-models-default",
        "Default Inference Model",
        "openrouter/z-ai/glm-5.2",
        default_model,
        "models",
        "default_model",
    );
    let embedding_model_input = kask_string_input(
        "kask-models-embedding",
        "Embedding Model",
        "openrouter/z-ai/glm-5.2",
        embedding_model,
        "models",
        "embedding_model",
    );
    let classifier_model_input = kask_string_input(
        "kask-models-classifier",
        "Classifier Model",
        "openrouter/z-ai/glm-5.2",
        classifier_model,
        "models",
        "classifier_model",
    );

    v_flex()
        .id("kask-models-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Models"))
                .child(
                    Label::new(
                        "Kask-wide model configuration. These provider-prefixed model \
                         names (e.g. \"openrouter/z-ai/glm-5.2\") override the kask \
                         defaults for inference, embedding, and classification. When \
                         fusion is enabled, the fusion judge model takes precedence \
                         for inference.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Default Inference Model"))
                .child(
                    Label::new(
                        "Provider-prefixed model for the Curator, skill cascade, and \
                         kask panel inference. Leave empty to use the kask default \
                         (openrouter/z-ai/glm-5.2). When fusion is enabled, the fusion \
                         judge model takes precedence.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(default_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Embedding Model"))
                .child(
                    Label::new(
                        "Provider-prefixed model for corpus indexing and memory semantic \
                         recall. Leave empty to fall back to the corpus MCP server's \
                         embedding_model setting, then to the kask default.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(embedding_model_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Classifier Model"))
                .child(
                    Label::new(
                        "Provider-prefixed model for guard/regulation classification \
                         tasks. Leave empty to use the kask default.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(classifier_model_input),
        )
        .into_any_element()
}

pub(crate) fn render_fusion_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let fusion = raw.and_then(|c| c.fusion).unwrap_or_default();
    let enabled = fusion.enabled.unwrap_or(false);
    let judge_model = fusion.judge_model.unwrap_or_default();
    let panel_models = fusion.panel_models.unwrap_or_default();
    let mode = fusion.mode.unwrap_or_else(|| "synthesis".to_string());
    let algo_method = fusion.algo_method.unwrap_or_else(|| "merge".to_string());
    let skills = fusion.skills.unwrap_or_default();
    let max_rounds = fusion
        .max_rounds
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "5".to_string());
    let openrouter_max_price = fusion
        .openrouter_max_price
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "1.0".to_string());
    let openrouter_min_intelligence = fusion
        .openrouter_min_intelligence
        .map(|v| format!("{v}"))
        .unwrap_or_else(|| "40.0".to_string());
    let coherence_threshold = fusion
        .coherence_threshold
        .map(|v| format!("{v}"))
        .unwrap_or_default();
    let panel_sizing_enabled = fusion.panel_sizing_enabled.unwrap_or(false);
    let pressure_adaptive_enabled = fusion.pressure_adaptive_enabled.unwrap_or(false);

    let enabled_toggle = SwitchField::new(
        "kask-fusion-enabled",
        Some("Enable Fusion"),
        Some(
            "When enabled, the Curator and kask panel route inference through a panel \
             of models judged by the configured judge model. When disabled, all \
             inference uses the single selected LanguageModel."
                .into(),
        ),
        if enabled {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let is_enabled = *state == ToggleState::Selected;
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .fusion
                        .get_or_insert_default()
                        .enabled = Some(is_enabled);
                },
            );
        },
    )
    .tab_index(0);

    let judge_input = kask_string_input(
        "kask-fusion-judge-model",
        "Judge Model",
        "OpenRouter/z-ai/glm-5.2",
        judge_model,
        "fusion",
        "judge_model",
    );

    let panel_input = kask_string_input(
        "kask-fusion-panel-models",
        "Panel Models",
        "OpenRouter/z-ai/glm-5.2, OpenRouter/qwen/qwen3-235b-a22b, OpenRouter/minimax/minimax3",
        panel_models,
        "fusion",
        "panel_models",
    );

    let mode_input = kask_string_input(
        "kask-fusion-mode",
        "Mode",
        "synthesis",
        mode,
        "fusion",
        "mode",
    );

    let algo_method_input = kask_string_input(
        "kask-fusion-algo-method",
        "Algo Method",
        "merge",
        algo_method,
        "fusion",
        "algo_method",
    );

    let skills_input = kask_string_input(
        "kask-fusion-skills",
        "Skills",
        "pragmatic-semantics, coding-guidelines",
        skills,
        "fusion",
        "skills",
    );

    let max_rounds_input = SettingsInputField::new("kask-fusion-max-rounds")
        .tab_index(0)
        .with_initial_text(max_rounds)
        .with_placeholder("5")
        .aria_label("Max Rounds")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value
                && let Ok(parsed) = text.parse::<u32>()
            {
                SettingsStore::global(cx).update_settings_file(
                    <dyn fs::Fs>::global(cx),
                    move |settings, _| {
                        settings
                            .kask
                            .get_or_insert_default()
                            .fusion
                            .get_or_insert_default()
                            .max_rounds = Some(parsed);
                    },
                );
            }
        });

    let openrouter_max_price_input = SettingsInputField::new("kask-fusion-or-max-price")
        .tab_index(0)
        .with_initial_text(openrouter_max_price)
        .with_placeholder("1.0")
        .aria_label("OpenRouter Max Price")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value
                && let Ok(parsed) = text.parse::<f64>()
            {
                SettingsStore::global(cx).update_settings_file(
                    <dyn fs::Fs>::global(cx),
                    move |settings, _| {
                        settings
                            .kask
                            .get_or_insert_default()
                            .fusion
                            .get_or_insert_default()
                            .openrouter_max_price = Some(parsed);
                    },
                );
            }
        });

    let openrouter_min_intelligence_input =
        SettingsInputField::new("kask-fusion-or-min-intelligence")
            .tab_index(0)
            .with_initial_text(openrouter_min_intelligence)
            .with_placeholder("40.0")
            .aria_label("OpenRouter Min Intelligence")
            .confirm_on_focus_out()
            .on_confirm(move |value, _window, cx| {
                if let Some(text) = value
                    && let Ok(parsed) = text.parse::<f64>()
                {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .fusion
                                .get_or_insert_default()
                                .openrouter_min_intelligence = Some(parsed);
                        },
                    );
                }
            });

    // Codette-inspired: coherence threshold for measured convergence.
    let coherence_threshold_input = SettingsInputField::new("kask-fusion-coherence-threshold")
        .tab_index(0)
        .with_initial_text(coherence_threshold)
        .with_placeholder("0.8")
        .aria_label("Coherence Threshold")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            let parsed = value.and_then(|t| t.parse::<f64>().ok());
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .fusion
                        .get_or_insert_default()
                        .coherence_threshold = parsed;
                },
            );
        });

    // Codette-inspired: panel sizing toggle.
    let panel_sizing_toggle = SwitchField::new(
        "kask-fusion-panel-sizing",
        Some("Panel Sizing"),
        Some(
            "When enabled, simple queries dispatch fewer panel models (1 for Simple, \
             2 for Medium, all for Complex). Reduces cost on simple queries. \
             Default: off (full panel always)."
                .into(),
        ),
        if panel_sizing_enabled {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let is_enabled = *state == ToggleState::Selected;
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .fusion
                        .get_or_insert_default()
                        .panel_sizing_enabled = Some(is_enabled);
                },
            );
        },
    );

    // Codette-inspired: pressure-adaptive degradation toggle.
    let pressure_adaptive_toggle = SwitchField::new(
        "kask-fusion-pressure-adaptive",
        Some("Pressure-Adaptive Degradation"),
        Some(
            "When enabled, panel size is reduced under high latency pressure \
             (rolling average of recent dispatch times). Degraded output is \
             better than hard failure. Default: off."
                .into(),
        ),
        if pressure_adaptive_enabled {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let is_enabled = *state == ToggleState::Selected;
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .fusion
                        .get_or_insert_default()
                        .pressure_adaptive_enabled = Some(is_enabled);
                },
            );
        },
    );

    // Build the mode options as a static hint label (the input is free-text
    // but we list the valid values so users know what to type).
    let mode_hint = FUSION_MODES
        .iter()
        .map(|(id, desc)| format!("{id} — {desc}"))
        .collect::<Vec<_>>()
        .join("\n");

    let algo_hint = ALGO_METHODS
        .iter()
        .map(|(id, desc)| format!("{id} — {desc}"))
        .collect::<Vec<_>>()
        .join("\n");

    v_flex()
        .id("kask-fusion-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .pb_16()
        .gap_4()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Fusion"))
                .child(
                    Label::new(
                        "Multi-model fusion inference. When enabled, inference is routed \
                         through a panel of models judged by the configured judge model \
                         according to the selected deliberation mode.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(enabled_toggle)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Judge Model"))
                .child(
                    Label::new(
                        "Provider-prefixed judge/fuser model (e.g. \"OpenRouter/z-ai/glm-5.2\"). \n                         Leave empty to use the kask default (OpenRouter/z-ai/glm-5.2).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(judge_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Panel Models"))
                .child(
                    Label::new(
                        "Comma-separated provider-prefixed panel models (e.g. \n                         \"OpenRouter/z-ai/glm-5.2, OpenRouter/qwen/qwen3-235b-a22b, \n                         OpenRouter/minimax/minimax3\"). Leave empty to use the kask \n                         default panel or auto-discovery.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(panel_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Mode"))
                .child(
                    Label::new(mode_hint)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(mode_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Algo Method"))
                .child(
                    Label::new(format!("{algo_hint}\nOnly used when mode == \"algo\"."))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(algo_method_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Skills"))
                .child(
                    Label::new(
                        "Comma-separated skill anchors injected into the judge's reasoning \
                         framework (e.g. \"pragmatic-semantics, coding-guidelines\"). \
                         Unknown anchors are silently dropped.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(skills_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Max Rounds"))
                .child(
                    Label::new("Maximum rounds for deliberation mode. Ignored for other modes.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(max_rounds_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("OpenRouter Auto-Discovery Thresholds"))
                .child(
                    Label::new(
                        "When the panel models field is empty or set to \"auto\", the panel \
                         is populated from OpenRouter models passing both thresholds. \
                         These gates also feed the default-model onboarding thresholds.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Max Price (USD per 1M prompt tokens)"))
                .child(openrouter_max_price_input),
        )
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Min Intelligence Index"))
                .child(openrouter_min_intelligence_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Codette-Inspired Enhancements"))
                .child(
                    Label::new(
                        "Experimental features inspired by the Codette multi-perspective \
                         reasoning architecture. All are opt-in and disabled by default.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Coherence Threshold"))
                .child(
                    Label::new(
                        "When set (0.0–1.0), the orchestrator computes epistemic tension ξ \
                         and coherence Γ from panel response embeddings in deliberation \
                         mode. If Γ exceeds this threshold, an advisory measured-convergence \
                         signal is emitted. Leave empty to disable. Requires an embedding \
                         API key (DI_API_KEY or OR_API_KEY).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(coherence_threshold_input),
        )
        .child(Divider::horizontal())
        .child(panel_sizing_toggle)
        .child(Divider::horizontal())
        .child(pressure_adaptive_toggle)
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Helpers (existing)
// ---------------------------------------------------------------------------

/// Read the raw `KaskSettingsContent` from the user settings file.
fn raw_kask_settings(cx: &App) -> Option<settings::KaskSettingsContent> {
    SettingsStore::global(cx)
        .raw_user_settings()
        .and_then(|user| user.content.kask.clone())
}
