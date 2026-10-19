//! Request/response types for the kanban MCP server tools.
//!
//! Each tool has a request struct and response struct serializable
//! for MCP JSON-RPC transport.

use hkask_types::AnyJsonValue;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Board tools ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardCreateRequest {
    /// Board name (required). Trimmed at the service boundary and rejected
    /// when empty after trimming or longer than 128 characters
    /// (`KANBAN_BOARD_NAME_MAX_CHARS` — reference model R1;
    /// `kask/docs/research/kanban-board-reference-models.md` §6.6).
    pub name: String,
    pub columns: Option<Vec<ColumnDefInput>>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ColumnDefInput {
    pub name: String,
    pub status: String,
    pub wip_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BoardCreateResponse {
    pub board_id: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ColumnInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub wip_limit: Option<u32>,
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
    /// Column definitions including WIP limits. Populated from the board's
    /// `ColumnDef` list so consumers (kanban panel, agent) can render WIP
    /// limits without a separate fetch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ColumnInfo>,
    /// Ontology concept: <https://w3id.org/pko#Procedure>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Goal tools ─────────────────────────────────────────────────────────────

/// A goal criterion input — an observable functional condition.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoalCriterionInput {
    /// Observable condition phrased functionally ("the user can do X",
    /// "Y no longer breaks").
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoalCreateRequest {
    /// The functional goal in the user's words — what the user will be able
    /// to do, or what stops being a problem. The agent interprets this; it
    /// never revises it.
    pub goal_text: String,
    /// 1–4 observable criteria (Fermi-decomposed from the goal).
    pub criteria: Vec<GoalCriterionInput>,
    /// The agent's intake prediction: probability (0.0–1.0) the goal will be
    /// achieved. Brier-scored at `kanban_goal_score`. Form-tolerant: a
    /// string-encoded number ("0.8") is coerced (the live 2026-09-27
    /// incident — three rmcp rejections of a string prediction); the
    /// 0.0–1.0 range check stays in the service.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "hkask_types::maybe_string::deserialize_optional_f64_from_maybe_string"
    )]
    pub prediction: Option<f64>,
    pub task_id: Option<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GoalCreateResponse {
    pub goal_id: String,
    pub goal_text: String,
    pub criteria_count: usize,
    pub prediction: Option<f64>,
}

/// One criterion judgment input for `kanban_goal_judge`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CriterionJudgmentInput {
    /// Index into the goal's criteria (0-based).
    pub index: usize,
    /// Whether the criterion is satisfied by the observed outcome.
    pub passed: bool,
    /// Evidence-grounded note for this criterion.
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoalJudgeRequest {
    pub goal_id: String,
    /// The verdict: "done" | "continue" | "blocked".
    pub verdict: String,
    /// Confidence in the verdict (0.0–1.0).
    pub confidence: f64,
    /// Per-criterion results — must cover every criterion of the goal
    /// exactly once (validated at judge time).
    #[serde(default)]
    pub criterion_results: Vec<CriterionJudgmentInput>,
    /// Overall reasoning, grounded in the observed outcome.
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GoalJudgeResponse {
    pub goal_id: String,
    pub verdict: String,
    pub verdict_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoalScoreRequest {
    pub goal_id: String,
    /// Whether the goal was achieved (the user's ground truth).
    pub achieved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GoalScoreResponse {
    pub goal_id: String,
    pub achieved: bool,
    /// Brier score of the intake prediction against the realized outcome.
    /// `null` when no intake prediction was recorded — surfaced, never faked.
    pub brier: Option<f64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoalMemoryAcknowledgeRequest {
    pub goal_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GoalMemoryAcknowledgeResponse {
    pub goal_id: String,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoalListRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GoalListResponse {
    pub goals: Vec<GoalInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GoalInfo {
    pub goal_id: String,
    pub goal_text: String,
    /// The criteria text in index order — a judge reads what each criterion
    /// says, not just how many exist.
    pub criteria: Vec<String>,
    pub criteria_count: usize,
    pub prediction: Option<f64>,
    pub latest_verdict: Option<String>,
    pub resolution: Option<String>,
    pub created_at: String,
}

// ── Task tools ─────────────────────────────────────────────────────────────

/// One goal-criterion citation for `kanban_task_create`'s `advances`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CriterionCitationInput {
    /// The goal being cited.
    pub goal_id: String,
    /// Index into the goal's criteria (0-based).
    pub criterion_index: usize,
    /// The criterion's description, verbatim — validated against the goal
    /// at creation and captured so the citation stays readable after the
    /// goal is resolved and pruned.
    pub criterion_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCreateRequest {
    pub board_id: String,
    pub title: String,
    pub description: Option<String>,
    pub criteria: Option<Vec<String>>,
    /// Goal criteria this task advances — the functional–technical join.
    /// Each citation is validated against the cited goal at creation.
    #[serde(default)]
    pub advances: Vec<CriterionCitationInput>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskCreateResponse {
    pub task_id: String,
    pub board_id: String,
    pub title: String,
    pub status: String,
    /// Number of goal-criterion citations recorded on the task.
    pub advances_count: usize,
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

/// The latest recorded activity on a task — a one-line status the kanban
/// widget renders on the card (R3). Derived from the task's most recent
/// comment by the server; the live per-tool-call hook ingest path is a
/// follow-up. This field is the passive-rendering seam: the data model and
/// card strip exist now, and swapping the data source (comments → live hooks)
/// is a later change that does not touch the widget.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskActivity {
    /// The activity text (e.g. the latest comment body or a spawn summary).
    pub text: String,
    /// The activity kind. Currently `"comment"` (derived from the comment
    /// thread). Future kinds: `"tool_call"`, `"delegation"`, `"verification"`.
    pub kind: String,
    /// ISO-8601 timestamp of the activity.
    pub at: String,
    /// Ontology concept: <https://w3id.org/pko#StepExecution>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskInfo {
    pub task_id: String,
    pub board_id: String,
    pub title: String,
    /// Preserved task text for list consumers, including edit prefill.
    pub description: Option<String>,
    pub status: String,
    pub assignee: Option<String>,
    /// The task's acceptance criteria text in index order — a reader sees
    /// what "done" means, not just how many criteria exist.
    pub criteria: Vec<String>,
    pub criteria_count: usize,
    /// Number of goal-criterion citations on the task.
    pub advances_count: usize,
    pub swarm_id: Option<String>,
    pub activity: Option<TaskActivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskMoveRequest {
    pub task_id: String,
    pub target_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskMoveResponse {
    pub task_id: String,
    pub previous_status: String,
    pub new_status: String,
    pub pko_execution_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskAssignRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskAssignResponse {
    pub task_id: String,
    pub assignee: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskVerifyRequest {
    pub task_id: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskVerifyResponse {
    pub task_id: String,
    pub passed: bool,
    pub reasoning: String,
    pub new_status: String,
}

// ── Comments ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCommentRequest {
    pub task_id: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskCommentResponse {
    pub comment_id: String,
    pub task_id: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskCommentsSinceRequest {
    pub task_id: String,
    /// Return only comments at or after this index (0-based).
    #[serde(default)]
    pub since_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskCommentsSinceResponse {
    pub task_id: String,
    pub comments: Vec<TaskCommentResponse>,
    /// Total comment count on the task (for cursor tracking).
    pub total_count: usize,
}

// ── Deliverables ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskAddDeliverableRequest {
    pub task_id: String,
    /// File path or URL pointing to work output.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskAddDeliverableResponse {
    pub task_id: String,
    pub deliverable_count: usize,
}

// ── Reopen ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskReopenRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskReopenResponse {
    pub task_id: String,
    pub new_status: String,
}

// ── Contract proposals ──────────────────────────────────────────────────────

/// A proposal template for a contract missing its user-facing `expect:` annotation.
/// Agents use this to compose and submit contract `expect:` annotation proposals.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ContractProposeExpect {
    pub board_id: String,
    /// Proposals for missing `expect:` annotations (arbitrary JSON array of
    /// `ExpectProposal`-shaped objects). Accepted as `AnyJsonValue` because
    /// `hkask_types::ExpectProposal` is not `JsonSchema`-derivable from this
    /// crate; the tool body deserializes into the typed struct.
    pub proposals: AnyJsonValue,
}

// ── Kata prompts ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskKataPromptRequest {
    pub task_id: String,
    /// Which kata to generate: "coaching" (5-question Coaching Kata
    /// dialogue), "improvement" (Improvement Kata PDCA cycle), or
    /// "practice" (Starter Kata observation drill — requires sub_problem).
    pub stage: String,
    /// The sub-problem to focus a practice-stage observation drill on.
    pub sub_problem: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskKataResponse {
    pub task_id: String,
    pub prompt: String,
}

// ── Spawn ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskSpawnRequest {
    pub task_id: String,
    pub idempotency_key: Option<String>,
    /// Delegation level: "minimal", "standard", or "maximal".
    pub delegation_level: String,
    /// Skills to delegate (e.g. ["bug-hunt", "tdd"]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegated_skills: Vec<String>,
    /// Memory scope: "none", "episodic", or "full".
    #[serde(default)]
    pub memory_scope: Option<String>,
    pub swarm_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskSpawnResponse {
    pub task_id: String,
    pub message: String,
    pub result_note_error: Option<String>,
}

// ── Delegation result (kanban-as-swarm-coordination) ──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskDelegateResultRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskDelegateResultResponse {
    pub task_id: String,
    /// Whether the task has a recorded delegation result.
    pub has_result: bool,
    pub delegate_result: Option<hkask_mcp_swarm::LocalDelegateResult>,
    pub deterministic_verdict: Option<hkask_mcp_swarm::TaskSuccessVerdict>,
    pub swarm_id: Option<String>,
}

// ── Board delete ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardDeleteRequest {
    pub board_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BoardDeleteResponse {
    pub board_id: String,
    /// Number of tasks deleted alongside the board.
    pub tasks_deleted: usize,
}

// ── Board update (rename) ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardUpdateRequest {
    pub board_id: String,
    /// New board name. Trimmed at the service boundary and required
    /// non-empty afterwards; capped at 128 characters
    /// (`KANBAN_BOARD_NAME_MAX_CHARS`) — the name is the board's addressing
    /// key (reference model R1/R6;
    /// `kask/docs/research/kanban-board-reference-models.md` §6.3, §6.6).
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BoardUpdateResponse {
    pub board_id: String,
    pub name: String,
}

// ── Task delete ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskDeleteRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskDeleteResponse {
    pub task_id: String,
}

// ── Task unassign ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskUnassignRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskUnassignResponse {
    pub task_id: String,
}

// ── Task update ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskUpdateRequest {
    pub task_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub criteria: Option<Vec<String>>,
    pub priority: Option<String>,
    pub labels: Option<Vec<String>>,
    pub advances: Option<Vec<CriterionCitationInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskUpdateResponse {
    pub task_id: String,
    pub title: String,
    /// Number of goal-criterion citations on the task after the update.
    pub advances_count: usize,
}

// ── Board export / import (mermaid) ─────────────────────────────────────────

/// Request for `kanban_board_export` — render a board as mermaid kanban
/// markdown (structure only: columns, task titles, task IDs).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BoardExportRequest {
    pub board_id: String,
}

/// Response for `kanban_board_export` — the mermaid kanban markdown plus a
/// small summary so callers can confirm the export captured the expected
/// shape without re-parsing the markdown.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BoardExportResponse {
    /// The mermaid kanban markdown. Render with `mermaid` directive `kanban`.
    pub markdown: String,
    pub board_id: String,
    pub board_name: String,
    /// Number of columns rendered.
    pub column_count: usize,
    /// Number of tasks rendered.
    pub task_count: usize,
}

/// Request for `kanban_board_import` — parse mermaid kanban markdown and
/// create a new board with tasks in the parsed columns.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardImportRequest {
    /// Current-format Mermaid kanban markdown from `kanban_board_export`.
    /// Every section must be preceded by
    /// `%% kanban column status: <wire-status>`; implicit legacy mappings are rejected.
    pub markdown: String,
    pub board_name: Option<String>,
    pub idempotency_key: Option<String>,
}

/// Response for `kanban_board_import` — the new board id and a summary of
/// what was created.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BoardImportResponse {
    pub board_id: String,
    pub board_name: String,
    /// Number of columns created on the new board.
    pub column_count: usize,
    /// Number of tasks created across all columns.
    pub task_count: usize,
}

#[cfg(test)]
mod goal_wire_form_tests {
    use super::GoalCreateRequest;

    /// The live 2026-09-27 incident: the agent emitted the intake prediction
    /// as a string three times and rmcp's macro layer rejected the whole call
    /// ("invalid type: string \"0.8\", expected f64"). The wire form is now
    /// tolerant; the 0.0–1.0 range validation stays in the service.
    #[test]
    fn goal_create_prediction_accepts_a_string_encoded_number() {
        let request: GoalCreateRequest = serde_json::from_value(serde_json::json!({
            "goal_text": "test goal",
            "criteria": [{ "description": "criterion" }],
            "prediction": "0.8"
        }))
        .expect("string-encoded prediction deserializes");
        assert_eq!(request.prediction, Some(0.8));
    }

    #[test]
    fn goal_create_prediction_accepts_a_number_unchanged() {
        let request: GoalCreateRequest = serde_json::from_value(serde_json::json!({
            "goal_text": "test goal",
            "criteria": [{ "description": "criterion" }],
            "prediction": 0.75
        }))
        .expect("numeric prediction deserializes");
        assert_eq!(request.prediction, Some(0.75));
    }

    #[test]
    fn goal_create_prediction_absent_yields_none() {
        let request: GoalCreateRequest = serde_json::from_value(serde_json::json!({
            "goal_text": "test goal",
            "criteria": [{ "description": "criterion" }]
        }))
        .expect("absent prediction deserializes");
        assert_eq!(request.prediction, None);
    }

    #[test]
    fn goal_create_prediction_non_numeric_string_is_rejected() {
        let result: Result<GoalCreateRequest, _> = serde_json::from_value(serde_json::json!({
            "goal_text": "test goal",
            "criteria": [{ "description": "criterion" }],
            "prediction": "high"
        }));
        assert!(
            result.is_err(),
            "form tolerance must not admit non-numeric strings"
        );
    }
}
