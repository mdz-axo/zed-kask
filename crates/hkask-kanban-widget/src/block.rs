//! The ```` ```kanban ```` block body model + parser.
//!
//! The shape mirrors the combined `kanban_board_list` + `kanban_task_list` MCP
//! tool responses from the `kata-kanban` server. The agent (curator) calls the
//! tools and emits the combined result as a ```` ```kanban ```` fenced block;
//! the widget parses it passively (no `ToolInvoker` — the data is already in the
//! chat stream).
//!
//! Fields are optional / defaulted so the parser is tolerant of partial bodies
//! and never fails on media-shaped or graph-shaped JSON (which have no `viz`
//! field or a different `viz` value).

use hkask_tool_invoker::BlockProvenance;
use serde::Deserialize;

/// The discriminator-tagged body of a ```` ```kanban ```` block.
///
/// `viz` selects the renderer; `"kanban"` renders the horizontal column layout.
/// The board data is a single board (inline `tasks`). The agent emits one
/// block per board when multiple boards are needed.
#[derive(Debug, Clone, Deserialize)]
pub struct KanbanBlockBody {
    #[serde(default)]
    pub viz: Option<String>,
    /// The board's id and name.
    #[serde(default)]
    pub board_id: Option<String>,
    #[serde(default)]
    pub board_name: Option<String>,
    /// The tasks for this board.
    #[serde(default)]
    pub tasks: Vec<TaskBody>,
    /// Server-authoritative provenance for re-issuing the originating MCP tool
    /// with modified args (T6 move affordance). `#[serde(default)]` so bodies
    /// emitted before provenance landed parse with an empty (non-dispatchable)
    /// provenance and the widget falls back to its read-only display.
    #[serde(default)]
    pub provenance: BlockProvenance,
}

/// One task on the board. Mirrors the `TaskInfo` struct from the deleted
/// `KanbanBoardView`.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskBody {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    /// Optional longer-form description. Rendered clamped to 3 lines with a
    /// "See more" expand affordance in `render_card`. `None` on tasks without
    /// a description.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub gas_remaining: Option<u64>,
    /// Ontology concept URI (e.g. `pko:Step`). Emitted by the kata-kanban server
    /// on every `TaskInfo` response. The widget carries it so the compose-back
    /// body can reference it and a future "explain this task" affordance can
    /// dispatch on it. `None` on older blocks.
    #[serde(default)]
    pub ontology: Option<String>,
    /// Priority label (e.g. `"high"`, `"P1"`). Rendered as a colored badge.
    /// `None` on tasks without an explicit priority.
    #[serde(default)]
    pub priority: Option<String>,
    /// Labels/tags. Rendered as muted chips.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Acceptance criteria (free-form strings). Rendered as a count
    /// ("✓ N criteria").
    #[serde(default)]
    pub criteria: Vec<String>,
}

impl KanbanBlockBody {
    /// Returns the (board_id, board_name, tasks) tuple the widget should
    /// render. The board name falls back to the board id, or `"Kanban Board"`
    /// when both are absent.
    pub fn board_with_tasks(&self) -> (String, String, &[TaskBody]) {
        let id = self.board_id.clone().unwrap_or_default();
        let name = self.board_name.clone().unwrap_or_else(|| {
            if id.is_empty() {
                "Kanban Board".to_string()
            } else {
                id.clone()
            }
        });
        (id, name, &self.tasks)
    }
}

