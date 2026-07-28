//! Media sub-page — TTS, STT, vision, and image generation model overrides.

use super::*;

pub(crate) fn render_media_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let media = raw.and_then(|c| c.media).unwrap_or_default();
    let tts_model = media.tts_model.unwrap_or_default();
    let stt_model = media.stt_model.unwrap_or_default();
    let vision_model = media.vision_model.unwrap_or_default();
    let image_gen_model = media.image_gen_model.unwrap_or_default();

    let tts_input = kask_string_input(
        "kask-media-tts-model",
        "TTS Model",
        "fal.ai/Qwen3-TTS",
        tts_model,
        "media",
        "tts_model",
    );
    let stt_input = kask_string_input(
        "kask-media-stt-model",
        "STT Model",
        "fal.ai/wizper",
        stt_model,
        "media",
        "stt_model",
    );
    let vision_input = kask_string_input(
        "kask-media-vision-model",
        "Vision Model",
        "KiloCode/Qwen/Qwen3-VL-235B-A22B-Instruct",
        vision_model,
        "media",
        "vision_model",
    );
    let image_gen_input = kask_string_input(
        "kask-media-image-gen-model",
        "Image Generation Model",
        "fal.ai/flux-2",
        image_gen_model,
        "media",
        "image_gen_model",
    );

    v_flex()
        .id("kask-media-page")
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
                .child(SettingsSectionHeader::new("Media"))
                .child(
                    Label::new(
                        "The media server provides image generation, OCR, TTS, and STT. \
                         Configure model overrides for each capability."
                    )
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("TTS Model"))
                .child(
                    Label::new("Text-to-speech model override. Leave empty for default (FA/qwen-3-tts).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(tts_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("STT Model"))
                .child(
                    Label::new("Speech-to-text model override. Leave empty for default (FA/wizper).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(stt_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Vision Model"))
                .child(
                    Label::new("Vision model override for OCR and image analysis. Leave empty for default.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(vision_input),
        )
        .child(Divider::horizontal())
        .child(
            v_flex()
                .gap_1()
                .child(Label::new("Image Generation Model"))
                .child(
                    Label::new("Image generation model override. Leave empty for default (FA/flux-2).")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(image_gen_input),
        )
        .into_any_element()
}
