//! General sub-page — kask-wide data directory configuration.

use super::*;

pub(crate) fn render_general_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let data_dir: String = raw.and_then(|c| c.data_dir).unwrap_or_default();

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
                        "The kask data directory is the root for all kask databases, \
                         agent state, and file-based stores. Every MCP server receives \
                         this path as HKASK_DATA_DIR so they resolve databases \
                         consistently. When empty, the runtime resolves a platform \
                         default (HKASK_DATA_DIR env var, XDG_DATA_HOME/hkask, or \
                         ~/.local/share/hkask).",
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
        .into_any_element()
}
