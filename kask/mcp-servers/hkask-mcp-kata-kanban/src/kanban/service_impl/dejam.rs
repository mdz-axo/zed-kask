use super::*;

impl KanbanService {
    /// Report stuck, idle, or unverified tasks on a board.
    pub fn unjam_report(&self, board_id: BoardId) -> Result<Vec<UnjamItem>, KanbanError> {
        let tasks = self.task_list(board_id, TaskFilter::all())?;
        let now = chrono::Utc::now();
        let mut items = Vec::new();

        for task in &tasks {
            if (task.status == TaskStatus::InProgress || task.status == TaskStatus::Review)
                && let Some(hours) = task.estimated_hours
            {
                let elapsed = (now - task.updated_at).num_hours();
                if (elapsed as f64) > hours * 2.0 {
                    items.push(UnjamItem {
                        task_id: task.id,
                        task_title: task.title.clone(),
                        issue: format!(
                            "Stuck in {} for {}h (estimated {}h)",
                            task.status, elapsed, hours
                        ),
                        suggestion: "Consider escalating or reassigning.".into(),
                    });
                }
            }

            if task.assignee.is_some()
                && (task.status == TaskStatus::Backlog || task.status == TaskStatus::Ready)
            {
                let elapsed = (now - task.updated_at).num_hours();
                if elapsed > 24 {
                    items.push(UnjamItem {
                        task_id: task.id,
                        task_title: task.title.clone(),
                        issue: format!("Assigned but not started for {}h", elapsed),
                        suggestion: "Consider unassigning or escalating.".into(),
                    });
                }
            }

            if task.status == TaskStatus::Done && task.verification.is_none() {
                items.push(UnjamItem {
                    task_id: task.id,
                    task_title: task.title.clone(),
                    issue: "Completed without verification.".into(),
                    suggestion: "Reopen and verify, or verify retroactively.".into(),
                });
            }

            // Report tasks that are out of rJoules
            if (task.status == TaskStatus::InProgress || task.status == TaskStatus::Review)
                && let Some(remaining) = task.rjoule_remaining
                && remaining == 0
            {
                items.push(UnjamItem {
                    task_id: task.id,
                    task_title: task.title.clone(),
                    issue: "Out of rJoules — inference budget exhausted.".into(),
                    suggestion: "Task will auto-complete. Add rJoules to continue.".into(),
                });
            }
        }

        Ok(items)
    }

    /// Auto-resolve jammed tasks: unassign idle, reopen unverified, gas-exhaust.
    pub fn unjam_fix(&self, board_id: BoardId) -> Result<Vec<UnjamFix>, KanbanError> {
        let tasks = self.task_list(board_id, TaskFilter::all())?;
        let now = chrono::Utc::now();
        let mut fixes = Vec::new();

        for task in &tasks {
            // Unassign tasks idle > 24h
            if task.assignee.is_some()
                && (task.status == TaskStatus::Backlog || task.status == TaskStatus::Ready)
            {
                let elapsed = (now - task.updated_at).num_hours();
                if elapsed > 24 {
                    match self.task_unassign(task.id, task.owner) {
                        Ok(_) => fixes.push(UnjamFix {
                            task_id: task.id,
                            task_title: task.title.clone(),
                            action: format!("Unassigned after {}h idle", elapsed),
                        }),
                        Err(e) => fixes.push(UnjamFix {
                            task_id: task.id,
                            task_title: task.title.clone(),
                            action: format!("Unassign failed: {}", e),
                        }),
                    }
                }
            }

            // Reopen Done tasks without verification
            if task.status == TaskStatus::Done && task.verification.is_none() {
                match self.task_reopen(task.id, task.owner) {
                    Ok(_) => fixes.push(UnjamFix {
                        task_id: task.id,
                        task_title: task.title.clone(),
                        action: "Reopened (was Done without verification)".into(),
                    }),
                    Err(e) => fixes.push(UnjamFix {
                        task_id: task.id,
                        task_title: task.title.clone(),
                        action: format!("Reopen failed: {}", e),
                    }),
                }
            }

            // rJoule exhaustion: same logic, separate budget.
            // `task_rjoule_exhaust`, which stamps an rJoule-specific verification
            // reason (distinct from the gas-exhaust reason).
            if (task.status == TaskStatus::InProgress || task.status == TaskStatus::Review)
                && let Some(remaining) = task.rjoule_remaining
                && remaining == 0
            {
                self.push_exhaust_fix(
                    task,
                    now,
                    &mut fixes,
                    |id| self.task_rjoule_exhaust(id),
                    "Auto-completed (rJoules exhausted, no response)",
                    "rJoule-exhaust failed",
                );
            }
        }

        Ok(fixes)
    }

