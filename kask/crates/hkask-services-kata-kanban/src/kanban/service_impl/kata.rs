use super::*;

impl KanbanService {
    pub fn task_coaching_prompt(&self, task_id: TaskId) -> Result<String, KanbanError> {
        let task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

        let target = if task.criteria.is_empty() {
            format!("Complete task '{}'", task.title)
        } else {
            task.criteria
                .iter()
                .map(|c| format!("- {}", c.description))
                .collect::<Vec<_>>()
                .join(
                    "
",
                )
        };

        let mut evidence = format!(
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
            evidence.push_str(
                "

Deliverables (file links = work output):",
            );
            for d in &task.deliverables {
                evidence.push_str(&format!(
                    "
  - {}",
                    d
                ));
            }
        }

        if !task.comments.is_empty() {
            evidence.push_str(
                "

Comment thread (agent/userpod communication):",
            );
            for c in &task.comments {
                evidence.push_str(&format!(
                    "
  [{}] {}: {}",
                    c.created_at.format("%H:%M"),
                    c.author.redacted_display(),
                    c.body,
                ));
            }
        }

        let actual = evidence;

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

             Q1 — Target Condition:
{target}

             Q2 — Actual Condition:
{actual}

             Q3 — Obstacles: What is preventing this task from reaching the target?              Which ONE obstacle are you addressing now?

             Q4 — Next Step: What experiment will you run? What do you expect?

             Q5 — How quickly can we go and see what we learned?

             Respond with your answers. The coach will guide, not solve.",
            title = task.title,
        ))
    }

    pub fn task_improvement_prompt(&self, task_id: TaskId) -> Result<String, KanbanError> {
        let task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

        let direction = task.description.as_deref().unwrap_or(&task.title);
        let mut current = format!(
            "Task '{}' is in status '{}'.
Evidence: {} deliverables, {} comments, {} criteria.",
            task.title,
            task.status,
            task.deliverables.len(),
            task.comments.len(),
            task.criteria.len()
        );
        if !task.deliverables.is_empty() {
            current.push_str(
                "
Deliverables:",
            );
            for d in &task.deliverables {
                current.push_str(&format!(
                    "
  {}",
                    d
                ));
            }
        }
        if !task.comments.is_empty() {
            current.push_str(
                "
Recent comments:",
            );
            for c in task.comments.iter().rev().take(3) {
                current.push_str(&format!(
                    "
  [{}] {}",
                    c.created_at.format("%H:%M"),
                    c.body
                ));
            }
        }

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

             Step 1 — Understand the Direction:
{direction}

             Step 2 — Grasp the Current Condition:
{current}

             Step 3 — Establish the Next Target Condition:
             What specific, measurable condition do you want to achieve?

             Step 4 — Iterate: What ONE experiment will you run? What do you predict?
             Plan → Do → Check → Act. Record your experiment and result.",
            title = task.title,
            direction = direction,
            current = current,
        ))
    }

    pub fn task_practice_prompt(
        &self,
        task_id: TaskId,
        sub_problem: &str,
    ) -> Result<String, KanbanError> {
        let task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

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

             List what you OBSERVE (facts, data, evidence):
             1.
2.
3.

             List what you INTERPRET (assumptions, guesses, theories):
             1.
2.
3.

             For each interpretation, ask: How would I test this?              What experiment would distinguish this interpretation from alternatives?",
            title = task.title,
            sub_problem = sub_problem,
        ))
    }
}
