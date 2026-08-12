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
    /// Ontology concept: <https://w3id.org/pko#Step>
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_remaining: Option<u64>,
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
pub struct TaskMoveResponse {
    pub task_id: String,
    pub previous_status: String,
    pub new_status: String,
    /// Ontology concept: <https://w3id.org/pko#ChangeOfStatus>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAssignRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskAssignResponse {
    pub task_id: String,
    pub assignee: String,
    /// Ontology concept: <https://www.w3.org/ns/prov#wasAssociatedWith>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Ontology concept: <https://w3id.org/pko#StepVerification>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Ontology concept: <https://www.w3.org/ns/prov#used>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Ontology concept: <https://www.w3.org/ns/prov#used>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Ontology concept: <https://w3id.org/pko#UserFeedbackOccurrence>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Ontology concept: <https://www.w3.org/ns/prov#wasGeneratedBy>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Ontology concept: <https://w3id.org/pko#ChangeOfStatus>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// Ontology concept: <https://w3id.org/pko#UserQuestionOccurrence>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
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
    /// The swarm this task belongs to, when the task is coordinated via a
    /// local swarm. Written to `Task.swarm_id` by `KanbanService::spawn_task`
    /// so `kanban_task_delegate_result` returns the durable link. `None` when
    /// the spawn is not scoped to a swarm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swarm_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskSpawnResponse {
    pub task_id: String,
    pub message: String,
    /// Ontology concept: <https://w3id.org/pko#StepExecution>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Delegation result (kanban-as-swarm-coordination) ──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskDelegateResultRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskDelegateResultResponse {
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
pub struct BoardDeleteRequest {
    pub board_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoardDeleteResponse {
    pub board_id: String,
    /// Number of tasks deleted alongside the board.
    pub tasks_deleted: usize,
    /// Ontology concept: <https://w3id.org/pko#Procedure>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Task delete ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskDeleteRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskDeleteResponse {
    pub task_id: String,
    /// Ontology concept: <https://w3id.org/pko#Step>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Task unassign ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskUnassignRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskUnassignResponse {
    pub task_id: String,
    /// Ontology concept: <https://w3id.org/pko#Step>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}

// ── Task update ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskUpdateRequest {
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskUpdateResponse {
    pub task_id: String,
    pub title: String,
    /// Ontology concept: <https://w3id.org/pko#Step>
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<String>,
}
