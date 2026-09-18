use crate::TrainingServer;
use crate::types::TrainEvaluateRequest;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_types::ports::{InferenceError, InferenceResult};
use hkask_types::template::LLMParameters;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use serde_json::json;

#[derive(Default)]
struct EvaluationSummary {
    correct: usize,
    generation_errors: usize,
    evaluator_errors: usize,
    inference_calls: usize,
    reported_tokens: u64,
    reported_cost: f64,
    unreported_usage_calls: usize,
    unreported_cost_calls: usize,
}

impl EvaluationSummary {
    fn record_call(&mut self, result: &Result<InferenceResult, InferenceError>) {
        self.inference_calls += 1;
        if let Ok(response) = result {
            if response.usage.reported {
                self.reported_tokens += u64::from(response.usage.total_tokens);
            } else {
                self.unreported_usage_calls += 1;
            }
            if let Some(cost) = response
                .cost_usd
                .filter(|cost| cost.is_finite() && *cost >= 0.0)
            {
                self.reported_cost += cost;
            } else {
                self.unreported_cost_calls += 1;
            }
        } else {
            // Failed calls may have consumed resources before their response was lost.
            self.unreported_usage_calls += 1;
            self.unreported_cost_calls += 1;
        }
    }

    fn report(&self, total: usize, valid: usize, skipped: usize) -> serde_json::Value {
        let errors = self.generation_errors + self.evaluator_errors;
        json!({
            "total_examples": total, "correct": self.correct,
            "valid_examples": valid, "skipped_invalid_examples": skipped,
            "excluded_by_limit": valid - total,
            "incorrect": total - self.correct - errors, "errors": errors,
            "generation_errors": self.generation_errors, "evaluator_errors": self.evaluator_errors,
            "accuracy": if total == 0 { 0.0 } else { self.correct as f64 / total as f64 },
            "inference_calls": self.inference_calls,
            "total_tokens_used": (self.unreported_usage_calls == 0).then_some(self.reported_tokens),
            "reported_tokens_used": self.reported_tokens,
            "unreported_usage_calls": self.unreported_usage_calls,
            "total_cost_usd": (self.unreported_cost_calls == 0).then_some(self.reported_cost),
            "reported_cost_usd": self.reported_cost,
            "unreported_cost_calls": self.unreported_cost_calls,
        })
    }
}

fn choice_letter(response: &str, choice_count: usize) -> Option<String> {
    let answer = response.trim().to_ascii_uppercase();
    let mut letters = answer.bytes();
    let letter = letters.next()?;
    (letters.next().is_none()
        && (b'A'..=b'F').contains(&letter)
        && usize::from(letter - b'A') < choice_count)
        .then_some(answer)
}

