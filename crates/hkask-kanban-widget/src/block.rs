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
/// The board data can be a single board (with inline `tasks`) or multiple
/// boards (with `boards` + `tasks_by_board`). The single-board shape is the
/// common case (the agent emits one board per block).
#[derive(Debug, Clone, Deserialize)]
pub struct KanbanBlockBody {
    #[serde(default)]
    pub viz: Option<String>,
    /// Single-board shape: the board's id and name.
    #[serde(default)]
    pub board_id: Option<String>,
    #[serde(default)]
    pub board_name: Option<String>,
    /// Single-board shape: the tasks for this board.
    #[serde(default)]
    pub tasks: Vec<TaskBody>,
    /// Multi-board shape: the list of boards.
    #[serde(default)]
    pub boards: Vec<BoardBody>,
    /// Multi-board shape: tasks keyed by board id.
    #[serde(default)]
    pub tasks_by_board: Vec<BoardTasksBody>,
    /// Server-authoritative provenance for re-issuing the originating MCP tool
    /// with modified args (T6 move affordance). `#[serde(default)]` so bodies
    /// emitted before provenance landed parse with an empty (non-dispatchable)
    /// provenance and the widget falls back to its read-only display.
    #[serde(default)]
    pub provenance: BlockProvenance,
}

/// One board in the multi-board shape.
#[derive(Debug, Clone, Deserialize)]
pub struct BoardBody {
    #[serde(default)]
    pub board_id: String,
    #[serde(default)]
    pub name: String,
}

/// Tasks for a single board in the multi-board shape.
#[derive(Debug, Clone, Deserialize)]
pub struct BoardTasksBody {
    #[serde(default)]
    pub board_id: String,
    #[serde(default)]
    pub tasks: Vec<TaskBody>,
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
}

impl KanbanBlockBody {
    /// Returns the list of (board_id, board_name, tasks) tuples that the widget
    /// should render. For the single-board shape, this is one entry. For the
    /// multi-board shape, it's one entry per board (boards with no tasks get
    /// an empty task list).
    pub fn boards_with_tasks(&self) -> Vec<(String, String, &[TaskBody])> {
        // Single-board shape: board_id + board_name + tasks. When `tasks` is
        // non-empty OR `boards` is empty, treat as single-board.
        if !self.tasks.is_empty() || self.boards.is_empty() {
            let id = self.board_id.clone().unwrap_or_default();
            let name = self.board_name.clone().unwrap_or_else(|| {
                if id.is_empty() {
                    "Kanban Board".to_string()
                } else {
                    id.clone()
                }
            });
            return vec![(id, name, &self.tasks)];
        }

        // Multi-board shape: boards + tasks_by_board.
        self.boards
            .iter()
            .map(|board| {
                let tasks = self
                    .tasks_by_board
                    .iter()
                    .find(|entry| entry.board_id == board.board_id)
                    .map(|entry| entry.tasks.as_slice())
                    .unwrap_or(&[]);
                (board.board_id.clone(), board.name.clone(), tasks)
            })
            .collect()
    }
}

/// Parse a ```` ```kanban ```` block body. Tolerant: missing `viz`/`tasks`/
/// `boards` default to `None`/empty rather than erroring, so media-shaped and
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
    fn parses_multi_board_shape() {
        let body = r#"{"viz":"kanban","boards":[
            {"board_id":"b1","name":"Sprint 1"},
            {"board_id":"b2","name":"Sprint 2"}
        ],"tasks_by_board":[
            {"board_id":"b1","tasks":[{"task_id":"t1","title":"A","status":"backlog"}]},
            {"board_id":"b2","tasks":[]}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("kanban"));
        let boards = parsed.boards_with_tasks();
        assert_eq!(boards.len(), 2);
        assert_eq!(boards[0].0, "b1");
        assert_eq!(boards[0].1, "Sprint 1");
        assert_eq!(boards[0].2.len(), 1);
        assert_eq!(boards[1].0, "b2");
        assert_eq!(boards[1].2.len(), 0);
    }

    #[test]
    fn single_board_with_tasks_takes_precedence_over_boards() {
        // When both `tasks` and `boards` are present, the single-board shape
        // (tasks non-empty) wins.
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog"}
        ],"boards":[{"board_id":"b2","name":"Other"}]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        let boards = parsed.boards_with_tasks();
        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].0, "b1");
    }

    #[test]
    fn empty_body_parses_as_single_empty_board() {
        let body = r#"{"viz":"kanban"}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("kanban"));
        // No tasks and no boards → single-board shape with empty tasks.
        let boards = parsed.boards_with_tasks();
        assert_eq!(boards.len(), 1);
        assert!(boards[0].2.is_empty());
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
