//! GPUI media widget for viewing images, video, and audio from the hkask
//! media storage (gallery, generated assets, ffmpeg outputs).
//!
//! All code lives on the kask side. The upstream bridge is a single
//! `media_block_renderer` callback wired at the D18 seam
//! (`crates/agent_ui/src/conversation_view.rs::render_agent_markdown`).
//!
//! ## Architecture
//!
//! ```text
//!  ```media { "kind": "video", "src": "/path/to/clip.mp4" } ```
//!                          │
//!                          ▼
//!          D18 seam: MarkdownElement.media_block_renderer
//!                          │
//!                          ▼
//!              hkask_media_widget::media_block_renderer()
//!                          │
//!                          ▼
//!                    MediaWidget view
//!                   ┌───────┴───────┐
//!              Image/Svg        Audio        Video
//!              │  gpui::img()   │  rodio     │  ffmpeg → RGBA → RenderImage
//!              │                │  transport │  transport controls
//! ```

pub mod audio_player;
pub mod media_ref;
pub mod media_widget;
pub mod transport;
pub mod video_decoder;

pub use media_ref::{MediaKind, MediaRef, MediaStorage, ResolvedMedia};
pub use media_widget::MediaWidget;

use gpui::{AnyElement, App, SharedString, Window};

/// The callback type registered at the D18 seam.
///
/// Called with the body text of a fenced code block. If the body is a
/// valid media reference (JSON with `kind` and `src`), returns `Some(element)`
/// to render the media widget; otherwise returns `None` to fall through to
/// the default code block renderer.
pub type MediaBlockRenderer = Box<dyn Fn(&str, &mut Window, &App) -> Option<AnyElement>>;

/// Create the media block renderer callback for the D18 seam.
///
/// Usage in `render_agent_markdown`:
///
/// ```ignore
/// MarkdownElement::new(markdown, style)
///     .media_block_renderer(hkask_media_widget::media_block_renderer())
/// ```
pub fn media_block_renderer() -> MediaBlockRenderer {
    Box::new(|body, window, cx| {
        // Only intercept blocks whose body looks like a media JSON reference.
        // The upstream code block renderer already handles the fenced language
        // — we check the body for a JSON object with a "kind" field.
        if !body.trim_start().starts_with('{') {
            return None;
        }
        match parse_media_block_body(body) {
            Ok(media_ref) => Some(render_media_ref(media_ref, window, cx)),
            Err(error) => {
                log::warn!(
                    "hkask-media-widget: failed to parse media block: {error}. Body: {body}"
                );
                None
            }
        }
    })
}

/// Render a `MediaRef` as a GPUI `AnyElement`.
fn render_media_ref(reference: MediaRef, _window: &mut Window, cx: &App) -> AnyElement {
    media_widget::render_media_ref(reference, cx)
}

/// Parse the JSON body of a ```` ```media ```` block.
///
/// Expected format:
/// ```json
/// { "kind": "video", "src": "/path/to/clip.mp4" }
/// { "kind": "audio", "src": "data:audio/wav;base64,..." }
/// { "kind": "image", "src": "/path/to/image.png" }
/// { "kind": "svg", "src": "/path/to/diagram.svg" }
/// ```
fn parse_media_block_body(body: &str) -> anyhow::Result<MediaRef> {
    let value: serde_json::Value = serde_json::from_str(body.trim())?;
    let kind = value
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("image");
    let src = value
        .get("src")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow::anyhow!("media block missing 'src' field"))?;

    let media_kind = match kind {
        "image" | "img" => MediaKind::Image,
        "svg" => MediaKind::Svg,
        "audio" => MediaKind::Audio,
        "video" => MediaKind::Video,
        other => {
            return Err(anyhow::anyhow!(
                "unknown media kind '{other}' — expected image, svg, audio, or video"
            ));
        }
    };

    Ok(MediaRef::new(SharedString::from(src), media_kind))
}
