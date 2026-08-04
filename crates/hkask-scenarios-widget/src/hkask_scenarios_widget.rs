#![forbid(unsafe_code)]
//! GPUI scenarios widget for rendering ```` ```scenarios ```` fenced blocks inline
//! in agent markdown. Renders the scenario pipeline overview, event matrix, and
//! sensitivity timeline from the `scenario_status` MCP tool response shape.
//!
//! The event-tree DAG is NOT rendered here — that is the `hkask-graph-widget`
//! crate's job (`viz: "event_tree"`). This widget handles the remaining
//! visualizations from the deleted `ScenariosView`: pipeline overview tiles,
//! calibration summary, event matrix (probability × uncertainty), event tree
//! list, and recent forecasts.
//!
//! Wired behind the D18 seam via [`hkask_viz_core::block_renderer`], which
//! composes this renderer with the media and graph renderers. The agent emits
//! a fenced block whose body is the (curated) `scenario_status` JSON, e.g.:
//!
//! ```text
//! ```scenarios
//! { "viz": "scenarios",
//!   "pipeline": { "forecast_count": 5, "resolved_count": 2, ... },
//!   "calibration": { "overall_brier": 0.15, ... },
//!   "event_tree": { "subject": "...", "nodes": [...] },
//!   "recent_forecasts": [...] }
//! ```
//! ```
#![warn(clippy::let_underscore_future)]

pub mod block;
pub mod view;

pub use view::ScenariosWidget;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_scenarios_body() {
        let body = r#"{"viz":"scenarios","pipeline":{"forecast_count":1,"resolved_count":0,"pending_count":1,"overall_brier":null,"recent_forecasts":[]}}"#;
        let parsed = block::parse_scenarios_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("scenarios"));
        assert_eq!(parsed.pipeline.forecast_count, 1);
    }

    #[test]
    fn falls_through_event_tree_bodies() {
        // event_tree bodies belong to hkask-graph-widget, not this widget.
        let body = r#"{"viz":"event_tree","nodes":[{"id":"e0"}]}"#;
        let parsed = block::parse_scenarios_body(body).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("scenarios"));
    }

    #[test]
    fn falls_through_media_bodies() {
        let body = r#"{"kind":"video","src":"/clip.mp4"}"#;
        let parsed = block::parse_scenarios_body(body).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("scenarios"));
    }

    #[test]
    fn falls_through_plain_text() {
        assert!(block::parse_scenarios_body("not json").is_err());
    }
}
