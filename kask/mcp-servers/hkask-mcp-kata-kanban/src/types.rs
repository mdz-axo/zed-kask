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
    /// Opaque client-generated key making this create replay-safe.
    ///
    /// A caller that retries after a lost connection sends the *same* key, and
    /// the server returns the original response instead of creating a second
    /// board. Optional and `#[serde(default)]` so existing callers are
    /// unaffected — they simply get no replay protection. See
    /// `crate::idempotency`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
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
pub(crate) struct BoardCreateResponse {
    pub board_id: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    /// Ontology concept: <https://w3id.org/pko#Procedure>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ColumnInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    /// Optional WIP limit — maximum tasks allowed in this column.
    /// None means no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// achieved. Brier-scored at `kanban_goal_score`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prediction: Option<f64>,
    /// Optional link to the kanban task executing this goal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// Opaque client-generated key making this create replay-safe. See
    /// [`BoardCreateRequest::idempotency_key`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GoalCreateResponse {
    pub goal_id: String,
    pub goal_text: String,
    pub criteria_count: usize,
    pub prediction: Option<f64>,
    /// Ontology concept emitted on the response: `pplan:Step` (P-Plan,
    /// soft-reused by PKO) via `kanban_type_to_pko("Goal")`. Operator
    /// decision 2026-08-30: goals anchor on the PKO family — boards =
    /// `pko:Procedure`, tasks and goals = `pplan:Step`, verdicts =
    /// `pko:StepVerification` — so the whole kanban graph is one linked
    /// dataset in a published ontology. The goal's h_mem record anchors on
    /// the same term, so the wire surface and the stored record agree.
    /// (PKO publishes no Goal class — the former `pko:Goal` was fabricated;
    /// the interim IAO:0000005 anchor was rejected as opaque.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Ontology concept: `pko:StepVerification` (judging a goal's criteria
    /// against the realized outcome is a verification occurrence).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Names the missing-prediction case so the caller can distinguish
    /// "not computable" from "perfectly calibrated".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Ontology concept: `pko:StepVerification` (scoring a goal's realized
    /// outcome is a verification occurrence).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    pub criteria_count: usize,
    pub prediction: Option<f64>,
    /// Latest verdict ("done" | "continue" | "blocked"), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_verdict: Option<String>,
    /// Resolution state: "achieved" | "not-achieved" | null (unresolved).
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// ephemeral goal is gone.
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
    /// Opaque client-generated key making this create replay-safe. See
    /// [`BoardCreateRequest::idempotency_key`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,

