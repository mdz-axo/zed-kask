use super::*;

impl KanbanService {
    /// Read a task for spawning: only its owner may delegate it. Writes
    /// nothing — the task's record of a spawn is the comment made once the
    /// spawn port has accepted the request.
    pub(crate) fn task_for_spawn(
        &self,
        task_id: TaskId,
        actor: WebID,
    ) -> Result<Task, KanbanError> {
        let task = self.require_task(task_id)?;
        Self::require_task_owner(&task, actor)?;
        Ok(task)
    }
}
