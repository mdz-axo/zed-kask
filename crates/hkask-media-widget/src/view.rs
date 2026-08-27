//! The `MediaWidget` GPUI view — renders ```` ```media ```` fenced blocks
//! inline in agent markdown.
//!
//! Rendering dispatches on the `kind` field:
//! - **image**: displays the image via `img()` if the src is a URL or data URI,
//!   or a placeholder with the path if it's a local file (GPUI can't read
//!   arbitrary files from the MCP server's filesystem).
//! - **video**: shows a video placeholder with the source path/URL.
//! - **audio**: shows an audio placeholder with the source path/URL.
//! - **svg**: renders the SVG inline.
//!
//! The widget is cached across renders by `hkask-viz-core`'s LRU cache.

use gpui::{
    AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, img,
};
use theme::ActiveTheme;
use ui::{Color, Label, LabelCommon, LabelSize};

use crate::block::MediaBlockBody;

/// The media widget view. Renders inline in agent markdown via the D18 seam
/// composed by `hkask-viz-core`.
pub struct MediaWidget {
    body: MediaBlockBody,
}

impl MediaWidget {
    /// Construct a new media widget from a parsed block body.
    pub fn new(body: MediaBlockBody, _cx: &mut Context<Self>) -> Self {
        Self { body }
    }

    /// The media kind ("image", "video", "audio", "svg"), defaulting to "image".
    fn kind(&self) -> &str {
        self.body.kind.as_deref().unwrap_or("image")
    }

    /// The media source (URL, file path, or data URI).
    fn src(&self) -> &str {
        self.body.src.as_deref().unwrap_or("")
    }

    /// Whether the source is a data URI (base64-encoded inline content).
    fn is_data_uri(&self) -> bool {
        self.src().starts_with("data:")
    }

    /// Whether the source is an HTTP/HTTPS URL.
    fn is_http_url(&self) -> bool {
        self.src().starts_with("http://") || self.src().starts_with("https://")
    }

    /// Render an image element. Uses GPUI's `img()` for URLs and data URIs.
    /// For local file paths, shows a placeholder (the MCP server's filesystem
    /// is not accessible from the GPUI process).
    fn render_image(&self, cx: &mut App) -> AnyElement {
        if self.is_data_uri() || self.is_http_url() {
            let src = self.src().to_string();
            return div()
                .id("media-image")
                .max_w_full()
                .child(
                    img(SharedString::from(src))
                        .max_w_full()
                        .max_h(px(400.))
                        .object_fit(gpui::ObjectFit::Contain),
                )
                .into_any_element();
        }

        // Local file path — show a placeholder with the path.
        self.render_placeholder("image", cx)
    }

    /// Render a video placeholder. GPUI does not have a native video element,
    /// so we show a styled placeholder with the source path/URL.
    fn render_video(&self, cx: &mut App) -> AnyElement {
        self.render_placeholder("video", cx)
    }

    /// Render an audio placeholder. GPUI does not have a native audio element,
    /// so we show a styled placeholder with the source path/URL.
    fn render_audio(&self, cx: &mut App) -> AnyElement {
        self.render_placeholder("audio", cx)
    }

    /// Render a styled placeholder for media types GPUI can't render natively.
    fn render_placeholder(&self, kind: &str, cx: &mut App) -> AnyElement {
        let theme = cx.theme();
        let src = self.src();
        let display_src = if src.len() > 80 {
            format!("{}…", &src[..77])
        } else {
            src.to_string()
        };

        let icon = match kind {
            "video" => "🎬",
            "audio" => "🎵",
            _ => "🖼",
        };

        div()
            .id(("media-placeholder", SharedString::from(kind.to_string())))
            .py_2()
            .px_3()
            .my_1()
            .rounded_md()
            .border_1()
            .border_color(theme.colors().border)
            .bg(theme.colors().editor_background_opacity)
            .flex()
            .items_center()
            .gap_2()
            .child(
                Label::new(SharedString::from(format!("{icon} {kind}")))
                    .color(Color::Muted)
                    .size(LabelSize::Small),
            )
            .child(
                div()
                    .flex_1()
                    .child(
                        Label::new(SharedString::from(display_src))
                            .color(Color::Muted)
                            .size(LabelSize::Small),
                    ),
            )
            .into_any_element()
    }
}

impl Render for MediaWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.kind() {
            "image" => self.render_image(cx),
            "video" => self.render_video(cx),
            "audio" => self.render_audio(cx),
            "svg" => self.render_image(cx), // SVG renders like an image
            _ => self.render_placeholder("media", cx),
        }
    }
}
