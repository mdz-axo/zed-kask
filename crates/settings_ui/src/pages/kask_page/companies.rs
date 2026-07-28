//! Companies sub-page — superforecasting staleness and Fermi defaults.

use super::*;

pub(crate) fn render_companies_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let companies: kask_bridge::KaskCompaniesSettings = raw
        .and_then(|c| c.companies)
        .map(Into::into)
        .unwrap_or_default();
    let staleness_days = companies.chronic_staleness_days.to_string();
    let fermi_defaults = companies.fermi_defaults;

    let staleness_input = kask_string_input(
        "kask-companies-staleness-days",
        "Chronic Staleness Days",
        "0",
        staleness_days,
        "companies",
        "chronic_staleness_days",
    );
    let fermi_input = kask_string_input(
        "kask-companies-fermi-defaults",
        "Fermi Defaults (JSON)",
        "{\"growth\": [...], \"margin\": [...]}",
        fermi_defaults,
        "companies",
        "fermi_defaults",
    );

    v_flex()
        .id("kask-companies-page")
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
                .child(SettingsSectionHeader::new("Companies"))
                .child(
                    Label::new(
                        "The companies server provides company research and filings. \
                         Configure superforecasting parameters."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Chronic Staleness Days"))
                .child(
                    Label::new("Staleness threshold in days for the superforecasting learning state. 0 uses the default.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(staleness_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Fermi Defaults"))
                .child(
                    Label::new("JSON with growth and margin question arrays for Fermi decomposition. Leave empty for hardcoded defaults.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(fermi_input),
        )
        .into_any_element()
}
