//! Prediction-markets sub-page — calibration data directory, cache TTL,
//! base-event registry for the prediction-markets data service.

use super::*;

pub(crate) fn render_prediction_markets_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let prediction_markets: kask_bridge::KaskPredictionMarketsSettings = raw
        .and_then(|c| c.prediction_markets)
        .map(Into::into)
        .unwrap_or_default();

    let data_dir_input = kask_string_input(
        "kask-prediction-markets-data-dir",
        "Data Directory",
        "(in-memory)",
        prediction_markets.data_dir,
        "prediction_markets",
        "data_dir",
    );
    let cache_ttl_input = kask_string_input(
        "kask-prediction-markets-cache-ttl",
        "Cache TTL (seconds)",
        "60",
        if prediction_markets.cache_ttl_secs > 0 {
            prediction_markets.cache_ttl_secs.to_string()
        } else {
            String::new()
        },
        "prediction_markets",
        "cache_ttl_secs",
    );
    let base_events_input = kask_string_input(
        "kask-prediction-markets-base-events",
        "Base Events",
        "economics:KXFEDDECISION,politics:KXPREZ-28",
        prediction_markets.base_events,
        "prediction_markets",
        "base_events",
    );

    v_flex()
        .id("kask-prediction-markets-page")
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
                .child(SettingsSectionHeader::new("Prediction Markets"))
                .child(
                    Label::new(
                        "The prediction-markets server is a read-only data service for \
                         Polymarket and Kalshi market-implied probabilities, with calibration, \
                         volatility, and constant-maturity-prediction analytics. No trading \
                         credentials are used or accepted.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Calibration Data Directory"))
                .child(
                    Label::new(
                        "Directory for the calibration journal (resolved-market outcomes). \
                         Leave empty for in-memory.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(data_dir_input),
        )
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Cache TTL (seconds)"))
                .child(
                    Label::new("Market-data response cache lifetime. Empty = server default (60).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(cache_ttl_input),
        )
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Base-Event Registry"))
                .child(
                    Label::new(
                        "Benchmark events for constant-maturity predictions, as \
                         \"domain:series\" pairs separated by commas. Only registered series \
                         can serve as CMP base events.",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .child(base_events_input),
        )
        .into_any_element()
}
