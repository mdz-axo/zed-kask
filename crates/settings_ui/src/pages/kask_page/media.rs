//! Media sub-page — media server model configuration (TTS, STT, Vision,
//! Image Gen models) and gallery storage path.
//!
//! The media server's configuration is env-var-based (not in
//! `kask.settings.json`). The models are resolved at server startup via
//! `hkask_mcp_media::models::resolve(env_key, default)`. This page shows
//! the current resolved values and the env-var names the operator can set
//! to override them, plus the gallery DB path.

use super::*;

pub(crate) fn render_media_page(
    _settings_window: &SettingsWindow,
    scroll_handle: &ScrollHandle,
    _window: &mut Window,
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    // Resolve the current model values the same way the media server does.
    let tts_model = hkask_mcp_media::models::tts_model();
    let stt_model = hkask_mcp_media::models::stt_model();
    let vision_model = hkask_mcp_media::models::vision_model();
    let image_gen_model = hkask_mcp_media::models::image_gen_model();

    let tts_input = kask_string_input(
        "kask-media-tts",
        "TTS Model",
        hkask_mcp_media::models::TTS_DEFAULT,
        tts_model,
        "media",
        "tts_model",
    );
    let stt_input = kask_string_input(
        "kask-media-stt",
        "STT Model",
        hkask_mcp_media::models::STT_DEFAULT,
        stt_model,
        "media",
        "stt_model",
    );
    let vision_input = kask_string_input(
        "kask-media-vision",
        "Vision Model",
        hkask_mcp_media::models::VISION_DEFAULT,
        vision_model,
        "media",
        "vision_model",
    );
    let image_gen_input = kask_string_input(
        "kask-media-image-gen",
        "Image Generation Model",
        hkask_mcp_media::models::IMAGE_GEN_DEFAULT,
        image_gen_model,
        "media",
        "image_gen_model",
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
                 speech-to-text, vision, and image generation. Models are \
                 resolved at server startup from env vars — set them in your \
                 environment or via the kask MCP server env configuration.",
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
                .child(image_gen_input),
        )
        .child(div().mt_4())
        .child(
            Label::new(
                "Env vars: HKASK_MEDIA_TTS_MODEL, HKASK_MEDIA_STT_MODEL, \
                 HKASK_MEDIA_VISION_MODEL, HKASK_MEDIA_IMAGE_GEN_MODEL, \
                 HKASK_MEDIA_VIDEO_MODEL, HKASK_MEDIA_DB. The gallery DB \
                 lives at mcp/media/gallery.db under the kask data dir. \
                 The media panel (View > Media or the status bar button) \
                 provides a Steer-mode conversation scoped to the media \
                 MCP server.",
            )
            .color(Color::Muted)
            .size(LabelSize::XSmall),
        )
        .into_any_element()
}
