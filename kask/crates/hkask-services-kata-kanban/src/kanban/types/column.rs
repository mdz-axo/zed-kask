use super::*;

// ── Column Definition ──────────────────────────────────────────────────────

/// ColumnDef — definition of a column on a board.
///
/// Each column maps to a `TaskStatus`. The column ordering on a board
/// determines the valid state transitions.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnDef {
    /// Unique identifier for this column.
    pub id: ColumnId,
    /// Display name (e.g., "Backlog", "In Progress").
    pub name: String,
    /// The task status this column represents.
    pub status: TaskStatus,
    /// Position in the column ordering (0-based).
    pub position: u32,
    /// Optional WIP limit — maximum tasks allowed in this column.
    /// None means no limit. Per Anderson: WIP limits are the core
    /// mechanism that exposes system problems and stimulates collaboration.
    pub wip_limit: Option<u32>,
}

impl ColumnDef {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  name is non-empty; status is valid; position is >= 0
    /// post: returns a new ColumnDef with a random ColumnId, no WIP limit
    pub fn new(name: String, status: TaskStatus, position: u32) -> Self {
        Self {
            id: ColumnId::new(),
            name,
            status,
            position,
            wip_limit: None,
        }
    }

    pub fn with_wip_limit(mut self, limit: u32) -> Self {
        self.wip_limit = Some(limit);
        self
    }
}
