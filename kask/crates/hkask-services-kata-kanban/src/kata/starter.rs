use super::*;

impl KataEngine {
    /// Run the starter kata practice routine.
    ///
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated starter kata execution
    /// pre:  manifest has at least one practice routine
    /// pre:  state is initialized with learner_bot
    /// post: returns KataResult with all practices executed and status recorded
    /// post: if manifest has no practices → Err(KataError::NoSteps)
    /// post: records automaticity and streak data from history if available
    pub async fn run_starter(
        &self,
        manifest: &KataManifest,
        state: &mut KataState,
    ) -> Result<KataResult, KataError> {
        let total = manifest.practices.len();
        if total == 0 {
            return Err(KataError::NoSteps(manifest.manifest.id.clone()));
        }

        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

        if let Some(ref history) = self.history {
            let auto = history.compute_automaticity(&state.learner_bot, &today);
            let streak = history.current_streak(&state.learner_bot, &today);
            let needs_intervention = history.needs_habit_intervention(&state.learner_bot, &today);

            if manifest.ledger.emit_spans {
                tracing::info!(
                    target: "reg.kata",
                    namespace = %manifest.ledger.span_namespace,
                    bot = %state.learner_bot,
                    automaticity = auto,
                    streak_days = streak,
                    needs_intervention = needs_intervention,
                    "REG"
                );
            }

            if needs_intervention {
                tracing::warn!(
                    target: "reg.kata",
                    namespace = %manifest.ledger.span_namespace,
                    bot = %state.learner_bot,
                    days_since_last = history.days_since_last(&state.learner_bot, &today),
                    "REG"
                );
            }
        }

        for practice in &manifest.practices {
            let output = serde_json::json!({
                "practice": practice.name,
                "description": practice.description,
                "frequency": practice.frequency,
                "duration_minutes": practice.duration_minutes,
                "steps": practice.steps,
                "success_criteria": practice.success_criteria,
                "reg_spans": practice.reg_spans,
                "status": "executed",
                "date": today,
            });
            state
                .step_outputs
                .insert(practice.name.clone(), output.clone());
            state.current_step += 1;

            state.step_experiences.push(StepExperience {
                userpod: state.learner_bot.clone(),
                kata_type: "starter".into(),
                step_label: practice.name.clone(),
                action: "practice_routine".into(),
                output_summary: practice.description.clone(),
                gas_used: 0,
                timestamp: now_rfc3339(),
            });

            if manifest.ledger.emit_spans {
                tracing::info!(
                    target: "reg.kata",
                    namespace = %manifest.ledger.span_namespace,
                    practice = %practice.name,
                    bot = %state.learner_bot,
                    "REG"
                );
            }

            self.increment_ledger_variety(
                &manifest.ledger.span_namespace,
                "kata.practices.completed",
            )
            .await;
        }

        Ok(KataResult {
            manifest_id: manifest.manifest.id.clone(),
            kata_type: "starter".into(),
            steps_completed: total,
            total_steps: total,
            gas_consumed: 0,
            gas_cap: manifest.gas.cap,
            state: state.clone(),
            outcome: None,
            improvement_signal: None,
            step_experiences: vec![],
            automaticity_delta: None,
        })
    }
}
