use super::*;

impl KanbanService {
    pub fn decompose_prompt(
        &self,
        board_id: BoardId,
        project_description: &str,
        target_task_points: Option<u32>,
        target_hours: Option<f64>,
    ) -> Result<String, KanbanError> {
        self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;
        let sizing_guidance = match (target_task_points, target_hours) {
            (Some(p), Some(h)) => {
                format!("Each task should be approximately {p} story points or {h} hours.")
            }
            (Some(p), None) => format!("Each task should be approximately {p} story points."),
            (None, Some(h)) => format!("Each task should be approximately {h} hours."),
            (None, None) => "Aim for tasks of 2-8 hours each.".to_string(),
        };
        let prompt = format!(
            "Decompose this project into kanban tasks.

             Project: {project_description}
             Sizing: {sizing_guidance}

             Return JSON with a tasks array. Each task: title, description, story_points (int),             estimated_hours (float), labels (array), criteria (array), priority, dependencies.             Include recomposition strategy.",
            project_description = project_description,
            sizing_guidance = sizing_guidance
        );
        Ok(prompt)
    }

    pub fn decompose_populate(
        &self,
        board_id: BoardId,
        owner: WebID,
        json_output: &str,
    ) -> Result<(usize, Option<String>), KanbanError> {
        self.board_get(board_id)?.ok_or_else(|| {
            KanbanError::NotFound(NotFound {
                entity_type: "board".to_string(),
                id: board_id.to_string(),
            })
        })?;

        let tasks_array = Self::validate_decompose_json(json_output)?;
        let phase_map = self.create_phases_from_recomposition(board_id, json_output)?;

        let mut created = 0usize;
        for task_val in &tasks_array {
            let spec = Self::build_task_spec_from_json(task_val, &phase_map);
            self.task_create(board_id, spec, owner)?;
            created += 1;
        }

        let parsed: serde_json::Value = serde_json::from_str(json_output)
            .map_err(|e| KanbanError::InvalidInput(format!("Invalid JSON: {e}")))?;
        let recomposition = parsed["recomposition"]["strategy"]
            .as_str()
            .or_else(|| parsed["recomposition"].as_str())
            .map(String::from);

        Ok((created, recomposition))
    }

    fn validate_decompose_json(json_output: &str) -> Result<Vec<serde_json::Value>, KanbanError> {
        let parsed: serde_json::Value = serde_json::from_str(json_output)
            .map_err(|e| KanbanError::InvalidInput(format!("Invalid JSON: {e}")))?;
        if parsed.get("tasks").is_none() {
            return Err(KanbanError::InvalidInput(
                "JSON must have a tasks array at top level".into(),
            ));
        }
        let tasks_array = parsed["tasks"]
            .as_array()
            .ok_or_else(|| KanbanError::InvalidInput("'tasks' must be an array".into()))?;
        if tasks_array.is_empty() {
            return Err(KanbanError::InvalidInput("'tasks' array is empty".into()));
        }
        for (i, task_val) in tasks_array.iter().enumerate() {
            if task_val
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                return Err(KanbanError::InvalidInput(format!(
                    "Task {} is missing 'title' field",
                    i + 1
                )));
            }
        }
        Ok(tasks_array.clone())
    }

    fn create_phases_from_recomposition(
        &self,
        board_id: BoardId,
        json_output: &str,
    ) -> Result<std::collections::HashMap<String, hkask_types::PhaseId>, KanbanError> {
        let parsed: serde_json::Value = serde_json::from_str(json_output)
            .map_err(|e| KanbanError::InvalidInput(format!("Invalid JSON: {e}")))?;
        let mut phase_map = std::collections::HashMap::new();
        if let Some(phases) = parsed["recomposition"]["phases"].as_array() {
            for (i, phase_val) in phases.iter().enumerate() {
                let name = phase_val["name"].as_str().unwrap_or("Unnamed");
                let desc = phase_val["description"].as_str();
                let mut phase = super::KanbanPhase::new(name.to_string(), i as u32);
                if let Some(d) = desc {
                    phase = phase.with_description(d.to_string());
                }
                if let Some(labels) = phase_val["task_labels"].as_array() {
                    for label in labels {
                        if let Some(l) = label.as_str() {
                            phase_map.insert(l.to_lowercase(), phase.id);
                        }
                    }
                }
                self.board_add_phase(board_id, &phase.name, phase.order)?;
            }
        }
        Ok(phase_map)
    }

    fn build_task_spec_from_json(
        task_val: &serde_json::Value,
        phase_map: &std::collections::HashMap<String, hkask_types::PhaseId>,
    ) -> TaskSpec {
        let title = task_val["title"].as_str().unwrap_or("Untitled");
        let description = task_val["description"].as_str().map(|s| s.to_string());
        let story_points = task_val["story_points"].as_u64().map(|n| n as u32);
        let estimated_hours = task_val["estimated_hours"].as_f64();
        let labels: Vec<String> = task_val["labels"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let criteria: Vec<super::VerificationCriterion> = task_val["criteria"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| {
                        v.as_str()
                            .map(|s| super::VerificationCriterion::new(s.into()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let priority = task_val["priority"]
            .as_str()
            .and_then(super::Priority::parse_str);

        let mut spec = TaskSpec::new(title.into());
        if let Some(d) = description {
            spec = spec.with_description(d);
        }
        if !criteria.is_empty() {
            spec = spec.with_criteria(criteria);
        }
        if let Some(sp) = story_points {
            spec = spec.with_story_points(sp);
        }
        if let Some(eh) = estimated_hours {
            spec = spec.with_estimated_hours(eh);
        }
        if let Some(p) = priority {
            spec = spec.with_priority(p);
        }
        if !phase_map.is_empty() && !labels.is_empty() {
            for label in &labels {
                if let Some(pid) = phase_map.get(&label.to_lowercase()) {
                    spec = spec.with_phase(*pid);
                    break;
                }
            }
        }
        if !labels.is_empty() {
            spec = spec.with_labels(labels);
        }
        spec
    }
}