    /// Shared branch for the gas/rJoule exhaustion auto-complete in `unjam_fix`:
    /// if the task has been at zero budget for > 1 hour (grace period for the
    /// delegator to respond), call `exhaust` and push an `UnjamFix` recording
    /// the outcome. `ok_action` is the success label; `err_prefix` prefixes the
    /// failure label (followed by `: {error}`).
    fn push_exhaust_fix<E>(
        &self,
        task: &Task,
        now: chrono::DateTime<chrono::Utc>,
        fixes: &mut Vec<UnjamFix>,
        exhaust: impl Fn(TaskId) -> Result<Task, E>,
        ok_action: &'static str,
        err_prefix: &'static str,
    ) where
        E: std::fmt::Display,
    {
        let idle = (now - task.updated_at).num_minutes();
        if idle <= 60 {
            return;
        }
        let action = match exhaust(task.id) {
            Ok(_) => ok_action.to_string(),
            Err(error) => format!("{}: {}", err_prefix, error),
        };
        fixes.push(UnjamFix {
            task_id: task.id,
            task_title: task.title.clone(),
            action,
        });
    }

    /// Mark a task as Done due to gas exhaustion.
    ///
    /// Gas exhaustion is a completion path: subagents burn gas/rJoules from a
    /// budget explicitly set on the task. When gas hits zero mid-work, the
    /// task auto-completes. The delegator can reopen with more gas to continue.
    ///
    /// Internal authority: called only by the regulation/unjam loop, not
    /// exposed as an MCP tool. Must not be exposed as a tool without an
    /// actor/authority check.
        self.exhaust_task(
            task_id,
            "Gas exhausted — subagent budget consumed.",
        )
    }

    /// Mark a task as Done due to rJoule exhaustion.
    ///
    /// rJoule exhaustion is a completion path: subagents burn rJoules (inference
    /// spend) from a budget explicitly set on the task. When rJoules hit zero
    /// mid-work, the task auto-completes with an rJoule-specific verification
    /// reason (distinct from the gas-exhaust reason). The delegator can reopen
    /// with more rJoules to continue.
    ///
    /// Internal authority: called only by the regulation/unjam loop, not
    /// exposed as an MCP tool. Must not be exposed as a tool without an
    /// actor/authority check.
    pub fn task_rjoule_exhaust(&self, task_id: TaskId) -> Result<Task, KanbanError> {
        self.exhaust_task(
            task_id,
            "rJoules exhausted — inference budget consumed.",
            "task_rjoule_exhausted",
        )
    }

    /// Shared completion path for budget exhaustion: stamp a failed
    /// `Verification`, set `Done`, and emit the `REG` span. The `reason`
    /// distinguishes gas vs rJoule in the verification record; `operation`
    /// distinguishes them in the tracing span.
    fn exhaust_task(
        &self,
        task_id: TaskId,
        reason: &str,
        operation: &'static str,
    ) -> Result<Task, KanbanError> {
        let mut task = self.require_task(task_id)?;

        let verification = Verification::new(false, reason.to_string(), task.owner);
        task.verification = Some(verification);
        task.status = TaskStatus::Done;
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;

        tracing::info!(
            target: "hkask.kanban",
            operation = operation,
            task_id = %task_id,
            board_id = %task.board_id,
            "REG"
        );

        Ok(task)
    }

    /// Deduct gas from a task's remaining budget.
    ///
    /// Called by the subagent execution framework after each inference step,
    /// template execution, or tool dispatch. Logs a GasEntry recording what
    /// consumed the gas and how much.
    ///
    /// `reason` describes the cost: "inference: deepseek-v4 (500 tokens)",
    /// "template: bug-hunt", "tool: kanban_task_list", etc.
    ///
    /// Internal authority: called only by the gas-accountant closure wired via
    /// `gas_accountant_for`, not exposed as an MCP tool. Must not be exposed as
    /// a tool without an actor/authority check.

    /// Deduct rJoules from a task's inference/API budget.
    ///
    /// Same pattern as `task_consume_gas` but for the rJoule budget
    /// (250k rJoules ≈ $1 inference spend). Logs a GasEntry with kind
    /// "rjoule_spend".
    ///
    /// Internal authority: called only by the inference accounting path, not
    /// exposed as an MCP tool. Must not be exposed as a tool without an
    /// actor/authority check.
    pub fn task_consume_rjoules(
        &self,
        task_id: TaskId,
        amount: u64,
        reason: &str,
    ) -> Result<u64, KanbanError> {
        let mut task = self.require_task(task_id)?;

        let remaining = task.rjoule_remaining.unwrap_or(0);
        let new_remaining = remaining.saturating_sub(amount);
        task.rjoule_remaining = Some(new_remaining);
        task.spend_log
            .push(GasEntry::rjoule_spend(amount, reason.to_string()));
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;

        Ok(new_remaining)
    }
}
