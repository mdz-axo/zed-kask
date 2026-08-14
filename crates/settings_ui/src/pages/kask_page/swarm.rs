//! Swarm sub-page — Agent Bestiary World backend mode, credit ceiling,
//! curator consent default, local agent/swarm directories, default agent
//! model, A2A HTTP gateway, and local semantic-memory configuration.

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
    let mode = swarm.mode;
    let api_url = swarm.api_url;
    let max_credits = swarm.max_credits_per_dispatch.to_string();
    let curator_consent_default = swarm.curator_consent_default;
    let local_agents_dir = swarm.local_agents_dir;
    let local_swarms_dir = swarm.local_swarms_dir;
    let skills_dir = swarm.skills_dir;
    let default_agent_model = swarm.default_agent_model;
    let a2a_http_enabled = swarm.a2a_http_enabled;
    let memory_passphrase = swarm.memory_passphrase;
    let memory_db_path = swarm.memory_db_path;
    let embedding_dim = swarm.embedding_dim.to_string();

    // Mode toggle: Abw (remote) vs Local (zed-kask substrate).
    let mode_is_local = mode == kask_bridge::SwarmModeConfig::Local;
    let mode_toggle = SwitchField::new(
        "kask-swarm-mode-local",
        Some("Local Mode"),
        Some(
            "Route swarm dispatches to zed-kask's local substrate crates instead of the \
             remote Agent Bestiary World service (v2 §15)."
                .into(),
        ),
        if mode_is_local {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let local = *state == ToggleState::Selected;
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    settings
                        .kask
                        .get_or_insert_default()
                        .swarm
                        .get_or_insert_default()
                        .mode = Some(if local {
                        settings_content::SwarmModeContent::Local
                    } else {
                        settings_content::SwarmModeContent::Abw
                    });
                },
            );
        },
    )
    .tab_index(0);

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

    let local_agents_dir_input = SettingsInputField::new("kask-swarm-local-agents-dir")
        .tab_index(4)
        .with_initial_text(local_agents_dir)
        .with_placeholder("agents/local/curated")
        .aria_label("Local Agents Directory")
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
                            .local_agents_dir = if parsed.is_empty() {
                            None
                        } else {
                            Some(parsed)
                        };
                    },
                );
            }
        });

    let local_swarms_dir_input = SettingsInputField::new("kask-swarm-local-swarms-dir")
        .tab_index(5)
        .with_initial_text(local_swarms_dir)
        .with_placeholder("agents/local/swarms")
        .aria_label("Local Swarms Directory")
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
                            .local_swarms_dir = if parsed.is_empty() {
                            None
                        } else {
                            Some(parsed)
                        };
                    },
                );
            }
        });

    let skills_dir_input = SettingsInputField::new("kask-swarm-skills-dir")
        .tab_index(6)
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
        .tab_index(7)
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
    .tab_index(8);

    let memory_passphrase_input = SettingsInputField::new("kask-swarm-memory-passphrase")
        .tab_index(9)
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

    let memory_db_path_input = SettingsInputField::new("kask-swarm-memory-db-path")
        .tab_index(10)
        .with_initial_text(memory_db_path)
        .with_placeholder("swarm_memory.db")
        .aria_label("Memory DB Path")
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
                            .memory_db_path = if parsed.is_empty() {
                            None
                        } else {
                            Some(parsed)
                        };
                    },
                );
            }
        });

    let embedding_dim_input = SettingsInputField::new("kask-swarm-embedding-dim")
        .tab_index(11)
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
                         per-dispatch credit ceiling, and the curator consent default.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(mode_toggle)
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
                .child(Label::new("Local Agents Directory"))
                .child(
                    Label::new(
                        "Directory containing local agent cards (<id>/agent_card.json), read \
                         in Local mode. Leave empty for the default (agents/local/curated). \
                         Or set HKASK_LOCAL_AGENTS_DIR.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(local_agents_dir_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Local Swarms Directory"))
                .child(
                    Label::new(
                        "Directory containing local swarms (<id>/swarm.json), the local \
                         replica of an ABW workspace roster. Leave empty for the default \
                         (agents/local/swarms). Or set HKASK_LOCAL_SWARMS_DIR.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(local_swarms_dir_input),
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
                .child(Label::new("Memory DB Path"))
                .child(
                    Label::new(
                        "On-disk path for the local swarm semantic-memory DB. Leave empty \
                         for the default (<hkask data dir>/swarm_memory.db). Or set \
                         HKASK_SWARM_MEMORY_DB.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(memory_db_path_input),
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
