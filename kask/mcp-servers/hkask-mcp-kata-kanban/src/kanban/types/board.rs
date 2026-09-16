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
    /// The agent who created/manages this board.
    pub owner: WebID,
    /// Columns in display order (position-sorted).
    pub columns: Vec<ColumnDef>,
    /// Project phases for work grouping and reassembly.
    pub(crate) phases: Vec<KanbanPhase>,
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
    /// pre:  status is a valid TaskStatus
    /// post: returns the ColumnDef matching the given status, if present
    pub fn column_for_status(&self, status: TaskStatus) -> Option<&ColumnDef> {
        self.columns.iter().find(|c| c.status == status)
    }

    /// Returns whether two statuses are adjacent in this board's configured
    /// column order. Custom boards may omit standard lifecycle columns.
    pub fn can_transition(&self, from: TaskStatus, target: TaskStatus) -> bool {
        let mut columns = self.columns.iter().collect::<Vec<_>>();
        columns.sort_by_key(|column| column.position);
        columns.windows(2).any(|pair| {
            let [left, right] = pair else {
                return false;
            };
            (left.status == from && right.status == target)
                || (left.status == target && right.status == from)
        })
    }
}
