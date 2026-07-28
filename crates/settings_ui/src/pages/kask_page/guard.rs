//! Guard / Regulation sub-page — `direct_chat_strategy` configuration.

use super::*;

pub(crate) fn render_guard_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let guard: kask_bridge::KaskGuardSettings = raw
        .and_then(|c| c.guard)
        .map(Into::into)
        .unwrap_or_default();
    let strategy = guard.direct_chat_strategy;

    let strategy_input = SettingsInputField::new("kask-guard-direct-chat-strategy")
        .tab_index(0)
        .with_initial_text(strategy)
        .with_placeholder("cascade_only")
        .aria_label("Direct-Chat Strategy")
        .confirm_on_focus_out()
        .on_confirm(move |value, _window, cx| {
            if let Some(text) = value {
                let trimmed = text.trim().to_string();
                if matches!(trimmed.as_str(), "buffer" | "incremental" | "cascade_only") {
                    SettingsStore::global(cx).update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        move |settings, _| {
                            settings
                                .kask
                                .get_or_insert_default()
                                .guard
                                .get_or_insert_default()
                                .direct_chat_strategy = Some(trimmed);
                        },
                    );
                }
            }
        });

    v_flex()
        .id("kask-guard-page")
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
                .child(SettingsSectionHeader::new("Guard / Regulation"))
                .child(
                    Label::new(
                        "The guard layer wraps the inference path. The direct-chat \
                         strategy controls how guard output is streamed: \"buffer\" \
                         (wait for full guard output), \"incremental\" (stream guard \
                         and inference interleaved), or \"cascade_only\" (no guard on \
                         direct chat — only the Curator cascade runs).",
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Direct-Chat Strategy"))
                .child(
                    Label::new("One of: \"buffer\", \"incremental\", or \"cascade_only\".")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(strategy_input),
        )
        .into_any_element()
}
