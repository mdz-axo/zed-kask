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
//!
//! The create-and-cache pattern (guard → parse → `viz` check → construct) lives
//! in `hkask_viz_core::VizWidget`, implemented for [`GraphWidget`] there.
#![warn(clippy::let_underscore_future)]

pub mod block;
pub mod layout;
pub mod propagate;
pub mod view;

pub use view::GraphWidget;

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
