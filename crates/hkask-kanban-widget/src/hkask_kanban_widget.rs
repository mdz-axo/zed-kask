//! GPUI kanban widget for rendering ```` ```kanban ```` fenced blocks inline
//! in agent markdown. Renders a horizontal column layout (Backlog, Ready, In
//! Progress, Review, Done) from JSON data already in the chat stream — a
//! passive renderer, no `ToolInvoker` fetches.
//!
//! Wired behind the D18 seam via [`hkask_viz_core::block_renderer`], which
//! composes this renderer with the media and graph renderers. The agent (curator)
//! calls the `kanban_board_list` + `kanban_task_list` MCP tools and emits the
//! combined result as a fenced block whose body is the combined JSON, e.g.:
//!
//! ```text
//! ```kanban
//! { "viz": "kanban", "board_id": "b1", "board_name": "Sprint 1",
//!   "tasks": [ { "task_id": "t1", "title": "…", "status": "backlog",
//!                 "assignee": "alice" } ] }
//! ```
//! ```
#![warn(clippy::let_underscore_future)]

pub mod block;
pub mod view;

use gpui::{App, AppContext, Entity};

pub use view::KanbanWidget;

/// Create a `KanbanWidget` entity from a block body, without wrapping it in an
/// element. Used by `hkask_viz_core::block_renderer` to cache the entity across
/// renders (so board/column state survives re-renders).
///
/// Self-selects on a JSON body whose `viz` field equals `"kanban"`. Other bodies
/// (including media blocks claimed first by the media renderer, and graph blocks
/// claimed by the graph renderer) fall through with `None`.
///
/// Returns `None` if the body is not a valid kanban block.
pub fn create_kanban_widget(body: &str, cx: &mut App) -> Option<Entity<view::KanbanWidget>> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    match block::parse_kanban_body(body) {
        Ok(parsed) if parsed.viz.as_deref() == Some("kanban") => {
            Some(cx.new(|cx| view::KanbanWidget::new(parsed, cx)))
        }
        Ok(_) => None,
        Err(error) => {
            log::warn!("hkask-kanban-widget: malformed kanban block: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pins the D18 kanban block self-selection: a valid kanban body is claimed;
    // media-shaped, graph-shaped, and plain-text bodies fall through. See
    // DIVERGENCE.md D18 and the .rules "Tests must pin deliberate zed-kask
    // deviations from upstream".

    #[test]
    fn selects_kanban_body() {
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[]}"#;
        let parsed = block::parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("kanban"));
    }

    #[test]
    fn falls_through_non_kanban_bodies() {
        // A media-shaped body has no `viz` field → parsed (viz None) but not
        // claimed by the kanban renderer.
        let media = r#"{"kind":"video","src":"/clip.mp4"}"#;
        let parsed = block::parse_kanban_body(media).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("kanban"));

        // A graph-shaped body has a different `viz` → not claimed.
        let graph = r#"{"viz":"event_tree","nodes":[]}"#;
        let parsed = block::parse_kanban_body(graph).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("kanban"));

        // Plain text is not JSON → parse fails → renderer returns None.
        assert!(block::parse_kanban_body("not json").is_err());
    }
}
