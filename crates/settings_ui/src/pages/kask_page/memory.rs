//! Memory sub-page — consolidation cadence, confidence floor, recall limit,
//! recall minimum confidence, auto-inject and federated curator sources.

use super::*;

pub(crate) fn render_memory_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let kask: kask_bridge::KaskSettings = raw_kask_settings(cx).map(Into::into).unwrap_or_default();
    let sources = kask.federated_sources();
    let memory = kask.memory;
    let cadence = memory.consolidation_cadence_secs.to_string();
    let confidence_floor = memory.confidence_floor.to_string();
    let recall_limit = memory.recall_limit.to_string();
    let recall_min_confidence = memory.recall_min_confidence.to_string();
    let auto_inject = memory.auto_inject;
    let memory_life_days = memory.memory_life_days.to_string();
    let federated_auto_inject = memory.federated_auto_inject;
    let selected_ids = memory.federated_source_ids.clone();
    let source_rows: Vec<AnyElement> = match &sources {
        Ok(registered) => registered
            .iter()
            .map(|(id, display_name)| {
                let source_id = id.clone();
                let selected = selected_ids.contains(id);
                v_flex()
                    .gap_1()
                    .child(
                        SwitchField::new(
                            format!("kask-federated-source-{id}"),
                            Some(display_name.clone()),
                            Some(format!("Source ID: {id}").into()),
                            selected,
                            move |state, _window, cx| {
                                // The manifest may have changed since this row was rendered.
                                let current: kask_bridge::KaskSettings =
                                    raw_kask_settings(cx).map(Into::into).unwrap_or_default();
                                let Ok(registered) = current.federated_sources() else {
                                    return;
                                };
                                if !registered.iter().any(|(id, _)| id == &source_id) {
                                    return;
                                }
                                let source_id = source_id.clone();
                                let selected = *state == ToggleState::Selected;
                                SettingsStore::global(cx).update_settings_file(
                                    <dyn fs::Fs>::global(cx),
                                    move |settings, _| {
                                        let ids = settings
                                            .kask
                                            .get_or_insert_default()
                                            .memory
                                            .get_or_insert_default()
                                            .federated_source_ids
                                            .get_or_insert_default();
                                        ids.retain(|id| {
                                            id != &source_id
                                                && registered.iter().any(|(key, _)| key == id)
                                        });
                                        if selected {
                                            ids.push(source_id);
                                        }
                                    },
                                );
                            },
                        )
                        .tab_index(0),
                    )
                    .into_any_element()
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    let sources_error = sources.is_err();
    let sources_message = match &sources {
        Err(error) => format!("Federated sources unavailable or invalid: {error}"),
        Ok(registered) if registered.is_empty() => {
            "No federated sources are registered in the curator manifest.".to_string()
        }
        Ok(registered)
            if selected_ids
                .iter()
                .any(|id| !registered.iter().any(|(key, _)| key == id)) =>
        {
            "Some saved source IDs are not registered; review your selection.".to_string()
        }
        Ok(_) => {
            "Choose one or more registered sources below before enabling injection.".to_string()
        }
    };

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
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_2()
                .child(SettingsSectionHeader::new(
                    "Curator federated chat injection",
                ))
                .child(
                    Label::new(
                        "Changes to curator chat injection take effect after restarting Zed-Kask.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(
                    Label::new(sources_message)
                        .size(LabelSize::Small)
                        .color(if sources_error {
                            Color::Error
                        } else {
                            Color::Muted
                        }),
                )
                .child(
                    SwitchField::new(
                        "kask-memory-federated-auto-inject",
                        Some("Auto-Inject Federated Sources"),
                        Some(
                            "Opt in to injecting selected registered sources into curator chat."
                                .into(),
                        ),
                        federated_auto_inject,
                        move |state, _window, cx| {
                            let enabled = *state == ToggleState::Selected;
                            if enabled {
                                let current: kask_bridge::KaskSettings =
                                    raw_kask_settings(cx).map(Into::into).unwrap_or_default();
                                if !current.federated_sources().is_ok_and(|registered| {
                                    !current.memory.federated_source_ids.is_empty()
                                        && current
                                            .memory
                                            .federated_source_ids
                                            .iter()
                                            .all(|id| registered.iter().any(|(key, _)| key == id))
                                }) {
                                    return;
                                }
                            }
                            SettingsStore::global(cx).update_settings_file(
                                <dyn fs::Fs>::global(cx),
                                move |settings, _| {
                                    settings
                                        .kask
                                        .get_or_insert_default()
                                        .memory
                                        .get_or_insert_default()
                                        .federated_auto_inject = Some(enabled);
                                },
                            );
                        },
                    )
                    .tab_index(0),
                )
                .children(source_rows),
        )
        .into_any_element()
}
