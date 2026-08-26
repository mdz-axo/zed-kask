//! Memory sub-page — consolidation cadence, confidence floor, recall limit,
//! recall minimum confidence, and auto-inject toggle.

use super::*;

pub(crate) fn render_memory_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let memory: kask_bridge::KaskMemorySettings = raw
        .and_then(|c| c.memory)
        .map(Into::into)
        .unwrap_or_default();
    let cadence = memory.consolidation_cadence_secs.to_string();
    let confidence_floor = memory.confidence_floor.to_string();
    let recall_limit = memory.recall_limit.to_string();
    let recall_min_confidence = memory.recall_min_confidence.to_string();
    let auto_inject = memory.auto_inject;
    let memory_life_days = memory.memory_life_days.to_string();

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

    let memory_life_input = SettingsInputField::new("kask-memory-life-days")
        .tab_index(0)
        .with_initial_text(memory_life_days)
        .with_placeholder("180")
        .aria_label("Memory Life (days)")
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
                                .memory_life_days = Some(parsed);
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
                .child(Label::new("Memory Life (days)"))
                .child(
                    Label::new(
                        "Memory life S in days (Wozniak-Gorzelanczyk forgetting curve \
                         R(t) = exp(-t/S)). After S days without recall, confidence \
                         decays to ≈36.8%; the half-life is S·ln(2). Recalling a \
                         memory resets its decay clock. Overridden by the \
                         HKASK_MEMORY_LIFE_DAYS env var.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(memory_life_input),
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
