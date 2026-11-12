//! Request/response types for the kanban MCP server tools.
//!
//! Each tool has a request struct and response struct serializable
//! for MCP JSON-RPC transport.

use schemars::{JsonSchema, json_schema};
use serde::{Deserialize, Serialize};

// ── Closed vocabularies ────────────────────────────────────────────────────
//
// These fields stay strings on the wire: padding, wrapping quotes and case
// are tolerated when the value is parsed, and a miss is refused naming the
// set. The schema names the accepted values so a caller sees them before the
// call instead of learning them from a refusal (mcp-tool-review KK-21).

/// The kata stages `kanban_task_kata_prompt` accepts.
pub(crate) const KATA_STAGES: [&str; 3] = ["coaching", "improvement", "practice"];
/// The listing views `kanban_task_list` accepts.
pub(crate) const TASK_VIEWS: [&str; 2] = ["full", "compact"];

fn task_status_names() -> Vec<&'static str> {
    crate::TaskStatus::STANDARD_ORDER
        .iter()
        .map(crate::TaskStatus::as_str)
        .collect()
}

fn task_status_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    json_schema!({"type": "string", "enum": task_status_names()})
}

fn optional_task_status_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    json_schema!({"anyOf": [{"type": "string", "enum": task_status_names()}, {"type": "null"}]})
}

fn task_view_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    json_schema!({"anyOf": [{"type": "string", "enum": TASK_VIEWS}, {"type": "null"}]})
}

/// The priorities, plus the empty string that clears one.
fn priority_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut names: Vec<&str> = crate::Priority::ALL
        .iter()
        .map(crate::Priority::as_str)
        .collect();
    names.push("");
    json_schema!({"anyOf": [{"type": "string", "enum": names}, {"type": "null"}]})
}

fn kata_stage_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    json_schema!({"type": "string", "enum": KATA_STAGES})
}

fn goal_verdict_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let names: Vec<&str> = crate::GoalVerdictValue::ALL
        .iter()
        .map(crate::GoalVerdictValue::as_str)
        .collect();
    json_schema!({"type": "string", "enum": names})
}

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
    /// Structural Done-gate policy: when `true`, the Done column is
    /// reachable only through `kanban_task_verify` (Review → Done), never
    /// an ordinary column move. Omitted/false except for boards created
    /// with the reserved "Algedonic review" name, which are stamped by
    /// convention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_requires_verification: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ColumnDefInput {
    pub name: String,
    #[schemars(schema_with = "task_status_schema")]
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

/// One of the caller's board rows that could not be decoded — surfaced
/// degradation, never a silent skip (the task listing's rule, applied to
/// boards).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UndecodableBoardInfo {
    /// The board id whose row could not be decoded.
    pub board_id: String,
    /// The decode error.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardListResponse {
    pub boards: Vec<BoardInfo>,
    /// The caller's board rows that could not be decoded — the listing
    /// degrades visibly instead of shrinking.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undecodable_boards: Vec<UndecodableBoardInfo>,
    /// The per-database change counter (P1, the kanzo changes-counter
    /// pattern): a monotonic stamp moved by every write. A poller that
    /// remembers the last value can skip re-fetching derived state until
    /// it moves. Defaults to 0 for pre-P1 servers — a poller comparing
    /// against it over-fetches, which is the pre-P1 behavior, never a
    /// missed change.
    #[serde(default)]
    pub changes: u64,
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
    /// True when Done is reachable only through `kanban_task_verify` on
    /// this board; an ordinary move to Done is refused.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub done_requires_verification: bool,
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
#[serde(deny_unknown_fields)]
pub struct GoalCreateRequest {
    /// The functional goal in the user's words — what the user will be able
    /// to do, or what stops being a problem. The agent interprets this; it
    /// never revises it.
    pub goal_text: String,
    /// 1–4 observable criteria (Fermi-decomposed from the goal).
    pub criteria: Vec<GoalCriterionInput>,
    /// Source locator for the immutable specification, not a human receipt.
    pub source: crate::GoalSource,
    /// Optional prospectively frozen forecast. No forecast is synthesized.
    pub forecast: Option<crate::GoalForecast>,
    pub supersedes: Option<String>,
    pub task_id: Option<String>,
    pub idempotency_key: Option<String>,
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
    #[schemars(schema_with = "goal_verdict_schema")]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalGetRequest {
    pub goal_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoalListRequest {}

// ── Task tools ─────────────────────────────────────────────────────────────

/// One goal-criterion citation for `kanban_task_create`'s `advances`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CriterionCitationInput {
    /// The goal being cited.
    pub goal_id: String,
    /// Index into the goal's criteria (0-based).
    pub criterion_index: usize,
    /// The criterion's description, verbatim — validated against the immutable
    /// specification. Canonical history remains available after closure.
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
    #[schemars(schema_with = "optional_task_status_schema")]
    pub status: Option<String>,
    /// Response view (P2): omit (or "full") for the complete `TaskInfo`
    /// objects — the panel's shape; "compact" returns token-frugal
    /// one-struct summaries with Done capped and operator-reserved tasks
    /// skipped, both disclosed by count (the kanzo agent-channel pattern).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "task_view_schema")]
    pub view: Option<String>,
}

