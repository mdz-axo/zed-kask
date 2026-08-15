#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hkask-mcp-kata-kanban — Kata-Kanban workflow coordination MCP server.
//!
//! Provides 20 MCP tools for kanban board and task management.
//! All tools carry the caller's WebID for P12 compliance.
//!
//! The KanbanServer struct and tool methods are exported from the library
//! target to enable fuzz testing (P5 Testing Discipline, P4 Clear Boundaries).

pub mod idempotency;
pub mod kanban;
pub mod kata;
pub mod pko;
pub mod types;

// Re-export the kata-kanban service API at crate root (folded from hkask-services-kata-kanban).
pub use kanban::{
    Board, ColumnDef, KanbanError, KanbanService, Priority, SpawnSpec, Task, TaskFilter, TaskSpec,
    TaskStatus, UnjamFix, UnjamItem, Verification, VerificationCriterion,
};
pub use kata::{
    ImprovementDirection, ImprovementSignal, KataEngine, KataError, KataHistory, KataManifest,
    KataResult, KataState, KataStep, PracticeEntry, StepExperience, TaskGasAccountantFn,
};

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_mcp_server::server::{McpToolError, ServerContext, execute_tool_semantic, resolve_credential};
use hkask_mcp_swarm::{
    LazyLocalSwarmRuntime, LocalAgentCapabilities, LocalAgentCard, LocalAgentRegistry,
};
use hkask_storage::HMemStore;
use pko::kanban_type_to_pko;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use std::sync::Arc;
use types::*;

// ── Server ──────────────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct KanbanServer {
        pub service: KanbanService,
        /// Local swarm runtime — kanban_task_spawn delegates task execution to a
        /// local agent (ledger-funded inference + guard + skill cascade). Shared
        /// ledger path with hkask-mcp-swarm so operator funding is reusable.
        /// Used as the fallback when the worktree spawn port is unavailable.
        pub local_runtime: Arc<LazyLocalSwarmRuntime>,
        /// Local agent registry — reusable expert agents (cards on disk). When a
        /// spawn's `delegated_skills` are covered by an existing card, it is
        /// reused; otherwise a task-specific agent is built in-memory.
        pub local_registry: Arc<LocalAgentRegistry>,
        /// Worktree spawn port — when available, `kanban_task_spawn` creates a
        /// worktree-backed agent thread (isolated git worktree) via the zed IPC
        /// bridge instead of the in-memory `LazyLocalSwarmRuntime`. When
        /// unavailable (no IPC socket, no active workspace), falls back to
        /// in-memory spawn.
        pub worktree_spawn_port: Arc<dyn hkask_types::WorktreeSpawnPort>,
        /// Replay protection for the three tools a duplicate call would harm
        /// (`kanban_board_create`, `kanban_task_create`, `kanban_task_spawn`).
        /// Shares the kanban database, so protection has the same durability as
        /// the writes it guards. See `crate::idempotency`.
        pub idempotency: Arc<idempotency::IdempotencyStore>,
    }
);

/// Run `work` under replay protection when the caller supplied a key.
///
/// Without a key this is a plain pass-through, so the three protected tools keep
/// working for callers that do not opt in.
///
/// With a key, the three outcomes map to what the client can safely do:
/// - first call → run the work, record the response for later replays;
/// - replay of a completed call → return the original response verbatim, marked
///   `replayed: true`, without re-running;
/// - replay of a call that never completed → refuse, because whether the work
///   landed is exactly what is unknown. Re-running could duplicate it and
///   claiming success could invent a result.
///
/// A failed call releases the claim so a retry starts clean rather than
/// inheriting an "outcome unknown" verdict for work that demonstrably did not
/// happen.
async fn with_idempotency<F>(
    store: &idempotency::IdempotencyStore,
    tool: &'static str,
    key: Option<&str>,
    work: F,
) -> Result<serde_json::Value, McpToolError>
where
    F: std::future::Future<Output = Result<serde_json::Value, McpToolError>>,
{
    let Some(key) = key else {
        return work.await;
    };
    idempotency::IdempotencyStore::validate_key(key)
        .map_err(|error| McpToolError::invalid_argument(error.to_string()))?;

    // Fail closed: if the claim cannot be recorded, the caller asked for replay
    // protection and must not be handed a call that silently lacks it.
    let reservation = store.reserve(tool, key).map_err(|error| {
        McpToolError::unavailable(format!(
            "replay-protection store unavailable, refusing to run {tool} unprotected: {error}"
        ))
    })?;

    match reservation {
        idempotency::Reservation::Replay { response } => {
            // Return the first call's result. `replayed` lets a caller
            // distinguish "your retry was absorbed" from "this ran now".
            // `response` was written by this same function's Fresh arm via
            // `serde_json::to_string`, so a parse failure means our own store is
            // corrupt — not a caller error and not a per-variant domain error.
            let mut value: serde_json::Value = serde_json::from_str(&response).map_err(|e| {
                McpToolError::internal(format!("stored idempotent response is not JSON: {e}")) // rr0044-ok: deserialize-own-struct
            })?;
            if let Some(object) = value.as_object_mut() {
                object.insert("replayed".to_string(), serde_json::Value::Bool(true));
            }
            Ok(value)
        }
        idempotency::Reservation::Pending => Err(McpToolError::unavailable(format!(
            "a previous {tool} call with this idempotency_key did not complete — its \
             outcome is unknown. Re-read the board to see whether it took effect; do not \
             reuse this key."
        ))),
        idempotency::Reservation::Fresh => match work.await {
            Ok(mut value) => {
                // Tell the caller when the guarantee is only process-local, so a
                // restart-crossing retry is not wrongly believed to be protected.
                if !store.is_durable()
                    && let Some(object) = value.as_object_mut()
                {
                    object.insert(
                        "idempotency_durable".to_string(),
                        serde_json::Value::Bool(false),
                    );
                }
                match serde_json::to_string(&value) {
                    Ok(response) => store.record(tool, key, &response),
                    Err(error) => {
                        // The work succeeded; only bookkeeping failed. Release so a
                        // retry re-runs rather than being told "outcome unknown".
                        tracing::warn!(
                            target: "hkask.mcp.kata_kanban",
                            tool = %tool,
                            %error,
                            "could not serialize response for replay protection - \
                             releasing the claim"
                        );
                        store.release(tool, key);
                    }
                }
                Ok(value)
            }
            Err(error) => {
                // Clean failure: nothing landed, so free the key for a retry.
                store.release(tool, key);
                Err(error)
            }
        },
    }
}

/// Build a task-specific local agent card for `kanban_task_spawn` when no
/// reusable expert agent covers the requested skills. The agent runs in-memory
/// (not persisted to the registry) with the delegated skills as its declared
/// skill set — `AgentExecutor::run` executes each skill cascade against the
/// task before the LLM call. An empty `model` lets the inference port pick its
/// default; an empty `mcp_tools` set means the agent runs skill + LLM only.
fn build_task_agent_card(
    task_id: hkask_types::TaskId,
    title: &str,
    skills: &[String],
) -> LocalAgentCard {
    LocalAgentCard {
        agent_id: format!("kanban-task-{task_id}"),
        agent_type: "task".to_string(),
        description: title.to_string(),
        accepts: vec!["task".to_string()],
        produces: vec!["task_result".to_string()],
        dependencies: Default::default(),
        capabilities: LocalAgentCapabilities {
            min_provider_class: "local".to_string(),
            system_prompt: Some(
                "You are a task-execution agent spawned by the kata-kanban. \
                 Complete the assigned task using your declared skills. \
                 Produce a concise result the spawning agent can record."
                    .to_string(),
            ),
            skills: skills.to_vec(),
            ..Default::default()
        },
        cloud_id: None,
        ..Default::default()
    }
}

