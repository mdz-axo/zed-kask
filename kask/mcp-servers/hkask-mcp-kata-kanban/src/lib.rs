#![forbid(unsafe_code)]
//! hkask-mcp-kata-kanban — Kata-Kanban workflow coordination MCP server.
//!
//! Provides 18 MCP tools for kanban board and task management.
//! All tools carry the caller's WebID for P12 compliance.
//!
//! The KanbanServer struct and tool methods are exported from the library
//! target to enable fuzz testing (P5 Testing Discipline, P4 Clear Boundaries).

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

pub mod pko;
pub mod types;

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_mcp_server::server::{McpToolError, ServerContext, execute_tool_semantic};
use hkask_services_kata_kanban::KanbanError;
use hkask_services_kata_kanban::KanbanService;
use hkask_services_kata_kanban::{TaskFilter, TaskSpec, VerificationCriterion};
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
    }
);

#[tool_router(server_handler)]
impl KanbanServer {
    #[tool(description = "Create a new kanban board with optional custom columns")]
    pub async fn kanban_board_create(
        &self,
        Parameters(BoardCreateRequest { name, columns }): Parameters<BoardCreateRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_board_create",
            kanban_type_to_pko("kanban_board_create"),
            async {
                let column_defs = match columns {
                    Some(inputs) => inputs
                        .into_iter()
                        .enumerate()
                        .map(|(i, input)| {
                            match hkask_services_kata_kanban::TaskStatus::parse_str(&input.status) {
                                Some(s) => {
                                    let mut col = hkask_services_kata_kanban::ColumnDef::new(
                                        input.name, s, i as u32,
                                    );
                                    if let Some(wip) = input.wip_limit {
                                        col = col.with_wip_limit(wip);
                                    }
                                    Ok(col)
                                }
                                None => Err(format!("invalid status: {}", input.status)),
                            }
                        })
                        .collect::<Result<Vec<_>, _>>(),
                    None => Ok(default_columns()),
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
                            })
                            .collect(),
                        pko: kanban_type_to_pko("Board").map(|s| s.to_string()),
                    })
                    .unwrap()),
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
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
                                pko: kanban_type_to_pko("Board").map(|s| s.to_string()),
                            })
                            .collect(),
                    })
                    .unwrap()),
                    Err(e) => Err(map_kanban_error(e)),
                }
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
            gas_budget,
            rjoule_budget,
        }): Parameters<TaskCreateRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_create",
            kanban_type_to_pko("kanban_task_create"),
            async {
                let bid = match board_id.parse::<hkask_types::BoardId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid board_id: {e}"
                        )));
                    }
                };
                let mut spec = TaskSpec::new(title);
                if let Some(d) = description {
                    spec = spec.with_description(d);
                }
                if let Some(cs) = criteria {
                    spec = spec
                        .with_criteria(cs.into_iter().map(VerificationCriterion::new).collect());
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
                        pko: kanban_type_to_pko("Task").map(|s| s.to_string()),
                    })
                    .unwrap()),
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
                let bid = match board_id.parse::<hkask_types::BoardId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid board_id: {e}"
                        )));
                    }
                };
                let filter = match status {
                    Some(s) => match hkask_services_kata_kanban::TaskStatus::parse_str(&s) {
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
                            .map(|t| TaskInfo {
                                task_id: t.id.to_string(),
                                board_id: t.board_id.to_string(),
                                title: t.title,
                                status: t.status.to_string(),
                                assignee: t.assignee.map(|a| a.to_string()),
                                criteria_count: t.criteria.len(),
                                gas_remaining: t.gas_remaining,
                                rjoule_remaining: t.rjoule_remaining,
                                pko: kanban_type_to_pko("Task").map(|s| s.to_string()),
                            })
                            .collect(),
                    })
                    .unwrap()),
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                let previous_status = match self.service.task_get(tid) {
                    Ok(Some(t)) => t.status.to_string(),
                    Ok(None) => {
                        return Err(McpToolError::not_found(format!(
                            "task not found: {task_id}"
                        )));
                    }
                    Err(e) => return Err(map_kanban_error(e)),
                };
                let target = match hkask_services_kata_kanban::TaskStatus::parse_str(&target_status)
                {
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
                        pko: kanban_type_to_pko("kanban_task_move").map(|s| s.to_string()),
                    })
                    .unwrap()),
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                match self.service.task_claim(tid, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskAssignResponse {
                        task_id: task.id.to_string(),
                        assignee: task.assignee.map(|a| a.to_string()).unwrap_or_default(),
                        pko: kanban_type_to_pko("kanban_task_assign").map(|s| s.to_string()),
                    })
                    .unwrap()),
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    #[tool(description = "Verify a task against its acceptance criteria")]
    pub async fn kanban_task_verify(
        &self,
        Parameters(TaskVerifyRequest { task_id, evidence }): Parameters<TaskVerifyRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_verify",
            kanban_type_to_pko("kanban_task_verify"),
            async {
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                if evidence.trim().is_empty() {
                    return Err(McpToolError::invalid_argument("evidence must not be empty"));
                }
                match self.service.task_verify(tid, &evidence, self.webid) {
                    Ok((task, verification)) => Ok(serde_json::to_value(TaskVerifyResponse {
                        task_id: task.id.to_string(),
                        passed: verification.passed,
                        reasoning: verification.reasoning,
                        new_status: task.status.to_string(),
                        pko: kanban_type_to_pko("kanban_task_verify").map(|s| s.to_string()),
                    })
                    .unwrap()),
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                if amount == 0 {
                    return Err(McpToolError::invalid_argument("amount must be > 0"));
                }
                match self.service.task_add_gas(tid, amount, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskAddGasResponse {
                        task_id: task.id.to_string(),
                        new_gas_remaining: task.gas_remaining.unwrap_or(0),
                        pko: kanban_type_to_pko("kanban_task_add_gas").map(|s| s.to_string()),
                    })
                    .unwrap()),
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                if amount == 0 {
                    return Err(McpToolError::invalid_argument("amount must be > 0"));
                }
                match self.service.task_add_rjoules(tid, amount, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskAddRjoulesResponse {
                        task_id: task.id.to_string(),
                        new_rjoule_remaining: task.rjoule_remaining.unwrap_or(0),
                        pko: kanban_type_to_pko("kanban_task_add_rjoules").map(|s| s.to_string()),
                    })
                    .unwrap()),
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
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
                        pko: kanban_type_to_pko("Comment").map(|s| s.to_string()),
                    })
                    .unwrap()),
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
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
                                pko: kanban_type_to_pko("Comment").map(|s| s.to_string()),
                            })
                            .collect();
                        Ok(serde_json::to_value(TaskCommentsSinceResponse {
                            task_id: tid.to_string(),
                            comments: mapped,
                            total_count: total,
                        })
                        .unwrap())
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                if path.trim().is_empty() {
                    return Err(McpToolError::invalid_argument("path must not be empty"));
                }
                match self.service.task_add_deliverable(tid, &path, self.webid) {
                    Ok(task) => Ok(serde_json::to_value(TaskAddDeliverableResponse {
                        task_id: task.id.to_string(),
                        deliverable_count: task.deliverables.len(),
                        pko: kanban_type_to_pko("kanban_task_add_deliverable")
                            .map(|s| s.to_string()),
                    })
                    .unwrap()),
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
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
                Ok(serde_json::to_value(TaskReopenResponse {
                    task_id: task.id.to_string(),
                    new_status: task.status.to_string(),
                    gas_remaining: task.gas_remaining,
                    rjoule_remaining: task.rjoule_remaining,
                    pko: kanban_type_to_pko("kanban_task_reopen").map(|s| s.to_string()),
                })
                .unwrap())
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                match self.service.task_coaching_prompt(tid) {
                    Ok(prompt) => Ok(serde_json::to_value(TaskKataResponse {
                        task_id: tid.to_string(),
                        prompt,
                        pko: kanban_type_to_pko("kanban_task_kata_coaching").map(|s| s.to_string()),
                    })
                    .unwrap()),
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                match self.service.task_improvement_prompt(tid) {
                    Ok(prompt) => Ok(serde_json::to_value(TaskKataResponse {
                        task_id: tid.to_string(),
                        prompt,
                        pko: kanban_type_to_pko("kanban_task_kata_improvement")
                            .map(|s| s.to_string()),
                    })
                    .unwrap()),
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
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                match self.service.task_practice_prompt(tid, &sub_problem) {
                    Ok(prompt) => Ok(serde_json::to_value(TaskKataResponse {
                        task_id: tid.to_string(),
                        prompt,
                        pko: kanban_type_to_pko("kanban_task_kata_practice").map(|s| s.to_string()),
                    })
                    .unwrap()),
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    // ── Spawn — activate a subagent pod for task execution ─────────────────

    #[tool(description = "Spawn a subagent for task execution with delegated skills and budgets")]
    pub async fn kanban_task_spawn(
        &self,
        Parameters(TaskSpawnRequest {
            task_id,
            delegation_level,
            delegated_skills,
            memory_scope,
            gas_budget,
            rjoule_budget,
        }): Parameters<TaskSpawnRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "kanban_task_spawn",
            kanban_type_to_pko("kanban_task_spawn"),
            async {
                let tid = match task_id.parse::<hkask_types::TaskId>() {
                    Ok(id) => id,
                    Err(e) => {
                        return Err(McpToolError::invalid_argument(format!(
                            "invalid task_id: {e}"
                        )));
                    }
                };
                // Apply budgets before spawn if specified
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
                let spec = hkask_services_kata_kanban::SpawnSpec::new(tid)
                    .with_level(&delegation_level)
                    .with_skills(delegated_skills);
                let spec = if let Some(ref ms) = memory_scope {
                    spec.with_memory(ms)
                } else {
                    spec
                };
                match self.service.spawn_task(tid, spec, self.webid) {
                    Ok(message) => Ok(serde_json::to_value(TaskSpawnResponse {
                        task_id: tid.to_string(),
                        message,
                        pko: kanban_type_to_pko("kanban_task_spawn").map(|s| s.to_string()),
                    })
                    .unwrap()),
                    Err(e) => Err(map_kanban_error(e)),
                }
            },
        )
        .await
    }

    /// Create kanban tasks for contracts missing `expect:` user-voice annotations.
    ///
    /// Takes a JSON list of ExpectProposal structs (from test-harness
    /// `propose_missing_expect_annotations`) and creates a task per contract gap.
    /// Owning userpods can claim and resolve these tasks by submitting
    /// `expect:` annotation PRs (P2 consent required for merge).
    ///
    /// contract: P3-svc-kanban-009
    /// expect: "I can create kanban tasks from contract expectation gaps so userpods can ground them" \[P3\]
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
            proposals_json,
        }): Parameters<ContractProposeExpect>,
    ) -> String {
        execute_tool_semantic(self, "contract_propose_expect", kanban_type_to_pko("contract_propose_expect"), async {
            let bid = match board_id.parse::<hkask_types::BoardId>() {
                Ok(id) => id,
                Err(e) => return Err(McpToolError::invalid_argument(format!("invalid board_id: {e}"))),
            };

            let proposals: Vec<hkask_test_harness::ExpectProposal> =
                match serde_json::from_str(&proposals_json) {
                    Ok(p) => p,
                    Err(e) => return Err(McpToolError::invalid_argument(format!("invalid proposals JSON: {e}"))),
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
            }))
        })
        .await
    }
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
        KanbanError::Internal(msg) => McpToolError::internal(msg),
    }
}

