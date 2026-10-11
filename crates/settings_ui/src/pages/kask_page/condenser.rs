//! Condenser sub-page — compression profile and auto-compress tool
//! results.

use super::*;

pub(crate) fn render_condenser_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let condenser: kask_bridge::KaskCondenserSettings = raw
        .and_then(|c| c.condenser)
        .map(Into::into)
        .unwrap_or_default();
    let profile = condenser.profile.as_str();
    let auto_compress = condenser.auto_compress_tool_results;

    let profile_input = kask_input_field(
        "kask-condenser-profile",
        0,
        profile.to_string(),
        "normal",
        "Compression Profile",
        |text| Some(text.trim().to_string()),
        move |parsed, settings| {
            settings
                .kask
                .get_or_insert_default()
                .condenser
                .get_or_insert_default()
                .profile = Some(parsed);
        },
    );

    v_flex()
        .id("kask-condenser-page")
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
                .child(SettingsSectionHeader::new("Condenser"))
                .child(
                    Label::new(
                        "The condenser compresses tool output and manages context \
                         in inference threads. Configure the compression profile \
                         and automatic tool-result compression."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Compression Profile"))
                .child(
                    Label::new("Profile: heavy (10% retention), normal (20%), soft (60%), or light (95%).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(profile_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Auto-Compress Tool Results"))
                .child(
                    Label::new("Whether to automatically compress tool results before they enter the message history.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    kask_toggle(
                        "kask-condenser-auto-compress",
                        Some("Auto-Compress Tool Results"),
                        Some(
                            "Whether to automatically compress tool results before they enter the message history.".into(),
                        ),
                        auto_compress,
                        0,
                        move |value, settings| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .condenser
                                .get_or_insert_default()
                                .auto_compress_tool_results = Some(value);
                        },
                    ),
                ),
        )
        .into_any_element()
}
