//! KanbanService — core kanban board and task coordination.
//!
//! Implements kanban board and task coordination operations.
//! Every operation carries ownership tracking (P12) and enforces agent consent
//! on assignment (P1). State transitions are column-ordered.
//!
//! ## Who may read and write
//!
//! Mutations check the caller: task changes need the task's owner or
//! assignee (delete, verify, reopen, unassign, update and spawn need the
//! owner); board rename, delete and export need the board's owner; goals are
//! owner-scoped throughout.
//!
//! Task reads do NOT check the caller, and neither does creating a task on a
//! board: `task_list`, `task_get`, `task_comments_since`, the kata prompts
//! and `task_create` answer any caller who holds the id. This is deliberate
//! (operator ruling 2026-11-11, mcp-tool-review KK-15), and is pinned by
//! `task_reads_and_task_creation_are_open_to_any_caller`. In production the
//! server runs as one fixed identity — the bridge passes `HKASK_WEBID` only
//! to the curator server — so an ownership check here would tell nobody from
//! anybody. If identities are ever distinguished per caller, revisit this
//! paragraph first: these five paths are where one caller could read or add
//! to another's board.
//!
//! Persistence: boards and tasks stored as RDF h_mems via HMemStore (MDS §2).
//! HMem scheme:
//!   kanban:board → {board_id} → JSON Board
//!   kanban:task  → {task_id}  → JSON Task
//!   kanban:board_tasks:{board_id} → {task_id} → task_id (index)

use hkask_storage::{HMem, HMemError, HMemStore, TxnReads};
use hkask_types::Dimension;
use hkask_types::HMemOntology;
use hkask_types::NotFound;
use hkask_types::WebID;
use hkask_types::id::{BoardId, TaskId};
use hkask_types::kanban_wire::{ALGEDONIC_BOARD_NAME, KANBAN_BOARD_NAME_MAX_CHARS};
use serde_json::Value;

use super::types::{BoardFault, BoardListReport, KanbanError, TaskIndexFault, TaskListReport};

use crate::kanban::mermaid::{ParsedBoard, columns_from_parsed};
use crate::kanban::{
    Board, ColumnDef, CriterionCitation, Priority, Task, TaskSpec, TaskStatus, Verification,
    VerificationCriterion,
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
    pub(super) goal_clock: std::sync::Arc<dyn Fn() -> chrono::DateTime<chrono::Utc> + Send + Sync>,
    /// One-shot fault injection for the replay-protection integration
    /// suite: the next N `task_comment` calls fail instead of writing
    /// (`#[doc(hidden)]` arm below). Arc-shared across clones so a test can
    /// arm the fault on the service it keeps while the server holds a
    /// clone. Zero in production — nothing reads it except `task_comment`.
    pub(crate) comment_faults: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// One-shot scheduling boundary for deterministic mutation interleavings.
    #[cfg(test)]
    pub(super) before_task_write:
        std::sync::Arc<std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>>,
}

// HMem entity prefixes
const BOARD_ENTITY: &str = "kanban:board";
const TASK_ENTITY: &str = "kanban:task";
const BOARD_TASKS_PREFIX: &str = "kanban:board_tasks:";

/// The per-database change counter's h_mem coordinates (P1, the kanzo
/// `schema.sql` pattern: one monotonic number every write moves, so a
/// poller detects change with a single-cell read).
const CHANGES_ENTITY: &str = "kanban:changes";
const CHANGES_ATTRIBUTE: &str = "revision";

impl KanbanService {
    /// Create a KanbanService backed by the given HMemStore.
    ///
    /// pre:  store must have the h_mems table initialized
    /// post: returns a KanbanService ready for use
    #[must_use]
    pub fn new(store: HMemStore) -> Self {
        Self {
            store,
            goal_clock: std::sync::Arc::new(chrono::Utc::now),
            comment_faults: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            #[cfg(test)]
            before_task_write: Default::default(),
        }
    }

