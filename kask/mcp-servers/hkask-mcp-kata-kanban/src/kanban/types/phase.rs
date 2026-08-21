use super::*;

// ── Kanban phase ───────────────────────────────────────────────────────────

/// Kanban phase — a grouping category for tasks within a board.
///
/// Phases group work for reassembly: when all tasks in a phase complete,
/// their deliverables can be composed into a coherent output.
/// Unlike columns (which track workflow state), phases track project structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct KanbanPhase {
    pub id: PhaseId,
    pub name: String,
    pub description: Option<String>,
    /// Display order (0-based).
    pub order: u32,
    /// When the phase was created.
    pub created_at: DateTime<Utc>,
}
