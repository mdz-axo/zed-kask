//! Portfolio sub-page — transactions directory for the portfolio server.

use super::*;

pub(crate) fn render_portfolio_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let portfolio: kask_bridge::KaskPortfolioSettings = raw
        .and_then(|c| c.portfolio)
        .map(Into::into)
        .unwrap_or_default();
    let transactions_dir = portfolio.transactions_dir;

    let transactions_dir_input = kask_string_input(
        "kask-portfolio-transactions-dir",
        "Transactions Directory",
        "transactions",
        transactions_dir,
        "portfolio",
        "transactions_dir",
    );

    v_flex()
        .id("kask-portfolio-page")
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
                .child(SettingsSectionHeader::new("Portfolio"))
                .child(
                    Label::new(
                        "The portfolio server is a general-purpose transaction-ledger portfolio \
                         store (stocks, prediction-event portfolios, CMP indices) with \
                         materialized daily holdings and returns views. Configure the \
                         transactions directory the dashboard auto-loads from.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Transactions Directory"))
                .child(
                    Label::new(
                        "Directory for portfolio transaction files (CSV/JSON). The portfolio \
                         dashboard auto-loads any new files from this directory. Leave empty \
                         for the default (<kask_data_dir>/mcp/portfolio/transactions/). Or set \
                         HKASK_TRANSACTIONS_DIR.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(transactions_dir_input),
        )
        .into_any_element()
}
