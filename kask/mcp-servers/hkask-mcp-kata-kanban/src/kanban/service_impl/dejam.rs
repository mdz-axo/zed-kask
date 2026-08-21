use super::*;

impl KanbanService {
    /// Mark a task as Done due to rJoule exhaustion.
    ///
    /// rJoule exhaustion is a completion path: subagents burn rJoules (inference
    /// spend) from a budget explicitly set on the task. When rJoules hit zero
    /// mid-work, the task auto-completes with an rJoule-specific verification
    /// reason. The delegator can reopen with more rJoules to continue.
    ///
    /// Internal authority: called only by the regulation/unjam loop, not
    /// exposed as an MCP tool. Must not be exposed as a tool without an
    /// actor/authority check.
    #[allow(dead_code)]
    pub(crate) fn task_rjoule_exhaust(&self, task_id: TaskId) -> Result<Task, KanbanError> {
        self.exhaust_task(
            task_id,
            "rJoules exhausted — inference budget consumed.",
            "task_rjoule_exhausted",
        )
    }

    /// Shared completion path for budget exhaustion: stamp a failed
    /// `Verification`, set `Done`, and emit the `REG` span. The `reason`
    /// is the verification record; `operation` distinguishes them in the
    /// tracing span.
    #[allow(dead_code)]
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
}
