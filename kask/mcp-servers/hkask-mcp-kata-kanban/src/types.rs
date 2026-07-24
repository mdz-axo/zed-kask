//! Request/response types for the kanban MCP server tools.
//!
//! Each tool has a request struct and response struct serializable
//! for MCP JSON-RPC transport.

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContractProposeExpect {
    pub board_id: String,
    /// JSON array of ExpectProposal structs from hkask-test-harness
    pub proposals_json: String,
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

// ── Spawn ───────────────────────────────────────────────────────────────────

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
