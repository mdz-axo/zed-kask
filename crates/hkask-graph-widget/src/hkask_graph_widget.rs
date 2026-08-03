//! GPUI graph widget for rendering ```` ```graph ```` fenced blocks inline in
//! agent markdown. The first viz type is the MAIA event-tree DAG, which
//! consumes the `scenario_quantify` output shape produced by
//! `hkask-mcp-scenarios` (binomial events with conditional dependencies).
//!
//! Wired behind the D18 seam via [`hkask_viz_core::block_renderer`], which
//! composes this renderer with the media renderer. The agent emits a fenced
//! block whose body is the (curated) `scenario_quantify` JSON, e.g.:
//!
//! ```text
//! ```graph
//! { "viz": "event_tree", "subject": "…", "joint_probability": 0.12,
//!   "nodes": [ { "id": "e0", "name": "…", "marginal_probability": 0.7,
//!                "depends_on": [ { "parent_event_ids": [] } ] }, … ] }
//! ```
//! ```
//!
//! Edges are child-side: each node lists its parents in
//! `depends_on[].parent_event_ids` (the server's edge model) or, as a tolerant
//! fallback, in a flat `parents` array.
#![warn(clippy::let_underscore_future)]

pub mod block;
pub mod layout;
pub mod propagate;
pub mod view;

use gpui::{AnyElement, App, AppContext, Entity, Window};

/// The graph block renderer callback type (mirrors
/// `markdown::MediaBlockRendererFn` — same erased `dyn Fn` type).
pub type GraphBlockRenderer = Box<dyn Fn(&str, &mut Window, &mut App) -> Option<AnyElement>>;

pub use view::GraphWidget;

/// Create the graph block renderer for the D18 seam.
///
/// Self-selects on a JSON body whose `viz` field equals `"event_tree"`. Other
/// bodies (including media blocks, which are claimed first by the media
/// renderer in [`hkask_viz_core::block_renderer`]) fall through with `None` so
/// the default code-block renderer handles them.
pub fn graph_block_renderer() -> GraphBlockRenderer {
    Box::new(|body, window, cx| {
        // Only JSON-shaped bodies can be graph blocks; skip everything else
        // silently (the renderer is invoked for every fenced block).
        if !body.trim_start().starts_with('{') {
            return None;
        }
        match block::parse_graph_body(body) {
            Ok(parsed) if parsed.viz.as_deref() == Some("event_tree") => {
                Some(view::render_event_tree(parsed, body, window, cx))
            }
            Ok(_) => None,
            Err(error) => {
                log::warn!("hkask-graph-widget: malformed graph block: {error}");
                None
            }
        }
    })
}

/// Create a `GraphWidget` entity from a block body, without wrapping it in an
/// element. Used by `hkask_viz_core::block_renderer` to cache the entity across
/// renders (so pan/zoom/evidence state survives re-renders).
///
/// Returns `None` if the body is not a valid `event_tree` graph block.
pub fn create_graph_widget(body: &str, cx: &mut App) -> Option<Entity<view::GraphWidget>> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    match block::parse_graph_body(body) {
        Ok(parsed) if parsed.viz.as_deref() == Some("event_tree") => {
            Some(cx.new(|cx| view::GraphWidget::new(parsed, cx)))
        }
        Ok(_) => None,
        Err(error) => {
            log::warn!("hkask-graph-widget: malformed graph block: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pins the D18 graph block self-selection: a valid event_tree body is
    // claimed; media-shaped and plain-text bodies fall through. See
    // DIVERGENCE.md D18 and the .rules "Tests must pin deliberate zed-kask
    // deviations from upstream".
    #[test]
    fn selects_event_tree_body() {
        let body = r#"{"viz":"event_tree","nodes":[{"id":"e0","name":"root"}]}"#;
        let parsed = block::parse_graph_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("event_tree"));
        assert_eq!(parsed.nodes.len(), 1);
    }

    #[test]
    fn falls_through_non_graph_bodies() {
        // A media-shaped body has no `viz` field → parsed (viz None) but not
        // claimed by the graph renderer.
        let media = r#"{"kind":"video","src":"/clip.mp4"}"#;
        let parsed = block::parse_graph_body(media).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("event_tree"));

        // Plain text is not JSON → parse fails → renderer returns None.
        assert!(block::parse_graph_body("not json").is_err());
    }
}
