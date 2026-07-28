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
                if elapsed > (hours as i64) * 2 {
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

            // Report tasks that are out of gas
            if (task.status == TaskStatus::InProgress || task.status == TaskStatus::Review)
                && let Some(remaining) = task.gas_remaining
                && remaining == 0
            {
                items.push(UnjamItem {
                    task_id: task.id,
                    task_title: task.title.clone(),
                    issue: "Out of gas — software budget exhausted.".into(),
                    suggestion: "Task will auto-complete. Reopen with more gas to continue.".into(),
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

            // Gas exhaustion: auto-complete tasks that have run out of gas
            // and have been sitting at zero gas for > 1 hour (grace period for
            // the delegator to respond with more gas or verification).
            if (task.status == TaskStatus::InProgress || task.status == TaskStatus::Review)
                && let Some(remaining) = task.gas_remaining
                && remaining == 0
            {
                let idle = (now - task.updated_at).num_minutes();
                if idle > 60 {
                    match self.task_gas_exhaust(task.id) {
                        Ok(_) => fixes.push(UnjamFix {
                            task_id: task.id,
                            task_title: task.title.clone(),
                            action: "Auto-completed (gas exhausted, no response)".into(),
                        }),
                        Err(e) => fixes.push(UnjamFix {
                            task_id: task.id,
                            task_title: task.title.clone(),
                            action: format!("Gas-exhaust failed: {}", e),
                        }),
                    }
                }
            }

            // rJoule exhaustion: same logic, separate budget
            if (task.status == TaskStatus::InProgress || task.status == TaskStatus::Review)
                && let Some(remaining) = task.rjoule_remaining
                && remaining == 0
            {
                let idle = (now - task.updated_at).num_minutes();
                if idle > 60 {
                    match self.task_gas_exhaust(task.id) {
                        Ok(_) => fixes.push(UnjamFix {
                            task_id: task.id,
                            task_title: task.title.clone(),
                            action: "Auto-completed (rJoules exhausted, no response)".into(),
                        }),
                        Err(e) => fixes.push(UnjamFix {
                            task_id: task.id,
                            task_title: task.title.clone(),
                            action: format!("rJoule-exhaust failed: {}", e),
                        }),
                    }
                }
            }
        }

        Ok(fixes)
    }

    /// Mark a task as Done due to gas exhaustion.
    ///
    /// Gas exhaustion is a completion path: subagents burn gas/rJoules from a
    /// budget explicitly set on the task. When gas hits zero mid-work, the
    /// task auto-completes. The delegator can reopen with more gas to continue.
    pub fn task_gas_exhaust(&self, task_id: TaskId) -> Result<Task, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

        let v = Verification::new(
            false,
            "Gas exhausted — subagent budget consumed.".into(),
            task.owner,
        );
        task.verification = Some(v);
        task.status = TaskStatus::Done;
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;

        tracing::info!(
            target: "hkask.kanban",
            operation = "task_gas_exhausted",
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
    pub fn task_consume_gas(
        &self,
        task_id: TaskId,
        amount: u64,
        reason: &str,
    ) -> Result<u64, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

        let remaining = task.gas_remaining.unwrap_or(0);
        let new_remaining = remaining.saturating_sub(amount);
        task.gas_remaining = Some(new_remaining);
        task.gas_spend
            .push(GasEntry::gas_spend(amount, reason.to_string()));
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;

        Ok(new_remaining)
    }

    /// Deduct rJoules from a task's inference/API budget.
    ///
    /// Same pattern as `task_consume_gas` but for the rJoule budget
    /// (250k rJoules ≈ $1 inference spend). Logs a GasEntry with kind
    /// "rjoule_spend".
    pub fn task_consume_rjoules(
        &self,
        task_id: TaskId,
        amount: u64,
        reason: &str,
    ) -> Result<u64, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

        let remaining = task.rjoule_remaining.unwrap_or(0);
        let new_remaining = remaining.saturating_sub(amount);
        task.rjoule_remaining = Some(new_remaining);
        task.gas_spend
            .push(GasEntry::rjoule_spend(amount, reason.to_string()));
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;

        Ok(new_remaining)
    }
}
