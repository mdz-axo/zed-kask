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
//!              hkask_viz_core::block_renderer() → create_media_widget
//!                          │
//!                          ▼
//!                    MediaWidget view
//!                   ┌───────┴───────┐
//!              Image/Svg        Audio        Video
//!              │  gpui::img()   │  rodio     │  ffmpeg → RGBA → RenderImage
//!              │                │  transport │  transport controls
//! ```
#![warn(clippy::let_underscore_future)]

pub mod audio_player;
pub mod media_ref;
pub mod media_widget;
pub mod simple_slider;
pub mod transport;
pub mod video_decoder;

pub use media_ref::{MediaBlockBody, MediaKind, MediaRef, MediaStorage, ResolvedMedia};
pub use media_widget::MediaWidget;

use gpui::{App, AppContext, Entity, Window};

/// Create a `MediaWidget` entity from a block body, without wrapping it in an
/// element. Used by `hkask_viz_core::block_renderer` to cache the entity across
/// renders (so audio/video playback and widget state survive re-renders).
///
/// Returns `None` if the body is not a valid media block (non-JSON, missing
/// `src`, unknown kind). The caller falls through to the next renderer or the
/// default code-block renderer.
pub fn create_media_widget(
    body: &str,
    _window: &mut Window,
    cx: &mut App,
) -> Option<Entity<MediaWidget>> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    match MediaBlockBody::parse(body) {
        Ok(block_body) => {
            let media_ref = block_body.to_media_ref().map_err(|error| {
                log::warn!(
                    "hkask-media-widget: failed to resolve media ref from block: {error}. Body: {body}"
                );
                error
            }).ok()?;
            Some(cx.new(|cx| {
                let mut widget = MediaWidget::new_with_block(media_ref, block_body, cx);
                widget.load(cx);
                widget
            }))
        }
        Err(error) => {
            log::warn!("hkask-media-widget: failed to parse media block: {error}. Body: {body}");
            None
        }
    }
}
