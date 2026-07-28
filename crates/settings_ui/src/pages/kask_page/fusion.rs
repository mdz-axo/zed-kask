//! Fusion sub-page — multi-model fusion inference configuration: judge,
//! panel, deliberation mode, algo method, skill anchors, OpenRouter
//! auto-discovery thresholds, and Codette-inspired enhancements.

use super::*;

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

pub(crate) fn render_fusion_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let fusion: kask_bridge::KaskFusionSettings = raw
        .and_then(|c| c.fusion)
        .map(Into::into)
        .unwrap_or_default();
    let enabled = fusion.enabled;
    let judge_model = fusion.judge_model;
    let panel_models = fusion.panel_models;
    let mode = fusion.mode;
    let algo_method = fusion.algo_method;
    let skills = fusion.skills;
    let max_rounds = fusion.max_rounds.to_string();
    let openrouter_max_price = fusion.openrouter_max_price.to_string();
    let openrouter_min_intelligence = fusion.openrouter_min_intelligence.to_string();
    let coherence_threshold = fusion
        .coherence_threshold
        .map(|v| format!("{v}"))
        .unwrap_or_default();
    let panel_sizing_enabled = fusion.panel_sizing_enabled;
    let pressure_adaptive_enabled = fusion.pressure_adaptive_enabled;

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
