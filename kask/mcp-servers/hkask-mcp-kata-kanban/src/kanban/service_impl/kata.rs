//! Kata state for the `kanban_task_kata_*` MCP tools.
//!
//! `task_kata_record` persists the durable Improvement Kata state on a task
//! (target condition, focus obstacle, last experiment) — the write side the
//! prompts read back. The prompt methods produce a human-readable summary
//! of the task's current state — including that kata state — for display in
//! the chat stream. They are **not** the authoritative kata methodology —
//! that lives in the `kata-improvement` skill
//! (`.agents/skills/kata-improvement/SKILL.md`, Coaching Kata section) and
//! its registry templates (`kask/registry/templates/kata-improvement/`),
//! which execute via skill execution the inference port with structured
//! JSON output.
//!
//! The split is deliberate: the MCP tools return a sync result for
//! immediate human display and durable state for the next session; the
//! registry templates run skill execution with step chaining. This module
//! gathers the task-specific evidence (criteria, deliverables, comments,
//! kata state, status) that makes the display useful, then points the
//! learner to the canonical skill for the methodology.
//!
//! `kanban_task_kata_prompt` dispatches on its `stage` parameter to call
//! the prompt methods — the contract is "produce a display summary for
//! this stage", not "produce the authoritative kata questions".

use super::*;

/// One kata-state field update. `None` keeps the field; `focus_obstacle`'s
/// double-Option distinguishes "keep" (`None`) from "clear"
/// (`Some(None)`) from "set" (`Some(Some(text))`) — the `task_update`
/// precedent. At least one field must be present (the service refuses an
/// empty update instead of recording a no-op).
#[derive(Debug, Clone)]
pub(crate) struct KataStateUpdate {
    pub target_condition: Option<String>,
    pub focus_obstacle: Option<Option<String>>,
    pub last_experiment: Option<KataExperimentRecord>,
}

