use super::*;

impl KanbanService {
    pub(crate) fn spawn_task(
        &self,
        task_id: TaskId,
        spawn_spec: super::SpawnSpec,
        actor: WebID,
    ) -> Result<String, KanbanError> {
        let mut task = self.require_task(task_id)?;
        Self::require_task_owner(&task, actor)?;

        // Write the durable swarm link before anything else so both the
        // worktree path and the in-memory fallback path expose it via
        // `kanban_task_delegate_result` without each having to set it.
        task.swarm_id = spawn_spec.swarm_id.clone();

        let spawn_note = format!(
            "Spawn configured: level={}, skills={:?}, memory={}, tools={:?}",
            spawn_spec.delegation_level,
            spawn_spec.delegated_skills,
            spawn_spec.memory_scope,
            spawn_spec.tool_servers,
        );
        let comment = super::Comment::new(task_id, task.owner, spawn_note);
        task.comments.push(comment);
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(format!(
            "Spawn configured for '{}'. Skills: {:?}",
            task.title, spawn_spec.delegated_skills
        ))
    }
}
