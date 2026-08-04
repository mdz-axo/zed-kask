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

use gpui::{AnyElement, App, AppContext, Entity, Window};

/// The scenarios block renderer callback type (mirrors
/// `markdown::MediaBlockRendererFn`).
pub type ScenariosBlockRenderer = Box<dyn Fn(&str, &mut Window, &mut App) -> Option<AnyElement>>;

pub use view::ScenariosWidget;

/// Create a `ScenariosWidget` entity from a block body, without wrapping it in
/// an element. Used by `hkask_viz_core::block_renderer` to cache the entity
/// across renders.
///
/// Returns `None` if the body is not a valid `scenarios` block (including
/// `event_tree` bodies, which belong to `hkask-graph-widget`).
pub fn create_scenarios_widget(body: &str, cx: &mut App) -> Option<Entity<view::ScenariosWidget>> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    match block::parse_scenarios_body(body) {
        Ok(parsed) if parsed.viz.as_deref() == Some("scenarios") => {
            Some(cx.new(|cx| view::ScenariosWidget::new(parsed, cx)))
        }
        Ok(_) => None,
        Err(error) => {
            log::warn!("hkask-scenarios-widget: malformed scenarios block: {error}");
            None
        }
    }
}

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
