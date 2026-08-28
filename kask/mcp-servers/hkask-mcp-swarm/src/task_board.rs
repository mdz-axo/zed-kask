//! Task board — persistent per-swarm task tracking for swarm-intelligence.
//!
//! The swarm-intelligence PDCA loop re-measures swarm-state distance each
//! cycle, but individual task progress was ephemeral — recomputed from the
//! last `delegate_results` array each invocation. The task board closes that
//! gap: `swarm_execute_plan_local` writes task status here, and the
//! `swarm_task_board` MCP tool lets the Curator's ORIENT phase query durable
//! task progress ("task 3 failed twice, task 5 succeeded") without
//! re-deriving it from delegate_results.
//!
//! Persistence mirrors `LocalSwarmRegistry`: one JSON file per swarm
//! (`<dir>/<swarm_id>/task_board.json`), reloaded from disk on every read.
//! The board is a flat list of `TaskEntry` items keyed by `task_id`.

use crate::error::LocalSwarmError;
use serde::{Deserialize, Serialize};

/// A single task on the board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaskEntry {
    /// Stable task identifier. When the caller supplies a `task_id` in the
    /// plan delegation, it is used directly; otherwise a synthetic id is
    /// derived from the agent + task hash.
    pub task_id: String,
    /// The agent assigned to this task.
    pub agent_name: String,
    /// The task text.
    pub task: String,
    /// Current status: `pending`, `in_progress`, `complete`, `failed`.
    pub status: TaskStatus,
    /// Number of times this task has been attempted.
    pub attempt_count: u32,
    /// Number of times this task has failed.
    pub fail_count: u32,
    /// Brief result summary from the last attempt (truncated for storage).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_result_summary: Option<String>,
    /// Whether the last attempt's deterministic evaluator passed (when an
    /// evaluator was provided). `None` = no evaluator was run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_evaluator_pass: Option<bool>,
    /// RFC 3339 timestamp of the last attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<String>,
}

/// Task lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Not yet attempted.
    Pending,
    /// Currently running (between plan execution and result check).
    InProgress,
    /// Completed successfully (evaluator passed, or no evaluator + non-empty response).
    Complete,
    /// Failed (evaluator failed, or delegation errored).
    Failed,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, TaskStatus::Complete | TaskStatus::Failed)
    }
}

/// The task board — a flat list of tasks for one swarm.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskBoard {
    #[serde(default)]
    pub tasks: Vec<TaskEntry>,
}

