//! Swarm sub-page — Agent Bestiary World backend mode, credit ceiling,
//! curator consent default, and local agent/swarm directories.

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
        .into_any_element()
}
