use super::*;

impl KataEngine {
    pub(super) async fn run_improvement(
        &self,
        manifest: &KataManifest,
        state: &mut KataState,
    ) -> Result<KataResult, KataError> {
        self.run_improvement_from(manifest, state).await
    }

    /// Run an improvement kata cycle from the given manifest.
    ///
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated improvement kata execution
    /// pre:  manifest has at least one step
    /// pre:  state is initialized with learner_bot and gas budget
    /// post: returns KataResult with all steps executed, outputs validated against schema
    /// post: if manifest has no steps → Err(KataError::NoSteps)
    /// post: if gas exceeded → Err(KataError::GasExceeded)
    pub async fn run_improvement_from(
        &self,
        manifest: &KataManifest,
        state: &mut KataState,
    ) -> Result<KataResult, KataError> {
        let total_steps = manifest.steps.len();
        if total_steps == 0 {
            return Err(KataError::NoSteps(manifest.manifest.id.clone()));
        }

        for step in &manifest.steps {
            if (step.ordinal as usize) <= state.current_step && !state.step_outputs.is_empty() {
                continue;
            }

            if manifest.ledger.emit_spans {
                tracing::info!(
                    target: "reg.kata",
                    namespace = %manifest.ledger.span_namespace,
                    step = step.ordinal,
                    action = %step.action,
                    bot = %state.learner_bot,
                    "REG"
                );
            }

            let step_gas = step.gas_cap.unwrap_or(2000);
            if state.gas_consumed + step_gas > manifest.gas.cap {
                return Err(KataError::GasExceeded {
                    consumed: state.gas_consumed,
                    cap: manifest.gas.cap,
                });
            }

            let output = self.execute_step(manifest, step, state).await?;

            let check_result = self.check_step_output(step, &output);
            if manifest.ledger.emit_spans {
                tracing::info!(
                    target: "reg.kata",
                    namespace = %manifest.ledger.span_namespace,
                    step = step.ordinal,
                    passed_check = check_result,
                    "REG"
                );
            }

            state
                .step_outputs
                .insert(step.ordinal.to_string(), output.clone());
            state.gas_consumed += step_gas;
            state.current_step = step.ordinal as usize;

            let summary = output
                .get("response")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .chars()
                .take(200)
                .collect::<String>();
            state.step_experiences.push(StepExperience {
                userpod: state.learner_bot.clone(),
                kata_type: "improvement".into(),
                step_label: format!("{}", step.ordinal),
                action: step.action.clone(),
                output_summary: summary,
                gas_used: step_gas,
                timestamp: now_rfc3339(),
            });

            if manifest.ledger.emit_spans {
                tracing::info!(
                    target: "reg.kata",
                    namespace = %manifest.ledger.span_namespace,
                    step = step.ordinal,
                    gas = state.gas_consumed,
                    "REG"
                );
            }

            if let Some(ref obs) = self.ledger_observer {
                obs(&manifest.ledger.span_namespace, step.ordinal, &step.action);
            }

            self.increment_ledger_variety(
                &manifest.ledger.span_namespace,
                "kata.practices.completed",
            )
            .await;
        }

        Ok(KataResult {
            manifest_id: manifest.manifest.id.clone(),
            kata_type: "improvement".into(),
            steps_completed: total_steps,
            total_steps,
            gas_consumed: state.gas_consumed,
            gas_cap: manifest.gas.cap,
            state: state.clone(),
            outcome: None,
            improvement_signal: None,
            step_experiences: vec![],
            automaticity_delta: None,
        })
    }

    fn check_step_output(&self, step: &KataStep, output: &serde_json::Value) -> bool {
        let schema = match &step.output_schema {
            Some(s) => s,
            None => return true,
        };

        if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
            for key in props.keys() {
                if output.get(key).is_none() {
                    if let Some(resp) = output.get("response") {
                        if resp.get(key).is_none() {
                            tracing::debug!(
                                target: "reg.kata",
                                step = step.ordinal,
                                missing = %key,
                                "Step output missing expected field"
                            );
                            return false;
                        }
                    } else {
                        tracing::debug!(
                            target: "reg.kata",
                            step = step.ordinal,
                            missing = %key,
                            "Step output missing expected field"
                        );
                        return false;
                    }
                }
            }
        }
        true
    }
}
