use super::*;

impl KataEngine {
    pub(super) async fn run_coaching(
        &self,
        manifest: &KataManifest,
        state: &mut KataState,
    ) -> Result<KataResult, KataError> {
        self.run_coaching_from(manifest, state).await
    }

    /// Run a full coaching kata cycle from the given manifest.
    ///
    /// \[P9\] Motivating: Homeostatic Self-Regulation — regulated coaching kata execution
    /// pre:  manifest has at least one question
    /// pre:  state is initialized with learner_bot and gas budget
    /// post: returns KataResult with all questions answered and step experiences recorded
    /// post: if manifest has no questions → Err(KataError::NoSteps)
    /// post: if gas exceeded → Err(KataError::GasExceeded)
    pub async fn run_coaching_from(
        &self,
        manifest: &KataManifest,
        state: &mut KataState,
    ) -> Result<KataResult, KataError> {
        let total = manifest.questions.len();
        if total == 0 {
            return Err(KataError::NoSteps(manifest.manifest.id.clone()));
        }

        let ik_context = state.ik_state_ref.clone();

        for q in &manifest.questions {
            if (q.number as usize) <= state.current_step && !state.step_outputs.is_empty() {
                continue;
            }

            if manifest.ledger.emit_spans {
                tracing::info!(
                    target: "reg.kata",
                    namespace = %manifest.ledger.span_namespace,
                    question = q.number,
                    bot = %state.learner_bot,
                    has_ik_state = ik_context.is_some(),
                    "REG"
                );
            }
            let step_gas = 2000;
            if state.gas_consumed + step_gas > manifest.gas.cap {
                return Err(KataError::GasExceeded {
                    consumed: state.gas_consumed,
                    cap: manifest.gas.cap,
                });
            }

            let prev_context = state
                .step_outputs
                .iter()
                .map(|(k, v)| {
                    let text = v.get("response").and_then(|r| r.as_str()).unwrap_or("");
                    format!("Q{}: {}", k.trim_start_matches('q'), text)
                })
                .collect::<Vec<_>>()
                .join("\n");

            let ik_data_section = match &ik_context {
                Some(ik_ref) => format!(
                    "\nThe learner's current Improvement Kata storyboard:\n{}\n\n",
                    ik_ref
                ),
                None => String::new(),
            };

            let prompt = format!(
                "You are a Toyota Kata coach conducting a 5-question coaching cycle.\n\
                 Your role: ask questions that reveal the learner's thinking pattern.\n\
                 Never give solutions. Never say 'you should'. Only ask.\n\
                 {ik_data}\n\
                 Previous answers from the learner:\n\
                 {prev}\n\n\
                 Now ask Question {n}: {q}\n\
                 Context: {desc}\n\n\
                 Ask the question in a way that makes the learner think.\n\
                 Then, as the learner, respond with specific data and observations\n\
                 from your Improvement Kata storyboard.",
                ik_data = ik_data_section,
                prev = if prev_context.is_empty() {
                    "(first question — no prior answers)"
                } else {
                    &prev_context
                },
                n = q.number,
                q = q.question,
                desc = q.description,
            );

            let response = self
                .inference
                .generate(&prompt, &default_llm_params(), None)
                .await
                .map_err(|e| {
                    KataError::InferenceFailed(format!("Coaching Q{}: {}", q.number, e))
                })?;

            // Deduct actual token cost from the bound kanban task's gas budget.
            // No-op when no task gas accountant is configured (standalone kata execution).
            self.deduct_task_gas(&response, &format!("coaching-q{}", q.number));

            state.step_outputs.insert(
                format!("q{}", q.number),
                serde_json::json!({"response": response.text, "question": q.question}),
            );
            state.gas_consumed += step_gas;
            state.current_step = q.number as usize;

            state.step_experiences.push(StepExperience {
                userpod: state.learner_bot.clone(),
                kata_type: "coaching".into(),
                step_label: format!("q{}", q.number),
                action: "coaching_question".into(),
                output_summary: response.text.chars().take(200).collect(),
                gas_used: step_gas,
                timestamp: now_rfc3339(),
            });

            if let Some(ref obs) = self.ledger_observer {
                obs(
                    &manifest.ledger.span_namespace,
                    q.number,
                    "coaching_question",
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
            kata_type: "coaching".into(),
            steps_completed: total,
            total_steps: total,
            gas_consumed: state.gas_consumed,
            gas_cap: manifest.gas.cap,
            state: state.clone(),
            outcome: None,
            improvement_signal: None,
            step_experiences: vec![],
            automaticity_delta: None,
        })
    }
}