impl KanbanService {
    /// Display summary for the Coaching Kata stage (task in Backlog).
    ///
    /// Gathers the task's target (criteria or title) and actual condition
    /// (status, assignee, deliverables, comments), then references the
    /// `kata-improvement` skill's Coaching Kata section for the 5-question methodology.
    pub(crate) fn task_coaching_prompt(&self, task_id: TaskId) -> Result<String, KanbanError> {
        let task = self.require_task(task_id)?;

        let target = if task.criteria.is_empty() {
            format!("Complete task '{}'", task.title)
        } else {
            task.criteria
                .iter()
                .map(|c| format!("- {}", c.description))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let actual = self.task_evidence_summary(&task);

        // P9: Regulation span — kata prompt generated for human display
        tracing::info!(
            target: "reg.kata",
            operation = "coaching_prompt_generated",
            task_id = %task.id,
            title = %task.title,
            "REG"
        );

        Ok(format!(
            "Coaching Kata — Task: {title}

Target Condition:
{target}

Actual Condition:
{actual}

The 5-question Coaching Kata methodology (Q1 Target → Q2 Actual → Q3 Obstacles \
→ Q4 Next Step → Q5 Feedback) is defined in the `kata-improvement` skill's Coaching Kata \
section. Invoke `skill kata-improvement` with this task as context for the full guided cycle.",
            title = task.title,
        ))
    }

    /// Display summary for the Improvement Kata stage (task in Ready).
    ///
    /// Gathers the task's direction (description or title) and current
    /// condition (status, deliverables, comments), then references the
    /// `kata-improvement` skill for the 4-step PDCA methodology.
    pub(crate) fn task_improvement_prompt(&self, task_id: TaskId) -> Result<String, KanbanError> {
        let task = self.require_task(task_id)?;

        let direction = task.description.as_deref().unwrap_or(&task.title);
        let current = self.task_evidence_summary(&task);

        // P9: Regulation span — kata prompt generated for human display
        tracing::info!(
            target: "reg.kata",
            operation = "improvement_prompt_generated",
            task_id = %task.id,
            title = %task.title,
            "REG"
        );

        Ok(format!(
            "Improvement Kata — Task: {title}

Direction:
{direction}

Current Condition:
{current}

The 4-step Improvement Kata methodology (Step 1 Direction → Step 2 Current → \
Step 3 Target → Step 4 Experiment/PDCA) is defined in the `kata-improvement` \
skill. Invoke `skill kata-improvement` with this task as context for the full \
guided cycle.",
            title = task.title,
            direction = direction,
            current = current,
        ))
    }

    /// Display summary for the Starter Kata practice stage (task in Progress).
    ///
    /// Gathers the task's current state and frames the `sub_problem` as the
    /// focus for an observation drill, then references the `kata-improvement`
    /// skill's beginner_mode drills.
    pub(crate) fn task_practice_prompt(
        &self,
        task_id: TaskId,
        sub_problem: &str,
    ) -> Result<String, KanbanError> {
        let task = self.require_task(task_id)?;
        let evidence = self.task_evidence_summary(&task);

        // P9: Regulation span — kata prompt generated for human display
        tracing::info!(
            target: "reg.kata",
            operation = "practice_prompt_generated",
            task_id = %task.id,
            sub_problem = %sub_problem,
            "REG"
        );

        Ok(format!(
            "Starter Kata — Observation Drill
Task: {title}
Focus: {sub_problem}

Current evidence:
{evidence}

The Observation Drill (separate facts from interpretations, then design a \
distinguishing experiment) is defined in the `kata-improvement` skill's \
beginner_mode drills. Invoke `skill kata-improvement` with this task as \
context for the full guided drill.",
            title = task.title,
            sub_problem = sub_problem,
            evidence = evidence,
        ))
    }

    /// Record durable Improvement Kata state on a task — the write side the
    /// kata prompts read back, so a new session continues the routine
    /// instead of restarting from zero. Authority is owner-or-assignee
    /// (the learner works the task). Convergent by construction (the same
    /// input re-applies the same state), so like `task_update` it needs no
    /// idempotency key.
    ///
    /// Creating the state requires `target_condition` (the kata's step-3
    /// artifact — an obstacle or experiment cannot be recorded against a
    /// target nobody wrote down); afterwards every field updates
    /// independently.
    pub(crate) fn task_kata_record(
        &self,
        task_id: TaskId,
        actor: WebID,
        update: KataStateUpdate,
    ) -> Result<Task, KanbanError> {
        if update.target_condition.is_none()
            && update.focus_obstacle.is_none()
            && update.last_experiment.is_none()
        {
            return Err(KanbanError::InvalidInput(
                "kata record requires at least one field: target_condition, focus_obstacle, or last_experiment"
                    .to_string(),
            ));
        }
        let (task, ()) = self.mutate_task(task_id, |task| {
            Self::require_task_actor(task, actor)?;
            if let Some(experiment) = &update.last_experiment
                && (experiment.next_step.trim().is_empty()
                    || experiment.prediction.trim().is_empty())
            {
                return Err(KanbanError::InvalidInput(
                    "last_experiment requires a non-empty next_step and prediction".to_string(),
                ));
            }
            let Some(state) = task.kata_state.as_mut() else {
                let Some(target_condition) = update.target_condition.clone() else {
                    return Err(KanbanError::InvalidInput(
                        "recording kata state on a task with none requires target_condition"
                            .to_string(),
                    ));
                };
                task.kata_state = Some(TaskKataState {
                    target_condition,
                    focus_obstacle: update.focus_obstacle.clone().flatten(),
                    last_experiment: update.last_experiment.clone(),
                });
                return Ok(());
            };
            if let Some(target_condition) = &update.target_condition {
                state.target_condition = target_condition.clone();
            }
            if let Some(focus_obstacle) = &update.focus_obstacle {
                state.focus_obstacle = focus_obstacle.clone();
            }
            if let Some(experiment) = &update.last_experiment {
                state.last_experiment = Some(experiment.clone());
            }
            Ok(())
        })?;

        // P9: Regulation span — kata state recorded
        tracing::info!(
            target: "reg.kata",
            operation = "kata_state_recorded",
            task_id = %task.id,
            title = %task.title,
            "REG"
        );

        Ok(task)
    }

    /// Build a human-readable evidence summary from a task's current state.
    ///
    /// Shared by all three kata prompt methods. Includes status, assignee,
    /// estimates, deliverables, and recent comments — the task-specific data
    /// that makes the display useful regardless of which kata stage the task
    /// is in.
    fn task_evidence_summary(&self, task: &Task) -> String {
        let mut summary = format!(
            "Status: {}
Assignee: {}
Est. hours: {}
Story points: {}
Updated: {}",
            task.status,
            task.assignee
                .map(|a| a.redacted_display())
                .unwrap_or_else(|| "none".into()),
            task.estimated_hours
                .map_or("?".into(), |h| format!("{}h", h)),
            task.story_points.map_or("?".into(), |p| format!("{}pt", p)),
            task.updated_at.format("%Y-%m-%d %H:%M"),
        );

        if let Some(kata) = &task.kata_state {
            summary.push_str("\n\nKata state (recorded via kanban_task_kata_record):");
            summary.push_str(&format!("\nTarget condition: {}", kata.target_condition));
            match &kata.focus_obstacle {
                Some(obstacle) => summary.push_str(&format!("\nFocus obstacle: {obstacle}")),
                None => summary.push_str("\nFocus obstacle: none recorded"),
            }
            match &kata.last_experiment {
                Some(experiment) => {
                    summary.push_str(&format!(
                        "\nLast experiment: {} (expect: {})",
                        experiment.next_step, experiment.prediction
                    ));
                    match &experiment.outcome {
                        Some(outcome) => summary.push_str(&format!(" — observed: {outcome}")),
                        None => summary.push_str(" — outcome: pending_check"),
                    }
                }
                None => summary.push_str("\nLast experiment: none yet"),
            }
        }

        if !task.deliverables.is_empty() {
            summary.push_str("\n\nDeliverables (file links = work output):");
            for d in &task.deliverables {
                summary.push_str(&format!("\n  - {d}"));
            }
        }

        if !task.comments.is_empty() {
            summary.push_str("\n\nComment thread (agent communication):");
            for c in task.comments.iter().rev().take(3) {
                summary.push_str(&format!(
                    "\n  [{}] {}: {}",
                    c.created_at.format("%H:%M"),
                    c.author.redacted_display(),
                    c.body,
                ));
            }
        }

        summary
    }
}
