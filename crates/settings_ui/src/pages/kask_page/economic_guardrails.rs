//! Economic Guardrails sub-page — OpenRouter output-price de-listing filter.
//!
//! This page reads/writes `language_models.open_router.max_output_price_per_million_tokens`,
//! a Zed upstream setting (not `kask.*`). It is surfaced under Settings → Kask because
//! it functions as an economic guardrail: models fetched from OpenRouter whose reported
//! output price exceeds the threshold are silently removed from the model picker,
//! protecting users from accidentally selecting models that charge tens or hundreds
//! of dollars per million output tokens.

use super::*;

/// Read the raw `max_output_price_per_million_tokens` value from the user
/// settings file. Returns `None` when the path is unset (filter disabled).
fn raw_max_output_price(cx: &App) -> Option<f64> {
    SettingsStore::global(cx)
        .raw_user_settings()
        .and_then(|user| user.content.language_models.clone())
        .and_then(|lm| lm.open_router)
        .and_then(|or| or.max_output_price_per_million_tokens)
}

pub(crate) fn render_economic_guardrails_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let current = raw_max_output_price(cx);
    // Default threshold used when the user enables the filter but hasn't set a
    // value yet. Matches the docs example in `docs/src/ai/use-a-gateway.md`.
    const DEFAULT_THRESHOLD: f64 = 5.0;
    let enabled = current.is_some();
    let threshold_text = current
        .map(|v| v.to_string())
        .unwrap_or_else(|| DEFAULT_THRESHOLD.to_string());

    let enable_toggle = SwitchField::new(
        "kask-economic-guardrails-enabled",
        Some("De-list Expensive Models"),
        Some(
            "When enabled, OpenRouter models whose reported output price exceeds the \
             threshold are removed from the model picker. Models with no reported \
             price (e.g. openrouter/auto) and models you explicitly list under \
             available_models are always kept."
                .into(),
        ),
        if enabled {
            ToggleState::Selected
        } else {
            ToggleState::Unselected
        },
        move |state, _window, cx| {
            let new_enabled = *state == ToggleState::Selected;
            SettingsStore::global(cx).update_settings_file(
                <dyn fs::Fs>::global(cx),
                move |settings, _| {
                    let open_router = settings
                        .language_models
                        .get_or_insert_default()
                        .open_router
                        .get_or_insert_default();
                    if new_enabled {
                        // Preserve an existing threshold if present; otherwise use the default.
                        if open_router.max_output_price_per_million_tokens.is_none() {
                            open_router.max_output_price_per_million_tokens =
                                Some(DEFAULT_THRESHOLD);
                        }
                    } else {
                        open_router.max_output_price_per_million_tokens = None;
                    }
                },
            );
        },
    )
    .tab_index(0);

    let threshold_input = SettingsInputField::new("kask-economic-guardrails-threshold")
        .tab_index(0)
        .with_initial_text(threshold_text)
        .with_placeholder("5.0")
        .aria_label("Max Output Price (USD per Million Tokens)")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            // Only write when the filter is enabled; editing the threshold while
            // the toggle is off does not silently enable the filter. The toggle
            // controls enable/disable; this field controls the value.
            if !enabled {
                return;
            }
            if let Some(text) = value {
                let trimmed = text.trim();
                // Empty input disables the filter.
                if trimmed.is_empty() {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .language_models
                                .get_or_insert_default()
                                .open_router
                                .get_or_insert_default()
                                .max_output_price_per_million_tokens = None;
                        },
                    );
                    return;
                }
                if let Ok(parsed) = trimmed.parse::<f64>() {
                    if parsed.is_finite() && parsed >= 0.0 {
                        SettingsStore::global(cx).update_settings_file(
                            <dyn fs::Fs>::global(cx),
                            move |settings, _| {
                                settings
                                    .language_models
                                    .get_or_insert_default()
                                    .open_router
                                    .get_or_insert_default()
                                    .max_output_price_per_million_tokens = Some(parsed);
                            },
                        );
                    }
                }
            }
        });

    v_flex()
        .id("kask-economic-guardrails-page")
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
                .child(SettingsSectionHeader::new("Economic Guardrails"))
                .child(
                    Label::new(
                        "Cost-protection settings for inference. The OpenRouter output-price \
                         filter de-lists models whose reported output price exceeds a threshold, \
                         preventing accidental selection of expensive models. The filter runs \
                         every time models are fetched from OpenRouter (at startup and on \
                         settings change).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(enable_toggle)
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Max Output Price (USD per Million Tokens)"))
                .child(
                    Label::new(
                        "Models costing more than this per million output tokens are hidden \
                         from the picker. Set to 0 to hide every priced model. Clear the field \
                         to disable the filter. Example: 5.0 hides models above $5/M output tokens.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(threshold_input),
        )
        .into_any_element()
}