/// Derive a one-line `TaskActivity` from a task's most recent comment (R3).
/// The spawn/delegation flow already appends durable comments ("Spawn
/// executed: agent=…, tokens=…", "Spawned worktree agent for task …"), so
/// this surfaces the latest one as the card's status strip without a new
/// ingest channel. The live per-tool-call hook path is a follow-up that
/// swaps this data source without touching the widget.
fn derive_task_activity(task: &Task) -> Option<TaskActivity> {
    let comment = task.comments.last()?;
    Some(TaskActivity {
        text: comment.body.clone(),
        kind: "comment".to_string(),
        at: comment.created_at.to_rfc3339(),
        ontology: kanban_type_to_pko("Task").map(|s| s.to_string()),
    })
}

#[cfg(test)]
mod tool_surface_tests {
    use super::*;

    // Pins the registered tool-surface count end-to-end. Catches silent
    // registration drops — a `#[tool]` impl block without `#[tool_router]`
    // silently registers nothing (`cargo check` passes on an unwired orphan).
    // Mirrors the swarm pin.
    #[test]
    fn tool_surface_is_exactly_23_registered_tools() {
        let n = KanbanServer::tool_router().list_all().len();
        assert_eq!(
            n, 25,
            "kata-kanban registered tool surface changed; got {n}"
        );
    }
}

#[tool_router(server_handler)]
impl KanbanServer {
    #[tool(description = "Create a new kanban board with optional custom columns")]
    pub async fn kanban_board_create(
        &self,
        Parameters(BoardCreateRequest {
            name,
            columns,
            idempotency_key,
        }): Parameters<BoardCreateRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_board_create",
            kanban_type_to_pko("kanban_board_create"),
            with_idempotency(
                &self.idempotency,
                "kanban_board_create",
                idempotency_key.as_deref(),
                async {
                    let column_defs = match columns {
                        Some(inputs) => inputs
                            .into_iter()
                            .enumerate()
                            .map(
                                |(i, input)| match crate::TaskStatus::parse_str(&input.status) {
                                    Some(s) => {
                                        let mut col =
                                            crate::ColumnDef::new(input.name, s, i as u32);
                                        if let Some(wip) = input.wip_limit {
                                            col = col.with_wip_limit(wip);
                                        }
                                        Ok(col)
                                    }
                                    None => Err(format!("invalid status: {}", input.status)),
                                },
                            )
                            .collect::<Result<Vec<_>, _>>(),
                        None => Ok(crate::KanbanService::standard_columns()),
                    };
                    let cols = match column_defs {
                        Ok(c) => c,
                        Err(e) => return Err(McpToolError::invalid_argument(e)),
                    };
                    match self.service.board_create(self.webid, &name, &cols) {
                        Ok(board) => Ok(serde_json::to_value(BoardCreateResponse {
                            board_id: board.id.to_string(),
                            name: board.name,
                            columns: board
                                .columns
                                .iter()
                                .map(|c| ColumnInfo {
                                    id: c.id.to_string(),
                                    name: c.name.clone(),
                                    status: c.status.to_string(),
                                    wip_limit: c.wip_limit,
                                })
                                .collect(),
                            ontology: kanban_type_to_pko("Board").map(|s| s.to_string()),
                        })
                        .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                        Err(e) => Err(map_kanban_error(e)),
                    }
                },
            ),
        )
        .await
    }