    /// Goal lifecycle clock boundary; production defaults to UTC. An injected
    /// clock is shared across declaration, native ingress and scheduled reads,
    /// never used to mutate a frozen horizon after admission.
    #[doc(hidden)]
    pub fn with_goal_clock(
        mut self,
        clock: std::sync::Arc<dyn Fn() -> chrono::DateTime<chrono::Utc> + Send + Sync>,
    ) -> Self {
        self.goal_clock = clock;
        self
    }
    pub fn goal_now(&self) -> chrono::DateTime<chrono::Utc> {
        (self.goal_clock)()
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

    /// Move the per-database change counter (P1) — one monotonic number
    /// every write moves, so a poller that remembers the last value can
    /// skip re-fetching derived state until it moves (the kanzo
    /// `schema.sql` changes-counter pattern, adapted to the h_mem store:
    /// one atomic cell instead of SQL triggers). The bump runs after the
    /// mutation it follows commits, so a failed mutation never moves the
    /// counter; and it is a single `IMMEDIATE` update, so concurrent
    /// server processes sharing this database cannot lose a bump to a
    /// read-modify-write race.
    fn bump_changes(&self, owner: WebID) {
        let bumped: Result<Option<()>, KanbanError> = self
            .store
            .update_value_atomic::<(), TaskWriteError>(
                CHANGES_ENTITY,
                CHANGES_ATTRIBUTE,
                |current| {
                    let current: u64 = serde_json::from_value(current).unwrap_or(0);
                    Ok((Some(serde_json::Value::from(current + 1)), ()))
                },
            )
            .map_err(KanbanError::from);
        match bumped {
            Ok(Some(())) => {}
            Ok(None) => {
                // No counter row yet (a fresh database whose very first
                // mutation is running): create it at 1. A concurrent
                // creator wins the insert; the loser's retry keeps the
                // counter monotonic either way.
                let counter = HMem::new(
                    CHANGES_ENTITY,
                    CHANGES_ATTRIBUTE,
                    serde_json::Value::from(1u64),
                    owner,
                );
                if self.store.insert(&counter).is_err() {
                    // Another process created the row first — bump it.
                    let _ = self.store.update_value_atomic::<(), TaskWriteError>(
                        CHANGES_ENTITY,
                        CHANGES_ATTRIBUTE,
                        |current| {
                            let current: u64 = serde_json::from_value(current).unwrap_or(0);
                            Ok((Some(serde_json::Value::from(current + 1)), ()))
                        },
                    );
                }
            }
            Err(error) => {
                // A counter that fails to move can only make a poller miss
                // one change until the next write — never a wrong one. The
                // mutation that called this has already committed; do not
                // fail the call for the counter.
                tracing::warn!(
                    target: "hkask.kanban",
                    "change counter bump failed: {error}"
                );
            }
        }
    }

    /// Read the change counter (P1) — the poller's cheap single-cell read.
    /// A missing row reads as 0: a fresh database has had no writes, and
    /// the first write creates the row. A corrupt value resets to 0 —
    /// monotonicity is lost only across that corruption, and pollers
    /// over-fetch rather than miss a change.
    pub(crate) fn changes_revision(&self) -> Result<u64, KanbanError> {
        let h_mems = self
            .store
            .query_by_entity_attribute(CHANGES_ENTITY, CHANGES_ATTRIBUTE)
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;
        if let Some(h_mem) = h_mems.into_iter().next() {
            Ok(serde_json::from_value(h_mem.value).unwrap_or(0))
        } else {
            Ok(0)
        }
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

    fn build_board(
        owner: WebID,
        name: &str,
        columns: &[ColumnDef],
        done_requires_verification: Option<bool>,
    ) -> Result<Board, KanbanError> {
        let name = Self::validate_board_name(name)?;
        Self::validate_columns(columns)?;
        let mut board = Board::new(name.to_string(), owner, columns.to_vec());
        // The Done-gate policy is structural, persisted on the row. An
        // explicit choice wins; `None` falls back to the reserved-name
        // convention (boards named "Algedonic review" are verification-
        // gated) so the operator's existing review discipline needs no
        // call-site change.
        board.done_requires_verification =
            done_requires_verification.unwrap_or(board.name == ALGEDONIC_BOARD_NAME);
        Ok(board)
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
    /// Create a board deferring the Done-gate policy to the reserved-name
    /// convention — the test-suite convenience over the real path
    /// ([`board_create_with_policy`]); production callers (the MCP tool)
    /// pass their policy explicitly.
    #[cfg(test)]
    pub(crate) fn board_create(
        &self,
        owner: WebID,
        name: &str,
        columns: &[ColumnDef],
    ) -> Result<Board, KanbanError> {
        self.board_create_with_policy(owner, name, columns, None)
    }

    /// Create a board with an explicit Done-gate policy: when true, the
    /// Done column is reachable only through `kanban_task_verify`, never
    /// an ordinary column move. `None` defers to the reserved-name
    /// convention stamping in `build_board` — the policy is always
    /// persisted structurally, never re-derived from the name at move
    /// time.
    pub(crate) fn board_create_with_policy(
        &self,
        owner: WebID,
        name: &str,
        columns: &[ColumnDef],
        done_requires_verification: Option<bool>,
    ) -> Result<Board, KanbanError> {
        let board = Self::build_board(owner, name, columns, done_requires_verification)?;
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

        self.bump_changes(owner);

        Ok(board)
    }

    /// Publish an imported board and all of its tasks as one aggregate.
    ///
    /// expect: "An imported board appears complete in the parsed workflow or does not appear."
    /// [P3] Motivating: Generative Space — imported work is immediately usable.
    /// [P2] Constraining: Transparent Imperfection — publication failure leaves no partial board.
    /// pre: parsed contains a representable one-to-one column/status mapping;
    ///      no column holds more tasks than the WIP limit it declares; every
    ///      task title carries text
    /// post: board, task payloads, and board indexes commit together; tasks keep their parsed columns
    pub(crate) fn board_import(
        &self,
        owner: WebID,
        name: &str,
        parsed: &ParsedBoard,
    ) -> Result<(Board, usize), KanbanError> {
        let columns = columns_from_parsed(parsed)
            .map_err(|error| KanbanError::InvalidInput(error.to_string()))?;
        // The document is checked against its own declarations before
        // anything is built: a board must not be born over a limit it
        // states (mcp-tool-review KK-16 — three tasks imported into a WIP-1
        // column), and an imported title meets the rule a created one does.
        // Both are faults of the input, so both are `InvalidInput`.
        for column in &parsed.columns {
            if let Some(limit) = column.wip_limit
                && column.tasks.len() > limit as usize
            {
                return Err(KanbanError::InvalidInput(format!(
                    "column '{}' lists {} tasks but declares a WIP limit of {limit}",
                    column.name,
                    column.tasks.len()
                )));
            }
            for title in &column.tasks {
                Self::validate_task_title(title)?;
            }
        }
        let board = Self::build_board(owner, name, &columns, None)?;
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
        self.bump_changes(owner);
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

    /// List all boards for a given owner. A row of the owner's that fails
    /// to decode is surfaced in `undecodable`, never silently dropped — the
    /// rule `task_list` already follows (mcp-tool-review KK-02: a corrupt
    /// board row used to vanish from the listing with no signal).
    ///
    /// pre:  owner is a valid WebID
    /// post: returns the owner's readable boards plus the owner's rows
    ///       that could not be decoded
    #[must_use = "result must be used"]
    pub(crate) fn board_list(&self, owner: &WebID) -> Result<BoardListReport, KanbanError> {
        let h_mems = self
            .store
            .query_by_entity(BOARD_ENTITY)
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;

        let mut boards: Vec<Board> = Vec::new();
        let mut undecodable = Vec::new();
        for t in h_mems {
            if t.access.owner_webid != *owner {
                continue;
            }
            match serde_json::from_value::<Board>(t.value) {
                Ok(board) => boards.push(board),
                Err(error) => undecodable.push(BoardFault {
                    board_id: t.attribute,
                    reason: format!("board row failed to decode: {error}"),
                }),
            }
        }

        boards.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        Ok(BoardListReport {
            boards,
            undecodable,
        })
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
        // Atomic stable-key write — the same h_mem row (ID and confidence
        // preserved), so the PKO procedure anchoring (and the h_mem id
        // `board_get` resolves) genuinely survives the rename, per this
        // method's contract. The prior resolve-then-`HMemStore::update`
        // form re-keyed the row (delete + reinsert, new ID) on every
        // rename — it never delivered the same-row contract, and it
        // carried the same resolve→write race as the task path
        // (card 84c7e7b8).
        let renamed = self.store.update_value_atomic::<(), TaskWriteError>(
            BOARD_ENTITY,
            &board_id.to_string(),
            |_| Ok((Some(value), ())),
        )?;
        if renamed.is_none() {
            return Err(KanbanError::Internal(format!(
                "board {board_id} has no h_mem row to rename"
            )));
        }

        // P9: Regulation span
        tracing::info!(
            target: "hkask.kanban",
            operation = "board_renamed",
            board_id = %board_id,
            name = %new_name,
            "REG"
        );

        self.bump_changes(board.owner);

        Ok(board)
    }

    /// Persist the Done-gate policy on a legacy board — the one-time
    /// migration that retires the name-keyed check: after the first move
    /// evaluation the policy lives on the row, so renaming the board keeps
    /// the gate. The write transforms the current row (setting only the
    /// flag), so a concurrent rename cannot be clobbered; it is convergent,
    /// so concurrent migrations cannot disagree.
    fn board_set_done_gate(&self, board_id: BoardId) -> Result<Board, KanbanError> {
        let board = self
            .store
            .update_value_atomic::<_, TaskWriteError>(
                BOARD_ENTITY,
                &board_id.to_string(),
                |current| {
                    let mut board: Board = serde_json::from_value(current).map_err(|error| {
                        TaskWriteError(KanbanError::Internal(format!(
                            "deserialization failed: {error}"
                        )))
                    })?;
                    board.done_requires_verification = true;
                    let value = serde_json::to_value(&board).map_err(|error| {
                        TaskWriteError(KanbanError::Internal(format!(
                            "serialization failed: {error}"
                        )))
                    })?;
                    Ok((Some(value), board))
                },
            )?
            .ok_or_else(|| {
                KanbanError::NotFound(NotFound {
                    entity_type: "board".to_string(),
                    id: board_id.to_string(),
                })
            })?;
        self.bump_changes(board.owner);
        Ok(board)
    }

    // ── Task operations ───────────────────────────────────────────────────

    /// Validate goal citations against the live goal store: the cited goal
    /// must exist, the index must be in range, and the captured text must
    /// match the immutable criterion verbatim. Canonical goals are retained
    /// after closure and memory delivery.
    fn validate_goal_citations(
        &self,
        citations: &[CriterionCitation],
        actor: WebID,
    ) -> Result<(), KanbanError> {
        for citation in citations {
            let goal = self.goal_get(citation.goal_id, actor)?.ok_or_else(|| {
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

    /// A task title must carry text: the title is how a task is addressed
    /// on the board, and the mermaid export writes one title per line, so a
    /// blank title is a task that export/import silently loses
    /// (mcp-tool-review KK-03).
    fn validate_task_title(title: &str) -> Result<(), KanbanError> {
        if title.trim().is_empty() {
            return Err(KanbanError::InvalidInput("task title is empty".into()));
        }
        Ok(())
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
        Self::validate_task_title(&spec.title)?;
        self.validate_goal_citations(&spec.advances, owner)?;
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

        self.bump_changes(owner);

        Ok(task)
    }

    /// Walk a board's task index and read every indexed task row — the
    /// shared spine of the per-board task reads (the listing and the WIP
    /// admission). The board's existence check stays per-caller (the list
    /// tool verifies it; the WIP count runs on a board the caller already
    /// verified). A row that is missing or fails to decode is not silently
    /// skipped: the read returns it as a [`TaskIndexFault`] so listings
    /// surface the degradation in their response and the WIP admission can
    /// fail closed (an unreadable row can only undercount, and
    /// undercounting over-admits).
    fn board_tasks_with_faults(
        &self,
        board_id: BoardId,
    ) -> Result<(Vec<Task>, Vec<TaskIndexFault>), KanbanError> {
        let index_entity = format!("{BOARD_TASKS_PREFIX}{board_id}");
        let index_triples = self
            .store
            .query_by_entity(&index_entity)
            .map_err(|e| KanbanError::Internal(format!("index query failed: {e}")))?;
        let mut tasks = Vec::new();
        let mut undecodable = Vec::new();
        for idx_t in &index_triples {
            let Some(task_id_str) = idx_t.value.as_str() else {
                undecodable.push(TaskIndexFault {
                    task_id: idx_t.attribute.clone(),
                    reason: "index entry value is not a task id string".to_string(),
                });
                continue;
            };
            let task_triples = self
                .store
                .query_by_entity_attribute(TASK_ENTITY, task_id_str)
                .map_err(|e| KanbanError::Internal(format!("task query failed: {e}")))?;
            let Some(row) = task_triples.into_iter().next() else {
                undecodable.push(TaskIndexFault {
                    task_id: task_id_str.to_string(),
                    reason: "indexed task row is missing".to_string(),
                });
                continue;
            };
            match serde_json::from_value::<Task>(row.value) {
                Ok(task) => tasks.push(task),
                Err(error) => undecodable.push(TaskIndexFault {
                    task_id: task_id_str.to_string(),
                    reason: format!("task row failed to decode: {error}"),
                }),
            }
        }
        Ok((tasks, undecodable))
    }

    /// List tasks on a board, optionally only those in one status. Indexed rows that cannot
    /// be read are surfaced in `undecodable` — never silently dropped.
    ///
    /// pre:  board_id refers to an existing board
    /// post: returns the readable tasks matching the filter plus the
    ///       indexed rows that could not be read
    #[must_use = "result must be used"]
    pub(crate) fn task_list(
        &self,
        board_id: BoardId,
        status: Option<TaskStatus>,
    ) -> Result<TaskListReport, KanbanError> {
        // Verify board exists
        self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;

        let (mut tasks, undecodable) = self.board_tasks_with_faults(board_id)?;
        if let Some(status) = status {
            tasks.retain(|task| task.status == status);
        }
        tasks.sort_by_key(|b| std::cmp::Reverse(b.created_at));

        Ok(TaskListReport { tasks, undecodable })
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

    /// Count a board's tasks in `status` from rows read inside the caller's
    /// write transaction — the WIP admission's in-lock view. A sibling row
    /// that is missing or fails to decode fails the count visibly: an
    /// unreadable row can only undercount, and undercounting over-admits,
    /// so the move is refused with the offending row named rather than
    /// admitting against a number known to be a lower bound.
    fn count_status_in_txn(
        reads: &TxnReads<'_, '_>,
        board_id: BoardId,
        status: TaskStatus,
    ) -> Result<usize, KanbanError> {
        let index_entity = format!("{BOARD_TASKS_PREFIX}{board_id}");
        let indexed = reads.attributes_by_entity(&index_entity).map_err(|error| {
            KanbanError::Internal(format!("WIP count index read failed: {error}"))
        })?;
        let mut count = 0usize;
        for task_id in indexed {
            let Some(value) = reads.value_by_key(TASK_ENTITY, &task_id).map_err(|error| {
                KanbanError::Internal(format!(
                    "WIP count unreliable: task {task_id} row read failed: {error}"
                ))
            })?
            else {
                return Err(KanbanError::Internal(format!(
                    "WIP count unreliable: indexed task {task_id} has no row"
                )));
            };
            let task: Task = serde_json::from_value(value).map_err(|error| {
                KanbanError::Internal(format!(
                    "WIP count unreliable: task {task_id} failed to decode: {error}"
                ))
            })?;
            if task.status == status {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Move a task to a new column (state transition).
    ///
    /// WIP admission is in-transaction: the sibling-task count is read
    /// through the same `IMMEDIATE` transaction that commits the move
    /// (`mutate_task_scanning`), so two concurrent movers into a full
    /// column cannot both pass the count — the pre-fix shape counted
    /// before the transaction, leaving exactly that window (observed red:
    /// `concurrent_moves_into_a_wip_one_column_admit_exactly_one`).
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
        let task = self.require_task(task_id)?;

        Self::require_task_actor(&task, actor)?;

        let board = self.board_get(task.board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: task.board_id.to_string(),
            })
        })?;
        // Legacy migration: boards created before the structural Done-gate
        // flag carry the reserved name without the flag. Stamp the flag
        // once, here, so the gate lives on the row and survives a rename
        // (a name-keyed check moves with the name instead).
        let board = if board.name == ALGEDONIC_BOARD_NAME && !board.done_requires_verification {
            self.board_set_done_gate(board.id)?
        } else {
            board
        };
        let (task, from_status) = self.mutate_task_scanning(task_id, |task, reads| {
            Self::require_task_actor(task, actor)?;
            // On verification-gated boards, the Done status is a
            // verification outcome, never an ordinary column move.
            if (board.done_requires_verification && target == TaskStatus::Done)
                || !board.can_transition(task.status, target)
            {
                return Err(KanbanError::InvalidTransition {
                    task: task_id,
                    from: task.status,
                    to: target,
                });
            }
            if let Some((column, limit)) = board
                .column_for_status(target)
                .and_then(|column| column.wip_limit.map(|limit| (column.name.clone(), limit)))
            {
                let count = Self::count_status_in_txn(reads, task.board_id, target)?;
                if count >= limit as usize {
                    return Err(KanbanError::WipLimitExceeded {
                        column,
                        limit,
                        current: count as u32,
                    });
                }
            }
            let from_status = task.status;
            task.status = target;
            Ok(from_status)
        })?;

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
        let (task, ()) = self.mutate_task(task_id, |task| {
            if task.assignee.is_some() {
                return Err(KanbanError::PermissionDenied(
                    "task is already assigned".into(),
                ));
            }

            // P4: operator-reserved tasks are the operator's own work —
            // the claim boundary is structural (this refusal), not an
            // instruction the agent might not follow.
            if task.reserved {
                return Err(KanbanError::PermissionDenied(
                    "task is reserved for the operator — agents do not claim reserved tasks".into(),
                ));
            }

            // P4: Clear Boundaries — only tasks in Backlog or Ready can be claimed.
            // InProgress/Review/Done tasks are not claimable.
            if !matches!(task.status, TaskStatus::Backlog | TaskStatus::Ready) {
                return Err(KanbanError::NotInRequiredStatus {
                    task: task_id,
                    action: "claimed",
                    status: task.status,
                    required: "backlog or ready",
                });
            }

            task.assignee = Some(actor);
            Ok(())
        })?;

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

    /// Pull the next claimable Ready task for the actor (P3 — the
    /// reference-model pull primitive: work moves only when downstream
    /// capacity admits it, never by push). One transaction per attempt:
    /// inside the write lock the candidate must still be Ready, unassigned
    /// and not reserved, and the In Progress column's WIP limit admits the
    /// move — the same in-transaction admission `task_move` uses, so two
    /// concurrent claimers cannot both pass the count. Candidate selection
    /// happens outside the lock; a candidate taken by a concurrent claimer
    /// is skipped and the next is tried, so concurrent callers get
    /// different tasks.
    ///
    /// pre:  board_id refers to an existing board; actor is authenticated
    /// post: returns the oldest unassigned Ready task moved to In Progress
    ///       and assigned to the actor (or `None` when no claimable task
    ///       remains), plus the count of operator-reserved Ready tasks the
    ///       pull skipped — disclosed, never silent.
    pub(crate) fn claim_next(
        &self,
        board_id: BoardId,
        actor: WebID,
    ) -> Result<(Option<Task>, usize), KanbanError> {
        let board = self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;
        // The legacy Done-gate stamp `task_move` performs, kept identical
        // so a gated board pulls under the same policy a direct move sees.
        let board = if board.name == ALGEDONIC_BOARD_NAME && !board.done_requires_verification {
            self.board_set_done_gate(board.id)?
        } else {
            board
        };
        let (mut tasks, faults) = self.board_tasks_with_faults(board_id)?;
        if !faults.is_empty() {
            // Fail closed, the WIP admission's rule: an unreadable indexed
            // row could be a claimable task, and a pull that skips it
            // decides work allocation on a known-lower view.
            return Err(KanbanError::Internal(format!(
                "board {board_id} has unreadable indexed task rows — the pull refuses to \
                 decide on an incomplete view"
            )));
        }
        let reserved_ready = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Ready && t.reserved && t.assignee.is_none())
            .count();
        // Oldest first — the head of the Ready column is the first queued.
        tasks.retain(|t| t.status == TaskStatus::Ready && t.assignee.is_none() && !t.reserved);
        tasks.sort_by_key(|t| t.created_at);
        for candidate in tasks {
            match self.claim_next_once(candidate.id, actor, &board) {
                Ok(task) => {
                    tracing::info!(
                        target: "hkask.kanban",
                        operation = "task_claimed_next",
                        task_id = %task.id,
                        board_id = %board_id,
                        actor = %actor,
                        "REG"
                    );
                    return Ok((Some(task), reserved_ready));
                }
                // Taken by a concurrent claimer, deleted, reserved since
                // selection, or moved under us: skip to the next candidate.
                Err(
                    KanbanError::PermissionDenied(_)
                    | KanbanError::InvalidTransition { .. }
                    | KanbanError::NotFound(_),
                ) => continue,
                // In Progress is full, or the view is unreadable: stop and
                // surface the error rather than grinding candidates against
                // the same full column.
                Err(error) => return Err(error),
            }
        }
        Ok((None, reserved_ready))
    }

    /// One claim attempt under the task write lock — the in-transaction
    /// guards `claim_next` retries around. Never call this directly: it
    /// assumes the caller verified the board.
    fn claim_next_once(
        &self,
        task_id: TaskId,
        actor: WebID,
        board: &Board,
    ) -> Result<Task, KanbanError> {
        let (task, ()) = self.mutate_task_scanning(task_id, |task, reads| {
            if task.reserved {
                return Err(KanbanError::PermissionDenied(
                    "task is reserved for the operator — agents do not claim reserved tasks".into(),
                ));
            }
            if task.assignee.is_some() {
                return Err(KanbanError::PermissionDenied(
                    "task is already assigned".into(),
                ));
            }
            if task.status != TaskStatus::Ready
                || !board.can_transition(task.status, TaskStatus::InProgress)
            {
                return Err(KanbanError::InvalidTransition {
                    task: task_id,
                    from: task.status,
                    to: TaskStatus::InProgress,
                });
            }
            // The same in-transaction WIP admission `task_move` uses: the
            // count and the claim commit share one transaction.
            if let Some((column, limit)) = board
                .column_for_status(TaskStatus::InProgress)
                .and_then(|column| column.wip_limit.map(|limit| (column.name.clone(), limit)))
            {
                let count =
                    Self::count_status_in_txn(reads, task.board_id, TaskStatus::InProgress)?;
                if count >= limit as usize {
                    return Err(KanbanError::WipLimitExceeded {
                        column,
                        limit,
                        current: count as u32,
                    });
                }
            }
            task.assignee = Some(actor);
            task.status = TaskStatus::InProgress;
            Ok(())
        })?;
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
        let (task, verification) = self.mutate_task(task_id, |task| {
            Self::require_task_owner(task, verifier)?;

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
            Ok(verification)
        })?;

        // P9: Regulation span
        tracing::info!(
            target: "hkask.kanban",
            operation = "task_verified",
            task_id = %task_id,
            passed = verification.passed,
            verifier = %verifier,
            "REG"
        );

        Ok((task, verification))
    }

    // ── Spawn ─────────────────────────────────────────────────────

    // Moved to spawn.rs.

    // ── Comments (mini-REPL per task) ──────────────────────────────

    // Moved to comments.rs.

    // ── Deliverables (file path / URL links) ──────────────────────

    // Moved to comments.rs.

    // ── Lifecycle operations (P0) ─────────────────────────────────────

    /// Delete a task and its board index entry.
    ///
    /// pre:  task_id is valid; actor is the task owner — a destructive op
    /// authorizes like board_delete (owner-only, never the assignee)
    /// post: task h_mem and index h_mem are deleted from the database
    #[must_use = "result must be used"]
    pub(crate) fn task_delete(&self, task_id: TaskId, actor: WebID) -> Result<(), KanbanError> {
        let task = self.require_task(task_id)?;
        Self::require_task_owner(&task, actor)?;

        let index_entity = format!("{BOARD_TASKS_PREFIX}{}", task.board_id);
        let deleted = self
            .store
            .delete_related_keys_atomic(
                TASK_ENTITY,
                &task_id.to_string(),
                &index_entity,
                &task_id.to_string(),
            )
            .map_err(|error| {
                KanbanError::Internal(format!("atomic task deletion failed: {error}"))
            })?;
        if !deleted {
            return Err(KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            }));
        }
        self.bump_changes(actor);
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
        let (task, ()) = self.mutate_task(task_id, |task| {
            Self::require_task_owner(task, actor)?;
            task.assignee = None;
            Ok(())
        })?;
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
        reserved: Option<bool>,
    ) -> Result<Task, KanbanError> {
        if let Some(new_title) = &title {
            Self::validate_task_title(new_title)?;
        }
        // Validate cross-record citations before taking the task write lock.
        if let Some(new_advances) = &advances {
            Self::require_task_owner(&self.require_task(task_id)?, actor)?;
            self.validate_goal_citations(new_advances, actor)?;
        }
        let (task, ()) = self.mutate_task(task_id, |task| {
            Self::require_task_owner(task, actor)?;
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
                task.advances = new_advances;
            }
            if let Some(reserved_update) = reserved {
                task.reserved = reserved_update;
            }
            Ok(())
        })?;
        Ok(task)
    }

    /// Reopen a completed task — move from Done back to InProgress.
    ///
    /// A reopen is an entry into In Progress like any other, so it passes
    /// the same in-transaction WIP admission `task_move` uses, and it needs
    /// a column to land in. Before this (mcp-tool-review KK-04) a reopen
    /// put a second task into a WIP-1 column that had just refused a move,
    /// and on a board with no In Progress column it left the task in a
    /// status no column shows and no move could leave.
    ///
    /// pre:  task_id refers to a task in Done status; the board has an
    ///       In Progress column with capacity
    /// post: task moves to InProgress, verification cleared
    #[must_use = "result must be used"]
    pub(crate) fn task_reopen(&self, task_id: TaskId, actor: WebID) -> Result<Task, KanbanError> {
        let task = self.require_task(task_id)?;
        let board = self.board_get(task.board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: task.board_id.to_string(),
            })
        })?;
        let (task, ()) = self.mutate_task_scanning(task_id, |task, reads| {
            Self::require_task_owner(task, actor)?;

            if task.status != TaskStatus::Done {
                return Err(KanbanError::NotInRequiredStatus {
                    task: task_id,
                    action: "reopened",
                    status: task.status,
                    required: "done",
                });
            }
            let Some(column) = board.column_for_status(TaskStatus::InProgress) else {
                return Err(KanbanError::InvalidTransition {
                    task: task_id,
                    from: task.status,
                    to: TaskStatus::InProgress,
                });
            };
            if let Some(limit) = column.wip_limit {
                let count =
                    Self::count_status_in_txn(reads, task.board_id, TaskStatus::InProgress)?;
                if count >= limit as usize {
                    return Err(KanbanError::WipLimitExceeded {
                        column: column.name.clone(),
                        limit,
                        current: count as u32,
                    });
                }
            }

            task.status = TaskStatus::InProgress;
            task.verification = None;
            Ok(())
        })?;
        Ok(task)
    }

    /// Delete a board and all its tasks.
    ///
    /// pre:  board_id is valid; actor is a valid WebID — destructive ops
    ///       carry the actor, like `task_delete`
    /// post: board h_mem and all associated task/index h_mems are deleted from the database
    #[must_use = "result must be used"]
    pub(crate) fn board_delete(
        &self,
        board_id: BoardId,
        actor: WebID,
    ) -> Result<usize, KanbanError> {
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
        self.bump_changes(actor);
        Ok(task_ids.len())
    }

    // ── Kata Integration (task-scoped scientific thinking) ──────────

    // Moved to kata.rs.

    // ── Helpers ───────────────────────────────────────────────────────

    #[cfg(test)]
    fn pause_before_task_write(&self) {
        let pause = self
            .before_task_write
            .lock()
            .expect("task scheduling hook lock")
            .take();
        if let Some(pause) = pause {
            pause();
        }
    }

    /// Transform the latest task under the existing storage write lock. Callers
    /// must validate task-dependent authority/state here, not on a snapshot.
    /// Do not query the store in `change`: it already holds a pooled connection.
    pub(super) fn mutate_task<T>(
        &self,
        task_id: TaskId,
        change: impl FnOnce(&mut Task) -> Result<T, KanbanError>,
    ) -> Result<(Task, T), KanbanError> {
        #[cfg(test)]
        self.pause_before_task_write();
        let (task, result) = self
            .store
            .update_value_atomic::<_, TaskWriteError>(
                TASK_ENTITY,
                &task_id.to_string(),
                |current| {
                    let mut task: Task = serde_json::from_value(current).map_err(|error| {
                        TaskWriteError(KanbanError::Internal(format!(
                            "deserialization failed: {error}"
                        )))
                    })?;
                    let result = change(&mut task).map_err(TaskWriteError)?;
                    task.updated_at = chrono::Utc::now();
                    let value = serde_json::to_value(&task).map_err(|error| {
                        TaskWriteError(KanbanError::Internal(format!(
                            "serialization failed: {error}"
                        )))
                    })?;
                    Ok((Some(value), (task, result)))
                },
            )?
            .ok_or_else(|| {
                KanbanError::NotFound(NotFound {
                    entity_type: "task".to_string(),
                    id: task_id.to_string(),
                })
            })?;
        // P1: every successful task mutation moves the change counter — the
        // panel's cheap-poll signal. Bumped after the write commits, so a
        // failed mutation never moves the counter.
        self.bump_changes(task.owner);
        Ok((task, result))
    }

    /// Transform the latest task under the existing storage write lock, with
    /// in-transaction reads over related rows — the admission path for moves
    /// that must count sibling tasks: the count and the commit share one
    /// `IMMEDIATE` transaction, so a full column cannot admit two concurrent
    /// movers (each mover's count includes the other's committed move).
    /// Callers must validate task-dependent authority/state in `change`, not
    /// on a snapshot; `change` must not write through `reads` (the guard is
    /// read-only, and a nested pool acquisition would deadlock the
    /// connection the transaction already holds).
    pub(super) fn mutate_task_scanning<T>(
        &self,
        task_id: TaskId,
        change: impl FnOnce(&mut Task, &TxnReads<'_, '_>) -> Result<T, KanbanError>,
    ) -> Result<(Task, T), KanbanError> {
        #[cfg(test)]
        self.pause_before_task_write();
        let (task, result) = self
            .store
            .update_value_atomic_with_reads::<_, TaskWriteError>(
                TASK_ENTITY,
                &task_id.to_string(),
                |current, reads| {
                    let mut task: Task = serde_json::from_value(current).map_err(|error| {
                        TaskWriteError(KanbanError::Internal(format!(
                            "deserialization failed: {error}"
                        )))
                    })?;
                    let result = change(&mut task, reads).map_err(TaskWriteError)?;
                    task.updated_at = chrono::Utc::now();
                    let value = serde_json::to_value(&task).map_err(|error| {
                        TaskWriteError(KanbanError::Internal(format!(
                            "serialization failed: {error}"
                        )))
                    })?;
                    Ok((Some(value), (task, result)))
                },
            )?
            .ok_or_else(|| {
                KanbanError::NotFound(NotFound {
                    entity_type: "task".to_string(),
                    id: task_id.to_string(),
                })
            })?;
        // P1: every successful task mutation moves the change counter — the
        // panel's cheap-poll signal (shared with `mutate_task`).
        self.bump_changes(task.owner);
        Ok((task, result))
    }
}

/// The task/board write path's `HMemError` mapping. The crate-wide
/// `From<HMemError> for KanbanError` (goals.rs) carries the goal path's
/// "atomic goal transition failed" text; task and board writes keep their
/// operator-facing "h_mem update failed" storage error. Domain validation
/// errors pass through unchanged so callers retain their typed classification.
struct TaskWriteError(KanbanError);

impl From<HMemError> for TaskWriteError {
    fn from(error: HMemError) -> Self {
        TaskWriteError(KanbanError::Internal(format!(
            "h_mem update failed: {error}"
        )))
    }
}

impl From<TaskWriteError> for KanbanError {
    fn from(error: TaskWriteError) -> Self {
        error.0
    }
}
