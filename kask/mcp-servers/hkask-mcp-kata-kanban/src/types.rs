//! Request/response types for the kanban MCP server tools.
//!
//! Each tool has a request struct and response struct serializable
//! for MCP JSON-RPC transport.

use hkask_mcp_server::AnyJsonValue;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Board tools ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardCreateRequest {
    pub name: String,
    pub columns: Option<Vec<ColumnDefInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ColumnDefInput {
    pub name: String,
    pub status: String,
    /// Optional WIP (work-in-progress) limit for this column.
    /// When set, task moves into this column will be rejected if the
    /// column already has this many tasks in the target status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wip_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardCreateResponse {
    pub board_id: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    /// PKO concept: <https://w3id.org/pko#Procedure>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ColumnInfo {
    pub id: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardListRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardListResponse {
    pub boards: Vec<BoardInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardInfo {
    pub board_id: String,
    pub name: String,
    pub column_count: usize,
    /// PKO concept: <https://w3id.org/pko#Procedure>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

// ── Task tools ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCreateRequest {
    pub board_id: String,
    pub title: String,
    pub description: Option<String>,
    pub criteria: Option<Vec<String>>,

    /// Gas/rJoule budget for the subagent working on this task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_budget: Option<u64>,
    /// Inference/API rJoule budget (250k ≈ $1 spend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rjoule_budget: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCreateResponse {
    pub task_id: String,
    pub board_id: String,
    pub title: String,
    pub status: String,
    /// PKO concept: <https://w3id.org/pko#Step>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskListRequest {
    pub board_id: String,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskListResponse {
    pub tasks: Vec<TaskInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskInfo {
    pub task_id: String,
    pub board_id: String,
    pub title: String,
    pub status: String,
    pub assignee: Option<String>,
    pub criteria_count: usize,
    /// Remaining gas/rJoules in the subagent's budget (None = no budget set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_remaining: Option<u64>,
    /// Remaining rJoules for inference/API calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rjoule_remaining: Option<u64>,
    /// PKO concept: <https://w3id.org/pko#Step>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskMoveRequest {
    pub task_id: String,
    pub target_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskMoveResponse {
    pub task_id: String,
    pub previous_status: String,
    pub new_status: String,
    /// PKO concept: <https://w3id.org/pko#ChangeOfStatus>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAssignRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAssignResponse {
    pub task_id: String,
    pub assignee: String,
    /// PKO concept: <https://www.w3.org/ns/prov#wasAssociatedWith>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskVerifyRequest {
    pub task_id: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskVerifyResponse {
    pub task_id: String,
    pub passed: bool,
    pub reasoning: String,
    pub new_status: String,
    /// PKO concept: <https://w3id.org/pko#StepVerification>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

// ── Gas management ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAddGasRequest {
    pub task_id: String,
    /// Amount of gas/rJoules to add to the task's remaining budget.
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAddGasResponse {
    pub task_id: String,
    pub new_gas_remaining: u64,
    /// PKO concept: <https://www.w3.org/ns/prov#used>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAddRjoulesRequest {
    pub task_id: String,
    /// Amount of rJoules to add to the inference/API budget.
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAddRjoulesResponse {
    pub task_id: String,
    pub new_rjoule_remaining: u64,
    /// PKO concept: <https://www.w3.org/ns/prov#used>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

// ── Comments ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCommentRequest {
    pub task_id: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCommentResponse {
    pub comment_id: String,
    pub task_id: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
    /// PKO concept: <https://w3id.org/pko#UserFeedbackOccurrence>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCommentsSinceRequest {
    pub task_id: String,
    /// Return only comments at or after this index (0-based).
    #[serde(default)]
    pub since_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCommentsSinceResponse {
    pub task_id: String,
    pub comments: Vec<TaskCommentResponse>,
    /// Total comment count on the task (for cursor tracking).
    pub total_count: usize,
}

// ── Deliverables ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAddDeliverableRequest {
    pub task_id: String,
    /// File path or URL pointing to work output.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAddDeliverableResponse {
    pub task_id: String,
    pub deliverable_count: usize,
    /// PKO concept: <https://www.w3.org/ns/prov#wasGeneratedBy>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

// ── Reopen ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskReopenRequest {
    pub task_id: String,
    /// Optional new gas budget to grant on reopen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_budget: Option<u64>,
    /// Optional new rJoule budget to grant on reopen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rjoule_budget: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskReopenResponse {
    pub task_id: String,
    pub new_status: String,
    pub gas_remaining: Option<u64>,
    pub rjoule_remaining: Option<u64>,
    /// PKO concept: <https://w3id.org/pko#ChangeOfStatus>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

// ── Contract proposals ──────────────────────────────────────────────────────

/// A proposal template for a contract missing its user-facing `expect:` annotation.
/// Agents use this to compose and submit contract grounding proposals.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContractProposeExpect {
    pub board_id: String,
    /// Proposals for missing `expect:` annotations (arbitrary JSON array of
    /// `ExpectProposal`-shaped objects). Accepted as `AnyJsonValue` because
    /// `hkask_types::ExpectProposal` is not `JsonSchema`-derivable from this
    /// crate; the tool body deserializes into the typed struct.
    pub proposals: AnyJsonValue,
}

// ── Kata prompts ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskKataCoachingRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskKataImprovementRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskKataPracticeRequest {
    pub task_id: String,
    /// What specific sub-problem to focus the observation drill on.
    pub sub_problem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskKataResponse {
    pub task_id: String,
    pub prompt: String,
    /// PKO concept: <https://w3id.org/pko#UserQuestionOccurrence>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

// ── Spawn ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskSpawnRequest {
    pub task_id: String,
    /// Delegation level: "minimal", "standard", or "maximal".
    pub delegation_level: String,
    /// Skills to delegate (e.g. ["bug-hunt", "tdd"]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegated_skills: Vec<String>,
    /// Memory scope: "none", "episodic", or "full".
    #[serde(default)]
    pub memory_scope: Option<String>,
    /// Gas budget to grant on spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_budget: Option<u64>,
    /// rJoule budget to grant on spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rjoule_budget: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskSpawnResponse {
    pub task_id: String,
    pub message: String,
    /// PKO concept: <https://w3id.org/pko#StepExecution>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko: Option<String>,
}

// ── Kanban block body emission (T6 widget sovereignty) ───────────────────────
//
// The agent (curator) emits a ```kanban fenced block whose body is the
// combined `kanban_board_list` + `kanban_task_list` output. This helper is the
// authoritative composition site: it bakes `provenance` into the block body so
// the kanban widget can re-issue `kanban_task_move` with modified args without
// re-explaining the request to the agent. See `hkask_tool_invoker::BlockProvenance`
// — "MCP servers bake it into their `display_hint` blocks (authoritative)".

/// The MCP server name that hosts the kanban tools. The widget's fallback
/// dispatch target and the value baked into emitted block provenance.
pub const KANBAN_BLOCK_SERVER: &str = "hkask-mcp-kata-kanban";
/// The MCP tool that produced the rendered board (the task listing). The
/// widget reads this to know the block is server-authoritative for the
/// `hkask-mcp-kata-kanban` server; the move affordance dispatches a different
/// tool (`kanban_task_move`) on the same server.
pub const KANBAN_BLOCK_TOOL: &str = "kanban_task_list";

/// Build the ```kanban block body JSON the agent emits inline, with
/// server-authoritative `provenance` baked in. The widget parses this body
/// (see `hkask-kanban-widget::block::KanbanBlockBody`) and uses the provenance
/// to dispatch `kanban_task_move` via the governed `shared_tool_invoker()`.
///
/// `request_args` is the args the producing `kanban_task_list` call was
/// invoked with (e.g. `{ "board_id": "b1", "status": null }`), so the widget
/// can re-identify the board. `span_id` is the `reg.*` trace span id when
/// available, else `None`.
pub fn build_kanban_block_body(
    board_id: &str,
    board_name: &str,
    tasks: &[TaskInfo],
    request_args: &serde_json::Value,
    span_id: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "viz": "kanban",
        "board_id": board_id,
        "board_name": board_name,
        "tasks": tasks,
        "provenance": {
            "tool": KANBAN_BLOCK_TOOL,
            "server": KANBAN_BLOCK_SERVER,
            "args": request_args,
            "span_id": span_id,
        }
    })
}

#[cfg(test)]
mod block_body_tests {
    use super::*;

    fn sample_task(task_id: &str, status: &str) -> TaskInfo {
        TaskInfo {
            task_id: task_id.to_string(),
            board_id: "b1".to_string(),
            title: format!("Task {task_id}"),
            status: status.to_string(),
            assignee: None,
            criteria_count: 0,
            gas_remaining: None,
            rjoule_remaining: None,
            pko: None,
        }
    }

    #[test]
    fn build_kanban_block_body_carries_non_empty_provenance_tool() {
        // Acceptance criterion 2: the emitted kanban block body carries a
        // non-empty `provenance.tool`.
        let tasks = vec![sample_task("t1", "backlog")];
        let body = build_kanban_block_body(
            "b1",
            "Sprint 1",
            &tasks,
            &serde_json::json!({ "board_id": "b1" }),
            None,
        );
        assert_eq!(body["viz"], "kanban");
        assert_eq!(body["board_id"], "b1");
        assert_eq!(body["board_name"], "Sprint 1");
        assert!(body["tasks"].is_array());
        assert_eq!(body["tasks"].as_array().map(|a| a.len()), Some(1));
        let provenance = body
            .get("provenance")
            .expect("provenance baked into block body");
        assert_eq!(provenance["tool"], KANBAN_BLOCK_TOOL);
        assert_eq!(provenance["server"], KANBAN_BLOCK_SERVER);
        assert_eq!(provenance["args"]["board_id"], "b1");
        assert!(provenance.get("span_id").is_some());
    }

    #[test]
    fn build_kanban_block_body_serializes_task_info_into_widget_shape() {
        // The widget's `TaskBody` expects task_id/title/status/assignee/
        // gas_remaining; `TaskInfo` carries those (plus extra fields the
        // tolerant parser ignores). Verify the serialized task round-trips
        // through the widget's parser.
        let tasks = vec![TaskInfo {
            task_id: "t1".to_string(),
            board_id: "b1".to_string(),
            title: "Task A".to_string(),
            status: "backlog".to_string(),
            assignee: Some("alice".to_string()),
            criteria_count: 2,
            gas_remaining: Some(100),
            rjoule_remaining: None,
            pko: None,
        }];
        let body = build_kanban_block_body(
            "b1",
            "Sprint 1",
            &tasks,
            &serde_json::json!({ "board_id": "b1" }),
            Some("span-42"),
        );
        assert_eq!(body["provenance"]["span_id"], "span-42");
        let task_obj = body["tasks"].get(0).expect("first task present");
        assert_eq!(task_obj["task_id"], "t1");
        assert_eq!(task_obj["title"], "Task A");
        assert_eq!(task_obj["status"], "backlog");
        assert_eq!(task_obj["assignee"], "alice");
        assert_eq!(task_obj["gas_remaining"], 100);
    }
}