impl TaskBoard {
    /// Load a swarm's task board from `<dir>/<swarm_id>/task_board.json`.
    /// Returns an empty board if the file does not exist (a new swarm has
    /// no tasks yet — absence ≠ error).
    pub fn load(dir: &str, swarm_id: &str) -> Result<Self, LocalSwarmError> {
        let path = board_path(dir, swarm_id);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path).map_err(|e| {
            LocalSwarmError::Io(format!("failed to read task board {}: {e}", path.display()))
        })?;
        serde_json::from_str(&content).map_err(|e| {
            LocalSwarmError::InvalidInput(format!(
                "failed to parse task board {}: {e}",
                path.display()
            ))
        })
    }

    /// Write the task board to `<dir>/<swarm_id>/task_board.json`.
    pub fn save(&self, dir: &str, swarm_id: &str) -> Result<(), LocalSwarmError> {
        let path = board_path(dir, swarm_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                LocalSwarmError::Io(format!(
                    "failed to create task board dir {}: {e}",
                    parent.display()
                ))
            })?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|e| {
            LocalSwarmError::InvalidInput(format!("failed to serialize task board: {e}"))
        })?;
        std::fs::write(&path, content).map_err(|e| {
            LocalSwarmError::Io(format!(
                "failed to write task board {}: {e}",
                path.display()
            ))
        })
    }

    /// Record or update a task after a delegation attempt. If a task with
    /// the same `task_id` exists, its status/counts are updated; otherwise a
    /// new entry is appended.
    pub fn record_attempt(
        &mut self,
        task_id: &str,
        agent_name: &str,
        task: &str,
        evaluator_pass: Option<bool>,
        result_summary: Option<String>,
    ) {
        let now = chrono::Utc::now().to_rfc3339();
        let status = match evaluator_pass {
            Some(true) => TaskStatus::Complete,
            Some(false) => TaskStatus::Failed,
            None => TaskStatus::Complete, // no evaluator + non-empty response = complete
        };

        if let Some(entry) = self.tasks.iter_mut().find(|t| t.task_id == task_id) {
            entry.attempt_count += 1;
            if status == TaskStatus::Failed {
                entry.fail_count += 1;
            }
            entry.status = status;
            entry.last_result_summary = result_summary;
            entry.last_evaluator_pass = evaluator_pass;
            entry.last_attempt_at = Some(now);
        } else {
            self.tasks.push(TaskEntry {
                task_id: task_id.to_string(),
                agent_name: agent_name.to_string(),
                task: task.to_string(),
                status,
                attempt_count: 1,
                fail_count: if status == TaskStatus::Failed { 1 } else { 0 },
                last_result_summary: result_summary,
                last_evaluator_pass: evaluator_pass,
                last_attempt_at: Some(now),
            });
        }
    }

    /// Mark a task as failed (delegation errored before producing a result).
    pub fn record_failure(&mut self, task_id: &str, agent_name: &str, task: &str, error: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        let summary = Some(format!("error: {error}"));
        if let Some(entry) = self.tasks.iter_mut().find(|t| t.task_id == task_id) {
            entry.attempt_count += 1;
            entry.fail_count += 1;
            entry.status = TaskStatus::Failed;
            entry.last_result_summary = summary;
            entry.last_evaluator_pass = Some(false);
            entry.last_attempt_at = Some(now);
        } else {
            self.tasks.push(TaskEntry {
                task_id: task_id.to_string(),
                agent_name: agent_name.to_string(),
                task: task.to_string(),
                status: TaskStatus::Failed,
                attempt_count: 1,
                fail_count: 1,
                last_result_summary: summary,
                last_evaluator_pass: Some(false),
                last_attempt_at: Some(now),
            });
        }
    }

    /// Whether all tasks are terminal (complete or failed).
    pub fn all_terminal(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.iter().all(|t| t.status.is_terminal())
    }

    /// Whether all tasks are complete (no failures).
    pub fn all_complete(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.iter().all(|t| t.status == TaskStatus::Complete)
    }

    /// Count of tasks by status.
    pub fn counts(&self) -> TaskCounts {
        let mut pending = 0;
        let mut in_progress = 0;
        let mut complete = 0;
        let mut failed = 0;
        for t in &self.tasks {
            match t.status {
                TaskStatus::Pending => pending += 1,
                TaskStatus::InProgress => in_progress += 1,
                TaskStatus::Complete => complete += 1,
                TaskStatus::Failed => failed += 1,
            }
        }
        TaskCounts {
            total: self.tasks.len(),
            pending,
            in_progress,
            complete,
            failed,
        }
    }
}

/// Summary counts for a task board.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCounts {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub complete: usize,
    pub failed: usize,
}

/// Compute the file path for a swarm's task board.
fn board_path(dir: &str, swarm_id: &str) -> std::path::PathBuf {
    let safe_id = crate::sanitize::sanitize_agent_id(swarm_id).unwrap_or_default();
    std::path::Path::new(dir)
        .join(safe_id)
        .join("task_board.json")
}

