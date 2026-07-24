use super::*;

// ── Board ──────────────────────────────────────────────────────────────────

/// Board — a kanban board containing columns and tasks.
///
/// Every board carries an `owner: WebID` for P12 compliance.
/// Boards are isolated — only members (agents assigned to the board)
/// can view or modify its contents.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    /// Unique board identifier.
    pub id: BoardId,
    /// Human-readable board name.
    pub name: String,
    /// The userpod who created/manages this board.
    pub owner: WebID,
    /// Columns in display order (position-sorted).
    pub columns: Vec<ColumnDef>,
    /// Project phases for work grouping and reassembly.
    pub phases: Vec<KanbanPhase>,
    /// When the board was created.
    pub created_at: DateTime<Utc>,
}

impl Board {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  name is non-empty; owner is a valid WebID; columns is non-empty
    /// post: returns a new Board with created_at=now and a random BoardId
    pub fn new(name: String, owner: WebID, columns: Vec<ColumnDef>) -> Self {
        Self {
            id: BoardId::new(),
            name,
            owner,
            columns,
            phases: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self.columns is sorted by position
    /// post: returns the first column (by position) — typically Backlog
    pub fn first_column(&self) -> Option<&ColumnDef> {
        self.columns.first()
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self.columns is sorted by position
    /// post: returns the last column (by position) — typically Done
    pub fn last_column(&self) -> Option<&ColumnDef> {
        self.columns.last()
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  status is a valid TaskStatus
    /// post: returns the ColumnDef matching the given status, if present
    pub fn column_for_status(&self, status: TaskStatus) -> Option<&ColumnDef> {
        self.columns.iter().find(|c| c.status == status)
    }
}