pub fn default_columns() -> Vec<hkask_services_kata_kanban::ColumnDef> {
    hkask_services_kata_kanban::KanbanService::standard_columns()
}

/// Run the kanban MCP server (used by binary target).
pub async fn run(
    userpod: String,
    daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-kata-kanban",
        env!("CARGO_PKG_VERSION"),
        |ctx: ServerContext| {
            (|| -> anyhow::Result<KanbanServer> {
                // Use the standard per-agent kanban DB path when not explicitly set.
                let kanban_db_path = ctx
                    .credentials
                    .get("HKASK_KANBAN_DB")
                    .cloned()
                    .unwrap_or_else(|| {
                        let relative_path = hkask_types::agent_paths::userpod_kanban_db(&userpod);
                        let default_path =
                            hkask_types::agent_paths::resolve_under_data_dir(&relative_path);
                        if let Some(parent) = default_path.parent() {
                            std::fs::create_dir_all(parent).ok();
                        }
                        tracing::info!(
                            target: "hkask.mcp.kata_kanban",
                            path = %default_path.display(),
                            userpod = %userpod,
                            "Using default per-agent kanban database"
                        );
                        default_path.to_string_lossy().to_string()
                    });
                let db = if let Some(passphrase) = ctx.credentials.get("HKASK_DB_PASSPHRASE") {
                    hkask_storage::open_or_repair(&kanban_db_path, passphrase)
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                } else {
                    // No passphrase configured — use a deterministic default key so
                    // the computed kanban_db_path is actually used for persistence.
                    // This is NOT encrypted; users should set HKASK_DB_PASSPHRASE
                    // for production deployments.
                    let default_key = format!("__k4nb4n__{}__d3f4ult__", userpod);
                    hkask_storage::Database::open(&kanban_db_path, &default_key)
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                };
                let pool = db.sqlite_pool().map_err(|e| anyhow::anyhow!("pool: {e}"))?;
                let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                    Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool));
                let store = HMemStore::from_driver(driver);
                store
                    .driver()
                    .execute_batch(
                        "CREATE TABLE IF NOT EXISTS h_mems (
                        id TEXT PRIMARY KEY, entity TEXT NOT NULL, attribute TEXT NOT NULL,
                        value TEXT NOT NULL, valid_from TEXT NOT NULL, valid_to TEXT,
                        confidence REAL NOT NULL, perspective TEXT, visibility TEXT NOT NULL,
                        owner_webid TEXT NOT NULL
                    )",
                    )
                    .expect("DDL batch must succeed");
                let service = KanbanService::new(store);
                Ok(KanbanServer::new(
                    ctx.webid,
                    userpod.clone(),
                    daemon_client.clone(),
                    service,
                ))
            })()
            .map_err(|e| hkask_mcp_server::McpError::UnexpectedResponse {
                context: "kanban server init".into(),
                detail: e.to_string(),
            })
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_KANBAN_DB",
                "Path to per-agent kanban database file (defaults to agents/{userpod}/kanban.db)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_DB_PASSPHRASE",
                "SQLCipher encryption passphrase (resolved via hkask keystore chain when not set)",
            ),
        ],
    )
    .await
}
