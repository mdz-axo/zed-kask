//! Media sub-page — research transcription and visual analysis models.
//!
//! Reads from `kask_bridge::KaskMediaSettings` (the `"kask.media"` section
//! in settings.json) and writes via `kask_string_input` — the same pattern
//! as every other kask settings sub-page. The media MCP server reads these
//! as env vars (`HKASK_MEDIA_STT_MODEL`, `HKASK_MEDIA_VISION_MODEL`) emitted by
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

    let stt_input = kask_string_input(
        "kask-media-stt",
        "STT Model",
        "Provider/model-id (clear to disable transcription)",
        media.stt_model,
        "media",
        "stt_model",
    );
    let vision_input = kask_string_input(
        "kask-media-vision",
        "Vision Model",
        "Provider/model-id (clear to disable visual analysis)",
        media.vision_model,
        "media",
        "vision_model",
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
                "Curate existing research evidence: ingest recordings and images, \
                 organize assets, annotate observations, and extract source-linked excerpts. \
                 Configure speech-to-text and vision models below. Settings provide defaults; \
                 explicitly clearing a model disables that pipeline with a visible error. \
                 Open the media panel from View > Media or the status bar button.",
            )
            .color(Color::Muted)
            .size(LabelSize::Small),
        )
        .child(div().mt_4())
        .child(v_flex().gap_3().child(stt_input).child(vision_input))
        .into_any_element()
}