/// One indexed task row that could not be read for a listing — surfaced
/// degradation, never a silent skip. These rows are excluded from `tasks`
/// and from the server's WIP counts (which fail closed on them instead of
/// admitting against a count known to be a lower bound).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UndecodableTaskInfo {
    /// The indexed task id whose row could not be read.
    pub task_id: String,
    /// Why the row could not be read (missing row, or the decode error).
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskListResponse {
    pub tasks: Vec<TaskInfo>,
    /// Indexed task rows that could not be read — the board listing
    /// degrades visibly instead of shrinking.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undecodable_tasks: Vec<UndecodableTaskInfo>,
    /// The compact view's task summaries (P2) — present only when the
    /// request set `view: "compact"` (then `tasks` is empty). One struct
    /// per task carrying the dispatch identity and state, not the full
    /// work context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compact_tasks: Vec<CompactTaskInfo>,
    /// Done tasks hidden by the compact view's cap ("an old board has
    /// thousands of done tasks" — the kanzo pattern). Explicitly
    /// disclosed: a truncated listing never reads as complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_not_shown: Option<usize>,
    /// Operator-reserved tasks skipped by the compact view (P4) — the
    /// boundary is disclosed with a count, never silent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_not_shown: Option<usize>,
}

/// One task summary in the compact listing view (P2) — the token-frugal
/// agent read (the kanzo one-line pattern, adapted to a JSON struct).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompactTaskInfo {
    pub task_id: String,
    pub title: String,
    pub status: String,
    /// The assigned agent, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    /// Operator-reserved tasks are normally skipped in this view; the
    /// marker stays for the direct read paths so a reserved task is
    /// recognizable wherever it appears.
    #[serde(default)]
    pub reserved: bool,
}

/// The latest recorded activity on a task — a one-line status the kanban
/// widget renders on the card. Derived from the task's most recent
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
    /// Number of goal-criterion citations on the task.
    pub advances_count: usize,
    pub activity: Option<TaskActivity>,
    /// Operator-reserved marker (P4) — reserved tasks are the operator's
    /// own work; claim tools refuse them and the compact listing skips
    /// them with a disclosed count.
    #[serde(default)]
    pub reserved: bool,
    /// The task's priority, when one is set. `kanban_task_update` writes it;
    /// this is its read path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    /// The task's labels (`kanban_task_update` writes them).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// Deliverables attached with `kanban_task_add_deliverable`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deliverables: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskMoveRequest {
    pub task_id: String,
    #[schemars(schema_with = "task_status_schema")]
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
    /// Total comment count on the task — the cursor a poller passes as the
    /// next `since_index`. Independent of the requested index.
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

// ── Kata prompts ───────────────────────────────────────────────────────

/// One PDCA experiment record: the next step taken, the prediction it
/// tested, and the observed outcome once checked (`None` until checked —
/// never filled from a plan).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KataExperimentInfo {
    /// The next step taken (one change at a time).
    pub next_step: String,
    /// The prediction the step tests — what you expect to learn.
    pub prediction: String,
    /// The observed outcome once checked; `None` until checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

