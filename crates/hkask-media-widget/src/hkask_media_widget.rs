//! GPUI media widget for rendering ```` ```media ```` fenced blocks inline in
//! agent markdown. Renders images, video placeholders, and audio placeholders
//! emitted by the `hkask-mcp-media` server's `media_block` helper.
//!
//! Wired behind the D18 seam via [`hkask_viz_core::block_renderer`], which
//! composes this renderer with the other viz widgets. The agent emits a fenced
//! block whose body is JSON with `viz: "media"`, `kind`, and `src` fields:
//!
//! ```text
//! ```media
//! {"viz":"media","kind":"image","src":"/path/to/image.png","ontology":"omc:CreativeWork"}
//! ```
//! ```
//!
//! The create-and-cache pattern (guard → parse → `viz` check → construct) lives
//! in `hkask_viz_core::VizWidget`, implemented for [`MediaWidget`] there.

#![warn(clippy::let_underscore_future)]

pub mod block;
pub mod view;

pub use view::MediaWidget;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_media_body() {
        let body = r#"{"viz":"media","kind":"image","src":"/tmp/img.png"}"#;
        let parsed = block::parse_media_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("media"));
        assert_eq!(parsed.kind.as_deref(), Some("image"));
    }

    #[test]
    fn falls_through_non_media_bodies() {
        let graph = r#"{"viz":"event_tree","nodes":[]}"#;
        let parsed = block::parse_media_body(graph).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("media"));
    }
}
