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
        let (_, comment) = self.mutate_task(task_id, |task| {
            Self::require_task_actor(task, author)?;
            let comment = Comment::new(task_id, author, body.to_string());
            task.comments.push(comment.clone());
            Ok(comment)
        })?;
        Ok(comment)
    }

    /// Fetch comments starting from a given index (for incremental polling),
    /// with the task's total comment count — the poller's next cursor. The
    /// total comes from the task, never from the requested index: adding
    /// the two (mcp-tool-review KK-06) reported 5 comments on a task with 2
    /// when asked for index 5, and a poller that stored it skipped the next
    /// three comments.
    pub(crate) fn task_comments_since(
        &self,
        task_id: TaskId,
        since_index: usize,
    ) -> Result<(Vec<Comment>, usize), KanbanError> {
        let task = self.require_task(task_id)?;
        let total = task.comments.len();
        Ok((task.comments.into_iter().skip(since_index).collect(), total))
    }

    pub(crate) fn task_add_deliverable(
        &self,
        task_id: TaskId,
        path: &str,
        actor: WebID,
    ) -> Result<Task, KanbanError> {
        let (task, ()) = self.mutate_task(task_id, |task| {
            Self::require_task_actor(task, actor)?;
            task.deliverables.push(path.to_string());
            Ok(())
        })?;
        Ok(task)
    }
}
