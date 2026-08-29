//! Media sub-page — media server model configuration (TTS, STT, Vision,
//! Image Gen, Video models).
//!
//! Reads from `kask_bridge::KaskMediaSettings` (the `"kask.media"` section
//! in settings.json) and writes via `kask_string_input` — the same pattern
//! as every other kask settings sub-page. The media MCP server reads these
//! as env vars (`HKASK_MEDIA_TTS_MODEL` etc.) emitted by
//! `mcp_env::emit_media_env` at server launch.

use super::*;

pub(crate) fn render_media_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let raw = raw_kask_settings(cx);
    let media: kask_bridge::KaskMediaSettings = raw
        .and_then(|c| c.media)
        .map(Into::into)
        .unwrap_or_default();

    let tts_input = kask_string_input(
        "kask-media-tts",
        "TTS Model",
        kask_bridge::DEFAULT_TTS_MODEL,
        media.tts_model,
        "media",
        "tts_model",
    );
    let stt_input = kask_string_input(
        "kask-media-stt",
        "STT Model",
        kask_bridge::DEFAULT_STT_MODEL,
        media.stt_model,
        "media",
        "stt_model",
    );
    let vision_input = kask_string_input(
        "kask-media-vision",
        "Vision Model",
        kask_bridge::DEFAULT_VISION_MODEL,
        media.vision_model,
        "media",
        "vision_model",
    );
    let image_gen_input = kask_string_input(
        "kask-media-image-gen",
        "Image Generation Model",
        kask_bridge::DEFAULT_IMAGE_GEN_MODEL,
        media.image_gen_model,
        "media",
        "image_gen_model",
    );
    let video_input = kask_string_input(
        "kask-media-video",
        "Video Generation Model",
        kask_bridge::DEFAULT_VIDEO_MODEL,
        media.video_model,
        "media",
        "video_model",
    );

    v_flex()
        .id("kask-media-page")
        .size_full()
        .pt_2p5()
        .px_8()
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .child(Headline::new("Media Server").size(HeadlineSize::XLarge))
        .child(
            Label::new(
                "Configure the media MCP server: model overrides for TTS, \
                 speech-to-text, vision, image generation, and video generation. \
                 When empty, the server falls back to the kask default models. \
                 The media panel (View > Media or the status bar button) provides \
                 a Steer-mode conversation scoped to the media MCP server.",
            )
            .color(Color::Muted)
            .size(LabelSize::Small),
        )
        .child(div().mt_4())
        .child(
            v_flex()
                .gap_3()
                .child(tts_input)
                .child(stt_input)
                .child(vision_input)
                .child(image_gen_input)
                .child(video_input),
        )
        .into_any_element()
}
