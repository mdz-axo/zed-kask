//! KanbanService — core kanban board and task coordination.
//!
//! Implements kanban board and task coordination operations.
//! Every operation carries ownership tracking (P12) and enforces agent consent
//! on assignment (P1). State transitions are column-ordered.
//!
//! Persistence: boards and tasks stored as RDF h_mems via HMemStore (MDS §2).
//! HMem scheme:
//!   kanban:board → {board_id} → JSON Board
//!   kanban:task  → {task_id}  → JSON Task
//!   kanban:board_tasks:{board_id} → {task_id} → task_id (index)

use hkask_storage::{HMem, HMemStore};
use hkask_types::Dimension;
use hkask_types::HMemOntology;
use hkask_types::NotFound;
use hkask_types::WebID;
use hkask_types::id::{BoardId, HMemId, TaskId};
use hkask_types::kanban_wire::KANBAN_BOARD_NAME_MAX_CHARS;
use serde_json::Value;

use super::types::KanbanError;

use crate::kanban::mermaid::{ParsedBoard, columns_from_parsed};
use crate::kanban::{
    Board, ColumnDef, CriterionCitation, Priority, Task, TaskFilter, TaskSpec, TaskStatus,
    Verification, VerificationCriterion,
};

/// Core kanban coordination service.
///
/// Persists boards, tasks, and goals as RDF h_mems in a HMemStore.
/// Public surface: board and task coordination operations.
///
/// `Clone` is required because the REPL caches the service in `ReplState`
/// and clones it per-command to avoid holding a borrow across the REPL loop.
#[derive(Clone)]
pub struct KanbanService {
    pub(crate) store: HMemStore,
    /// One-shot fault injection for the replay-protection integration
    /// suite: the next N `task_comment` calls fail instead of writing
    /// (`#[doc(hidden)]` arm below). Arc-shared across clones so a test can
    /// arm the fault on the service it keeps while the server holds a
    /// clone. Zero in production — nothing reads it except `task_comment`.
    pub(crate) comment_faults: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

// HMem entity prefixes
const BOARD_ENTITY: &str = "kanban:board";
const TASK_ENTITY: &str = "kanban:task";
const BOARD_TASKS_PREFIX: &str = "kanban:board_tasks:";

impl KanbanService {
    /// Create a KanbanService backed by the given HMemStore.
    ///
    /// pre:  store must have the h_mems table initialized
    /// post: returns a KanbanService ready for use
    #[must_use]
    pub fn new(store: HMemStore) -> Self {
        Self {
            store,
            comment_faults: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// Arm a one-shot `task_comment` fault for the replay-protection
    /// integration suite: the next `fail_next` comment writes fail with a
    /// typed error instead of reaching the store. `#[doc(hidden)]` because
    /// this exists for `tests/idempotent_creates.rs`, not for downstream
    /// consumers — production callers never touch it.
    #[doc(hidden)]
    pub fn fail_next_comments(&self, fail_next: usize) {
        self.comment_faults
            .store(fail_next, std::sync::atomic::Ordering::SeqCst);
    }

    /// Authority check: the actor must be the task owner or assignee.
    ///
    /// pre:  task refers to an existing task
    /// post: returns Ok iff actor is the task owner or the current assignee
    pub(super) fn require_task_actor(task: &Task, actor: WebID) -> Result<(), KanbanError> {
        if task.owner == actor || task.assignee == Some(actor) {
            Ok(())
        } else {
            Err(KanbanError::PermissionDenied(format!(
                "actor {actor} is not the task owner or assignee"
            )))
        }
    }

    pub(super) fn require_task_owner(task: &Task, actor: WebID) -> Result<(), KanbanError> {
        if task.owner == actor {
            Ok(())
        } else {
            Err(KanbanError::PermissionDenied(format!(
                "actor {actor} does not own task {}",
                task.id
            )))
        }
    }

    /// Fetch a task by id or return `KanbanError::NotFound`.
    ///
    /// Replaces the repeated `self.task_get(id)?.ok_or_else(|| KanbanError::NotFound(...))`
    /// preamble across the service_impl submodules.
    pub(super) fn require_task(&self, task_id: TaskId) -> Result<Task, KanbanError> {
        self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })
    }