    #[tool(description = "List all kanban boards owned by the caller")]
    pub async fn kanban_board_list(
        &self,
        Parameters(BoardListRequest {}): Parameters<BoardListRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_board_list",
            kanban_type_to_pko("kanban_board_list"),
            async {
                match self.service.board_list(&self.webid) {
                    Ok(boards) => Ok(serde_json::to_value(BoardListResponse {
                        boards: boards
                            .into_iter()
                            .map(|b| BoardInfo {
                                board_id: b.id.to_string(),
                                name: b.name,
                                column_count: b.columns.len(),
                                columns: b
                                    .columns
                                    .iter()
                                    .map(|c| ColumnInfo {
                                        id: c.id.to_string(),
                                        name: c.name.clone(),
                                        status: c.status.to_string(),
                                        wip_limit: c.wip_limit,
                                    })
                                    .collect(),
                                ontology: kanban_type_to_pko("Board").map(|s| s.to_string()),
                            })
                            .collect(),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    /// Delete a kanban board and all its tasks. Exposes the existing
    /// `KanbanService::board_delete` method as an MCP tool — closes the gap
    /// where `board_delete` was service-only and unreachable via MCP.
    ///
    /// contract: P3-svc-kanban-011
    /// expect: "I can delete a kanban board I own via MCP" \[P3\]
    /// pre:  board_id is a valid board id owned by the caller
    /// post: the board and all its tasks are deleted; returns the task count
    #[tool(
        description = "Delete a kanban board and all its tasks. Exposes KanbanService::board_delete as an MCP tool."
    )]
    pub async fn kanban_board_delete(
        &self,
        Parameters(BoardDeleteRequest { board_id }): Parameters<BoardDeleteRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_board_delete",
            kanban_type_to_pko("kanban_board_delete"),
            async {
                let bid = parse_board_id(&board_id)?;
                // Verify ownership before delete — only the board owner can
                // delete it (P12).
                let board = self
                    .service
                    .board_get(bid)
                    .map_err(map_kanban_error)?
                    .ok_or_else(|| McpToolError::not_found(format!("board {bid} not found")))?;
                if board.owner != self.webid {
                    return Err(McpToolError::invalid_argument(format!(
                        "board {bid} is not owned by caller — cannot delete"
                    )));
                }
                let tasks_deleted = self.service.board_delete(bid).map_err(map_kanban_error)?;
                serde_json::to_value(BoardDeleteResponse {
                    board_id: bid.to_string(),
                    tasks_deleted,
                    ontology: kanban_type_to_pko("kanban_board_delete").map(|s| s.to_string()),
                })
                .map_err(|e| McpToolError::internal(e.to_string())) // rr0044-ok: serialize-own-struct
            },
        )
        .await
    }

    #[tool(description = "Create a new task on a kanban board")]
    pub async fn kanban_task_create(
        &self,
        Parameters(TaskCreateRequest {
            board_id,
            title,
            description,
            criteria,
            idempotency_key,
            gas_budget,
            rjoule_budget,
        }): Parameters<TaskCreateRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_create",
            kanban_type_to_pko("kanban_task_create"),
            with_idempotency(
                &self.idempotency,
                "kanban_task_create",
                idempotency_key.as_deref(),
                async {
                    let bid = parse_board_id(&board_id)?;
                    let mut spec = TaskSpec::new(title);
                    if let Some(d) = description {
                        spec = spec.with_description(d);
                    }
                    if let Some(cs) = criteria {
                        spec = spec.with_criteria(
                            cs.into_iter().map(VerificationCriterion::new).collect(),
                        );
                    }
                    if let Some(gas) = gas_budget {
                        spec = spec.with_gas_budget(gas);
                    }
                    if let Some(rj) = rjoule_budget {
                        spec.rjoule_budget = Some(rj);
                    }

                    match self.service.task_create(bid, spec, self.webid) {
                        Ok(task) => Ok(serde_json::to_value(TaskCreateResponse {
                            task_id: task.id.to_string(),
                            board_id: task.board_id.to_string(),
                            title: task.title,
                            status: task.status.to_string(),
                            ontology: kanban_type_to_pko("Task").map(|s| s.to_string()),
                        })
                        .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                        Err(e) => Err(map_kanban_error(e)),
                    }
                },
            ),
        )
        .await
    }

    #[tool(
        description = "Update editable fields on a task (title, description, criteria, priority, labels). Only the task owner can edit."
    )]
    pub async fn kanban_task_update(
        &self,
        Parameters(TaskUpdateRequest {
            task_id,
            title,
            description,
            criteria,
            priority,
            labels,
        }): Parameters<TaskUpdateRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_update",
            kanban_type_to_pko("kanban_task_update"),
            async {
                let tid = parse_task_id(&task_id)?;

                // The MCP layer uses single-Option for description and priority:
                // None means "no change," a present value means "set to this."
                // An empty string for description clears the field; an empty
                // string for priority clears the field.
                let description_update =
                    description.map(|d| if d.is_empty() { None } else { Some(d) });
                let priority_update = match priority {
                    None => None,
                    Some(ref p) if p.is_empty() => Some(None),
                    Some(p) => {
                        let parsed = Priority::parse_str(&p).ok_or_else(|| {
                            McpToolError::invalid_argument(format!("invalid priority: {p}"))
                        })?;
                        Some(Some(parsed))
                    }
                };

                let criteria_update =
                    criteria.map(|cs| cs.into_iter().map(VerificationCriterion::new).collect());

                match self.service.task_update(
                    tid,
                    self.webid,
                    title,
                    description_update,
                    criteria_update,
                    priority_update,
                    labels,
                ) {
                    Ok(task) => Ok(serde_json::to_value(TaskUpdateResponse {
                        task_id: task.id.to_string(),
                        title: task.title,
                        ontology: kanban_type_to_pko("Task").map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(description = "List tasks on a kanban board, optionally filtered by status")]
    pub async fn kanban_task_list(
        &self,
        Parameters(TaskListRequest { board_id, status }): Parameters<TaskListRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_list",
            kanban_type_to_pko("kanban_task_list"),
            async {
                let bid = parse_board_id(&board_id)?;
                let filter = match status {
                    Some(s) => match crate::TaskStatus::parse_str(&s) {
                        Some(st) => TaskFilter::by_status(st),
                        None => {
                            return Err(McpToolError::invalid_argument(format!(
                                "invalid status: {s}"
                            )));
                        }
                    },
                    None => TaskFilter::all(),
                };
                match self.service.task_list(bid, filter) {
                    Ok(tasks) => Ok(serde_json::to_value(TaskListResponse {
                        tasks: tasks
                            .into_iter()
                            .map(|t| {
                                let activity = derive_task_activity(&t);
                                TaskInfo {
                                    task_id: t.id.to_string(),
                                    board_id: t.board_id.to_string(),
                                    title: t.title,
                                    status: t.status.to_string(),
                                    assignee: t.assignee.map(|a| a.to_string()),
                                    criteria_count: t.criteria.len(),
                                    gas_remaining: t.gas_remaining,
                                    rjoule_remaining: t.rjoule_remaining,
                                    swarm_id: t.swarm_id,
                                    activity,
                                    ontology: kanban_type_to_pko("Task").map(|s| s.to_string()),
                                }
                            })
                            .collect(),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(description = "Move a task to a new column (status transition)")]
    pub async fn kanban_task_move(
        &self,
        Parameters(TaskMoveRequest {
            task_id,
            target_status,
        }): Parameters<TaskMoveRequest>,
    ) -> String {
        use pko::kanban_type_to_pko;

        execute_tool_semantic(
            self,
            "kanban_task_move",
            kanban_type_to_pko("kanban_task_move"),
            async {
                let tid = parse_task_id(&task_id)?;
                let previous_status = match self.service.task_get(tid) {
                    Ok(Some(t)) => t.status.to_string(),
                    Ok(None) => {
                        return Err(McpToolError::not_found(format!(
                            "task not found: {task_id}"
                        )));
                    }
                    Err(e) => return Err(map_kanban_error(e)),
                };
                let target = match crate::TaskStatus::parse_str(&target_status) {
                    Some(s) => s,
                    None => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid target_status: {target_status}"
                        )));
                    }
                };
                match self.service.task_move(tid, target, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskMoveResponse {
                        task_id: task.id.to_string(),
                        previous_status,
                        new_status: task.status.to_string(),
                        ontology: kanban_type_to_pko("kanban_task_move").map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(description = "Claim an unassigned task as the authenticated caller")]
    pub async fn kanban_task_assign(
        &self,
        Parameters(TaskAssignRequest { task_id }): Parameters<TaskAssignRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_assign",
            kanban_type_to_pko("kanban_task_assign"),
            async {
                let tid = parse_task_id(&task_id)?;
                match self.service.task_claim(tid, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskAssignResponse {
                        task_id: task.id.to_string(),
                        assignee: task.assignee.map(|a| a.to_string()).unwrap_or_default(),
                        ontology: kanban_type_to_pko("kanban_task_assign").map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(
        description = "Delete a task and its board index entry. Exposes KanbanService::task_delete as an MCP tool."
    )]
    pub async fn kanban_task_delete(
        &self,
        Parameters(TaskDeleteRequest { task_id }): Parameters<TaskDeleteRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_delete",
            kanban_type_to_pko("kanban_task_delete"),
            async {
                let tid = parse_task_id(&task_id)?;
                self.service.task_delete(tid).map_err(map_kanban_error)?;
                serde_json::to_value(TaskDeleteResponse {
                    task_id: tid.to_string(),
                    ontology: kanban_type_to_pko("Task").map(|s| s.to_string()),
                })
                .map_err(|e| McpToolError::internal(e.to_string())) // rr0044-ok: serialize-own-struct
            },
        )
        .await
    }

    #[tool(
        description = "Unassign a task — remove the current assignee. Only the task owner can unassign."
    )]
    pub async fn kanban_task_unassign(
        &self,
        Parameters(TaskUnassignRequest { task_id }): Parameters<TaskUnassignRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_unassign",
            kanban_type_to_pko("kanban_task_unassign"),
            async {
                let tid = parse_task_id(&task_id)?;
                match self.service.task_unassign(tid, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskUnassignResponse {
                        task_id: task.id.to_string(),
                        ontology: kanban_type_to_pko("Task").map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(
        description = "Record verification evidence for a task in Review. The evidence text is the pass signal; acceptance criteria are guidance, not a gate."
    )]
    pub async fn kanban_task_verify(
        &self,
        Parameters(TaskVerifyRequest { task_id, evidence }): Parameters<TaskVerifyRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_verify",
            kanban_type_to_pko("kanban_task_verify"),
            async {
                let tid = parse_task_id(&task_id)?;
                if evidence.trim().is_empty() {
                    return Err(McpToolError::invalid_argument("evidence must not be empty"));
                }
                match self.service.task_verify(tid, &evidence, self.webid) {
                    Ok((task, verification)) => Ok(serde_json::to_value(TaskVerifyResponse {
                        task_id: task.id.to_string(),
                        passed: verification.passed,
                        reasoning: verification.reasoning,
                        new_status: task.status.to_string(),
                        ontology: kanban_type_to_pko("kanban_task_verify").map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(
        description = "Add gas/rJoules to a task's remaining budget so the subagent can continue"
    )]
    pub async fn kanban_task_add_gas(
        &self,
        Parameters(TaskAddGasRequest { task_id, amount }): Parameters<TaskAddGasRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_add_gas",
            kanban_type_to_pko("kanban_task_add_gas"),
            async {
                let tid = parse_task_id(&task_id)?;
                if amount == 0 {
                    return Err(McpToolError::invalid_argument("amount must be > 0"));
                }
                match self.service.task_add_gas(tid, amount, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskAddGasResponse {
                        task_id: task.id.to_string(),
                        new_gas_remaining: task.gas_remaining.unwrap_or(0),
                        ontology: kanban_type_to_pko("kanban_task_add_gas").map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(description = "Add rJoules to a task's inference/API budget (250k ≈ $1 spend)")]
    pub async fn kanban_task_add_rjoules(
        &self,
        Parameters(TaskAddRjoulesRequest { task_id, amount }): Parameters<TaskAddRjoulesRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_add_rjoules",
            kanban_type_to_pko("kanban_task_add_rjoules"),
            async {
                let tid = parse_task_id(&task_id)?;
                if amount == 0 {
                    return Err(McpToolError::invalid_argument("amount must be > 0"));
                }
                match self.service.task_add_rjoules(tid, amount, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskAddRjoulesResponse {
                        task_id: task.id.to_string(),
                        new_rjoule_remaining: task.rjoule_remaining.unwrap_or(0),
                        ontology: kanban_type_to_pko("kanban_task_add_rjoules")
                            .map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(
        description = "Add a comment to a task (feedback thread for subagent↔agent communication)"
    )]
    pub async fn kanban_task_comment(
        &self,
        Parameters(TaskCommentRequest { task_id, body }): Parameters<TaskCommentRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_comment",
            kanban_type_to_pko("kanban_task_comment"),
            async {
                let tid = parse_task_id(&task_id)?;
                if body.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "comment body must not be empty",
                    ));
                }
                match self.service.task_comment(tid, self.webid, &body) {
                    Ok(comment) => Ok(serde_json::to_value(TaskCommentResponse {
                        comment_id: comment.id.to_string(),
                        task_id: comment.task_id.to_string(),
                        author: comment.author.to_string(),
                        body: comment.body,
                        created_at: comment.created_at.to_rfc3339(),
                        ontology: kanban_type_to_pko("Comment").map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(
        description = "Fetch task comments starting from an index (for incremental memory ingestion)"
    )]
    pub async fn kanban_task_comments_since(
        &self,
        Parameters(TaskCommentsSinceRequest {
            task_id,
            since_index,
        }): Parameters<TaskCommentsSinceRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_comments_since",
            kanban_type_to_pko("kanban_task_comments_since"),
            async {
                let tid = parse_task_id(&task_id)?;
                match self.service.task_comments_since(tid, since_index) {
                    Ok(comments) => {
                        let total = comments.len() + since_index;
                        let mapped: Vec<TaskCommentResponse> = comments
                            .into_iter()
                            .map(|c| TaskCommentResponse {
                                comment_id: c.id.to_string(),
                                task_id: c.task_id.to_string(),
                                author: c.author.to_string(),
                                body: c.body,
                                created_at: c.created_at.to_rfc3339(),
                                ontology: kanban_type_to_pko("Comment").map(|s| s.to_string()),
                            })
                            .collect();
                        Ok(serde_json::to_value(TaskCommentsSinceResponse {
                            task_id: tid.to_string(),
                            comments: mapped,
                            total_count: total,
                        })
                        .map_err(|e| McpToolError::internal(e.to_string()))?) // rr0044-ok: serialize-own-struct
                    }
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(description = "Attach a deliverable (file path or URL) to a task as work output")]
    pub async fn kanban_task_add_deliverable(
        &self,
        Parameters(TaskAddDeliverableRequest { task_id, path }): Parameters<
            TaskAddDeliverableRequest,
        >,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_add_deliverable",
            kanban_type_to_pko("kanban_task_add_deliverable"),
            async {
                let tid = parse_task_id(&task_id)?;
                if path.trim().is_empty() {
                    return Err(McpToolError::invalid_argument("path must not be empty"));
                }
                match self.service.task_add_deliverable(tid, &path, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskAddDeliverableResponse {
                        task_id: task.id.to_string(),
                        deliverable_count: task.deliverables.len(),
                        ontology: kanban_type_to_pko("kanban_task_add_deliverable")
                            .map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(
        description = "Reopen a completed task (Done → InProgress) with optional new gas/rJoule budgets"
    )]
    pub async fn kanban_task_reopen(
        &self,
        Parameters(TaskReopenRequest {
            task_id,
            gas_budget,
            rjoule_budget,
        }): Parameters<TaskReopenRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_reopen",
            kanban_type_to_pko("kanban_task_reopen"),
            async {
                let tid = parse_task_id(&task_id)?;
                self.service
                    .task_reopen(tid, self.webid)
                    .map_err(map_kanban_error)?;
                // Apply new budgets if specified
                if let Some(g) = gas_budget {
                    self.service
                        .task_add_gas(tid, g, self.webid)
                        .map_err(map_kanban_error)?;
                }
                if let Some(r) = rjoule_budget {
                    self.service
                        .task_add_rjoules(tid, r, self.webid)
                        .map_err(map_kanban_error)?;
                }
                // Re-read to get final state
                let task = self
                    .service
                    .task_get(tid)
                    .map_err(map_kanban_error)?
                    .ok_or_else(|| McpToolError::not_found(format!("task {task_id}")))?;
                serde_json::to_value(TaskReopenResponse {
                    task_id: task.id.to_string(),
                    new_status: task.status.to_string(),
                    gas_remaining: task.gas_remaining,
                    rjoule_remaining: task.rjoule_remaining,
                    ontology: kanban_type_to_pko("kanban_task_reopen").map(|s| s.to_string()),
                })
                .map_err(|e| McpToolError::internal(e.to_string())) // rr0044-ok: serialize-own-struct
            },
        )
        .await
    }

    // ── Kata tools — scientific-thinking prompts scoped to a task ──────────

    #[tool(description = "Generate a Coaching Kata prompt (5-question dialogue) for a task")]
    pub async fn kanban_task_kata_coaching(
        &self,
        Parameters(TaskKataCoachingRequest { task_id }): Parameters<TaskKataCoachingRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_kata_coaching",
            kanban_type_to_pko("kanban_task_kata_coaching"),
            async {
                let tid = parse_task_id(&task_id)?;
                match self.service.task_coaching_prompt(tid) {
                    Ok(prompt) => Ok(serde_json::to_value(TaskKataResponse {
                        task_id: tid.to_string(),
                        prompt,
                        ontology: kanban_type_to_pko("kanban_task_kata_coaching")
                            .map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(description = "Generate an Improvement Kata prompt (PDCA cycle) for a task")]
    pub async fn kanban_task_kata_improvement(
        &self,
        Parameters(TaskKataImprovementRequest { task_id }): Parameters<TaskKataImprovementRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_kata_improvement",
            kanban_type_to_pko("kanban_task_kata_improvement"),
            async {
                let tid = parse_task_id(&task_id)?;
                match self.service.task_improvement_prompt(tid) {
                    Ok(prompt) => Ok(serde_json::to_value(TaskKataResponse {
                        task_id: tid.to_string(),
                        prompt,
                        ontology: kanban_type_to_pko("kanban_task_kata_improvement")
                            .map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(description = "Generate a Starter Kata observation drill prompt for a task sub-problem")]
    pub async fn kanban_task_kata_practice(
        &self,
        Parameters(TaskKataPracticeRequest {
            task_id,
            sub_problem,
        }): Parameters<TaskKataPracticeRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_kata_practice",
            kanban_type_to_pko("kanban_task_kata_practice"),
            async {
                let tid = parse_task_id(&task_id)?;
                match self.service.task_practice_prompt(tid, &sub_problem) {
                    Ok(prompt) => Ok(serde_json::to_value(TaskKataResponse {
                        task_id: tid.to_string(),
                        prompt,
                        ontology: kanban_type_to_pko("kanban_task_kata_practice")
                            .map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?), // rr0044-ok: serialize-own-struct
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    // ── Spawn — activate a subagent pod for task execution ─────────────────

    /// Spawn a subagent for task execution. Tries worktree-isolated spawn
    /// first (via the `WorktreeSpawnPort` IPC bridge → editor creates a git
    /// worktree + agent thread). On failure (no IPC socket, no active
    /// workspace), falls back to in-memory `LazyLocalSwarmRuntime::delegate()`
    /// (same process, same working tree). The delegation result is recorded
    /// on the task as a structured `LocalDelegateResult` + verdict. See
    /// `tasks/kanban-worktree-terminal-model.md` for the design (Option A:
    /// implemented).
    #[tool(description = "Spawn a subagent for task execution with delegated skills and budgets")]
    pub async fn kanban_task_spawn(
        &self,
        Parameters(TaskSpawnRequest {
            task_id,
            idempotency_key,
            delegation_level,
            delegated_skills,
            memory_scope,
            gas_budget,
            rjoule_budget,
            swarm_id,
        }): Parameters<TaskSpawnRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_spawn",
            kanban_type_to_pko("kanban_task_spawn"),
            with_idempotency(
                &self.idempotency,
                "kanban_task_spawn",
                idempotency_key.as_deref(),
                async {
                    let tid = self.validate_and_prepare_spawn(
                        &task_id,
                        &delegation_level,
                        &memory_scope,
                        &gas_budget,
                        &rjoule_budget,
                    )?;
                    let skills_for_agent = delegated_skills.clone();
                    let spec = crate::SpawnSpec::new(tid)
                        .with_level(&delegation_level)
                        .with_skills(delegated_skills)
                        .with_swarm(swarm_id.clone());
                    let spec = if let Some(ref ms) = memory_scope {
                        spec.with_memory(ms)
                    } else {
                        spec
                    };
                    // Record the spawn configuration on the task (config comment).
                    self.service
                        .spawn_task(tid, spec, self.webid)
                        .map_err(map_kanban_error)?;

                    let task = self
                        .service
                        .task_get(tid)
                        .map_err(map_kanban_error)?
                        .ok_or_else(|| McpToolError::not_found(format!("task {tid} not found")))?;

                    // Resolve the agent: reuse an expert agent whose declared skills
                    // cover the requested set; otherwise build a task-specific agent
                    // card in-memory ("create agents w/ skills for tasks").
                    let agent = self
                        .local_registry
                        .list()
                        .into_iter()
                        .find(|card| {
                            !skills_for_agent.is_empty()
                                && skills_for_agent
                                    .iter()
                                    .all(|s| card.capabilities.skills.iter().any(|cs| cs == s))
                        })
                        .unwrap_or_else(|| {
                            build_task_agent_card(tid, &task.title, &skills_for_agent)
                        });

                    // P1: Try worktree-isolated spawn first. When the zed IPC bridge
                    // is available and a workspace with an AgentPanel is open, this
                    // creates a worktree-backed agent thread (isolated git worktree).
                    // On failure (no IPC socket, no workspace, spawn error), falls
                    // back to the in-memory `LazyLocalSwarmRuntime` path below.
                    if let Some(response) = self
                        .spawn_via_worktree(tid, &task, &skills_for_agent)
                        .await?
                    {
                        return serde_json::to_value(response)
                            .map_err(|e| McpToolError::internal(e.to_string())); // rr0044-ok: serialize-own-struct
                    }

                    // Fallback: in-memory spawn via LazyLocalSwarmRuntime.
                    let response = self
                        .spawn_via_local_runtime(tid, &task, gas_budget, &agent)
                        .await?;
                    serde_json::to_value(response)
                        .map_err(|e| McpToolError::internal(e.to_string())) // rr0044-ok: serialize-own-struct
                },
            ),
        )
        .await
    }

    fn validate_and_prepare_spawn(
        &self,
        task_id: &str,
        delegation_level: &str,
        memory_scope: &Option<String>,
        gas_budget: &Option<u64>,
        rjoule_budget: &Option<u64>,
    ) -> Result<hkask_types::TaskId, McpToolError> {
        let tid = parse_task_id(task_id)?;
        match delegation_level {
            "minimal" | "standard" | "maximal" => {}
            other => {
                return Err(McpToolError::invalid_argument(format!(
                    "invalid delegation_level: {other} (expected minimal|standard|maximal)"
                )));
            }
        }
        if let Some(ms) = memory_scope {
            match ms.as_str() {
                "none" | "episodic" | "full" => {}
                other => {
                    return Err(McpToolError::invalid_argument(format!(
                        "invalid memory_scope: {other} (expected none|episodic|full)"
                    )));
                }
            }
        }
        if let Some(g) = gas_budget {
            self.service
                .task_add_gas(tid, *g, self.webid)
                .map_err(map_kanban_error)?;
        }
        if let Some(r) = rjoule_budget {
            self.service
                .task_add_rjoules(tid, *r, self.webid)
                .map_err(map_kanban_error)?;
        }
        Ok(tid)
    }

    async fn spawn_via_worktree(
        &self,
        tid: hkask_types::TaskId,
        task: &Task,
        skills_for_agent: &[String],
    ) -> Result<Option<TaskSpawnResponse>, McpToolError> {
        let task_text = match task.description.as_deref() {
            Some(desc) if !desc.trim().is_empty() => format!("{}: {}", task.title, desc),
            _ => task.title.clone(),
        };
        let spawn_prompt = format!(
            "You are working on kanban task '{}' (id: {}).\n\
             Task description: {}\n\
             Delegated skills: {}\n\
             Execute the task and report results via kanban_task_delegate_result.",
            task.title,
            tid,
            task_text,
            skills_for_agent.join(", ")
        );
        let spawn_title = format!("Kanban: {}", task.title);
        match self
            .worktree_spawn_port
            .create_worktree_thread(&spawn_prompt, &spawn_title, None, None)
            .await
        {
            Ok(message) => {
                let result_note = format!(
                    "Spawned worktree agent for task '{}' ({}). \
                     The agent runs in an isolated git worktree and will \
                     report results via kanban_task_delegate_result.\n\
                     {}",
                    task.title, tid, message
                );
                self.service
                    .task_comment(tid, self.webid, &result_note)
                    .map_err(map_kanban_error)?;
                if let Err(error) = self
                    .service
                    .task_move(tid, TaskStatus::InProgress, self.webid)
                {
                    tracing::warn!(
                        target: "hkask.mcp.kata_kanban",
                        task_id = %tid,
                        %error,
                        "could not advance task to InProgress after worktree spawn"
                    );
                }
                Ok(Some(TaskSpawnResponse {
                    task_id: tid.to_string(),
                    message: format!(
                        "Spawned worktree agent for task '{}' ({}). \
                         The agent runs in an isolated worktree.",
                        task.title, tid
                    ),
                    ontology: kanban_type_to_pko("kanban_task_spawn").map(|s| s.to_string()),
                }))
            }
            Err(e) => {
                tracing::info!(
                    target: "hkask.mcp.kata_kanban",
                    task_id = %tid,
                    error = %e,
                    "worktree spawn unavailable — falling back to in-memory LazyLocalSwarmRuntime"
                );
                Ok(None)
            }
        }
    }

    async fn spawn_via_local_runtime(
        &self,
        tid: hkask_types::TaskId,
        task: &Task,
        gas_budget: Option<u64>,
        agent: &LocalAgentCard,
    ) -> Result<TaskSpawnResponse, McpToolError> {
        let task_text = match task.description.as_deref() {
            Some(desc) if !desc.trim().is_empty() => format!("{}: {}", task.title, desc),
            _ => task.title.clone(),
        };
        let ceiling = match std::env::var("HKASK_ABW_MAX_CREDITS") {
            Ok(raw) => match raw.parse::<u32>() {
                Ok(value) => value,
                Err(_) => {
                    tracing::warn!(
                        "HKASK_ABW_MAX_CREDITS='{raw}' is not a valid u32; falling back to 50"
                    );
                    50
                }
            },
            Err(_) => 50,
        };
        let credits_authorized = gas_budget
            .map(|g| (g.min(u32::MAX as u64) as u32).min(ceiling))
            .unwrap_or(10)
            .min(ceiling);

        let runtime = self.local_runtime.get_or_init().await.map_err(|e| {
            McpToolError::unavailable(format!("local swarm runtime initialization failed: {e}"))
        })?;
        let result = runtime
            .delegate(agent, &task_text, credits_authorized, ceiling)
            .await
            .map_err(|e| {
                hkask_mcp_server::server::McpToolError::unavailable(format!(
                    "local swarm delegation failed: {e}"
                ))
            })?;

        let verdict = result.task_success.clone();
        if let Err(error) =
            self.service
                .task_record_delegation(tid, None, result.clone(), verdict, self.webid)
        {
            tracing::warn!(
                target: "hkask.mcp.kata_kanban",
                task_id = %tid,
                %error,
                "could not record structured delegation result — falling back to comment-only"
            );
        }
        // Render an unmeasured balance as "unknown", not as a number. The comment
        // is the operator's reconciliation record, so a fabricated figure here
        // would be indistinguishable from a real reading.
        let balance_note = match result.balance {
            Some(balance) => balance.to_string(),
            None => "unknown (ledger read failed)".to_string(),
        };
        // Surface the cap's understatement where it is visible: when the
        // delegation overran its authorized budget, the recorded cost is lower
        // than what was actually consumed.
        let cost_note = if result.cost_uncapped > result.cost {
            format!(
                "{} credits recorded ({} actual - capped at the authorized budget)",
                result.cost, result.cost_uncapped
            )
        } else {
            format!("{} credits", result.cost)
        };
        let result_note = format!(
            "Spawn executed: agent={agent_id}, model={model}, tokens={tokens}, \
             cost={cost_note}, balance={balance_note}, latency={latency_ms}ms\n\
             Response:\n{response}",
            agent_id = result.agent_id,
            model = result.model,
            tokens = result.tokens_used,
            latency_ms = result.latency_ms,
            response = result.response,
        );
        self.service
            .task_comment(tid, self.webid, &result_note)
            .map_err(map_kanban_error)?;
        if let Err(error) = self
            .service
            .task_move(tid, TaskStatus::InProgress, self.webid)
        {
            tracing::warn!(
                target: "hkask.mcp.kata_kanban",
                task_id = %tid,
                %error,
                "could not advance task to InProgress after spawn — delegation result still recorded"
            );
        }

        Ok(TaskSpawnResponse {
            task_id: tid.to_string(),
            message: format!(
                "Spawned agent '{}' for task '{}' ({} credits, {} tokens). Response recorded.",
                result.agent_id, task.title, result.cost, result.tokens_used
            ),
            ontology: kanban_type_to_pko("kanban_task_spawn").map(|s| s.to_string()),
        })
    }

    /// Read the structured delegation result and deterministic verdict for a
    /// task that was spawned via `kanban_task_spawn`. Returns the persisted
    /// `LocalDelegateResult` and `TaskSuccessVerdict` fields, enabling the
    /// swarm-intelligence SENSE phase and the Curator to query the durable
    /// coordination state without parsing free-text comments.
    ///
    /// contract: P3-svc-kanban-010
    /// expect: "I can read the structured delegation result for a spawned task" \[P3\]
    /// pre:  task_id is a valid task id
    /// post: returns the delegation result + verdict, or `has_result: false`
    #[tool(
        description = "Read the structured delegation result and deterministic verdict for a task spawned via kanban_task_spawn. Returns the persisted LocalDelegateResult and TaskSuccessVerdict."
    )]
    pub async fn kanban_task_delegate_result(
        &self,
        Parameters(TaskDelegateResultRequest { task_id }): Parameters<TaskDelegateResultRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_delegate_result",
            kanban_type_to_pko("kanban_task_delegate_result"),
            async {
                let tid = parse_task_id(&task_id)?;
                let task = self
                    .service
                    .task_get(tid)
                    .map_err(map_kanban_error)?
                    .ok_or_else(|| McpToolError::not_found(format!("task {tid} not found")))?;
                serde_json::to_value(TaskDelegateResultResponse {
                    task_id: tid.to_string(),
                    has_result: task.delegate_result.is_some(),
                    delegate_result: task.delegate_result.clone(),
                    deterministic_verdict: task.deterministic_verdict.clone(),
                    swarm_id: task.swarm_id,
                    ontology: kanban_type_to_pko("kanban_task_delegate_result")
                        .map(|s| s.to_string()),
                })
                .map_err(|e| McpToolError::internal(e.to_string())) // rr0044-ok: serialize-own-struct
            },
        )
        .await
    }

    /// Create kanban tasks for contracts missing `expect:` user-voice annotations.
    ///
    /// Takes a JSON list of ExpectProposal structs (from test-harness
    /// `propose_missing_expect_annotations`) and creates a task per contract gap.
    /// Owning agents can claim and resolve these tasks by submitting
    /// `expect:` annotation PRs (P2 consent required for merge).
    ///
    /// contract: P3-svc-kanban-009
    /// expect: "I can create kanban tasks from contract expectation gaps so agents can ground them" \[P3\]
    /// \[P5\] Constraining: Essentialism — one batch operation, no individual task editing
    /// pre:  proposals is a non-empty JSON array of ExpectProposal structs
    /// pre:  board_id is a valid board ID
    /// post: returns created task IDs (one per proposal)
    #[tool(
        description = "Create kanban tasks for contracts missing expect: annotations. Takes JSON from propose_missing_expect_annotations."
    )]
    pub async fn contract_propose_expect(
        &self,
        Parameters(ContractProposeExpect {
            board_id,
            proposals,
        }): Parameters<ContractProposeExpect>,
    ) -> String {
        execute_tool_semantic(self, "contract_propose_expect", kanban_type_to_pko("contract_propose_expect"), async {
            let bid = parse_board_id(&board_id)?;

            let proposals: Vec<hkask_types::ExpectProposal> =
                match serde_json::from_value(proposals.into_inner()) {
                    Ok(p) => p,
                    Err(e) => return Err(McpToolError::invalid_argument(format!("invalid proposals: {e}"))),
                };

            if proposals.is_empty() {
                return Err(McpToolError::invalid_argument("proposals must be non-empty"));
            }

            let mut created: Vec<String> = Vec::new();
            for prop in &proposals {
                let title = format!(
                    "contract({}): add expect: to {}",
                    prop.crate_name, prop.function,
                );
                let description = format!(
                    "File: {}:{}\nContract: {}\nPre: {}\nPost: {}\n\nTemplate:\n{}\n\nSuggested principle: {}\nConstraining: {:?}",
                    prop.file,
                    prop.line,
                    prop.contract_id,
                    prop.pre,
                    prop.post,
                    prop.expect_template,
                    prop.suggested_goal_principle,
                    prop.existing_constraining_principles,
                );
                let spec = TaskSpec::new(title).with_description(description);
                match self.service.task_create(bid, spec, self.webid) {
                    Ok(task) => created.push(task.id.to_string()),
                    Err(e) => {
                        return Err(map_kanban_error(e));
                    }
                }
            }

            Ok(serde_json::json!({
                "created": created.len(),
                "task_ids": created,
                "crate": proposals[0].crate_name,
                "pko": kanban_type_to_pko("contract_propose_expect").map(|s| s.to_string()),
            }))
        })
        .await
    }

    /// Export a kanban board as mermaid kanban markdown (structure only:
    /// columns, task titles, task IDs). The markdown round-trips through
    /// `kanban_board_import`. Only the board owner can export (P12).
    ///
    /// contract: P3-svc-kanban-012
    /// expect: "I can export a kanban board I own as mermaid markdown" \[P3\]
    /// pre:  board_id is a valid board id owned by the caller
    /// post: returns the mermaid markdown plus a structural summary
    #[tool(description = "Export a kanban board as mermaid kanban markdown (columns + task titles).")]
    pub async fn kanban_board_export(
        &self,
        Parameters(BoardExportRequest { board_id }): Parameters<BoardExportRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_board_export",
            kanban_type_to_pko("kanban_board_export"),
            async {
                let bid = parse_board_id(&board_id)?;
                let board = self
                    .service
                    .board_get(bid)
                    .map_err(map_kanban_error)?
                    .ok_or_else(|| {
                        McpToolError::not_found(format!("board {bid} not found"))
                    })?;
                if board.owner != self.webid {
                    return Err(McpToolError::permission_denied(format!(
                        "board {bid} is not owned by caller — cannot export"
                    )));
                }
                let tasks = self
                    .service
                    .task_list(bid, TaskFilter::all())
                    .map_err(map_kanban_error)?;
                let task_count = tasks.len();
                let column_count = board.columns.len();
                let markdown = kanban::mermaid::export_board_to_mermaid(&board, &tasks);
                Ok(serde_json::to_value(BoardExportResponse {
                    markdown,
                    board_id: bid.to_string(),
                    board_name: board.name,
                    column_count,
                    task_count,
                    ontology: kanban_type_to_pko("kanban_board_export").map(|s| s.to_string()),
                })
                .map_err(|e| McpToolError::internal(e.to_string()))?) // rr0044-ok: serialize-own-struct
            },
        )
        .await
    }

    /// Import mermaid kanban markdown as a new board. Parses the markdown,
    /// creates a board with columns matching the parsed sections (mapping
    /// section names to `TaskStatus` where possible), and re-creates each
    /// task in its parsed column's status by walking the transition chain.
    /// Replay-safe via `idempotency_key`.
    ///
    /// contract: P3-svc-kanban-013
    /// expect: "I can import mermaid kanban markdown as a new board" \[P3\]
    /// pre:  markdown contains a `kanban` directive and at least one `section`
    /// post: returns the new board id and a structural summary
    #[tool(description = "Import mermaid kanban markdown as a new board with tasks in parsed columns.")]
    pub async fn kanban_board_import(
        &self,
        Parameters(BoardImportRequest {
            markdown,
            board_name,
            idempotency_key,
        }): Parameters<BoardImportRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_board_import",
            kanban_type_to_pko("kanban_board_import"),
            with_idempotency(
                &self.idempotency,
                "kanban_board_import",
                idempotency_key.as_deref(),
                async {
                    let mut parsed = kanban::mermaid::parse_mermaid_kanban(&markdown)
                        .map_err(|e| McpToolError::invalid_argument(e))?;
                    if parsed.columns.is_empty() {
                        return Err(McpToolError::invalid_argument(
                            "mermaid kanban markdown has no sections — nothing to import",
                        ));
                    }
                    let name = board_name
                        .or(parsed.name.take())
                        .unwrap_or_else(|| "Imported Board".to_string());
                    let columns = kanban::mermaid::columns_from_parsed(&parsed);
                    let column_count = columns.len();
                    let board = self
                        .service
                        .board_create(self.webid, &name, &columns)
                        .map_err(map_kanban_error)?;

                    let mut task_count: usize = 0;
                    for column in &parsed.columns {
                        let target_status = board
                            .columns
                            .iter()
                            .find(|c| c.name == column.name)
                            .map(|c| c.status)
                            .unwrap_or(TaskStatus::Backlog);
                        for title in &column.tasks {
                            let spec = TaskSpec::new(title.clone());
                            let task = self
                                .service
                                .task_create(board.id, spec, self.webid)
                                .map_err(map_kanban_error)?;
                            task_count += 1;
                            // Walk the task forward from Backlog to the target
                            // status through valid transitions.
                            let mut current = TaskStatus::Backlog;
                            while current != target_status {
                                let next = match current.next() {
                                    Some(s) => s,
                                    None => break,
                                };
                                match self.service.task_move(task.id, next, self.webid) {
                                    Ok(moved) => {
                                        current = moved.status;
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            target: "hkask.mcp.kata_kanban",
                                            task_id = %task.id,
                                            target_status = %target_status,
                                            error = %e,
                                            "import: could not move task to target status, leaving at {current}",
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    Ok(serde_json::to_value(BoardImportResponse {
                        board_id: board.id.to_string(),
                        board_name: board.name,
                        column_count,
                        task_count,
                        ontology: kanban_type_to_pko("kanban_board_import")
                            .map(|s| s.to_string()),
                    })
                    .map_err(|e| McpToolError::internal(e.to_string()))?) // rr0044-ok: serialize-own-struct
                },
            ),
        )
        .await
    }
}

/// Parse a task id string or return an `invalid_argument` MCP error.
///
/// Replaces the repeated `match task_id.parse::<hkask_types::TaskId>() { Ok(id) => id, Err(e) => return Err(McpToolError::invalid_argument(...)) }`
/// block across the kanban tool methods.
fn parse_task_id(task_id: &str) -> Result<hkask_types::TaskId, McpToolError> {
    task_id
        .parse::<hkask_types::TaskId>()
        .map_err(|e| McpToolError::invalid_argument(format!("invalid task_id: {e}")))
}

/// Parse a board id string or return an `invalid_argument` MCP error.
fn parse_board_id(board_id: &str) -> Result<hkask_types::BoardId, McpToolError> {
    board_id
        .parse::<hkask_types::BoardId>()
        .map_err(|e| McpToolError::invalid_argument(format!("invalid board_id: {e}")))
}

/// Map a service-layer `KanbanError` to the correct `McpToolError` variant.
///
/// Each `KanbanError` variant maps to a semantically appropriate MCP error kind
/// so that callers can distinguish not-found, permission-denied, precondition
/// failures, and internal errors from simple invalid-input errors.
///
/// contract: kanban-error-mapping
/// expect: "I can distinguish not-found, permission, and workflow errors from invalid-input errors" \[P4\]
/// pre:  e is a valid KanbanError
/// post: returns McpToolError with appropriate McpErrorKind
fn map_kanban_error(e: KanbanError) -> McpToolError {
    match e {
        KanbanError::NotFound(nf) => McpToolError::not_found(nf.to_string()),
        KanbanError::InvalidInput(msg) => McpToolError::invalid_argument(msg),
        KanbanError::InvalidTransition { .. } => McpToolError::failed_precondition(e.to_string()),
        KanbanError::PermissionDenied(msg) => McpToolError::permission_denied(msg),
        KanbanError::WipLimitExceeded { .. } => McpToolError::failed_precondition(e.to_string()),
        KanbanError::Internal(msg) => McpToolError::internal(msg), // rr0044-ok: kanban internal-error arm
    }
}

/// Run the kanban MCP server (used by binary target).
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        hkask_types::kanban_wire::KANBAN_SERVER_NAME,
        env!("CARGO_PKG_VERSION"),
        |ctx: ServerContext| {
            (|| -> anyhow::Result<KanbanServer> {
                // D28 — Standardized Artifact Storage. Default DB path is
                // `{kask_data_dir}/mcp/kata-kanban/kanban.db`, resolved via
                // `resolve_under_data_dir`. Override via `HKASK_KANBAN_DB`.
                // `HKASK_KANBAN_DB` is a non-secret config path — read via
                // `std::env::var` (matching every other DB-path env var:
                // `HKASK_CURATOR_DB`, `HKASK_DB_PATH`, `HKASK_RSS_DB`, etc.)
                // and injected via `config_env`, not `credentials`.
                let kanban_db_path = std::env::var("HKASK_KANBAN_DB")
                    .unwrap_or_else(|_| {
                        let relative_path =
                            hkask_types::agent_paths::mcp_server_db("kata-kanban", "kanban");
                        let default_path =
                            hkask_types::agent_paths::resolve_under_data_dir(&relative_path);
                        if let Some(Err(error)) = default_path.parent().map(std::fs::create_dir_all)
                        {
                            tracing::warn!(
                                target: "hkask.mcp.kata_kanban",
                                path = %default_path.display(),
                                %error,
                                "Failed to create default kanban DB directory \
                                 — the subsequent DB open will surface the failure"
                            );
                        }
                        tracing::info!(
                            target: "hkask.mcp.kata_kanban",
                            path = %default_path.display(),
                            "Using default per-agent kanban database"
                        );
                        default_path.to_string_lossy().to_string()
                    });
                // Resolve the DB passphrase through the full keystore chain:
                //   1. `ctx.credentials` — populated by `build_mcp_server_env`
                //      from `kask://credentials/hkask_db_passphrase` (governed
                //      launch path).
                //   2. `std::env::var` — direct env override (matches the
                //      condenser's fallback at hkask_mcp_condenser.rs).
                //   3. `resolve_credential` — bridges to the keychain key
                //      `hkask-db-passphrase` that `provision_agent` writes to
                //      (identity.rs). Without this third leg, governed launch
                //      where `build_mcp_server_env` never populates
                //      `ctx.credentials` (keychain entry
                //      `kask://credentials/hkask_db_passphrase` absent because
                //      `provision_agent` writes under a different key) silently
                //      falls back to in-memory and loses all boards on restart.
                let passphrase = ctx
                    .credentials
                    .get("HKASK_DB_PASSPHRASE")
                    .cloned()
                    .or_else(|| {
                        std::env::var("HKASK_DB_PASSPHRASE")
                            .ok()
                            .filter(|value| !value.is_empty())
                    })
                    .or_else(|| {
                        match resolve_credential("HKASK_DB_PASSPHRASE") {
                            Ok(value) if !value.is_empty() => Some(value),
                            Ok(_) => None,
                            Err(error) => {
                                tracing::warn!(
                                    target: "hkask.mcp.kata_kanban",
                                    %error,
                                    "HKASK_DB_PASSPHRASE not found in credentials map, env, \
                                     or keychain — falling back to in-memory mode. \
                                     Kanban data will not persist across server restarts. \
                                     Set HKASK_DB_PASSPHRASE or run provision_agent \
                                     for encrypted persistent storage."
                                );
                                None
                            }
                        }
                    });
                let db = if let Some(passphrase) = passphrase {
                    hkask_storage::open_or_repair(&kanban_db_path, &passphrase)
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                } else {
                    // No passphrase configured — fall back to in-memory mode.
                    // All DBs should be encrypted at rest; using a hardcoded
                    // public key provides zero confidentiality. In-memory mode
                    // loses persistence but matches the security posture of
                    // the curator and condenser servers.
                    hkask_storage::Database::in_memory()
                        .map_err(|e| anyhow::anyhow!("in-memory DB: {e}"))?
                };
                let pool = db.sqlite_pool().map_err(|e| anyhow::anyhow!("pool: {e}"))?;
                let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                    Arc::new(hkask_storage::database::sqlite::SqliteDriver::new_labeled(
                        pool,
                        kanban_db_path.as_str(),
                    ));
                // Clone the handle before `HMemStore` takes ownership: replay
                // protection lives in the same database as the writes it guards.
                let idempotency_driver = driver.clone();
                let store = HMemStore::from_driver(driver)
                    .map_err(|e| anyhow::anyhow!("hmem store init: {e}"))?;
                let service = KanbanService::new(store);

                // Local swarm delegation surface (kanban_task_spawn). The ledger
                // path resolution mirrors hkask-mcp-swarm's `run()` exactly so
                // both processes share the same ledger file — operator funding
                // via `swarm_fund_local` is reusable here. Keep these in sync.
                let ledger_path = std::env::var("HKASK_SWARM_LEDGER_PATH")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| {
                        // D28 — Standardized Artifact Storage. Default
                        // ledger path is `mcp/swarm/ledger.db`.
                        hkask_types::agent_paths::resolve_under_data_dir(
                            &hkask_types::agent_paths::mcp_server_db("swarm", "ledger"),
                        )
                        .to_string_lossy()
                        .to_string()
                    });
                let skills_dir = std::env::var("HKASK_SKILLS_DIR")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .map(|raw| {
                        if std::path::Path::new(&raw).is_absolute() {
                            raw
                        } else {
                            hkask_types::agent_paths::resolve_under_data_dir(
                                std::path::Path::new(&raw),
                            )
                            .to_string_lossy()
                            .to_string()
                        }
                    });
                let local_runtime =
                    Arc::new(LazyLocalSwarmRuntime::lazy(ledger_path, skills_dir));

                // Local agent registry — same dir resolution as hkask-mcp-swarm
                // (relative paths resolve under the hKask data dir, not CWD).
                let local_agents_dir = std::env::var("HKASK_LOCAL_AGENTS_DIR")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "agents/local/curated".to_string());
                let local_agents_dir =
                    if std::path::Path::new(&local_agents_dir).is_absolute() {
                        local_agents_dir
                    } else {
                        hkask_types::agent_paths::resolve_under_data_dir(
                            std::path::Path::new(&local_agents_dir),
                        )
                        .to_string_lossy()
                        .to_string()
                    };
                let local_registry = Arc::new(LocalAgentRegistry::new(local_agents_dir));
                if let Err(error) = local_registry.load() {
                    tracing::warn!(
                        target: "hkask.mcp.kata_kanban",
                        %error,
                        "failed to load local agent cards — kanban_task_spawn will fall back to creating task-specific agents"
                    );
                }

                let worktree_spawn_port =
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current()
                            .block_on(hkask_inference::resolve_worktree_spawn_port())
                    });

                // Replay protection shares the kanban driver, so it inherits the
                // same durability and encryption as the writes it guards. When
                // the DB is in-memory (no passphrase), the store reports
                // `is_durable() == false` and every protected response carries
                // `idempotency_durable: false` — an operator must not be told a
                // call was replay-protected across restarts when it was not.
                let idempotency = match idempotency::IdempotencyStore::with_driver(
                    idempotency_driver,
                ) {
                    Ok(store) => {
                        if !store.is_durable() {
                            tracing::warn!(
                                target: "hkask.mcp.kata_kanban",
                                "Replay protection is process-local (in-memory kanban DB) — \
                                 a retry after a server restart may duplicate a create. \
                                 Set HKASK_DB_PASSPHRASE for durable replay protection."
                            );
                        }
                        store
                    }
                    Err(error) => {
                        // Fall back to the in-memory store rather than failing
                        // startup: the server is still useful, but say plainly
                        // that the guarantee is weaker.
                        tracing::warn!(
                            target: "hkask.mcp.kata_kanban",
                            %error,
                            "Could not initialise the replay-protection schema — falling back \
                             to process-local protection. Retries after a restart may \
                             duplicate a create."
                        );
                        idempotency::IdempotencyStore::default()
                    }
                };

                Ok(KanbanServer::new(ctx.webid, service, local_runtime, local_registry, worktree_spawn_port, Arc::new(idempotency)))
            })()
            .map_err(|e| hkask_mcp_server::McpError::UnexpectedResponse {
                context: "kanban server init".into(),
                detail: e.to_string(),
            })
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_DB_PASSPHRASE",
                "SQLCipher encryption passphrase (resolved via hkask keystore chain when not set)",
            ),
        ],
    )
    .await
}

// D28 — pins the default DB path resolution.
#[test]
fn default_db_path_follows_standardized_layout() {
    let relative = hkask_types::agent_paths::mcp_server_db("kata-kanban", "kanban");
    assert_eq!(
        relative,
        std::path::PathBuf::from("mcp")
            .join("kata-kanban")
            .join("kanban.db"),
        "kata-kanban default DB path must follow mcp/kata-kanban/kanban.db"
    );
}
