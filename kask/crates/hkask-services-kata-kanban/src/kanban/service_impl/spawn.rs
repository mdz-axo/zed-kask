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

        if let Some(ref pm) = self.pod_manager {
            let pod_name = Self::build_pod_name(&task.title);
            let capabilities: Vec<String> = spawn_spec
                .delegated_skills
                .iter()
                .map(|s| s.to_string())
                .collect();
            match Self::activate_pod(pm, "kanban-agent", &pod_name, &capabilities, &spawn_spec) {
                Ok(pod_note) => {
                    let comment = super::Comment::new(task_id, task.owner, pod_note);
                    task.comments.push(comment);
                    task.updated_at = chrono::Utc::now();
                    self.update_task_triple(&task)?;
                    return Ok(format!(
                        "Pod {} activated (webid: {}). Use /kanban note {} to communicate.",
                        pod_name,
                        hkask_types::WebID::from_persona(pod_name.as_bytes()).redacted_display(),
                        task_id
                    ));
                }
                Err(e) => return Ok(format!("Pod activation failed: {}", e)),
            }
        }

        let spawn_note = format!(
            "Spawn configured (no ActivePods): level={}, skills={:?}, memory={}, tools={:?}",
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
            "Spawn configured for '{}' (no ActivePods — string mode). Skills: {:?}",
            task.title, spawn_spec.delegated_skills
        ))
    }

    fn build_pod_name(title: &str) -> String {
        format!(
            "kanban-{}",
            title.chars().take(20).collect::<String>().replace(' ', "-")
        )
    }

    fn activate_pod(
        pm: &hkask_pods::pod::ActivePods,
        agent_type: &str,
        pod_name: &str,
        capabilities: &[String],
        spec: &super::SpawnSpec,
    ) -> Result<String, KanbanError> {
        let rt = tokio::runtime::Handle::current();
        let webid = hkask_types::WebID::from_persona(pod_name.as_bytes());
        let pod_id = rt
            .block_on(pm.create_pod(
                agent_type,
                pod_name,
                webid,
                capabilities.to_vec(),
                hkask_pods::pod::PodKind::UserPod,
            ))
            .map_err(|e| KanbanError::Internal(format!("Pod creation failed: {}", e)))?;
        rt.block_on(pm.activate_pod(&pod_id))
            .map_err(|e| KanbanError::Internal(format!("Pod activation failed: {}", e)))?;
        Ok(format!(
            "Pod activated: id={}, webid={}, skills={:?}, tools={:?}",
            pod_id,
            webid.redacted_display(),
            spec.delegated_skills,
            spec.tool_servers
        ))
    }
}
