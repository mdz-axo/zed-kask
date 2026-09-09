//! Research sub-page — research database path (feed substrate + run ledger).

use super::*;

pub(crate) fn render_research_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let research: kask_bridge::KaskResearchSettings = raw
        .and_then(|c| c.research)
        .map(Into::into)
        .unwrap_or_default();
    let research_db = research.research_db;

    let research_db_input = kask_string_input(
        "kask-research-research-db",
        "Research Database Path",
        "(server default: <data-dir>/mcp/research/research.db)",
        research_db,
        "research",
        "research_db",
    );

    v_flex()
        .id("kask-research-page")
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
                .child(SettingsSectionHeader::new("Research"))
                .child(
                    Label::new(
                        "The research server provides web search, extraction, and RSS feed \
                         management. Configure the research database path for the feed \
                         substrate and the research-run ledger. When empty, the server \
                         defaults to <data-dir>/mcp/research/research.db (databases stay in \
                         the internal data dir; artifact files go to \
                         ~/Documents/zk-data/).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Research Database Path"))
                .child(
                    Label::new(
                        "SQLite database path for the feed substrate and research-run \
                         ledger. Leave empty to use the server default \
                         (<data-dir>/mcp/research/research.db).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(research_db_input),
        )
        .into_any_element()
}
