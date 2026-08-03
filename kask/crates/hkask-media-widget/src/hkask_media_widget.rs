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

pub use media_ref::{GalleryMediaStorage, MediaKind, MediaRef, MediaStorage, ResolvedMedia};
pub use media_widget::MediaWidget;

use std::sync::Once;

use gpui::{AnyElement, App, AppContext, Entity, SharedString, Window};
use gpui_component::Theme;
use theme::ActiveTheme as _;

static THEME_INIT: Once = Once::new();

/// The callback type registered at the D18 seam.
///
/// Called with the body text of a fenced code block. If the body is a
/// valid media reference (JSON with `kind` and `src`), returns `Some(element)`
/// to render the media widget; otherwise returns `None` to fall through to
/// the default code block renderer.
pub type MediaBlockRenderer = Box<dyn Fn(&str, &mut Window, &mut App) -> Option<AnyElement>>;

/// Ensure the gpui-component theme is initialized, synced with the
/// window appearance, and populated with color values from the active Zed
/// theme. The system appearance sync runs at most once per process; the color
/// mapping runs on every call so Zed theme changes are picked up.
fn ensure_theme_initialized(window: &mut Window, cx: &mut App) {
    THEME_INIT.call_once(|| {
        Theme::sync_system_appearance(Some(window), cx);
    });
    sync_theme_colors(cx);
}

