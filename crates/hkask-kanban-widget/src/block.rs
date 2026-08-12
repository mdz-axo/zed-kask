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
    /// Optional column metadata (S8: WIP limits). When present, each entry's
    /// `status` matches a task `status` (case-insensitive) and carries a
    /// `wip_limit`. `#[serde(default)]` so older blocks parse with no column
    /// metadata (no WIP limits rendered).
    #[serde(default)]
    pub columns: Vec<ColumnBody>,
    /// Server-authoritative provenance for re-issuing the originating MCP tool
    /// with modified args (T6 move affordance). `#[serde(default)]` so bodies
    /// emitted before provenance landed parse with an empty (non-dispatchable)
    /// provenance and the widget falls back to its read-only display.
    #[serde(default)]
    pub provenance: BlockProvenance,
}

/// Column metadata for a kanban board (S8: WIP limits). The `status` matches a
/// task `status` (case-insensitive); `wip_limit` caps how many tasks may be in
/// the column simultaneously.
#[derive(Debug, Clone, Deserialize)]
pub struct ColumnBody {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub wip_limit: Option<u32>,
}

/// One task on the board. Mirrors the `TaskInfo` struct from the deleted
/// `KanbanBoardView`.
///
/// B3/RU4: carries the full task detail fields (comments, verification,
/// gas spend log) so the card-detail popover can render them passively from
/// the block body (D18 passive-render contract preserved — no `ToolInvoker`
/// fetch on card click). All extra fields are `#[serde(default)]` so older
/// blocks parse with empty collections / `None`.
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
    /// The swarm this task belongs to, when coordinated via a local swarm (R1).
    /// Emitted by the `kata-kanban` server on `TaskInfo`. The widget renders a
    /// visible swarm link badge on the card so the operator can see which swarm
    /// is running a task at a glance. `None` on tasks not scoped to a swarm.
    #[serde(default)]
    pub swarm_id: Option<String>,
    #[serde(default)]
    pub gas_remaining: Option<u64>,
    /// The latest recorded activity on this task (R3) — a one-line status strip
    /// rendered on the card. Emitted by the server (derived from the most recent
    /// comment). `None` on tasks with no recorded activity.
    #[serde(default)]
    pub activity: Option<TaskActivityBody>,
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
    /// ("✓ N criteria") on the card and as a full list in the detail popover.
    #[serde(default)]
    pub criteria: Vec<String>,
    /// Comments thread. Rendered only in the detail popover (B3). Empty on
    /// tasks with no comments.
    #[serde(default)]
    pub comments: Vec<CommentBody>,
    /// Verification result. Rendered only in the detail popover (B3). `None`
    /// on unverified tasks.
    #[serde(default)]
    pub verification: Option<VerificationBody>,
    /// Gas/rJoule spend log. Rendered only in the detail popover (B3). Empty
    /// on tasks with no spend entries.
    #[serde(default)]
    pub gas_spend: Vec<GasEntryBody>,
}

/// One comment on a task. Mirrors the server's `Comment` shape (author, body,
/// created_at) for passive rendering in the card-detail popover (B3).
#[derive(Debug, Clone, Deserialize)]
pub struct CommentBody {
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub body: String,
    /// ISO-8601 timestamp string as emitted by the server. Rendered as-is
    /// (no parsing — the widget is passive and does not normalize timestamps).
    #[serde(default)]
    pub created_at: String,
}

/// Verification result on a task. Mirrors the server's `Verification` shape
/// for passive rendering in the card-detail popover (B3).
#[derive(Debug, Clone, Deserialize)]
pub struct VerificationBody {
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub reason: String,
}

/// One entry in a task's gas/rJoule spend log. Mirrors the server's `GasEntry`
/// shape for passive rendering in the card-detail popover (B3). `kind`
/// distinguishes gas spend from rJoule spend (the server emits `"gas_spend"` /
/// `"rjoule_spend"`).
#[derive(Debug, Clone, Deserialize)]
pub struct GasEntryBody {
    #[serde(default)]
    pub amount: u64,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub kind: String,
}

