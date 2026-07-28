//! Scenarios sub-page — data directory for scenario persistence.

use super::*;

pub(crate) fn render_scenarios_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let scenarios = raw.and_then(|c| c.scenarios).unwrap_or_default();
    let data_dir = scenarios.data_dir.unwrap_or_default();

    let data_dir_input = kask_string_input(
        "kask-scenarios-data-dir",
        "Data Directory",
        "(in-memory)",
        data_dir,
        "scenarios",
        "data_dir",
    );

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
                         Configure the data directory for scenario persistence.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Data Directory"))
                .child(
                    Label::new(
                        "Directory for scenario data persistence. Leave empty for in-memory.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(data_dir_input),
        )
        .into_any_element()
}
