use super::*;

impl KataEngine {
    pub(super) async fn execute_step(
        &self,
        _manifest: &KataManifest,
        step: &KataStep,
        state: &KataState,
    ) -> Result<serde_json::Value, KataError> {
        let template_ref = step.template_ref.as_deref().unwrap_or("");

        let prompt = if !template_ref.is_empty() {
            self.render_template(template_ref, state)?
        } else {
            step.description.clone()
        };

        let result = if step.classifier {
            // Use the configured classifier model with its provider prefix.
            // The model string from HkaskSettings includes a routing prefix
            // (e.g., KC/qwen/...), so the inference router dispatches to
            // the correct provider automatically — no hardcoded DI/ prefix.
            let cls_model = HkaskSettings::load().classifier_model();
            self.inference
                .generate_with_model(&prompt, &default_llm_params(), Some(&cls_model), None)
                .await
                .map_err(|e| KataError::InferenceFailed(format!("Step {}: {}", step.ordinal, e)))?
        } else {
            self.inference
                .generate(&prompt, &default_llm_params(), None)
                .await
                .map_err(|e| KataError::InferenceFailed(format!("Step {}: {}", step.ordinal, e)))?
        };

        // Deduct actual token cost from the bound kanban task's gas budget.
        // No-op when no task gas accountant is configured (standalone kata execution).
        self.deduct_task_gas(&result, &format!("step-{}", step.ordinal));

        let response = result.text;

        if let Some(ref _schema) = step.output_schema {
            match serde_json::from_str::<serde_json::Value>(&response) {
                Ok(json) => return Ok(json),
                Err(_) => {
                    return Ok(serde_json::json!({"response": response}));
                }
            }
        }

        Ok(serde_json::json!({"response": response}))
    }

    fn render_template(&self, template_ref: &str, state: &KataState) -> Result<String, KataError> {
        let context_json = serde_json::to_value(&state.context).unwrap_or(serde_json::Value::Null);
        let steps_json =
            serde_json::to_value(&state.step_outputs).unwrap_or(serde_json::Value::Null);
        let metric_before_json = state
            .metric_before
            .clone()
            .unwrap_or(serde_json::Value::Null);
        let metric_after_json = state
            .metric_after
            .clone()
            .unwrap_or(serde_json::Value::Null);
        let ik_ref_json = serde_json::Value::String(state.ik_state_ref.clone().unwrap_or_default());

        let ctx = minijinja::context! {
            learner_bot => state.learner_bot.clone(),
            previous_steps => steps_json,
            context => context_json,
            metric_before => metric_before_json,
            metric_after => metric_after_json,
            ik_state_ref => ik_ref_json,
        };

        let template_content = match self.registry.get_entry(template_ref) {
            Ok(entry) => std::fs::read_to_string(&entry.source_path).map_err(|e| {
                KataError::TemplateNotFound(format!(
                    "Failed to read template '{}' at {}: {}",
                    template_ref, entry.source_path, e
                ))
            })?,
            Err(_) => {
                let disk_path = std::path::PathBuf::from("registry/templates").join(template_ref);
                let with_ext = disk_path.with_extension("j2");
                let read_path = if with_ext.exists() {
                    &with_ext
                } else {
                    &disk_path
                };
                std::fs::read_to_string(read_path).map_err(|_| {
                    KataError::TemplateNotFound(format!(
                        "Template '{}' not found in registry or at {} or {}",
                        template_ref,
                        disk_path.display(),
                        with_ext.display()
                    ))
                })?
            }
        };

        let env = minijinja::Environment::new();
        let rendered = env
            .render_str(&template_content, ctx)
            .map_err(|e| KataError::TemplateNotFound(format!("Render failed: {}", e)))?;

        Ok(rendered)
    }
}
