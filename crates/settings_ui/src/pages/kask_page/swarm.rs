//! Swarm sub-page — Agent Bestiary World backend mode, credit ceiling,
//! curator consent default, default agent model, A2A HTTP gateway, and
//! local semantic-memory configuration.
//!
//! No per-server path fields — the swarm server's local agent registry,
//! local swarms registry, and memory DB are derived from the global
//! `data_dir` as `mcp/swarm/agents/curated/`, `mcp/swarm/swarms/`, and
//! `mcp/swarm/memory.db` by `mcp_env()`. The server reads them via
//! `HKASK_LOCAL_AGENTS_DIR`, `HKASK_LOCAL_SWARMS_DIR`, and
//! `HKASK_SWARM_MEMORY_DB`.

use super::*;

pub(crate) fn render_swarm_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let swarm: kask_bridge::KaskSwarmSettings = raw
        .and_then(|c| c.swarm)
        .map(Into::into)
        .unwrap_or_default();
    let api_url = swarm.api_url;
    let max_credits = swarm.max_credits_per_dispatch.to_string();
    let curator_consent_default = swarm.curator_consent_default;
    let skills_dir = swarm.skills_dir;
    let default_agent_model = swarm.default_agent_model;
    let a2a_http_enabled = swarm.a2a_http_enabled;
    let memory_passphrase = swarm.memory_passphrase;
    let embedding_dim = swarm.embedding_dim.to_string();

    // ABW API key — the core credential for ABW mode. Lives in the keychain
    // under `kask://credentials/hkask_abw_api_key`, injected as
    // `HKASK_ABW_API_KEY` at server launch. Unlike data-service keys, this one
    // has no `ui_toggle` in `DATA_SERVICES` (it's not a data service — it's the
    // swarm backend auth credential), so it has no Data Services page row.
    // Without this field the operator had no UI path to configure it at all.
    let credentials_provider = zed_credentials::global(cx);
    let abw_credential_url = format!("{KASK_CREDENTIAL_NAMESPACE}/hkask_abw_api_key");
    let abw_key_configured = has_credential(
        &credentials_provider,
        &[&abw_credential_url],
        "HKASK_ABW_API_KEY",
    );
    let abw_api_key_field = if abw_key_configured {
        ConfiguredApiCard::new("kask-swarm-abw-api-key-reset", "ABW API Key Configured")
            .button_label("Reset Key")
            .button_tab_index(2)
            .on_click({
                let provider = credentials_provider.clone();
                let url = abw_credential_url;
                move |_, _, cx| {
                    delete_credential(&provider, &url, cx).detach();
                }
            })
            .into_any_element()
    } else {
        let provider = credentials_provider.clone();
        let url = abw_credential_url;
        v_flex()
            .gap_1()
            .child(Label::new("ABW API Key"))
            .child(
                Label::new(
                    "Agent Bestiary World Pro-tier API key (Authorization: Bearer). \
                     Required for authenticated tools (swarm_get_swarm, swarm_hire, \
                     swarm_delegate, etc.). Without it, the swarm server runs in \
                     catalogue-only mode. Stored in the keychain under \
                     kask://credentials/hkask_abw_api_key. Or set the \
                     HKASK_ABW_API_KEY env var and restart Zed.",
                )
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
            .child(
                SettingsInputField::new("kask-swarm-abw-api-key-input")
                    .tab_index(2)
                    .with_placeholder("xxxxxxxxxxxxxxxxxxxx")
                    .aria_label("ABW API Key")
                    .on_confirm(move |api_key, _window, cx| {
                        if let Some(key_value) = api_key.filter(|key_value| !key_value.is_empty()) {
                            write_credential(&provider, &url, &key_value, cx).detach();
                        }
                    }),
            )
            .into_any_element()
    };

    // The `kask.swarm.mode` setting is intentionally NOT exposed as a toggle
    // here. Both backends (cloud ABW + local substrate) are always available
    // in the swarm panel — the panel shows both and the operator picks the
    // target per action (Author/Compose forms have a Cloud/Local toggle).
    // The setting only controls a server-side startup warning, so exposing
    // it as a top-level toggle misleads operators into thinking it's an
    // either/or capability gate (it is not — both tool sets are always
    // registered). The setting remains in settings.json for the startup
    // warning and for Steer-mode context; advanced operators can set it
    // there if they want to suppress the ABW-key warning in local-only use.

    // Curator consent default toggle.
    let consent_toggle = SwitchField::new(
        "kask-swarm-curator-consent-default",
        Some("Curator Consent Default"),
        Some(
            "When enabled, Xaman Ek curator calls may be initiated without a per-call \
             consent token (S5 policy opt-in). Default false — sending task content to \
             a third-party curator requires explicit per-call consent."
                .into(),
        ),
        if curator_consent_default {
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
                        .swarm
                        .get_or_insert_default()
                        .curator_consent_default = Some(enabled);
                },
            );
        },
    )
    .tab_index(1);

    let api_url_input = SettingsInputField::new("kask-swarm-api-url")
        .tab_index(2)
        .with_initial_text(api_url)
        .with_placeholder("https://agent-bestiary.world")
        .aria_label("ABW API URL")
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
                            .swarm
                            .get_or_insert_default()
                            .api_url = if parsed.is_empty() {
                            None
                        } else {
                            Some(parsed)
                        };
                    },
                );
            }
        });

    let max_credits_input = SettingsInputField::new("kask-swarm-max-credits")
        .tab_index(3)
        .with_initial_text(max_credits)
        .with_placeholder("50")
        .aria_label("Max Credits Per Dispatch")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                if let Ok(parsed) = text.trim().parse::<u32>() {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .swarm
                                .get_or_insert_default()
                                .max_credits_per_dispatch = Some(parsed);
                        },
                    );
                }
            }
        });

    let skills_dir_input = SettingsInputField::new("kask-swarm-skills-dir")
        .tab_index(4)
        .with_initial_text(skills_dir)
        .with_placeholder(".agents/skills")
        .aria_label("Skills Directory")
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
                            .swarm
                            .get_or_insert_default()
                            .skills_dir = if parsed.is_empty() {
                            None
                        } else {
                            Some(parsed)
                        };
                    },
                );
            }
        });

    let default_agent_model_input = SettingsInputField::new("kask-swarm-default-agent-model")
        .tab_index(5)
        .with_initial_text(default_agent_model)
        .with_placeholder("claude-haiku-4-5-20251001")
        .aria_label("Default Agent Model")
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
                            .swarm
                            .get_or_insert_default()
                            .default_agent_model = if parsed.is_empty() {
                            None
                        } else {
                            Some(parsed)
                        };
                    },
                );
            }
        });

    let a2a_http_toggle = SwitchField::new(
        "kask-swarm-a2a-http-enabled",
        Some("A2A HTTP Gateway"),
        Some(
            "Enable the A2A HTTP gateway (loopback JSON-RPC server that exposes local \
             agents to external A2A clients). Opens a loopback port — only enable when \
             you need external A2A clients to reach your local agents. Or set \
             HKASK_A2A_HTTP_ENABLE=1."
                .into(),
        ),
        if a2a_http_enabled {
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
                        .swarm
                        .get_or_insert_default()
                        .a2a_http_enabled = Some(enabled);
                },
            );
        },
    )
    .tab_index(6);

    let memory_passphrase_input = SettingsInputField::new("kask-swarm-memory-passphrase")
        .tab_index(7)
        .with_initial_text(memory_passphrase)
        .with_placeholder("allostery")
        .aria_label("Memory Passphrase")
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
                            .swarm
                            .get_or_insert_default()
                            .memory_passphrase = if parsed.is_empty() {
                            None
                        } else {
                            Some(parsed)
                        };
                    },
                );
            }
        });

    let embedding_dim_input = SettingsInputField::new("kask-swarm-embedding-dim")
        .tab_index(8)
        .with_initial_text(embedding_dim)
        .with_placeholder("1024")
        .aria_label("Embedding Dimension")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                if let Ok(parsed) = text.trim().parse::<usize>() {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .swarm
                                .get_or_insert_default()
                                .embedding_dim = Some(parsed);
                        },
                    );
                }
            }
        });

    v_flex()
        .id("kask-swarm-page")
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
                .child(SettingsSectionHeader::new("Swarm"))
                .child(
                    Label::new(
                        "Agent Bestiary World agent swarms and the Xaman Ek curator. \
                         Configure the backend mode (remote ABW vs local substrate), the \
                         per-dispatch credit ceiling, and the curator consent default. \
                         Local agent cards, local swarms, and the semantic-memory DB \
                         persist to mcp/swarm/ under the shared kask data directory (set \
                         on the General page). There are no per-server path settings — \
                         all MCP servers share the single data directory.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(abw_api_key_field)
        .child(Divider::horizontal())
        .child(consent_toggle)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("ABW API URL"))
                .child(
                    Label::new(
                        "ABW API base URL override. Leave empty for the default \
                         (https://agent-bestiary.world). Or set HKASK_ABW_API_URL.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(api_url_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Max Credits Per Dispatch"))
                .child(
                    Label::new(
                        "Per-dispatch credit ceiling for spend tools. Dispatches estimated \
                         above this are refused before any credit is spent. Or set \
                         HKASK_ABW_MAX_CREDITS.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(max_credits_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Skills Directory"))
                .child(
                    Label::new(
                        "Directory containing the zed-kask skill corpus (.agents/skills/). \
                         Read to inject skill descriptions into the local agent's system \
                         prompt (skill-awareness). Leave empty to run skill-blind. Or set \
                         HKASK_SKILLS_DIR.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(skills_dir_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Default Agent Model"))
                .child(
                    Label::new(
                        "Default model id for newly created ABW agents when the caller omits \
                         `model`. Leave empty for the server default \
                         (claude-haiku-4-5-20251001). Or set \
                         HKASK_ABW_DEFAULT_AGENT_MODEL.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(default_agent_model_input),
        )
        .child(Divider::horizontal())
        .child(a2a_http_toggle)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Memory Passphrase"))
                .child(
                    Label::new(
                        "SQLCipher passphrase for the local swarm semantic-memory store. \
                         Must be >=8 chars. Leave empty for the pre-release default \
                         (allostery). Or set HKASK_SWARM_MEMORY_PASSPHRASE.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(memory_passphrase_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Embedding Dimension"))
                .child(
                    Label::new(
                        "Embedding vector dimension for the semantic-memory embedding \
                         store. Default 1024. Or set HKASK_SWARM_EMBEDDING_DIM.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(embedding_dim_input),
        )
        .into_any_element()
}
