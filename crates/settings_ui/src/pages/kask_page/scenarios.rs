//! Scenarios sub-page — scenario planning and Wardley mapping.

use super::*;

pub(crate) fn render_scenarios_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let _raw = raw_kask_settings(cx);
    // No per-server settings — the scenarios data dir is derived from the
    // global `data_dir` as `mcp/scenarios/` by `mcp_env()`. The server reads
    // it via `HKASK_SCENARIOS_DATA`.

    v_flex()
        .id("kask-scenarios-page")
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
                .child(SettingsSectionHeader::new("Scenarios"))
                .child(
                    Label::new(
                        "The scenarios server provides scenario planning and Wardley mapping. \
                         Scenario data persists to mcp/scenarios/ under the shared kask data \
                         directory (set on the General page). There are no per-server path \
                         settings — all MCP servers share the single data directory.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .into_any_element()
}
