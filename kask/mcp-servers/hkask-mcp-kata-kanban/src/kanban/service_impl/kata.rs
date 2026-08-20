//! Display-only kata prompt summaries for the `kanban_task_kata_*` MCP tools.
//!
//! These methods produce a human-readable summary of the task's current state
//! for display in the chat stream. They are **not** the authoritative kata
//! methodology — that lives in the `kata-coaching` and `kata-improvement`
//! skills (`.agents/skills/kata-*/SKILL.md`) and their registry templates
//! (`kask/registry/templates/kata-*/`), which execute via the skill cascade
//! the inference port with structured JSON output.
//!
//! The split is deliberate: the MCP tools return a sync `String` for immediate
//! human display; the registry templates run the async skill cascade with
//! rJoule accounting and step chaining. This module gathers the task-specific
//! evidence (criteria, deliverables, comments, status) that makes the display
//! useful, then points the learner to the canonical skill for the methodology.
//!
//! `socratic.rs` dispatches on `task.status` to call these methods — the
//! contract is "produce a display summary for this stage", not "produce the
//! authoritative kata questions".

use super::*;

impl KanbanService {
    /// Display summary for the Coaching Kata stage (task in Backlog).
    ///
    /// Gathers the task's target (criteria or title) and actual condition
    /// (status, assignee, deliverables, comments), then references the
    /// `kata-coaching` skill for the 5-question methodology.
    pub fn task_coaching_prompt(&self, task_id: TaskId) -> Result<String, KanbanError> {
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
→ Q4 Next Step → Q5 Feedback) is defined in the `kata-coaching` skill. Invoke \
`skill kata-coaching` with this task as context for the full guided cycle.",
            title = task.title,
        ))
    }

    /// Display summary for the Improvement Kata stage (task in Ready).
    ///
    /// Gathers the task's direction (description or title) and current
    /// condition (status, deliverables, comments), then references the
    /// `kata-improvement` skill for the 4-step PDCA methodology.
    pub fn task_improvement_prompt(&self, task_id: TaskId) -> Result<String, KanbanError> {
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
    pub fn task_practice_prompt(
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
