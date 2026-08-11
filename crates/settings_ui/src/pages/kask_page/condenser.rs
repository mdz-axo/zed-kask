//! Condenser sub-page — compression profile, auto-compress tool results,
//! and saliency window.

use super::*;

pub(crate) fn render_condenser_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let condenser: kask_bridge::KaskCondenserSettings = raw
        .and_then(|c| c.condenser)
        .map(Into::into)
        .unwrap_or_default();
    let profile = condenser.profile.as_str();
    let auto_compress = condenser.auto_compress_tool_results;
    let saliency_window = condenser.saliency_window.to_string();
    let persona_keywords = condenser.persona_keywords.join(", ");

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

    let persona_keywords_input = kask_string_input(
        "kask-condenser-persona-keywords",
        "Persona Keywords",
        "rust, llm, forecasting",
        persona_keywords,
        "condenser",
        "persona_keywords",
    );

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
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Persona Keywords"))
                .child(
                    Label::new(
                        "Comma-separated keywords for the condenser's word_rank saliency \
                         algorithm. Lines matching these keywords are prioritized when \
                         compressing tool output. Leave empty for no keyword \
                         prioritization. Or set HKASK_CONDENSER_PERSONA_KEYWORDS.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(persona_keywords_input),
        )
        .into_any_element()
}
