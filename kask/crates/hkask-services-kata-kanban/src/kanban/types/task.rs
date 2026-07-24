use super::*;

/// Task — a single work item on a kanban board.
///
/// Every task carries `owner: WebID` (P12) — the creator of the task.
/// Assignment is separate and requires agent consent (P1).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// Unique task identifier.
    pub id: TaskId,
    /// The board this task belongs to.
    pub board_id: BoardId,
    /// Short title.
    pub title: String,
    /// Optional longer description.
    pub description: Option<String>,
    /// Current status (determines which column the task is in).
    pub status: TaskStatus,
    /// The userpod who created this task (P12).
    pub owner: WebID,
    /// The agent assigned to work on this task (requires consent).
    pub assignee: Option<WebID>,
    /// Acceptance criteria for task completion.
    pub criteria: Vec<VerificationCriterion>,
    /// Verification result, if the task has been verified.
    pub verification: Option<Verification>,
    /// Story points (relative sizing, agile convention).
    pub story_points: Option<u32>,
    /// Estimated hours for completion.
    pub estimated_hours: Option<f64>,
    /// Priority level.
    pub priority: Option<Priority>,
    /// Labels/tags for categorization and filtering.
    pub labels: Vec<String>,
    /// Task comments — mini-REPL thread for in-process communication.
    pub comments: Vec<Comment>,
    /// Deliverable links — file paths or URLs pointing to work outputs.
    pub deliverables: Vec<String>,
    /// Optional phase grouping for work reassembly.
    pub phase_id: Option<PhaseId>,
    /// When the task was created.
    pub created_at: DateTime<Utc>,
    /// When the task was last updated.
    pub updated_at: DateTime<Utc>,
    /// Gas/rJoules remaining in the subagent's budget for this task.
    /// Initialized from `TaskSpec.gas_budget`. When this hits 0, the task
    /// auto-completes via the gas exhaustion completion path.
    pub gas_remaining: Option<u64>,
    /// rJoules remaining for inference/API calls (250k ≈ $1 spend).
    pub rjoule_remaining: Option<u64>,
    /// Audit trail of gas and rJoule consumption/refills.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gas_spend: Vec<GasEntry>,
}

impl Task {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  board_id is a valid BoardId; spec contains non-empty title; owner is valid
    /// post: returns a new Task with status=Backlog, created_at=now, updated_at=now
    pub fn new(board_id: BoardId, spec: TaskSpec, owner: WebID) -> Self {
        let now = Utc::now();
        Self {
            id: TaskId::new(),
            board_id,
            title: spec.title,
            description: spec.description,
            status: TaskStatus::Backlog,
            owner,
            assignee: None,
            criteria: spec.criteria,
            verification: None,
            story_points: None,
            estimated_hours: None,
            labels: Vec::new(),
            priority: None,
            comments: Vec::new(),
            deliverables: Vec::new(),
            phase_id: None,
            created_at: now,
            updated_at: now,
            gas_remaining: spec.gas_budget,
            rjoule_remaining: spec.rjoule_budget,
            gas_spend: Vec::new(),
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  target is a valid transition from self.status
    /// post: returns true iff self.status.can_transition_to(target)
    pub fn can_move_to(&self, target: TaskStatus) -> bool {
        self.status.can_transition_to(target)
    }
}

// ── Comment ────────────────────────────────────────────────────────────────

/// Comment — a text note appended to a task by an agent.
///
/// Forms a mini-REPL thread attached to each task: agents append notes
/// as they work, and the coordinating userpod responds inline.
/// Every comment carries `author: WebID` (P12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: CommentId,
    pub task_id: TaskId,
    pub author: WebID,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

impl Comment {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  task_id is valid; author is a valid WebID; body is non-empty
    /// post: returns a new Comment with created_at=now
    pub(crate) fn new(task_id: TaskId, author: WebID, body: String) -> Self {
        Self {
            id: CommentId::new(),
            task_id,
            author,
            body,
            created_at: Utc::now(),
        }
    }
}

// ── Filter ─────────────────────────────────────────────────────────────────

/// TaskFilter — criteria for listing/filtering tasks on a board.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskFilter {
    /// Filter by status (column).
    pub status: Option<TaskStatus>,
    /// Filter by assignee.
    pub assignee: Option<WebID>,
    /// Filter by priority.
    pub priority: Option<Priority>,
    /// Limit the number of results.
    pub limit: Option<usize>,
}

impl TaskFilter {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns an empty filter (matches all tasks)
    pub fn all() -> Self {
        Self {
            status: None,
            assignee: None,
            priority: None,
            limit: None,
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  arguments are valid
    /// post: returns expected result
    /// pre:  status is a valid TaskStatus
    /// post: returns a filter matching only tasks with the given status
    pub fn by_status(status: TaskStatus) -> Self {
        Self {
            status: Some(status),
            assignee: None,
            priority: None,
            limit: None,
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  assignee is a valid WebID
    /// post: returns a filter matching only tasks assigned to the given agent
    pub fn by_assignee(assignee: WebID) -> Self {
        Self {
            status: None,
            assignee: Some(assignee),
            priority: None,
            limit: None,
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  priority is a valid Priority
    /// post: returns a filter matching only tasks with the given priority
    pub fn by_priority(priority: Priority) -> Self {
        Self {
            status: None,
            assignee: None,
            priority: Some(priority),
            limit: None,
        }
    }
}
