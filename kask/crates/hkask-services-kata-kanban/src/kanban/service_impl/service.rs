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

use std::sync::Arc;

use hkask_storage::{HMem, HMemStore};
use hkask_types::NotFound;
use hkask_types::WebID;
use hkask_types::id::{BoardId, TaskId};
use serde_json::Value;

use super::types::KanbanError;

use crate::kanban::{
    Board, ColumnDef, GasEntry, Priority, Task, TaskFilter, TaskSpec, TaskStatus, Verification,
};

/// Core kanban coordination service.
///
/// Persists boards and tasks as RDF h_mems in a HMemStore.
/// Public surface: board and task coordination operations.
///
/// `Clone` is required because the REPL caches the service in `ReplState`
/// and clones it per-command to avoid holding a borrow across the REPL loop.
#[derive(Clone)]
pub struct KanbanService {
    pub(crate) store: HMemStore,
    pub(crate) pod_manager: Option<Arc<hkask_pods::pod::ActivePods>>,
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
            pod_manager: None,
        }
    }

    /// Attach a PodManager for live spawn capability.
    ///
    /// pre:  pm is a valid `Arc<PodManager>`
    /// post: returns Self with pod_manager set to Some(pm)
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_pod_manager(mut self, pm: Arc<hkask_pods::pod::ActivePods>) -> Self {
        self.pod_manager = Some(pm);
        self
    }

    /// Create a task-scoped gas accountant bound to a specific kanban task.
    ///
    /// The accountant decrements `task.gas_remaining` via `task_consume_gas`
    /// after each inference call. Attach it to a `KataEngine` via
    /// `with_task_gas_accountant` to close the per-task gas feedback loop.
    ///
    /// pre:  task_id refers to an existing task with a gas budget set
    /// post: returns a callback that deducts from the task
    #[must_use]
    pub fn gas_accountant_for(
        self: &Arc<Self>,
        task_id: TaskId,
    ) -> crate::kata::TaskGasAccountantFn {
        let service = Arc::clone(self);
        Arc::new(move |cost, reason| {
            service
                .task_consume_gas(task_id, cost, reason)
                .map_err(|e| {
                    crate::kata::KataError::InferenceFailed(format!("task gas deduction: {e}"))
                })
        })
    }

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

    // ── Board operations ──────────────────────────────────────────────────

    /// Create a new kanban board.
    ///
    /// pre:  owner is a valid WebID; name is non-empty; columns is non-empty
    /// post: board is persisted as a h_mem; returns the created Board
    #[must_use = "result must be used"]
    pub fn board_create(
        &self,
        owner: WebID,
        name: &str,
        columns: &[ColumnDef],
    ) -> Result<Board, KanbanError> {
        if name.is_empty() {
            return Err(KanbanError::InvalidInput("board name is empty".into()));
        }
        if columns.is_empty() {
            return Err(KanbanError::InvalidInput(
                "board must have at least one column".into(),
            ));
        }

        let board = Board::new(name.to_string(), owner, columns.to_vec());
        let value = serde_json::to_value(&board)
            .map_err(|e| KanbanError::Internal(format!("serialization failed: {e}")))?;

        let h_mem = HMem::new(BOARD_ENTITY, &board.id.to_string(), value, owner);
        self.store
            .insert(&h_mem)
            .map_err(|e| KanbanError::Internal(format!("h_mem insert failed: {e}")))?;

        // P9: Regulation span
        tracing::info!(
            target: "hkask.kanban",
            operation = "board_created",
            board_id = %board.id,
            name = %name,
            owner = %owner,
            "REG"
        );

        Ok(board)
    }

    /// Create a board from a YAML template file.
    ///
    /// pre:  template_path is a valid YAML file with board template schema
    /// post: board is created with template-defined columns, WIP limits, and phases
    #[must_use = "result must be used"]
    pub fn board_create_from_template(
        &self,
        owner: WebID,
        name: &str,
        template_yaml: &str,
    ) -> Result<Board, KanbanError> {
        #[derive(serde::Deserialize)]
        struct TemplateColumns {
            name: String,
            status: String,
            wip_limit: Option<u32>,
        }
        #[derive(serde::Deserialize)]
        struct TemplatePhase {
            name: String,
        }
        #[derive(serde::Deserialize)]
        struct BoardTemplate {
            columns: Vec<TemplateColumns>,
            phases: Vec<TemplatePhase>,
        }

        let template: BoardTemplate = serde_yaml_neo::from_str(template_yaml)
            .map_err(|e| KanbanError::InvalidInput(format!("Invalid template YAML: {e}")))?;

        let columns: Vec<ColumnDef> = template
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let status = TaskStatus::parse_str(&c.status).unwrap_or(TaskStatus::Backlog);
                let mut col = ColumnDef::new(c.name.clone(), status, i as u32);
                if let Some(wip) = c.wip_limit {
                    col = col.with_wip_limit(wip);
                }
                col
            })
            .collect();

        let board = self.board_create(owner, name, &columns)?;

        // Create phases
        for phase in &template.phases {
            self.board_add_phase(board.id, &phase.name, 0)?;
        }

        Ok(board)
    }

    /// List all board templates.
    ///
    /// post: returns Vec of known template names
    #[must_use]
    pub fn list_templates() -> Vec<String> {
        vec![
            "software-project".into(),
            "writing-project".into(),
            "scientific-research".into(),
            "investment-research".into(),
        ]
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
    /// post: returns all boards owned by this userpod
    #[must_use = "result must be used"]
    pub fn board_list(&self, owner: &WebID) -> Result<Vec<Board>, KanbanError> {
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

        boards.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(boards)
    }

    /// Get a board by ID.
    ///
    /// pre:  board_id is valid
    /// post: returns Some(Board) if found, None otherwise
    #[must_use = "result must be used"]
    pub fn board_get(&self, board_id: BoardId) -> Result<Option<Board>, KanbanError> {
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

    /// Render a text-based kanban board view.
    ///
    /// pre:  board_id refers to an existing board
    /// post: returns a formatted string showing columns with tasks arranged by status,
    ///       WIP limits, story points, labels, overdue indicators, and verification status
    #[must_use = "result must be used"]
    pub fn board_view(
        &self,
        board_id: BoardId,
        filter: Option<&str>,
    ) -> Result<String, KanbanError> {
        let board = self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;
        let mut tasks = self.task_list(board_id, TaskFilter::all())?;

        // Apply filter if present
        let filter_desc = if let Some(f) = filter {
            if let Some(s) = TaskStatus::parse_str(f) {
                tasks.retain(|t| t.status == s);
                Some(format!("status={}", s))
            } else if let Some(p) = Priority::parse_str(f) {
                tasks.retain(|t| t.priority == Some(p));
                Some(format!("priority={}", p))
            } else if let Ok(wid) = f.parse::<WebID>() {
                tasks.retain(|t| t.assignee == Some(wid));
                Some(format!("assignee={}", wid.redacted_display()))
            } else {
                let lower = f.to_lowercase();
                tasks.retain(|t| t.labels.iter().any(|l| l.to_lowercase().contains(&lower)));
                Some(format!("label~{}", f))
            }
        } else {
            None
        };

        let mut by_status: std::collections::HashMap<TaskStatus, Vec<&Task>> =
            std::collections::HashMap::new();
        for t in &tasks {
            by_status.entry(t.status).or_default().push(t);
        }

        let mut out = format!("{}  {}", board.name, board.id);
        if let Some(ref d) = filter_desc {
            out.push_str(&format!("  [{}]", d));
        }
        out.push_str("\n\n");

        for col in &board.columns {
            let count = by_status.get(&col.status).map(|v| v.len()).unwrap_or(0);
            if count == 0 && !tasks.is_empty() {
                continue;
            }
            let wip = col.wip_limit.map_or(String::new(), |l| format!("/{}", l));
            out.push_str(&format!("  {}{} ({})\n", col.name, wip, count));
        }
        out.push('\n');

        for col in &board.columns {
            let col_tasks = by_status
                .get(&col.status)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            if col_tasks.is_empty() {
                continue;
            }
            out.push_str(&format!("  {}:\n", col.name));
            for task in col_tasks {
                let idx = tasks.iter().position(|t| t.id == task.id).unwrap_or(0) + 1;
                let a = task
                    .assignee
                    .map(|a| format!(" <- {}", a.redacted_display()))
                    .unwrap_or_default();
                let p = task
                    .priority
                    .map(|p| match p {
                        Priority::Critical => " !!",
                        Priority::High => " !",
                        _ => "",
                    })
                    .unwrap_or("");
                out.push_str(&format!("    {}. {}{}{}\n", idx, task.title, p, a));
            }
            out.push('\n');
        }

        if tasks.is_empty() && filter.is_some() {
            out.push_str("  (no tasks match)\n");
        }

        Ok(out)
    }

    // ── Task operations ───────────────────────────────────────────────────

    /// Create a new task on a board.
    ///
    /// pre:  board_id refers to an existing board; spec.title is non-empty; owner is valid
    /// post: task is persisted as a h_mem; returns the created Task
    #[must_use = "result must be used"]
    pub fn task_create(
        &self,
        board_id: BoardId,
        spec: TaskSpec,
        owner: WebID,
    ) -> Result<Task, KanbanError> {
        // Verify board exists
        let board = self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;
        let _ = board;

        // Extract sizing fields before Task::new consumes the spec
        let sp = spec.story_points;
        let eh = spec.estimated_hours;
        let pr = spec.priority;
        let lbls = spec.labels.clone();
        let ph = spec.phase_id;
        let mut task = Task::new(board_id, spec, owner);
        task.story_points = sp;
        task.estimated_hours = eh;
        task.labels = lbls;
        task.priority = pr;
        task.phase_id = ph;
        let value = serde_json::to_value(&task)
            .map_err(|e| KanbanError::Internal(format!("serialization failed: {e}")))?;

        // Persist the task
        let h_mem = HMem::new(TASK_ENTITY, &task.id.to_string(), value, owner);
        self.store
            .insert(&h_mem)
            .map_err(|e| KanbanError::Internal(format!("h_mem insert failed: {e}")))?;

        // Persist board→task index
        let index_entity = format!("{BOARD_TASKS_PREFIX}{board_id}");
        let index_triple = HMem::new(
            &index_entity,
            &task.id.to_string(),
            Value::String(task.id.to_string()),
            owner,
        );
        self.store
            .insert(&index_triple)
            .map_err(|e| KanbanError::Internal(format!("index h_mem insert failed: {e}")))?;

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
    pub fn task_list(
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

        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
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
    pub fn task_get(&self, task_id: TaskId) -> Result<Option<Task>, KanbanError> {
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
    pub fn task_move(
        &self,
        task_id: TaskId,
        target: TaskStatus,
        actor: WebID,
    ) -> Result<Task, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

        Self::require_task_actor(&task, actor)?;

        if !task.can_move_to(target) {
            return Err(KanbanError::InvalidTransition {
                task: task_id,
                from: task.status,
                to: target,
            });
        }

        let from_status = task.status;

        // WIP limit enforcement (Anderson §4: "limit WIP to expose problems")
        if let Some(board) = self.board_get(task.board_id)?
            && let Some(col) = board.column_for_status(target)
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

        // Update the h_mem value
        let new_value = serde_json::to_value(&task)
            .map_err(|e| KanbanError::Internal(format!("serialization failed: {e}")))?;

        // Find the h_mem ID and update
        let h_mems = self
            .store
            .query_by_entity_attribute(TASK_ENTITY, &task_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;

        if let Some(t) = h_mems.into_iter().next() {
            self.store
                .update(&t.id, new_value, 1.0f64)
                .map_err(|e| KanbanError::Internal(format!("h_mem update failed: {e}")))?;
        }

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
    pub fn task_claim(&self, task_id: TaskId, actor: WebID) -> Result<Task, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

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

        let new_value = serde_json::to_value(&task)
            .map_err(|e| KanbanError::Internal(format!("serialization failed: {e}")))?;

        let h_mems = self
            .store
            .query_by_entity_attribute(TASK_ENTITY, &task_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;

        if let Some(t) = h_mems.into_iter().next() {
            self.store
                .update(&t.id, new_value, 1.0f64)
                .map_err(|e| KanbanError::Internal(format!("h_mem update failed: {e}")))?;
        }

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
    pub fn task_verify(
        &self,
        task_id: TaskId,
        evidence: &str,
        verifier: WebID,
    ) -> Result<(Task, Verification), KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

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

        let new_value = serde_json::to_value(&task)
            .map_err(|e| KanbanError::Internal(format!("serialization failed: {e}")))?;

        let h_mems = self
            .store
            .query_by_entity_attribute(TASK_ENTITY, &task_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;

        if let Some(t) = h_mems.into_iter().next() {
            self.store
                .update(&t.id, new_value, 1.0f64)
                .map_err(|e| KanbanError::Internal(format!("h_mem update failed: {e}")))?;
        }

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

    /// Delete a task and its board index entry.
    ///
    /// pre:  task_id is valid
    /// post: task h_mem and index h_mem are soft-deleted
    #[must_use = "result must be used"]
    pub fn task_delete(&self, task_id: TaskId) -> Result<(), KanbanError> {
        let task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

        // Close the task h_mem
        let h_mems = self
            .store
            .query_by_entity_attribute(TASK_ENTITY, &task_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;
        for t in &h_mems {
            self.store
                .close_by_id(&t.id)
                .map_err(|e| KanbanError::Internal(format!("h_mem close failed: {e}")))?;
        }

        // Close the index h_mem
        let index_entity = format!("{BOARD_TASKS_PREFIX}{}", task.board_id);
        let idx_triples = self
            .store
            .query_by_entity_attribute(&index_entity, &task_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("index query failed: {e}")))?;
        for t in &idx_triples {
            self.store
                .close_by_id(&t.id)
                .map_err(|e| KanbanError::Internal(format!("index close failed: {e}")))?;
        }

        Ok(())
    }

    /// Unassign a task — remove the assignee.
    ///
    /// Authority model: the task owner (creator) has unilateral unassignment
    /// authority. This is consistent with kanban semantics where the task
    /// creator owns the task lifecycle. The assignee's consent is not required
    /// for unassignment because the owner bears the responsibility for the
    /// task's completion. The `unjam_fix` auto-unassign uses the same path
    /// with `task.owner` as the actor after a 24h idle timeout.
    ///
    /// pre:  task_id is valid; actor is the task owner
    /// post: task.assignee is set to None; task.updated_at refreshed
    #[must_use = "result must be used"]
    pub fn task_unassign(&self, task_id: TaskId, actor: WebID) -> Result<Task, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;
        Self::require_task_owner(&task, actor)?;
        task.assignee = None;
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(task)
    }

    /// Reopen a completed task — move from Done back to InProgress.
    ///
    /// pre:  task_id refers to a task in Done status
    /// post: task moves to InProgress, verification cleared
    #[must_use = "result must be used"]
    pub fn task_reopen(&self, task_id: TaskId, actor: WebID) -> Result<Task, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

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

    /// Add gas (rJoules) to a task's remaining budget.
    ///
    /// Called by the delegating agent to refill a subagent's gas budget
    /// so it can continue work after exhausting its initial budget.
    #[must_use = "result must be used"]
    pub fn task_add_gas(
        &self,
        task_id: TaskId,
        amount: u64,
        actor: WebID,
    ) -> Result<Task, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;
        Self::require_task_owner(&task, actor)?;
        let current = task.gas_remaining.unwrap_or(0);
        task.gas_remaining = Some(current.saturating_add(amount));
        task.gas_spend.push(GasEntry::gas_refill(amount));
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        tracing::info!(
            target: "hkask.kanban",
            operation = "task_gas_added",
            task_id = %task_id,
            added = amount,
            new_remaining = task.gas_remaining,
            "REG"
        );
        Ok(task)
    }

    /// Add rJoules to a task's inference/API budget.
    #[must_use = "result must be used"]
    pub fn task_add_rjoules(
        &self,
        task_id: TaskId,
        amount: u64,
        actor: WebID,
    ) -> Result<Task, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;
        Self::require_task_owner(&task, actor)?;
        let current = task.rjoule_remaining.unwrap_or(0);
        task.rjoule_remaining = Some(current.saturating_add(amount));
        task.gas_spend.push(GasEntry::rjoule_refill(amount));
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        tracing::info!(
            target: "hkask.kanban",
            operation = "task_rjoules_added",
            task_id = %task_id,
            added = amount,
            new_remaining = task.rjoule_remaining,
            "REG"
        );
        Ok(task)
    }

    /// Delete a board and all its tasks.
    ///
    /// pre:  board_id is valid
    /// post: board h_mem and all associated task/index h_mems are soft-deleted
    #[must_use = "result must be used"]
    pub fn board_delete(&self, board_id: BoardId) -> Result<usize, KanbanError> {
        let board = self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;

        // Delete all tasks on this board
        let tasks = self.task_list(board_id, TaskFilter::all())?;
        let mut deleted_count = 0usize;
        for task in &tasks {
            match self.task_delete(task.id) {
                Ok(()) => deleted_count += 1,
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.kanban",
                        operation = "board_delete",
                        board_id = %board_id,
                        task_id = %task.id,
                        error = %e,
                        "Failed to delete task during board deletion"
                    );
                }
            }
        }

        // Close the board h_mem
        let h_mems = self
            .store
            .query_by_entity_attribute(BOARD_ENTITY, &board_id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;
        for t in &h_mems {
            self.store
                .close_by_id(&t.id)
                .map_err(|e| KanbanError::Internal(format!("h_mem close failed: {e}")))?;
        }
        let _ = board;

        Ok(deleted_count)
    }

    // ── De-jamming ────────────────────────────────────────────────────

    // Moved to dejam.rs.

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

    pub(crate) fn update_board_triple(&self, board: &Board) -> Result<(), KanbanError> {
        let new_value = serde_json::to_value(board)
            .map_err(|e| KanbanError::Internal(format!("serialization failed: {e}")))?;
        let h_mems = self
            .store
            .query_by_entity_attribute(BOARD_ENTITY, &board.id.to_string())
            .map_err(|e| KanbanError::Internal(format!("h_mem query failed: {e}")))?;
        if let Some(t) = h_mems.into_iter().next() {
            self.store
                .update(&t.id, new_value, 1.0f64)
                .map_err(|e| KanbanError::Internal(format!("h_mem update failed: {e}")))?;
        }
        Ok(())
    }
}
