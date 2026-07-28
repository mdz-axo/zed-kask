use super::*;

// ── Task Status ────────────────────────────────────────────────────────────

/// TaskStatus — lifecycle state of a kanban task.
///
/// Column ordering is strict: transitions may only advance forward
/// or regress one step backward. Skipping columns is prohibited.
///
/// ```text
/// Backlog → Ready → InProgress → Review → Done
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    /// Task is queued, not yet ready for work.
    Backlog,
    /// Task is ready to be picked up.
    Ready,
    /// Task is actively being worked on.
    InProgress,
    /// Task is complete and awaiting review/verification.
    Review,
    /// Task has been verified and is done.
    Done,
}

impl TaskStatus {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any valid TaskStatus
    /// post: returns the string representation (lowercase)
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Backlog => "backlog",
            TaskStatus::Ready => "ready",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Review => "review",
            TaskStatus::Done => "done",
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  s is a case-insensitive string
    /// post: returns Some(TaskStatus) if valid, None otherwise
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "backlog" => Some(TaskStatus::Backlog),
            "ready" => Some(TaskStatus::Ready),
            "in_progress" | "inprogress" | "in-progress" => Some(TaskStatus::InProgress),
            "review" => Some(TaskStatus::Review),
            "done" => Some(TaskStatus::Done),
            _ => None,
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any valid TaskStatus
    /// post: returns true iff the transition from self to `target` is valid
    ///       (forward one step, or backward one step — no skipping)
    pub fn can_transition_to(&self, target: TaskStatus) -> bool {
        use TaskStatus::*;
        matches!(
            (self, target),
            (Backlog, Ready)
                | (Ready, Backlog)
                | (Ready, InProgress)
                | (InProgress, Ready)
                | (InProgress, Review)
                | (Review, InProgress)
                | (Review, Done)
        )
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any valid TaskStatus
    /// post: returns the next status in the workflow, or None if already Done
    pub fn next(&self) -> Option<TaskStatus> {
        match self {
            TaskStatus::Backlog => Some(TaskStatus::Ready),
            TaskStatus::Ready => Some(TaskStatus::InProgress),
            TaskStatus::InProgress => Some(TaskStatus::Review),
            TaskStatus::Review => Some(TaskStatus::Done),
            TaskStatus::Done => None,
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_str(s).ok_or_else(|| format!("invalid TaskStatus: {s}"))
    }
}