    // ── Board operations ──────────────────────────────────────────────────

    /// Validate and normalize a board name. The service boundary is the
    /// single enforcement point for every caller (panel, MCP agents,
    /// import): names are trimmed, must be non-empty afterwards, and are
    /// capped at [`KANBAN_BOARD_NAME_MAX_CHARS`] characters (operator
    /// decision 2026-09-18, Planka-aligned — see
    /// `kask/docs/research/kanban-board-reference-models.md` §6.6). Returns
    /// the trimmed name.
    fn validate_board_name(name: &str) -> Result<&str, KanbanError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(KanbanError::InvalidInput("board name is empty".into()));
        }
        if trimmed.chars().count() > KANBAN_BOARD_NAME_MAX_CHARS {
            return Err(KanbanError::InvalidInput(format!(
                "board name is longer than {KANBAN_BOARD_NAME_MAX_CHARS} characters ({} given)",
                trimmed.chars().count()
            )));
        }
        Ok(trimmed)
    }

    fn validate_columns(columns: &[ColumnDef]) -> Result<(), KanbanError> {
        if columns.is_empty() {
            return Err(KanbanError::InvalidInput(
                "board must have at least one column".into(),
            ));
        }
        let statuses = columns
            .iter()
            .map(|column| column.status)
            .collect::<std::collections::HashSet<_>>();
        if statuses.len() != columns.len() {
            return Err(KanbanError::InvalidInput(
                "board column statuses must be unique".into(),
            ));
        }
        if !statuses.contains(&TaskStatus::Backlog) {
            return Err(KanbanError::InvalidInput(
                "board must contain a Backlog column for new tasks".into(),
            ));
        }
        Ok(())
    }

    fn build_board(owner: WebID, name: &str, columns: &[ColumnDef]) -> Result<Board, KanbanError> {
        let name = Self::validate_board_name(name)?;
        Self::validate_columns(columns)?;
        Ok(Board::new(name.to_string(), owner, columns.to_vec()))
    }

    fn board_h_mem(board: &Board) -> Result<HMem, KanbanError> {
        let value = serde_json::to_value(board)
            .map_err(|error| KanbanError::Internal(format!("serialization failed: {error}")))?;
        let ontology = HMemOntology {
            dimensions: vec![Dimension::How.as_str().to_string()],
            dc_type: hkask_bridge_ontology::pko::PROCEDURE.to_string(),
            dc_source: "kanban".to_string(),
            pko_procedure: Some(board.id.to_string()),
            pko_step: None,
            ..Default::default()
        };
        Ok(
            HMem::new(BOARD_ENTITY, &board.id.to_string(), value, board.owner)
                .with_ontology(ontology),
        )
    }

    /// Create a new kanban board.
    ///
    /// pre:  owner is a valid WebID; name is non-empty and within the cap;
    ///       columns is non-empty
    /// post: board is persisted as a h_mem; returns the created Board
    #[must_use = "result must be used"]
    pub(crate) fn board_create(
        &self,
        owner: WebID,
        name: &str,
        columns: &[ColumnDef],
    ) -> Result<Board, KanbanError> {
        let board = Self::build_board(owner, name, columns)?;
        let h_mem = Self::board_h_mem(&board)?;
        self.store
            .insert(&h_mem)
            .map_err(|e| KanbanError::Internal(format!("h_mem insert failed: {e}")))?;

        // P9: Regulation span
        tracing::info!(
            target: "hkask.kanban",
            operation = "board_created",
            board_id = %board.id,
            name = %board.name,
            owner = %owner,
            "REG"
        );

        Ok(board)
    }

    /// Publish an imported board and all of its tasks as one aggregate.
    ///
    /// expect: "An imported board appears complete in the parsed workflow or does not appear."
    /// [P3] Motivating: Generative Space — imported work is immediately usable.
    /// [P2] Constraining: Transparent Imperfection — publication failure leaves no partial board.
    /// pre: parsed contains a representable one-to-one column/status mapping
    /// post: board, task payloads, and board indexes commit together; tasks keep their parsed columns
    pub(crate) fn board_import(
        &self,
        owner: WebID,
        name: &str,
        parsed: &ParsedBoard,
    ) -> Result<(Board, usize), KanbanError> {
        let columns = columns_from_parsed(parsed)
            .map_err(|error| KanbanError::InvalidInput(error.to_string()))?;
        let board = Self::build_board(owner, name, &columns)?;
        let mut records = vec![Self::board_h_mem(&board)?];
        let mut task_count = 0usize;
        for (column, definition) in parsed.columns.iter().zip(&columns) {
            for title in &column.tasks {
                let mut task = Task::new(board.id, TaskSpec::new(title.clone()), owner);
                task.status = definition.status;
                records.extend(Self::task_h_mems(&task)?);
                task_count += 1;
            }
        }
        self.store.insert_batch_atomic(&records).map_err(|error| {
            KanbanError::Internal(format!("atomic board import failed: {error}"))
        })?;
        tracing::info!(
            target: "hkask.kanban",
            operation = "board_imported",
            board_id = %board.id,
            task_count,
            owner = %owner,
            "REG"
        );
        Ok((board, task_count))
    }

    /// The standard 5-column kanban board layout:
    /// Backlog → Ready → In Progress → Review → Done.
    ///
    /// This is the default layout used when no custom columns are provided
    /// to `board_create`. Exposed publicly so tests and downstream consumers
    /// share a single source of truth for the canonical column set.
    #[must_use]
    pub fn standard_columns() -> Vec<ColumnDef> {
        vec![
            ColumnDef::new("Backlog".into(), TaskStatus::Backlog, 0),
            ColumnDef::new("Ready".into(), TaskStatus::Ready, 1),
            ColumnDef::new("In Progress".into(), TaskStatus::InProgress, 2),
            ColumnDef::new("Review".into(), TaskStatus::Review, 3),
            ColumnDef::new("Done".into(), TaskStatus::Done, 4),
        ]
    }

    /// List all boards for a given owner.
    ///
    /// pre:  owner is a valid WebID
    /// post: returns all boards owned by this agent
    #[must_use = "result must be used"]
    pub(crate) fn board_list(&self, owner: &WebID) -> Result<Vec<Board>, KanbanError> {
        let h_mems = self
            .store
            .query_by_entity(BOARD_ENTITY)
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;

        let mut boards: Vec<Board> = Vec::new();
        for t in &h_mems {
            if t.access.owner_webid == *owner
                && let Ok(board) = serde_json::from_value::<Board>(t.value.clone())
            {
                boards.push(board);
            }
        }

        boards.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        Ok(boards)
    }

    /// Get a board by ID.
    ///
    /// pre:  board_id is valid
    /// post: returns Some(Board) if found, None otherwise
    #[must_use = "result must be used"]
    pub(crate) fn board_get(&self, board_id: BoardId) -> Result<Option<Board>, KanbanError> {
        let h_mems = self
            .store
            .query_by_entity_attribute(BOARD_ENTITY, &board_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;

        if let Some(t) = h_mems.into_iter().next() {
            let board = serde_json::from_value::<Board>(t.value)
                .map_err(|e| KanbanError::Internal(format!("deserialization failed: {e}")))?;
            Ok(Some(board))
        } else {
            Ok(None)
        }
    }

    /// Rename a board.
    ///
    /// The name is the board's addressing key (reference model R2/R6), so
    /// rename is a first-class operation — correcting a name must not
    /// require deleting and recreating the board (which would orphan the
    /// task links). The rename is convergent by construction: replaying the
    /// same call re-applies the same name, so it needs no idempotency key
    /// (the `task_update` class).
    ///
    /// pre:  board_id is valid; new_name is non-empty after trimming and
    ///       within the cap
    /// post: the board h_mem's name is updated in place (same h_mem id, PKO
    ///       procedure anchoring preserved); returns the renamed Board
    #[must_use = "result must be used"]
    pub(crate) fn board_rename(
        &self,
        board_id: BoardId,
        new_name: &str,
    ) -> Result<Board, KanbanError> {
        let new_name = Self::validate_board_name(new_name)?;
        let mut board = self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;
        board.name = new_name.to_string();
        let value = serde_json::to_value(&board)
            .map_err(|e| KanbanError::Internal(format!("serialization failed: {e}")))?;
        // Update the board h_mem in place — the same h_mem row, so the PKO
        // procedure anchoring (and the h_mem id `board_get` resolves)
        // survives the rename. Follows `update_task_triple`'s update
        // pattern.
        let h_mems = self
            .store
            .query_by_entity_attribute(BOARD_ENTITY, &board_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;
        let h_mem = h_mems.into_iter().next().ok_or_else(|| {
            KanbanError::Internal(format!("board {board_id} has no h_mem row to rename"))
        })?;
        self.store
            .update(&h_mem.id, value, 1.0f64)
            .map_err(|e| KanbanError::Internal(format!("h_mem update failed: {e}")))?;

        // P9: Regulation span
        tracing::info!(
            target: "hkask.kanban",
            operation = "board_renamed",
            board_id = %board_id,
            name = %new_name,
            "REG"
        );

        Ok(board)
    }

    // ── Task operations ───────────────────────────────────────────────────

    /// Validate goal citations against the live goal store: the cited goal
    /// must exist, the index must be in range, and the captured text must
    /// match the goal's criterion verbatim. After resolution the goal row
    /// is pruned and the citation survives as captured
    /// documentation — validation happens once, at the write that carries
    /// the citation (create or update).
    fn validate_goal_citations(&self, citations: &[CriterionCitation]) -> Result<(), KanbanError> {
        for citation in citations {
            let goal = self.goal_get(citation.goal_id)?.ok_or_else(|| {
                KanbanError::NotFound(NotFound {
                    entity_type: "goal".to_string(),
                    id: citation.goal_id.to_string(),
                })
            })?;
            let criterion = goal.criteria.get(citation.criterion_index).ok_or_else(|| {
                KanbanError::InvalidInput(format!(
                    "criterion index {} out of range (goal {} has {} criteria)",
                    citation.criterion_index,
                    citation.goal_id,
                    goal.criteria.len()
                ))
            })?;
            if citation.criterion_text != criterion.description {
                return Err(KanbanError::InvalidInput(format!(
                    "cited criterion text does not match goal {} criterion {} — cite the criterion verbatim",
                    citation.goal_id, citation.criterion_index
                )));
            }
        }
        Ok(())
    }

    fn task_h_mems(task: &Task) -> Result<[HMem; 2], KanbanError> {
        let value = serde_json::to_value(task)
            .map_err(|error| KanbanError::Internal(format!("serialization failed: {error}")))?;
        let task_ontology =
            HMemOntology::process(task.board_id.to_string(), task.id.to_string(), "kanban");
        let task_row = HMem::new(TASK_ENTITY, &task.id.to_string(), value, task.owner)
            .with_ontology(task_ontology);
        let index_entity = format!("{BOARD_TASKS_PREFIX}{}", task.board_id);
        let index_ontology = HMemOntology::process(
            task.board_id.to_string(),
            task.id.to_string(),
            "kanban:index",
        );
        let index_row = HMem::new(
            &index_entity,
            &task.id.to_string(),
            Value::String(task.id.to_string()),
            task.owner,
        )
        .with_ontology(index_ontology);
        Ok([task_row, index_row])
    }

    /// Create a new task on a board.
    ///
    /// pre:  board_id refers to an existing board; spec.title is non-empty; owner is valid
    /// post: task is persisted as a h_mem; returns the created Task
    #[must_use = "result must be used"]
    pub(crate) fn task_create(
        &self,
        board_id: BoardId,
        spec: TaskSpec,
        owner: WebID,
    ) -> Result<Task, KanbanError> {
        self.validate_goal_citations(&spec.advances)?;
        let task = Task::new(board_id, spec, owner);
        let records = Self::task_h_mems(&task)?;
        let board_id_text = board_id.to_string();
        let inserted = self
            .store
            .insert_batch_if_key_exists_atomic(&records, BOARD_ENTITY, &board_id_text)
            .map_err(|error| {
                KanbanError::Internal(format!("atomic task publication failed: {error}"))
            })?;
        if !inserted {
            return Err(KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id_text,
            }));
        }

        // P9: Regulation span
        tracing::info!(
            target: "hkask.kanban",
            operation = "task_created",
            task_id = %task.id,
            board_id = %board_id,
            owner = %owner,
            "REG"
        );

        Ok(task)
    }

    /// Count tasks in a given status on a board (for WIP enforcement).
    fn count_tasks_in_status(
        &self,
        board_id: BoardId,
        status: TaskStatus,
    ) -> Result<usize, KanbanError> {
        let index_entity = format!("{BOARD_TASKS_PREFIX}{board_id}");
        let index_triples = self
            .store
            .query_by_entity(&index_entity)
            .map_err(|e| KanbanError::Internal(format!("index query failed: {e}")))?;

        let mut count = 0usize;
        for idx_t in &index_triples {
            if let Some(task_id_str) = idx_t.value.as_str() {
                let task_triples = self
                    .store
                    .query_by_entity_attribute(TASK_ENTITY, task_id_str)
                    .map_err(|e| KanbanError::Internal(format!("task query failed: {e}")))?;
                for t in &task_triples {
                    if let Ok(task) = serde_json::from_value::<Task>(t.value.clone())
                        && task.status == status
                    {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }

    /// List tasks on a board, optionally filtered.
    ///
    /// pre:  board_id refers to an existing board
    /// post: returns tasks matching the filter; empty Vec if none match
    #[must_use = "result must be used"]
    pub(crate) fn task_list(
        &self,
        board_id: BoardId,
        filter: TaskFilter,
    ) -> Result<Vec<Task>, KanbanError> {
        // Verify board exists
        self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;

        let index_entity = format!("{BOARD_TASKS_PREFIX}{board_id}");

        // Get task IDs from the index
        let index_triples = self
            .store
            .query_by_entity(&index_entity)
            .map_err(|e| KanbanError::Internal(format!("index query failed: {e}")))?;

        let mut tasks: Vec<Task> = Vec::new();
        for idx_t in &index_triples {
            if let Some(task_id_str) = idx_t.value.as_str() {
                let task_triples = self
                    .store
                    .query_by_entity_attribute(TASK_ENTITY, task_id_str)
                    .map_err(|e| KanbanError::Internal(format!("task query failed: {e}")))?;

                for t in &task_triples {
                    if let Ok(task) = serde_json::from_value::<Task>(t.value.clone()) {
                        let status_match = filter.status.is_none_or(|s| task.status == s);
                        let assignee_match =
                            filter.assignee.is_none_or(|a| task.assignee == Some(a));
                        let priority_match =
                            filter.priority.is_none_or(|p| task.priority == Some(p));

                        if status_match && assignee_match && priority_match {
                            tasks.push(task);
                        }
                    }
                }
            }
        }

        tasks.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        if let Some(limit) = filter.limit {
            tasks.truncate(limit);
        }

        Ok(tasks)
    }

    /// Get a task by ID.
    ///
    /// pre:  task_id is valid
    /// post: returns Some(Task) if found, None otherwise
    #[must_use = "result must be used"]
    pub(crate) fn task_get(&self, task_id: TaskId) -> Result<Option<Task>, KanbanError> {
        let h_mems = self
            .store
            .query_by_entity_attribute(TASK_ENTITY, &task_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;

        if let Some(t) = h_mems.into_iter().next() {
            let task = serde_json::from_value::<Task>(t.value)
                .map_err(|e| KanbanError::Internal(format!("deserialization failed: {e}")))?;
            Ok(Some(task))
        } else {
            Ok(None)
        }
    }

    /// Move a task to a new column (state transition).
    ///
    /// pre:  task_id refers to an existing task; target is a valid transition from current status
    /// pre:  actor is a valid WebID (P12)
    /// post: task.status is updated; updated_at is refreshed
    #[must_use = "result must be used"]
    pub(crate) fn task_move(
        &self,
        task_id: TaskId,
        target: TaskStatus,
        actor: WebID,
    ) -> Result<Task, KanbanError> {
        let mut task = self.require_task(task_id)?;

        Self::require_task_actor(&task, actor)?;

        let board = self.board_get(task.board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: task.board_id.to_string(),
            })
        })?;
        if !board.can_transition(task.status, target) {
            return Err(KanbanError::InvalidTransition {
                task: task_id,
                from: task.status,
                to: target,
            });
        }

        let from_status = task.status;

        // WIP limit enforcement (Anderson §4: "limit WIP to expose problems")
        if let Some(col) = board.column_for_status(target)
            && let Some(wip_limit) = col.wip_limit
        {
            let current_count = self.count_tasks_in_status(task.board_id, target)?;
            if current_count >= wip_limit as usize {
                return Err(KanbanError::WipLimitExceeded {
                    column: col.name.clone(),
                    limit: wip_limit,
                    current: current_count as u32,
                });
            }
        }

        task.status = target;
        task.updated_at = chrono::Utc::now();
        let _ = actor;

        self.update_task_triple(&task)?;

        // P9: Regulation span
        tracing::info!(
            target: "hkask.kanban",
            operation = "task_moved",
            task_id = %task_id,
            from = %from_status,
            to = %target,
            actor = %actor,
            "REG"
        );

        Ok(task)
    }

    /// Claim an unassigned task as the authenticated actor.
    ///
    /// expect: "I can accept an unassigned task only as myself."
    /// \[P1\] Motivating: User Sovereignty — an agent supplies its own acceptance.
    /// pre:  task_id refers to an existing unassigned task; actor is authenticated.
    /// post: task.assignee is set to actor; a different agent cannot be assigned by this call.
    /// \[P12\] Constraining: No anonymous agency — the accepted assignment has an actor WebID.
    #[must_use = "result must be used"]
    pub(crate) fn task_claim(&self, task_id: TaskId, actor: WebID) -> Result<Task, KanbanError> {
        let mut task = self.require_task(task_id)?;

        if task.assignee.is_some() {
            return Err(KanbanError::PermissionDenied(
                "task is already assigned".into(),
            ));
        }

        // P4: Clear Boundaries — only tasks in Backlog or Ready can be claimed.
        // InProgress/Review/Done tasks are not claimable.
        if !matches!(task.status, TaskStatus::Backlog | TaskStatus::Ready) {
            return Err(KanbanError::InvalidTransition {
                task: task_id,
                from: task.status,
                to: task.status,
            });
        }

        task.assignee = Some(actor);
        task.updated_at = chrono::Utc::now();

        self.update_task_triple(&task)?;

        // P9: Regulation span
        tracing::info!(
            target: "hkask.kanban",
            operation = "task_assigned",
            task_id = %task_id,
            agent = %actor,
            "REG"
        );

        Ok(task)
    }

    /// Verify a task's completion against its acceptance criteria.
    ///
    /// pre:  task_id refers to an existing task in Review status
    /// pre:  verifier is a valid WebID
    /// post: task.verification is set; task moves to Done if passed
    #[must_use = "result must be used"]
    pub(crate) fn task_verify(
        &self,
        task_id: TaskId,
        evidence: &str,
        verifier: WebID,
    ) -> Result<(Task, Verification), KanbanError> {
        let mut task = self.require_task(task_id)?;

        Self::require_task_owner(&task, verifier)?;

        if task.status != TaskStatus::Review {
            return Err(KanbanError::InvalidTransition {
                task: task_id,
                from: task.status,
                to: TaskStatus::Done,
            });
        }

        // Task completion is user-feedback-driven.
        // Evidence (the user's confirmation text) IS the completion signal.
        // Criteria are informational — they guide work but don't gate completion.
        let passed = !evidence.trim().is_empty();
        let reasoning = if passed {
            let criteria_list: Vec<String> = task
                .criteria
                .iter()
                .map(|c| format!("  - {}", c.description))
                .collect();
            let criteria_block = if criteria_list.is_empty() {
                String::new()
            } else {
                format!("\nCriteria:\n{}", criteria_list.join("\n"))
            };
            format!(
                "User feedback received.{} Evidence length: {} chars.",
                criteria_block,
                evidence.len()
            )
        } else {
            "No evidence provided — task not verified.".into()
        };

        let verification = Verification::new(passed, reasoning, verifier);
        task.verification = Some(verification.clone());

        if passed {
            task.status = TaskStatus::Done;
        }
        task.updated_at = chrono::Utc::now();

        self.update_task_triple(&task)?;

        // P9: Regulation span
        tracing::info!(
            target: "hkask.kanban",
            operation = "task_verified",
            task_id = %task_id,
            passed = passed,
            verifier = %verifier,
            "REG"
        );

        Ok((task, verification))
    }

    // ── Decomposition + Spawn ─────────────────────────────────────────

    // Moved to decompose.rs and spawn.rs.

    // ── Comments (mini-REPL per task) ─────────────────────────────────

    // Moved to comments.rs.

    // ── Deliverables (file path / URL links) ──────────────────────────

    // Moved to comments.rs.

    // ── Phases ────────────────────────────────────────────────────────

    // Moved to phases.rs.

    // ── Lifecycle operations (P0) ─────────────────────────────────────

    fn task_record_ids(&self, task: &Task) -> Result<Vec<HMemId>, KanbanError> {
        let task_rows = self
            .store
            .query_by_entity_attribute(TASK_ENTITY, &task.id.to_string())
            .map_err(|error| KanbanError::Internal(format!("task h_mem query failed: {error}")))?;
        let index_entity = format!("{BOARD_TASKS_PREFIX}{}", task.board_id);
        let index_rows = self
            .store
            .query_by_entity_attribute(&index_entity, &task.id.to_string())
            .map_err(|error| KanbanError::Internal(format!("task index query failed: {error}")))?;
        Ok(task_rows
            .into_iter()
            .chain(index_rows)
            .map(|row| row.id)
            .collect())
    }

    /// Delete a task and its board index entry.
    ///
    /// pre:  task_id is valid
    /// post: task h_mem and index h_mem are deleted from the database
    #[must_use = "result must be used"]
    pub(crate) fn task_delete(&self, task_id: TaskId) -> Result<(), KanbanError> {
        let task = self.require_task(task_id)?;

        let record_ids = self.task_record_ids(&task)?;
        self.store
            .delete_batch_by_id_atomic(&record_ids)
            .map_err(|error| {
                KanbanError::Internal(format!("atomic task deletion failed: {error}"))
            })?;
        Ok(())
    }

    /// Unassign a task — remove the assignee.
    ///
    /// Authority model: the task owner (creator) has unilateral unassignment
    /// authority. This is consistent with kanban semantics where the task
    /// creator owns the task lifecycle. The assignee's consent is not required
    /// for unassignment because the owner bears the responsibility for the
    /// task's completion.
    ///
    /// pre:  task_id is valid; actor is the task owner
    /// post: task.assignee is set to None; task.updated_at refreshed
    #[must_use = "result must be used"]
    pub(crate) fn task_unassign(&self, task_id: TaskId, actor: WebID) -> Result<Task, KanbanError> {
        let mut task = self.require_task(task_id)?;
        Self::require_task_owner(&task, actor)?;
        task.assignee = None;
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(task)
    }

    /// Update editable fields on a task. Only the task owner can edit.
    /// Fields not provided (None) are left unchanged.
    ///
    /// The double-Option pattern for `description` and `priority` distinguishes
    /// "no change" (None) from "clear the field" (Some(None)) from "set a new
    /// value" (Some(Some(x))).
    ///
    /// pre:  task_id is valid; actor is the task owner
    /// post: provided fields are updated; task.updated_at refreshed
    // The double-Option fields are inherent to this patch API; grouping them
    // into a struct would obscure the None / Some(None) / Some(Some(x)) trichotomy
    // documented above, so we accept the argument count.
    #[allow(clippy::too_many_arguments)]
    #[must_use = "result must be used"]
    pub(crate) fn task_update(
        &self,
        task_id: TaskId,
        actor: WebID,
        title: Option<String>,
        description: Option<Option<String>>,
        criteria: Option<Vec<VerificationCriterion>>,
        priority: Option<Option<Priority>>,
        labels: Option<Vec<String>>,
        advances: Option<Vec<CriterionCitation>>,
    ) -> Result<Task, KanbanError> {
        let mut task = self.require_task(task_id)?;
        Self::require_task_owner(&task, actor)?;
        if let Some(new_title) = title {
            task.title = new_title;
        }
        if let Some(description_update) = description {
            task.description = description_update;
        }
        if let Some(new_criteria) = criteria {
            task.criteria = new_criteria;
        }
        if let Some(priority_update) = priority {
            task.priority = priority_update;
        }
        if let Some(new_labels) = labels {
            task.labels = new_labels;
        }
        if let Some(new_advances) = advances {
            // Same validation as creation: the citation is re-anchored against
            // the live goal store at every write that carries it.
            self.validate_goal_citations(&new_advances)?;
            task.advances = new_advances;
        }
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(task)
    }

    /// Reopen a completed task — move from Done back to InProgress.
    ///
    /// pre:  task_id refers to a task in Done status
    /// post: task moves to InProgress, verification cleared
    #[must_use = "result must be used"]
    pub(crate) fn task_reopen(&self, task_id: TaskId, actor: WebID) -> Result<Task, KanbanError> {
        let mut task = self.require_task(task_id)?;

        Self::require_task_owner(&task, actor)?;

        if task.status != TaskStatus::Done {
            return Err(KanbanError::InvalidTransition {
                task: task_id,
                from: task.status,
                to: TaskStatus::InProgress,
            });
        }

        task.status = TaskStatus::InProgress;
        task.verification = None;
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(task)
    }

    /// Delete a board and all its tasks.
    ///
    /// pre:  board_id is valid
    /// post: board h_mem and all associated task/index h_mems are deleted from the database
    #[must_use = "result must be used"]
    pub(crate) fn board_delete(&self, board_id: BoardId) -> Result<usize, KanbanError> {
        let board_id_text = board_id.to_string();
        let deleted = self
            .store
            .delete_by_pko_procedure_if_key_exists_atomic(
                &board_id_text,
                BOARD_ENTITY,
                &board_id_text,
            )
            .map_err(|error| {
                KanbanError::Internal(format!("atomic board deletion failed: {error}"))
            })?
            .ok_or_else(|| {
                KanbanError::NotFound(NotFound {
                    entity_type: "board".to_string(),
                    id: board_id_text.clone(),
                })
            })?;
        let task_ids = deleted
            .into_iter()
            .filter_map(|(entity, attribute)| (entity == TASK_ENTITY).then_some(attribute))
            .collect::<std::collections::HashSet<_>>();
        Ok(task_ids.len())
    }

    // ── LLM Verification ──────────────────────────────────────────────

    // Moved to verification.rs.

    // ── Kata Integration (task-scoped scientific thinking) ──────────

    // Moved to kata.rs.

    // ── Helpers ───────────────────────────────────────────────────────

    pub(crate) fn update_task_triple(&self, task: &Task) -> Result<(), KanbanError> {
        let new_value = serde_json::to_value(task)
            .map_err(|e| KanbanError::Internal(format!("serialization failed: {e}")))?;
        let h_mems = self
            .store
            .query_by_entity_attribute(TASK_ENTITY, &task.id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;
        if let Some(t) = h_mems.into_iter().next() {
            self.store
                .update(&t.id, new_value, 1.0f64)
                .map_err(|e| KanbanError::Internal(format!("h_mem update failed: {e}")))?;
        }
        Ok(())
    }
}