/// Parse a ```` ```kanban ```` block body. Tolerant: missing `viz`/`tasks`
/// default to `None`/empty rather than erroring, so media-shaped and
/// graph-shaped JSON parse (and are then rejected by the renderer on the `viz`
/// check) instead of being logged as a malformed kanban block.
pub fn parse_kanban_body(body: &str) -> anyhow::Result<KanbanBlockBody> {
    Ok(serde_json::from_str(body.trim())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_board_shape() {
        let body = r#"{"viz":"kanban","board_id":"b1","board_name":"Sprint 1","tasks":[
            {"task_id":"t1","title":"Task A","status":"backlog","assignee":"alice","gas_remaining":100},
            {"task_id":"t2","title":"Task B","status":"done"}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("kanban"));
        assert_eq!(parsed.board_id.as_deref(), Some("b1"));
        assert_eq!(parsed.board_name.as_deref(), Some("Sprint 1"));
        assert_eq!(parsed.tasks.len(), 2);
        assert_eq!(parsed.tasks[0].task_id, "t1");
        assert_eq!(parsed.tasks[0].assignee.as_deref(), Some("alice"));
        assert_eq!(parsed.tasks[0].gas_remaining, Some(100));
        assert_eq!(parsed.tasks[1].assignee, None);
        assert_eq!(parsed.tasks[1].gas_remaining, None);
    }

    #[test]
    fn empty_body_parses_as_single_empty_board() {
        let body = r#"{"viz":"kanban"}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("kanban"));
        // No tasks → empty task slice.
        let (id, name, tasks) = parsed.board_with_tasks();
        assert!(id.is_empty());
        assert_eq!(name, "Kanban Board");
        assert!(tasks.is_empty());
    }

    #[test]
    fn parses_task_description_field() {
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog","description":"Long-form description."}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.tasks.len(), 1);
        assert_eq!(
            parsed.tasks[0].description.as_deref(),
            Some("Long-form description.")
        );
    }

    #[test]
    fn description_defaults_to_none_when_absent() {
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog"}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.tasks.len(), 1);
        assert!(parsed.tasks[0].description.is_none());
    }

    #[test]
    fn parses_priority_labels_and_criteria_fields() {
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog","priority":"P1","labels":["backend","urgent"],"criteria":["compiles","tests pass"]}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.tasks.len(), 1);
        assert_eq!(parsed.tasks[0].priority.as_deref(), Some("P1"));
        assert_eq!(parsed.tasks[0].labels, vec!["backend", "urgent"]);
        assert_eq!(parsed.tasks[0].criteria, vec!["compiles", "tests pass"]);
    }

    #[test]
    fn priority_labels_and_criteria_default_empty_when_absent() {
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog"}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.tasks.len(), 1);
        assert!(parsed.tasks[0].priority.is_none());
        assert!(parsed.tasks[0].labels.is_empty());
        assert!(parsed.tasks[0].criteria.is_empty());
    }

    #[test]
    fn media_shaped_json_parses_without_error() {
        let media = r#"{"kind":"video","src":"/clip.mp4"}"#;
        let parsed = parse_kanban_body(media).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("kanban"));
    }

    #[test]
    fn graph_shaped_json_parses_without_error() {
        let graph = r#"{"viz":"event_tree","nodes":[]}"#;
        let parsed = parse_kanban_body(graph).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("kanban"));
    }

    #[test]
    fn non_json_fails() {
        assert!(parse_kanban_body("not json").is_err());
    }

    #[test]
    fn provenance_defaults_empty_when_absent() {
        // A body emitted before provenance lands has no `provenance` key.
        // Adding the field is non-breaking: provenance defaults empty and is
        // not dispatchable (T6 contract).
        let body = parse_kanban_body(r#"{"viz":"kanban"}"#).expect("valid body");
        assert!(!body.provenance.is_dispatchable());
        assert!(body.provenance.tool.is_none());
        assert!(body.provenance.server.is_none());
    }

    #[test]
    fn provenance_parses_when_present() {
        let json = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog"}],
            "provenance":{"tool":"kanban_task_list","server":"hkask-mcp-kata-kanban","args":{"board_id":"b1"}}}"#;
        let body = parse_kanban_body(json).expect("valid body parses");
        assert!(body.provenance.is_dispatchable());
        assert_eq!(body.provenance.tool.as_deref(), Some("kanban_task_list"));
        assert_eq!(
            body.provenance.server.as_deref(),
            Some("hkask-mcp-kata-kanban")
        );
        assert_eq!(body.provenance.args["board_id"], "b1");
    }
}