/// Derive a stable task_id from an agent name and task text when the caller
/// does not supply one. Uses a simple hash so the same (agent, task) pair
/// maps to the same task_id across invocations — the board accumulates
/// attempts rather than growing unboundedly.
pub fn derive_task_id(agent_name: &str, task: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    agent_name.hash(&mut hasher);
    task.hash(&mut hasher);
    format!("task-{agent_name}-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_attempt_appends_new_task() {
        let mut board = TaskBoard::default();
        board.record_attempt(
            "task-1",
            "analyst",
            "analyze X",
            Some(true),
            Some("done".into()),
        );
        assert_eq!(board.tasks.len(), 1);
        assert_eq!(board.tasks[0].status, TaskStatus::Complete);
        assert_eq!(board.tasks[0].attempt_count, 1);
        assert_eq!(board.tasks[0].fail_count, 0);
    }

    #[test]
    fn record_attempt_updates_existing_task() {
        let mut board = TaskBoard::default();
        board.record_attempt(
            "task-1",
            "analyst",
            "analyze X",
            Some(false),
            Some("fail".into()),
        );
        board.record_attempt(
            "task-1",
            "analyst",
            "analyze X",
            Some(true),
            Some("done".into()),
        );
        assert_eq!(
            board.tasks.len(),
            1,
            "same task_id updates, does not append"
        );
        assert_eq!(board.tasks[0].attempt_count, 2);
        assert_eq!(board.tasks[0].fail_count, 1);
        assert_eq!(board.tasks[0].status, TaskStatus::Complete);
    }

    #[test]
    fn record_failure_marks_failed() {
        let mut board = TaskBoard::default();
        board.record_failure("task-1", "analyst", "analyze X", "agent not found");
        assert_eq!(board.tasks[0].status, TaskStatus::Failed);
        assert_eq!(board.tasks[0].fail_count, 1);
        assert!(
            board.tasks[0]
                .last_result_summary
                .as_deref()
                .unwrap()
                .contains("agent not found")
        );
    }

    #[test]
    fn all_terminal_and_all_complete() {
        let mut board = TaskBoard::default();
        // Empty board is NOT all_terminal (no tasks).
        assert!(!board.all_terminal());
        assert!(!board.all_complete());

        board.record_attempt("t1", "a", "x", Some(true), None);
        // One complete task = all_terminal AND all_complete.
        assert!(board.all_terminal());
        assert!(board.all_complete());

        board.record_attempt("t2", "b", "y", Some(false), None);
        // complete + failed = all_terminal but NOT all_complete.
        assert!(board.all_terminal());
        assert!(!board.all_complete());

        board.record_attempt("t3", "c", "z", Some(true), None);
        assert!(!board.all_complete(), "still one failed");
    }

    #[test]
    fn counts_are_correct() {
        let mut board = TaskBoard::default();
        board.record_attempt("t1", "a", "x", Some(true), None);
        board.record_attempt("t2", "b", "y", Some(false), None);
        board.record_attempt("t3", "c", "z", Some(true), None);
        let counts = board.counts();
        assert_eq!(counts.total, 3);
        assert_eq!(counts.complete, 2);
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.pending, 0);
    }

    #[test]
    fn derive_task_id_is_stable() {
        let id1 = derive_task_id("analyst", "analyze market");
        let id2 = derive_task_id("analyst", "analyze market");
        let id3 = derive_task_id("analyst", "analyze different");
        assert_eq!(id1, id2, "same (agent, task) → same id");
        assert_ne!(id1, id3, "different task → different id");
        assert!(id1.starts_with("task-analyst-"));
    }

    #[test]
    fn save_and_load_round_trip() {
        // Use a temp dir for the round-trip test.
        let dir = std::env::temp_dir().join(format!(
            "hkask-task-board-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let swarm_id = "test-swarm";

        let mut board = TaskBoard::default();
        board.record_attempt(
            "t1",
            "analyst",
            "analyze X",
            Some(true),
            Some("done".into()),
        );
        board.record_failure("t2", "researcher", "search Y", "timeout");
        board.save(dir.to_str().unwrap(), swarm_id).expect("save");

        let loaded = TaskBoard::load(dir.to_str().unwrap(), swarm_id).expect("load");
        assert_eq!(loaded.tasks.len(), 2);
        assert_eq!(loaded.tasks[0].status, TaskStatus::Complete);
        assert_eq!(loaded.tasks[1].status, TaskStatus::Failed);

        // Clean up.
        let _ = std::fs::remove_dir_all(&dir);
    }
}
