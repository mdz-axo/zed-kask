use super::*;

impl KanbanService {
    pub fn task_comment(
        &self,
        task_id: TaskId,
        author: WebID,
        body: &str,
    ) -> Result<Comment, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;
        Self::require_task_actor(&task, author)?;
        let comment = Comment::new(task_id, author, body.to_string());
        task.comments.push(comment.clone());
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(comment)
    }

    pub fn task_comments(&self, task_id: TaskId) -> Result<Vec<Comment>, KanbanError> {
        let task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;
        Ok(task.comments)
    }

    /// Fetch comments starting from a given index (for incremental polling).
    pub fn task_comments_since(
        &self,
        task_id: TaskId,
        since_index: usize,
    ) -> Result<Vec<Comment>, KanbanError> {
        let task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;
        Ok(task.comments.into_iter().skip(since_index).collect())
    }

    pub fn task_add_deliverable(
        &self,
        task_id: TaskId,
        path: &str,
        actor: WebID,
    ) -> Result<Task, KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;
        Self::require_task_actor(&task, actor)?;
        task.deliverables.push(path.to_string());
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(task)
    }
}