    /// Inference/API rJoule budget (250k ≈ $1 spend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rjoule_budget: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskCreateResponse {
    pub task_id: String,
    pub board_id: String,
    pub title: String,
    pub status: String,
    /// Number of goal-criterion citations recorded on the task.
    pub advances_count: usize,
    /// Ontology concept: `pplan:Step` (P-Plan, soft-reused by PKO — PKO
    /// publishes no Step class of its own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    pub status: String,
    pub assignee: Option<String>,
    pub criteria_count: usize,
    /// Number of goal-criterion citations on the task.
    pub advances_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rjoule_remaining: Option<u64>,
    /// The swarm this task belongs to, when coordinated via a local swarm.
    /// Mirrors `Task.swarm_id` so the kanban widget can render a visible
    /// swarm↔kanban link on the card (R1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swarm_id: Option<String>,
    /// The latest recorded activity on this task (R3). `None` when the task
    /// has no comments yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Ontology concept: <https://w3id.org/pko#ChangeOfStatus>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
    /// The new status mapped to its PKO execution-status individual (the
    /// execution axis: pko:InProgress|Completed|Paused — PKO v2.0.0's
    /// published individuals). Statuses PKO publishes no individual for
    /// (todo/backlog/ready/review) omit the field rather than force a
    /// nonexistent status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Ontology concept: <https://www.w3.org/ns/prov#wasAssociatedWith>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskVerifyRequest {
    pub task_id: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskVerifyResponse {
    pub task_id: String,
    pub passed: bool,
    pub reasoning: String,
    pub new_status: String,
    /// Ontology concept: <https://w3id.org/pko#StepVerification>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskAddRjoulesRequest {
    pub task_id: String,
    /// Amount of rJoules to add to the inference/API budget.
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskAddRjoulesResponse {
    pub task_id: String,
    pub new_rjoule_remaining: u64,
    /// Ontology concept: <https://www.w3.org/ns/prov#used>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Comments ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskCommentRequest {
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
    /// Ontology concept: <https://w3id.org/pko#UserFeedbackOccurrence>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskCommentsSinceRequest {
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
    /// Ontology concept: <https://www.w3.org/ns/prov#wasGeneratedBy>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Reopen ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskReopenRequest {
    pub task_id: String,
    /// Optional new rJoule budget to grant on reopen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rjoule_budget: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskReopenResponse {
    pub task_id: String,
    pub new_status: String,
    pub rjoule_remaining: Option<u64>,
    /// Ontology concept: <https://w3id.org/pko#ChangeOfStatus>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
pub(crate) struct TaskKataCoachingRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskKataImprovementRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskKataPracticeRequest {
    pub task_id: String,
    /// What specific sub-problem to focus the observation drill on.
    pub sub_problem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskKataResponse {
    pub task_id: String,
    pub prompt: String,
    /// Ontology concept: <https://w3id.org/pko#UserQuestionOccurrence>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Spawn ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskSpawnRequest {
    pub task_id: String,
    /// Opaque client-generated key making this spawn replay-safe.
    ///
    /// Load-bearing here beyond duplicate rows: a spawn burns rJoules and starts a
    /// subagent, so a blind retry costs real budget. See
    /// [`BoardCreateRequest::idempotency_key`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Delegation level: "minimal", "standard", or "maximal".
    pub delegation_level: String,
    /// Skills to delegate (e.g. ["bug-hunt", "tdd"]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegated_skills: Vec<String>,
    /// Memory scope: "none", "episodic", or "full".
    #[serde(default)]
    pub memory_scope: Option<String>,
    /// rJoule budget to grant on spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rjoule_budget: Option<u64>,
    /// The swarm this task belongs to, when the task is coordinated via a
    /// local swarm. Written to `Task.swarm_id` by `KanbanService::spawn_task`
    /// so `kanban_task_delegate_result` returns the durable link. `None` when
    /// the spawn is not scoped to a swarm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swarm_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskSpawnResponse {
    pub task_id: String,
    pub message: String,
    /// Ontology concept: <https://w3id.org/pko#StepExecution>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// The structured delegation result, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegate_result: Option<hkask_mcp_swarm::LocalDelegateResult>,
    /// The deterministic task-success verdict, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deterministic_verdict: Option<hkask_mcp_swarm::TaskSuccessVerdict>,
    /// The swarm this task belongs to, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swarm_id: Option<String>,
    /// Ontology concept: <https://w3id.org/pko#StepExecution>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Board delete ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BoardDeleteRequest {
    pub board_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BoardDeleteResponse {
    pub board_id: String,
    /// Number of tasks deleted alongside the board.
    pub tasks_deleted: usize,
    /// Ontology concept: <https://w3id.org/pko#Procedure>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Task delete ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskDeleteRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskDeleteResponse {
    pub task_id: String,
    /// Ontology concept: `pplan:Step` (P-Plan, soft-reused by PKO — PKO
    /// publishes no Step class of its own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Task unassign ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskUnassignRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskUnassignResponse {
    pub task_id: String,
    /// Ontology concept: `pplan:Step` (P-Plan, soft-reused by PKO — PKO
    /// publishes no Step class of its own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Task update ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskUpdateRequest {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    /// Goal criteria this task advances — replaces the existing citations
    /// when present. Each citation is validated against the cited goal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advances: Option<Vec<CriterionCitationInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskUpdateResponse {
    pub task_id: String,
    pub title: String,
    /// Number of goal-criterion citations on the task after the update.
    pub advances_count: usize,
    /// Ontology concept: `pplan:Step` (P-Plan, soft-reused by PKO — PKO
    /// publishes no Step class of its own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Ontology concept: <https://w3id.org/pko#Procedure>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

/// Request for `kanban_board_import` — parse mermaid kanban markdown and
/// create a new board with tasks in the parsed columns.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BoardImportRequest {
    /// Mermaid kanban markdown (the output of `kanban_board_export`).
    pub markdown: String,
    /// Optional override for the board name. When `None`, the name parsed
    /// from the `%% kanban board: <name>` comment is used; when the markdown
    /// has no name comment either, the board is named `"Imported Board"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_name: Option<String>,
    /// Opaque client-generated key making this import replay-safe. A caller
    /// that retries after a lost connection sends the *same* key, and the
    /// server returns the original response instead of creating a second
    /// board. See `crate::idempotency`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Ontology concept: <https://w3id.org/pko#Procedure>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}
