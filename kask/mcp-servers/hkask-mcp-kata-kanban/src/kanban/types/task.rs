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
    /// The agent who created this task (P12).
    pub owner: WebID,
    /// The agent assigned to work on this task (requires consent).
    pub assignee: Option<WebID>,
    /// Acceptance criteria for task completion.
    pub(crate) criteria: Vec<VerificationCriterion>,
    /// Goal criteria this task advances (documentation-grade citations —
    /// see `CriterionCitation`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) advances: Vec<CriterionCitation>,
    /// Verification result, if the task has been verified.
    pub verification: Option<Verification>,
    /// Durable Improvement Kata state for this task, once a kata cycle has
    /// been recorded (`kanban_task_kata_record`). `None` until first
    /// recorded; read back by every kata prompt so a new session continues
    /// the routine instead of restarting from zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) kata_state: Option<crate::kanban::types::TaskKataState>,
    /// Priority level.
    pub priority: Option<Priority>,
    /// Labels/tags for categorization and filtering.
    pub labels: Vec<String>,
    /// Operator-reserved marker (P4): reserved tasks are the operator's
    /// own work. `kanban_task_claim` and `kanban_task_claim_next` refuse
    /// them and the compact listing skips them with a visible count —
    /// enforced at the service seam, never by instructions alone (the
    /// kanzo `human` flag adopted structurally, per our done-gate
    /// precedent).
    #[serde(default)]
    pub reserved: bool,
    /// Task comments — mini-REPL thread for in-process communication.
    pub comments: Vec<Comment>,
    /// Deliverable links — file paths or URLs pointing to work outputs.
    pub deliverables: Vec<String>,

    /// When the task was created.
    pub created_at: DateTime<Utc>,
    /// When the task was last updated.
    pub updated_at: DateTime<Utc>,
}

impl Task {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  board_id is a valid BoardId; spec contains non-empty title; owner is valid
    /// post: returns a new Task with status=Backlog, created_at=now, updated_at=now
    pub(crate) fn new(board_id: BoardId, spec: TaskSpec, owner: WebID) -> Self {
        let TaskSpec {
            title,
            description,
            criteria,
            advances,
        } = spec;
        let now = Utc::now();
        Self {
            id: TaskId::new(),
            board_id,
            title,
            description,
            status: TaskStatus::Backlog,
            owner,
            assignee: None,
            criteria,
            advances,
            verification: None,
            kata_state: None,
            labels: Vec::new(),
            priority: None,
            reserved: false,
            comments: Vec::new(),
            deliverables: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

// ── Comment ────────────────────────────────────────────────────────────────

/// Comment — a text note appended to a task by an agent.
///
/// Forms a mini-REPL thread attached to each task: agents append notes
/// as they work, and the coordinating agent responds inline.
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
