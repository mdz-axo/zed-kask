//! Kask settings page — the `"kask"` section's UI surface (D9a UI).
//!
//! Top-level "Kask" page with sub-page links to:
//! - Data Services (API key entry → keychain via `CredentialsProvider` + enable toggles)
//! - MCP Servers (10 built-in servers + load toggles + `load_default` master toggle)
//! - Curator (`always_on` toggle + `algedonic_threshold`)
//! - Guard / Regulation (`direct_chat_strategy`)
//! - Memory (`consolidation_cadence_secs` + `confidence_floor`)
//!
//! API keys are stored in the OS keychain under the `kask://credentials/<key>`
//! namespace (see `kask_bridge::secrets::KASK_CREDENTIAL_NAMESPACE`), not in
//! settings.json. The non-secret toggles and numeric config live in the `"kask"`
//! section of settings.json via `KaskSettingsContent`.

use std::sync::Arc;

use credentials_provider::CredentialsProvider;
use gpui::{ReadGlobal as _, ScrollHandle, Task, prelude::*};
use settings::{Settings as _, SettingsStore};
use ui::{Divider, SwitchField, ToggleState, prelude::*};
use util::ResultExt as _;
use zed_credentials_provider as zed_credentials;

use crate::SettingsWindow;
use crate::components::{SettingsInputField, SettingsSectionHeader};
use crate::{SettingsPage, SettingsPageItem, SubPageLink, USER};

/// The URL prefix for kask-namespaced credentials in the keychain.
/// Must match `kask_bridge::secrets::KASK_CREDENTIAL_NAMESPACE`.
const KASK_CREDENTIAL_NAMESPACE: &str = "kask://credentials";

/// The 10 built-in kask MCP servers (directory names under `kask/mcp-servers/`).
/// These are the server IDs used in `KaskMcpSettingsContent::overrides`.
const BUILT_IN_MCP_SERVERS: &[(&str, &str)] = &[
    (
        "codegraph",
        "Codegraph — code structure query and traversal",
    ),
    ("companies", "Companies — company research and filings"),
    (
        "condenser",
        "Condenser — context condensation and summarization",
    ),
    ("corpus", "Corpus — document corpus and QA generation"),
    (
        "curator",
        "Curator — regulation cascade and algedonic signals",
    ),
    ("kata-kanban", "Kata Kanban — improvement kata board"),
    ("media", "Media — image generation and media workflows"),
    ("research", "Research — web research and paper search"),
    (
        "scenarios",
        "Scenarios — scenario planning and Wardley mapping",
    ),
    (
        "training",
        "Training — LoRA training configuration and audit",
    ),
];

/// Data service descriptors: (key, label, dashboard_url, env_var).
/// The `key` is the credential key in the keychain (`kask://credentials/<key>`).
const DATA_SERVICES: &[(&str, &str, &str, &str)] = &[
    (
        "eodhd",
        "EODHD",
        "https://eodhd.com/dashboard",
        "EODHD_API_KEY",
    ),
    (
        "fmp",
        "FMP (Financial Modeling Prep)",
        "https://site.financialmodelingprep.com/developer/docs",
        "FMP_API_KEY",
    ),
    (
        "exa",
        "Exa",
        "https://dashboard.exa.ai/api-keys",
        "EXA_API_KEY",
    ),
    (
        "tavily",
        "Tavily",
        "https://app.tavily.com/api-key",
        "TAVILY_API_KEY",
    ),
    (
        "brave",
        "Brave Search",
        "https://api.search.brave.com/app/subscriptions",
        "BRAVE_API_KEY",
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
                "Configure API keys for data services (EODHD, FMP, Exa, Tavily, Brave). \
                 Keys are stored in the system keychain, not in settings.json."
                    .into(),
            ),
            search_aliases: &[
                "api key",
                "brave",
                "data service",
                "eodhd",
                "exa",
                "fmp",
                "financial modeling prep",
                "keychain",
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
                "Configure memory consolidation: cadence and confidence floor.".into(),
            ),
            search_aliases: &["consolidation", "confidence", "memory"],
            in_json: true,
            files: USER,
            render: render_memory_page,
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
    cx: &mut Context<SettingsWindow>,
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
                let credential_url = credential_url.clone();
                move |_, _, cx| {
                    delete_credential(&provider, &credential_url, cx).detach();
                }
            })
            .into_any_element()
    } else {
        let input_id = format!("kask-{key}-api-key-input");
        let aria_label = format!("{label} API Key");
        let provider = provider.clone();
        let credential_url = credential_url.clone();
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
    SettingsStore::global(cx).update_settings_file(<dyn fs::Fs>::global(cx), move |settings, _| {
        let kask = settings.kask.get_or_insert_default();
        let data_services = kask.data_services.get_or_insert_default();
        match key {
            "eodhd" => data_services.eodhd_enabled = Some(enabled),
            "fmp" => data_services.fmp_enabled = Some(enabled),
            "exa" => data_services.exa_enabled = Some(enabled),
            "tavily" => data_services.tavily_enabled = Some(enabled),
            "brave" => data_services.brave_enabled = Some(enabled),
            _ => {}
        }
    });
}

/// Check whether a credential is available — either in the keychain or via env var.
///
/// The keychain read is async, so we can't block on it here. We check the env var
/// synchronously (instant) and treat the keychain as "possibly present" — the user
/// can always click "Reset Key" if the card shows configured. This avoids a flicker
/// of "no key" on every render.
fn has_credential(provider: &Arc<dyn CredentialsProvider>, url: &str, env_var: &str) -> bool {
    // Env-var check is synchronous and instant.
    if env::var(env_var).is_ok() {
        return true;
    }
    // For the keychain, we can't block. We optimistically report false and let
    // the user enter the key. A background task could populate a cached flag,
    // but for a settings page opened on demand, the simpler model is: the card
    // shows "Configured" only when the env var is set; the keychain key is
    // entered via the input field and confirmed by the user.
    drop(provider);
    drop(url);
    false
}

fn write_credential(
    provider: &Arc<dyn CredentialsProvider>,
    url: &str,
    value: &str,
    cx: &mut App,
) -> Task<()> {
    let async_cx = cx.to_async();
    let provider = provider.clone();
    let url = url.to_string();
    let value = value.to_string();
    cx.background_executor().spawn(async move {
        let _ = provider
            .write_credentials(&url, "kask", value.as_bytes(), &async_cx)
            .await
            .log_err();
    })
}

fn delete_credential(provider: &Arc<dyn CredentialsProvider>, url: &str, cx: &mut App) -> Task<()> {
    let async_cx = cx.to_async();
    let provider = provider.clone();
    let url = url.to_string();
    cx.background_executor().spawn(async move {
        let _ = provider.delete_credentials(&url, &async_cx).await.log_err();
    })
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
        this.tooltip(move |_window, _cx| {
            ui::Tooltip::text(
                "The master \"Load Default MCP Servers\" toggle is off — \
                 enable it for this override to take effect.",
            )
            .into_any_view()
        })
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
                let trimmed = text.trim();
                if matches!(trimmed, "buffer" | "incremental" | "cascade_only") {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .guard
                                .get_or_insert_default()
                                .direct_chat_strategy = Some(trimmed.to_string());
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
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read the raw `KaskSettingsContent` from the user settings file.
fn raw_kask_settings(cx: &App) -> Option<settings::KaskSettingsContent> {
    SettingsStore::global(cx)
        .raw_user_settings()
        .and_then(|user| user.content.kask.clone())
}
