use super::*;

impl KanbanService {
    pub fn verification_prompt(
        &self,
        task_id: TaskId,
        evidence: &str,
    ) -> Result<String, KanbanError> {
        let task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

        if task.criteria.is_empty() {
            return Err(KanbanError::InvalidInput(
                "Task has no acceptance criteria".into(),
            ));
        }

        let criteria_text: Vec<String> = task
            .criteria
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{}. {}", i + 1, c.description))
            .collect();

        Ok(format!(
            "Verify whether this task satisfies its acceptance criteria.

             Task: {title}
             Evidence: {evidence}

             Criteria:
{criteria}

             Return JSON with: passed (bool), reasoning (string),              criteria_results (array of objects with: criterion, satisfied, evidence_found, feedback).              Be rigorous. A criterion is satisfied ONLY if concrete evidence exists.",
            title = task.title,
            evidence = evidence,
            criteria = criteria_text.join("
"),
        ))
    }

    pub fn verify_with_llm(
        &self,
        task_id: TaskId,
        verifier: WebID,
        llm_json: &str,
    ) -> Result<(Task, Verification), KanbanError> {
        let mut task = self.task_get(task_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "task".to_string(),
                id: task_id.to_string(),
            })
        })?;

        if task.status != TaskStatus::Review {
            return Err(KanbanError::InvalidTransition {
                task: task_id,
                from: task.status,
                to: TaskStatus::Done,
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(llm_json)
            .map_err(|e| KanbanError::InvalidInput(format!("Invalid LLM JSON: {e}")))?;

        let passed = parsed["passed"].as_bool().unwrap_or(false);
        let reasoning = parsed["reasoning"]
            .as_str()
            .unwrap_or("No reasoning provided")
            .to_string();

        let verification = Verification::new(passed, reasoning, verifier);
        task.verification = Some(verification.clone());

        if passed {
            task.status = TaskStatus::Done;
        }
        task.updated_at = chrono::Utc::now();
        self.update_task_triple(&task)?;

        Ok((task, verification))
    }
}