/// Request for `kanban_task_kata_record` — record durable Improvement Kata
/// state on a task. At least one field is required; a first record on a
/// task with no kata state requires `target_condition` (the kata's step-3
/// artifact cannot be skipped).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskKataRecordRequest {
    pub task_id: String,
    /// Set the target condition (the measurable target the routine
    /// converges toward).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_condition: Option<String>,
    /// The ONE obstacle currently being worked: omit the field to keep the
    /// recorded obstacle, send `null` to clear it, send a string to set it.
    #[serde(
        default,
        deserialize_with = "present_as_some",
        skip_serializing_if = "Option::is_none"
    )]
    pub focus_obstacle: Option<Option<String>>,
    /// Replace the last experiment record (`None` keeps the existing one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_experiment: Option<KataExperimentInfo>,
}

/// Deserialize a field that is present as `Some(value)`, so an explicit JSON
/// `null` reaches the handler as `Some(None)`. serde's own `Option<Option<T>>`
/// reads `null` as the outer `None` — the same as an absent field — which
/// made "null to clear" unreachable over the wire (mcp-tool-review KK-01).
fn present_as_some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Response for `kanban_task_kata_record` — the state now recorded on the
/// task, echoed back.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskKataRecordResponse {
    pub task_id: String,
    /// The recorded target condition.
    pub target_condition: String,
    /// The recorded focus obstacle, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_obstacle: Option<String>,
    /// The recorded last experiment, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_experiment: Option<KataExperimentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskKataPromptRequest {
    pub task_id: String,
    /// Which kata to generate: "coaching" (5-question Coaching Kata
    /// dialogue), "improvement" (Improvement Kata PDCA cycle), or
    /// "practice" (Starter Kata observation drill — requires sub_problem).
    #[schemars(schema_with = "kata_stage_schema")]
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

/// Request for `kanban_task_spawn`. Unknown fields are refused: the former
/// `delegation_level` and `memory_scope` were accepted, validated and had no
/// effect on the child (mcp-tool-review KK-18), so a caller still sending
/// them is told rather than silently ignored.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskSpawnRequest {
    pub task_id: String,
    pub idempotency_key: Option<String>,
    /// Skills named in the child's prompt (e.g. ["bug-hunt", "tdd"]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegated_skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskSpawnResponse {
    pub task_id: String,
    pub message: String,
    pub result_note_error: Option<String>,
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
pub struct TaskDeleteRequest {
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
pub struct TaskUpdateRequest {
    pub task_id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub criteria: Option<Vec<String>>,
    #[schemars(schema_with = "priority_schema")]
    pub priority: Option<String>,
    pub labels: Option<Vec<String>>,
    pub advances: Option<Vec<CriterionCitationInput>>,
    /// Set/clear the operator-reserved marker (P4): `true` reserves the
    /// task for the operator (claim tools refuse it; compact listings skip
    /// it with a disclosed count); `false` releases it. Omitted keeps the
    /// current value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TaskUpdateResponse {
    pub task_id: String,
    pub title: String,
    /// Number of goal-criterion citations on the task after the update.
    pub advances_count: usize,
}

// ── Claim next (the pull primitive, P3) ──────────────────────────────────

/// Request for `kanban_task_claim_next` — pull the next claimable Ready
/// task: one transaction assigns the caller and moves the task to
/// In Progress, honoring the In Progress WIP limit and skipping
/// operator-reserved tasks (the count is disclosed in the response).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClaimNextRequest {
    pub board_id: String,
    /// Replay protection: a retried call with the same key returns the
    /// originally claimed task instead of claiming a second one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// Response for `kanban_task_claim_next` — the claimed task plus a short
/// plain-text outcome (the kanzo token-frugality pattern), which discloses
/// the reserved-skip count when nothing was claimable.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ClaimNextResponse {
    pub board_id: String,
    /// The claimed task — `None` when no claimable Ready task remains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskInfo>,
    /// The claim outcome in plain text.
    pub message: String,
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
    /// Indexed task rows that could not be read and are therefore absent
    /// from the markdown — the export's silence about them is surfaced.
    pub undecodable_count: usize,
}

/// Request for `kanban_board_import` — parse mermaid kanban markdown and
/// create a new board with tasks in the parsed columns.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardImportRequest {
    /// Current-format Mermaid kanban markdown from `kanban_board_export`.
    /// Every section must be preceded by
    /// `%% kanban column status: <wire-status>`; implicit legacy mappings are rejected.
    /// An optional `%% kanban column wip: <limit>` line before a section
    /// sets that column's WIP limit.
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
