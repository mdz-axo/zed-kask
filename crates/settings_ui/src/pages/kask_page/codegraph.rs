//! Codegraph sub-page — database path for persistent code graph storage.

use super::*;

pub(crate) fn render_codegraph_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let codegraph: kask_bridge::KaskCodegraphSettings = raw
        .and_then(|c| c.codegraph)
        .map(Into::into)
        .unwrap_or_default();
    let db_path = codegraph.db_path;

    let db_input = kask_string_input(
        "kask-codegraph-db-path",
        "Database Path",
        "(in-memory)",
        db_path,
        "codegraph",
        "db_path",
    );

    v_flex()
        .id("kask-codegraph-page")
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
                .child(SettingsSectionHeader::new("Codegraph"))
                .child(
                    Label::new(
                        "The codegraph server provides code structure query and traversal. \
                         Configure the database path for persistent graph storage."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Database Path"))
                .child(
                    Label::new("SQLite database path for persistent code graph storage. Leave empty for in-memory.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(db_input),
        )
        .into_any_element()
}
