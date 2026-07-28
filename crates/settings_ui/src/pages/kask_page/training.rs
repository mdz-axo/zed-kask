//! Training sub-page — host selection and cache directory.

use super::*;

pub(crate) fn render_training_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    // Resolve via `From` so the UI shows the same defaults the runtime uses.
    let training: kask_bridge::KaskTrainingSettings = raw
        .and_then(|c| c.training)
        .map(Into::into)
        .unwrap_or_default();
    let host = training.host;
    let cache_dir = training.cache_dir;

    let host_input = kask_string_input(
        "kask-training-host",
        "Training Host",
        "deepinfra | nebius | runpod",
        host,
        "training",
        "host",
    );
    let cache_dir_input = kask_string_input(
        "kask-training-cache-dir",
        "Cache Directory",
        "(agent adapters dir)",
        cache_dir,
        "training",
        "cache_dir",
    );

    v_flex()
        .id("kask-training-page")
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
                .child(SettingsSectionHeader::new("Training"))
                .child(
                    Label::new(
                        "The training server provides LoRA training configuration and audit. \
                         Configure host selection and cache directory."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Training Host"))
                .child(
                    Label::new("Host override: deepinfra, nebius, or runpod. Leave empty for auto-detect from API keys.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(host_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Cache Directory"))
                .child(
                    Label::new("Cache directory for dataset pipeline. Leave empty for the agent adapters directory.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(cache_dir_input),
        )
        .into_any_element()
}
