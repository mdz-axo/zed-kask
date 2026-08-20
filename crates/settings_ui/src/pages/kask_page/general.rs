//! General sub-page — kask-wide data directory configuration.

use super::*;

pub(crate) fn render_general_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let (data_dir, tool_router, general): (
        String,
        kask_bridge::KaskToolRouterSettings,
        kask_bridge::KaskGeneralSettings,
    ) = raw
        .map(|c| {
            (
                c.data_dir.unwrap_or_default(),
                c.tool_router.map(Into::into).unwrap_or_default(),
                c.general.map(Into::into).unwrap_or_default(),
            )
        })
        .unwrap_or_default();

    let resolved_default = kask_bridge::resolve_data_dir()
        .to_string_lossy()
        .to_string();

    let data_dir_input = kask_string_input(
        "kask-general-data-dir",
        "Kask Data Directory",
        format!("Default: {resolved_default}"),
        data_dir,
        "kask",
        "data_dir",
    );
    let threshold_input = kask_string_input(
        "kask-tool-router-threshold",
        "Activation Threshold",
        "Default: 0.30",
        tool_router.threshold.to_string(),
        "tool_router",
        "threshold",
    );
    let complex_word_threshold_input = kask_string_input(
        "kask-tool-router-complex-word-threshold",
        "Complex-Word Threshold",
        "Default: 6",
        tool_router.complex_word_threshold.to_string(),
        "tool_router",
        "complex_word_threshold",
    );
    let max_concurrency_input = kask_string_input(
        "kask-general-max-concurrency",
        "Max Concurrency",
        "Default: 96",
        general.max_concurrency.to_string(),
        "general",
        "max_concurrency",
    );
    let concurrency_step_input = kask_string_input(
        "kask-general-concurrency-step",
        "Concurrency Step",
        "Default: 4",
        general.concurrency_step.to_string(),
        "general",
        "concurrency_step",
    );

    v_flex()
        .id("kask-general-page")
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
                .child(SettingsSectionHeader::new("General"))
                .child(
                    Label::new(
                        "The kask data directory is the root for all kask \
                         artifacts. It contains four class subdirectories: \
                         agents/ (per-agent files), mcp/ (MCP server \
                         databases), skills/ (user skills), and threads/ \
                         (archived chat threads). Every MCP server receives \
                         this path as HKASK_DATA_DIR so they resolve \
                         databases consistently. When empty, the runtime \
                         resolves a platform default (HKASK_DATA_DIR env \
                         var, XDG_DATA_HOME/hkask, or ~/.local/share/hkask).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Kask Data Directory"))
                .child(
                    Label::new(
                        "Root directory for all kask databases and agent state. \
                         Leave empty to use the platform default.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(data_dir_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Tool Router"))
                .child(
                    Label::new(
                        "The lazy tool router narrows the MCP tool set on complex or \
                         tool-directed requests, reducing the tool list the model must \
                         reason about. Activation threshold is the score for inclusion \
                         (0.0–1.0); complex-word threshold is the minimum word count \
                         that triggers routing. Defaults: 0.30 / 40.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(threshold_input)
                .child(complex_word_threshold_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(SettingsSectionHeader::new("Concurrency"))
                .child(
                    Label::new(
                        "Global inference concurrency — the process-wide ceiling on \
                         concurrent cloud inference provider calls. Applies to skill \
                         cascades, corpus OCR, and MCP tool calls. The limiter starts \
                         at the concurrency step and ramps up by the step on success \
                         until the maximum or a provider throttle (429/503). Providers \
                         throttle at different levels; OpenRouter scales \
                         to the ceiling. Changes require a restart to take effect.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(max_concurrency_input)
                .child(concurrency_step_input),
        )
        .into_any_element()
}