#[tool_router(router = evaluate_router, vis = "pub")]
impl TrainingServer {
    #[tool(
        description = "Evaluate a deployed model against a test dataset. Supports exact_match, contains, semantic (requires explicit judge_model; remains LLM-judged), and benchmark (one available choice letter). All attempted examples count in accuracy; failed calls and missing resource reports are surfaced."
    )]
    pub async fn training_evaluate(
        &self,
        Parameters(TrainEvaluateRequest {
            adapter_id,
            test_dataset_path,
            model,
            method,
            judge_model,
            max_examples,
        }): Parameters<TrainEvaluateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "training_evaluate", async {
            let eval_method = method.as_deref().unwrap_or("exact_match");
            if !matches!(eval_method, "exact_match" | "contains" | "semantic" | "benchmark") {
                return Err(McpToolError::invalid_argument("Unsupported evaluation method"));
            }
            if max_examples == Some(0) {
                return Err(McpToolError::invalid_argument("max_examples must be positive"));
            }
            if eval_method == "semantic" {
                if judge_model.as_deref().is_none_or(|judge| judge.trim().is_empty()) {
                    return Err(McpToolError::invalid_argument("semantic evaluation requires judge_model"));
                }
            } else if judge_model.is_some() {
                return Err(McpToolError::invalid_argument("judge_model is only supported for semantic evaluation"));
            }
            // Contain the LLM-supplied test dataset path before reading
            // (CWE-200): an absolute path like /etc/passwd or ~/.ssh/id_rsa must
            // not flow back into the evaluation context.
            let test_path = hkask_mcp_server::contain_for_read(&test_dataset_path)?;
            let raw = std::fs::read_to_string(&test_path).map_err(|e| {
                hkask_mcp_server::map_io_error(
                    e,
                    &format!("Failed to read test dataset '{}'", test_dataset_path),
                )
            })?;

            // Benchmark method: MMLU-style multiple-choice evaluation.
            // Dataset format: JSONL with {question, choices: [..], answer: "A"/"B"/.., category: ".."}
            if eval_method == "benchmark" {
                return self.eval_benchmark(
                    &raw,
                    &adapter_id,
                    &model,
                    max_examples,
                )
                .await;
            }

            // Standard methods: parse ChatML messages format.
            let mut examples: Vec<(String, String)> = Vec::new();
            let mut skipped = 0;
            for (i, line) in raw.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                skipped += 1;
                let record: serde_json::Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(target: "hkask.training.evaluate", line = i + 1, error = %e, "Skipping unparsable test line");
                        continue;
                    }
                };
                let messages = match record.get("messages").and_then(|m| m.as_array()) {
                    Some(ms) => ms,
                    None => continue,
                };
                let user_parts: Vec<&str> = messages
                    .iter()
                    .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .collect();
                if user_parts.iter().all(|part| part.trim().is_empty()) {
                    continue;
                }
                let input = user_parts.join("\n");
                let expected = messages
                    .iter()
                    .rev()
                    .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
                    .and_then(|m| m.get("content").and_then(|c| c.as_str()))
                    .unwrap_or("");
                if expected.trim().is_empty() {
                    continue;
                }
                examples.push((input, expected.to_string()));
                skipped -= 1;
            }

            if examples.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "No valid test examples found. For exact_match/contains/semantic, each line must have a 'messages' array. For benchmark, each line must have 'question', 'choices', and 'answer'.",
                ));
            }

            let valid = examples.len();
            let limit = max_examples.unwrap_or(valid).min(valid);
            examples.truncate(limit);

            let router = &self.inference_port;
            let mut summary = EvaluationSummary::default();
            let mut per_example: Vec<serde_json::Value> = Vec::new();

            for (i, (input, expected)) in examples.iter().enumerate() {
                let prompt = format!("{input}\n\nRespond concisely and accurately.");
                let params = LLMParameters {
                    temperature: 1.0,
                    ..Default::default()
                };
                let response = router
                    .generate_with_model(&prompt, &params, Some(&model), None)
                    .await;
                summary.record_call(&response);
                match response {
                    Ok(response) => {
                        let generated = response.text.trim();
                        let expected_trimmed = expected.trim();
                        let verdict = match eval_method {
                            "contains" => Ok(generated.contains(expected_trimmed)),
                            "semantic" => {
                                let judge_prompt = format!(
                                    "Judge whether the following response correctly answers the question.\n\n\
                                     QUESTION:\n{input}\n\n\
                                     EXPECTED ANSWER:\n{expected_trimmed}\n\n\
                                     GENERATED ANSWER:\n{generated}\n\n\
                                     Reply with ONLY 'CORRECT' or 'INCORRECT'."
                                );
                                let judge = router
                                    .generate_with_model(&judge_prompt, &params, judge_model.as_deref(), None)
                                    .await;
                                summary.record_call(&judge);
                                match judge {
                                    Ok(judge) => match judge.text.trim() {
                                        "CORRECT" => Ok(true),
                                        "INCORRECT" => Ok(false),
                                        _ => Err(InferenceError::Generation(
                                            "Semantic judge returned neither CORRECT nor INCORRECT".into(),
                                        )),
                                    },
                                    Err(error) => Err(error),
                                }
                            }
                            _ => Ok(generated == expected_trimmed),
                        };
                        let is_correct = match verdict {
                            Ok(value) => value,
                            Err(error) => {
                                summary.evaluator_errors += 1;
                                per_example.push(json!({
                                    "index": i, "input": input, "expected": expected_trimmed,
                                    "generated": generated, "correct": null,
                                    "status": "evaluator_error", "error": error.to_string(),
                                }));
                                continue;
                            }
                        };
                        if is_correct {
                            summary.correct += 1;
                        }
                        per_example.push(json!({
                            "index": i, "input": input, "expected": expected_trimmed,
                            "generated": generated, "correct": is_correct,
                            "status": if is_correct { "correct" } else { "incorrect" },
                            "tokens": response.usage.reported.then_some(response.usage.total_tokens),
                        }));
                    }
                    Err(e) => {
                        summary.generation_errors += 1;
                        tracing::warn!(target: "hkask.training.evaluate", example = i, error = %e, "Inference failed");
                        per_example.push(json!({"index": i, "input": input, "expected": expected.trim(),
                            "correct": null, "status": "generation_error", "error": e.to_string()}));
                    }
                }
            }

            let mut report = summary.report(examples.len(), valid, skipped);
            report["adapter_id"] = json!(adapter_id);
            report["model"] = json!(model);
            report["method"] = json!(eval_method);
            report["judge_model"] = json!(judge_model);
            report["evidence_source"] = json!(if eval_method == "semantic" { "llm_judged" } else { "deterministic_evaluator" });
            report["per_example"] = json!(per_example);
            Ok(report)
        })
        .await
    }

    /// MMLU-style benchmark evaluation. Each line in the dataset has:
    /// `question` (string), `choices` (2–6 non-empty strings), `answer` (available letter A–F),
    /// `category` (optional string for per-category scoring).
    async fn eval_benchmark(
        &self,
        raw: &str,
        adapter_id: &str,
        model: &str,
        max_examples: Option<usize>,
    ) -> Result<serde_json::Value, McpToolError> {
        let mut questions: Vec<(String, Vec<String>, String, String)> = Vec::new();
        let mut skipped = 0;
        for (i, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            skipped += 1;
            let record: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(target: "hkask.training.evaluate.benchmark", line = i + 1, error = %e, "Skipping unparsable line");
                    continue;
                }
            };
            let question = record
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let choices = record
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .map(|c| {
                            c.as_str()
                                .filter(|text| !text.trim().is_empty())
                                .map(String::from)
                        })
                        .collect::<Option<Vec<String>>>()
                });
            let Some(choices) = choices.filter(|choices| (2..=6).contains(&choices.len())) else {
                continue;
            };
            let answer = record.get("answer").and_then(|v| v.as_str()).unwrap_or("");
            let category = record
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            if question.trim().is_empty() {
                continue;
            }
            let Some(answer) = choice_letter(answer, choices.len()) else {
                continue;
            };
            questions.push((question.to_string(), choices, answer, category));
            skipped -= 1;
        }

        if questions.is_empty() {
            return Err(McpToolError::invalid_argument(
                "No valid benchmark questions found. Each line must have a non-empty 'question', 2–6 non-empty 'choices', and an available 'answer' letter A–F.",
            ));
        }

        let valid = questions.len();
        let limit = max_examples.unwrap_or(valid).min(valid);
        questions.truncate(limit);

        let router = &self.inference_port;
        let mut summary = EvaluationSummary::default();
        let mut per_category: std::collections::HashMap<String, (usize, usize)> =
            std::collections::HashMap::new();
        let mut per_example: Vec<serde_json::Value> = Vec::new();

        for (i, (question, choices, expected_answer, category)) in questions.iter().enumerate() {
            // Format multiple-choice prompt
            let letters = ["A", "B", "C", "D", "E", "F"];
            let mut prompt = format!("Question: {question}\n\n");
            for (j, choice) in choices.iter().enumerate() {
                if j < letters.len() {
                    prompt.push_str(&format!("{}) {choice}\n", letters[j]));
                }
            }
            prompt.push_str("\nAnswer with just the letter of one of the listed choices.");

            let params = LLMParameters {
                temperature: 1.0,
                ..Default::default()
            };

            let response = router
                .generate_with_model(&prompt, &params, Some(model), None)
                .await;
            summary.record_call(&response);
            match response {
                Ok(response) => {
                    let generated = response.text.trim();
                    let predicted = choice_letter(generated, choices.len());
                    let is_correct = predicted.as_deref() == Some(expected_answer.as_str());
                    if is_correct {
                        summary.correct += 1;
                    }
                    let entry = per_category.entry(category.clone()).or_insert((0, 0));
                    entry.0 += if is_correct { 1 } else { 0 };
                    entry.1 += 1;
                    per_example.push(json!({
                        "index": i, "category": category,
                        "question": question, "expected": expected_answer,
                        "predicted": predicted, "generated": generated,
                        "correct": is_correct,
                        "status": if is_correct { "correct" } else { "incorrect" },
                        "tokens": response.usage.reported.then_some(response.usage.total_tokens),
                    }));
                }
                Err(e) => {
                    summary.generation_errors += 1;
                    let entry = per_category.entry(category.clone()).or_insert((0, 0));
                    entry.1 += 1;
                    per_example.push(json!({
                        "index": i, "category": category, "error": e.to_string(),
                        "correct": null, "status": "generation_error",
                    }));
                }
            }
        }

        let category_results: serde_json::Value = per_category
            .iter()
            .map(|(cat, (c, t))| {
                let acc = if *t > 0 { *c as f64 / *t as f64 } else { 0.0 };
                (
                    cat.clone(),
                    json!({"correct": c, "total": t, "accuracy": acc}),
                )
            })
            .collect();

        let mut report = summary.report(questions.len(), valid, skipped);
        report["adapter_id"] = json!(adapter_id);
        report["model"] = json!(model);
        report["method"] = json!("benchmark");
        report["evidence_source"] = json!("deterministic_evaluator");
        report["per_category"] = category_results;
        report["per_example"] = json!(per_example);
        Ok(report)
    }
}