/// The latest recorded activity on a task (R3). Mirrors the server's
/// `TaskActivity` shape for passive rendering as a one-line status strip on
/// the card. See `TaskBody::activity`.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskActivityBody {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub at: String,
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
    fn parses_full_task_detail_fields() {
        // B3: comments, verification, and gas_spend parse from the block body.
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog",
             "criteria":["compiles"],
             "comments":[
               {"author":"alice","body":"Looks good","created_at":"2026-08-09T10:00:00Z"}
             ],
             "verification":{"passed":true,"reason":"tests pass"},
             "gas_spend":[
               {"amount":50,"reason":"inference","kind":"gas_spend"},
               {"amount":100,"reason":"tool call","kind":"rjoule_spend"}
             ]}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        let task = &parsed.tasks[0];
        assert_eq!(task.criteria, vec!["compiles"]);
        assert_eq!(task.comments.len(), 1);
        assert_eq!(task.comments[0].author, "alice");
        assert_eq!(task.comments[0].body, "Looks good");
        assert_eq!(task.comments[0].created_at, "2026-08-09T10:00:00Z");
        let verification = task.verification.as_ref().expect("verification present");
        assert!(verification.passed);
        assert_eq!(verification.reason, "tests pass");
        assert_eq!(task.gas_spend.len(), 2);
        assert_eq!(task.gas_spend[0].amount, 50);
        assert_eq!(task.gas_spend[0].kind, "gas_spend");
        assert_eq!(task.gas_spend[1].kind, "rjoule_spend");
    }

    #[test]
    fn full_detail_fields_default_empty_when_absent() {
        // B3: older blocks without comments/verification/gas_spend parse with
        // empty collections and `None` verification.
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog"}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        let task = &parsed.tasks[0];
        assert!(task.comments.is_empty());
        assert!(task.verification.is_none());
        assert!(task.gas_spend.is_empty());
    }

    #[test]
    fn task_body_parses_swarm_id_field() {
        // R1 S4 sensor: the kata-kanban server emits `swarm_id` on `TaskInfo`.
        // The widget's TaskBody MUST have a `swarm_id` field to receive it — if
        // absent, the link is silently dropped (the field-drop trap).
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"in_progress","swarm_id":"sw-42"}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.tasks[0].swarm_id.as_deref(), Some("sw-42"));
    }

    #[test]
    fn task_body_swarm_id_defaults_none_when_absent() {
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog"}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert!(parsed.tasks[0].swarm_id.is_none());
    }

    #[test]
    fn task_body_parses_activity_field() {
        // R3 S4 sensor: the kata-kanban server emits `activity` on `TaskInfo`.
        // The widget's TaskBody MUST parse it or the card status strip is
        // silently dropped (the field-drop trap).
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"in_progress",
             "activity":{"text":"Spawn executed: agent=beta","kind":"comment","at":"2026-08-11T12:00:00Z"}}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        let activity = parsed.tasks[0].activity.as_ref().expect("activity present");
        assert_eq!(activity.text, "Spawn executed: agent=beta");
        assert_eq!(activity.kind, "comment");
        assert_eq!(activity.at, "2026-08-11T12:00:00Z");
    }

    #[test]
    fn task_body_activity_defaults_none_when_absent() {
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"backlog"}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert!(parsed.tasks[0].activity.is_none());
    }

    #[test]
    fn parses_column_wip_limits() {
        // S8: column metadata with WIP limits parses from the block body.
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[
            {"task_id":"t1","title":"A","status":"in_progress"}
        ],"columns":[
            {"status":"in_progress","wip_limit":3},
            {"status":"review","wip_limit":2}
        ]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert_eq!(parsed.columns.len(), 2);
        assert_eq!(parsed.columns[0].status, "in_progress");
        assert_eq!(parsed.columns[0].wip_limit, Some(3));
        assert_eq!(parsed.columns[1].status, "review");
        assert_eq!(parsed.columns[1].wip_limit, Some(2));
    }

    #[test]
    fn columns_default_empty_when_absent() {
        // S8: older blocks without `columns` parse with an empty vec (no WIP
        // limits rendered).
        let body = r#"{"viz":"kanban","board_id":"b1","tasks":[]}"#;
        let parsed = parse_kanban_body(body).expect("valid body parses");
        assert!(parsed.columns.is_empty());
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
            "provenance":{"tool":"kanban_task_list","server":"kata-kanban","args":{"board_id":"b1"}}}"#;
        let body = parse_kanban_body(json).expect("valid body parses");
        assert!(body.provenance.is_dispatchable());
        assert_eq!(body.provenance.tool.as_deref(), Some("kanban_task_list"));
        assert_eq!(
            body.provenance.server.as_deref(),
            Some("kata-kanban")
        );
        assert_eq!(body.provenance.args["board_id"], "b1");
    }
}