/// Map Zed theme colors into the gpui-component `Theme` global so that
/// gpui-component widgets (Slider, Button, etc.) render with Zed's active
/// theme colors instead of gpui-component defaults.
///
/// This is a one-way adapter: Zed → gpui-component. It runs on every
/// `media_block_renderer` invocation, which fires during markdown rendering
/// — theme changes trigger re-renders, so colors stay in sync.
fn sync_theme_colors(cx: &mut App) {
    let zed_colors = cx.theme().colors();
    let zed_status = cx.theme().status();
    let gpui_theme = cx.global_mut::<Theme>();
    let colors = &mut gpui_theme.colors;

    colors.foreground = zed_colors.text;
    colors.muted_foreground = zed_colors.text_muted;
    colors.background = zed_colors.background;
    colors.panel_background = zed_colors.panel_background;
    colors.border = zed_colors.border;
    colors.input = zed_colors.border;
    colors.primary = zed_colors.text_accent;
    colors.primary_foreground = zed_colors.text;
    colors.primary_hover = zed_colors.text_accent;
    colors.primary_active = zed_colors.text_accent;
    colors.accent = zed_colors.text_accent;
    colors.accent_foreground = zed_colors.text;
    colors.secondary = zed_colors.element_background;
    colors.secondary_hover = zed_colors.element_hover;
    colors.secondary_active = zed_colors.element_active;
    colors.secondary_foreground = zed_colors.text;
    colors.danger = zed_status.error;
    colors.danger_foreground = zed_colors.text;
    colors.danger_hover = zed_status.error;
    colors.warning = zed_status.warning;
    colors.warning_foreground = zed_colors.text;
    colors.warning_hover = zed_status.warning;
    colors.success = zed_status.success;
    colors.success_foreground = zed_colors.text;
    colors.info = zed_status.info;
    colors.info_foreground = zed_colors.text;
    colors.title_bar = zed_colors.title_bar_background;
    colors.title_bar_border = zed_colors.border;
    colors.status_bar = zed_colors.status_bar_background;
    colors.status_bar_border = zed_colors.border;
    colors.tab = zed_colors.tab_inactive_background;
    colors.tab_active = zed_colors.tab_active_background;
    colors.tab_bar = zed_colors.tab_bar_background;
    colors.scrollbar = zed_colors.scrollbar_track_background;
    colors.scrollbar_thumb = zed_colors.scrollbar_thumb_background;
    colors.scrollbar_thumb_hover = zed_colors.scrollbar_thumb_hover_background;
    colors.slider_bar = zed_colors.scrollbar_track_background;
    colors.slider_thumb = zed_colors.scrollbar_thumb_background;
    colors.ring = zed_colors.text_accent;
    colors.selection = zed_colors.element_selection_background;
    colors.list = zed_colors.surface_background;
    colors.list_hover = zed_colors.element_hover;
    colors.list_active = zed_colors.element_active;
    colors.popover = zed_colors.elevated_surface_background;
    colors.popover_foreground = zed_colors.text;
    colors.overlay = zed_colors.elevated_surface_background;
    colors.caret = zed_colors.text_accent;
    colors.link = zed_colors.text_accent;
    colors.link_hover = zed_colors.text_accent;
    colors.link_active = zed_colors.text_accent;
    colors.muted = zed_colors.surface_background;
    colors.skeleton = zed_colors.element_background;
    colors.switch = zed_colors.element_background;
    colors.switch_thumb = zed_colors.text_accent;
    colors.progress_bar = zed_colors.scrollbar_track_background;
    colors.drag_border = zed_colors.border_focused;
    colors.drop_target = zed_colors.drop_target_background;
    colors.button = zed_colors.element_background;
    colors.button_hover = zed_colors.element_hover;
    colors.button_active = zed_colors.element_active;
    colors.button_foreground = zed_colors.text;
    colors.button_primary = zed_colors.text_accent;
    colors.button_primary_foreground = zed_colors.background;
    colors.button_primary_hover = zed_colors.text_accent;
    colors.button_primary_active = zed_colors.text_accent;
    colors.button_secondary = zed_colors.ghost_element_background;
    colors.button_secondary_foreground = zed_colors.text;
    colors.button_secondary_hover = zed_colors.ghost_element_hover;
    colors.button_secondary_active = zed_colors.ghost_element_active;
    colors.group_box = zed_colors.surface_background;
    colors.group_box_foreground = zed_colors.text_muted;
    colors.description_list_label = zed_colors.element_background;
    colors.description_list_label_foreground = zed_colors.text;
    colors.table = zed_colors.surface_background;
    colors.table_head = zed_colors.element_background;
    colors.table_head_foreground = zed_colors.text_muted;
    colors.table_row_border = zed_colors.border_variant;
    colors.table_hover = zed_colors.element_hover;
    colors.table_active = zed_colors.element_active;
    colors.table_active_border = zed_colors.border_selected;
    colors.tiles = zed_colors.surface_background;
    colors.sidebar = zed_colors.panel_background;
    colors.sidebar_foreground = zed_colors.text;
    colors.sidebar_border = zed_colors.border;
    colors.sidebar_accent = zed_colors.text_accent;
    colors.sidebar_accent_foreground = zed_colors.text;
    colors.window_border = zed_colors.border;

    gpui_theme.tokens = gpui_component::ThemeTokens::from(gpui_theme.colors);
}

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
        if !body.trim_start().starts_with('{') {
            return None;
        }
        match parse_media_block_body(body) {
            Ok(media_ref) => {
                ensure_theme_initialized(window, cx);
                Some(render_media_ref(media_ref, window, cx))
            }
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
fn render_media_ref(reference: MediaRef, _window: &mut Window, cx: &mut App) -> AnyElement {
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

/// Create a `MediaWidget` entity from a block body, without wrapping it in an
/// element. Used by `hkask_viz_core::block_renderer` to cache the entity across
/// renders (so audio/video playback and widget state survive re-renders).
///
/// Returns `None` if the body is not a valid media block (non-JSON, missing
/// `src`, unknown kind). The caller falls through to the next renderer or the
/// default code-block renderer.
pub fn create_media_widget(
    body: &str,
    window: &mut Window,
    cx: &mut App,
) -> Option<Entity<MediaWidget>> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    match parse_media_block_body(body) {
        Ok(media_ref) => {
            ensure_theme_initialized(window, cx);
            Some(cx.new(|cx| {
                let mut widget = MediaWidget::new(media_ref, cx);
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
