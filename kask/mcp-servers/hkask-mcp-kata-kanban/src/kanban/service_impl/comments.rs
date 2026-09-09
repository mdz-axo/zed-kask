use super::*;

impl KanbanService {
    pub(crate) fn task_comment(
        &self,
        task_id: TaskId,
        author: WebID,
        body: &str,
    ) -> Result<Comment, KanbanError> {
        // Test-seam fault (see `fail_next_comments`): armed only by the
        // replay-protection integration suite.
        if self
            .comment_faults
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0
        {
            self.comment_faults
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            return Err(KanbanError::Internal(
                "injected task_comment fault (test seam)".to_string(),
            ));
        }
        let mut task = self.require_task(task_id)?;
        Self::require_task_actor(&task, author)?;
        let comment = Comment::new(task_id, author, body.to_string());
        task.comments.push(comment.clone());
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(comment)
    }

    /// Fetch comments starting from a given index (for incremental polling).
    pub(crate) fn task_comments_since(
        &self,
        task_id: TaskId,
        since_index: usize,
    ) -> Result<Vec<Comment>, KanbanError> {
        let task = self.require_task(task_id)?;
        Ok(task.comments.into_iter().skip(since_index).collect())
    }

    pub(crate) fn task_add_deliverable(
        &self,
        task_id: TaskId,
        path: &str,
        actor: WebID,
    ) -> Result<Task, KanbanError> {
        let mut task = self.require_task(task_id)?;
        Self::require_task_actor(&task, actor)?;
        task.deliverables.push(path.to_string());
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;
        Ok(task)
    }
}
