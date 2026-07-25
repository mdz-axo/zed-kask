use super::*;

impl KanbanService {
    pub fn spawn_task(
        &self,
        task_id: TaskId,
        spawn_spec: super::SpawnSpec,
        actor: WebID,
    ) -> Result<String, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;
        Self::require_task_owner(&task, actor)?;

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
