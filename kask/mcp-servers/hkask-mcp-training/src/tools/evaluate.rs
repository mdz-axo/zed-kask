use crate::TrainingServer;
use crate::types::TrainEvaluateRequest;
use hkask_inference::InferenceRouter;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_types::InferencePort;
use hkask_types::template::LLMParameters;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool;
use serde_json::json;
use std::path::PathBuf;

impl TrainingServer {
    #[tool(
        description = "Evaluate a trained adapter against a test dataset. Supports exact_match, contains, semantic (LLM-as-judge), and benchmark (MMLU-style multiple-choice) evaluation methods. The model must be deployed and available for inference."
    )]
    pub async fn training_evaluate(
        &self,
        Parameters(TrainEvaluateRequest {
            adapter_id,
            test_dataset_path,
            model,
            method,
            max_examples,
        }): Parameters<TrainEvaluateRequest>,
    ) -> String {
        execute_tool(self, "training_evaluate", async {
            let test_path = PathBuf::from(&test_dataset_path);
            if !test_path.exists() {
                return Err(McpToolError::invalid_argument(format!(
                    "Test dataset file not found: {}",
                    test_dataset_path
                )));
            }

            let raw = match std::fs::read_to_string(&test_path) {
                Ok(r) => r,
                Err(e) => {
                    return Err(McpToolError::invalid_argument(format!(
                        "Failed to read test dataset: {e}"
                    )));
                }
            };

            let eval_method = method.as_deref().unwrap_or("exact_match");

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
            for (i, line) in raw.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let record: serde_json::Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(target: "hkask.training.evaluate", line = i + 1, error = %e, "Skipping unparseable test line");
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
                if user_parts.is_empty() {
                    continue;
                }
                let input = user_parts.join("\n");
                let expected = messages
                    .iter()
                    .rev()
                    .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
                    .and_then(|m| m.get("content").and_then(|c| c.as_str()))
                    .unwrap_or("");
                if expected.is_empty() {
                    continue;
                }
                examples.push((input, expected.to_string()));
            }

            if examples.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "No valid test examples found. For exact_match/contains/semantic, each line must have a 'messages' array. For benchmark, each line must have 'question', 'choices', and 'answer'.",
                ));
            }

            let limit = max_examples.unwrap_or(examples.len()).min(examples.len());
            examples.truncate(limit);

            let router = InferenceRouter::new(self.inference_config.clone());
            let mut correct = 0;
            let mut errors = 0;
            let mut total_tokens = 0u64;
            let mut per_example: Vec<serde_json::Value> = Vec::new();

            for (i, (input, expected)) in examples.iter().enumerate() {
                let prompt = format!("{input}\n\nRespond concisely and accurately.");
                let params = LLMParameters {
                    temperature: 1.0,
                    max_tokens: 512,
                    ..Default::default()
                };
                match router
                    .generate_with_model(&prompt, &params, Some(&model), None)
                    .await
                {
                    Ok(response) => {
                        total_tokens += response.usage.total_tokens as u64;
                        let generated = response.text.trim();
                        let expected_trimmed = expected.trim();
                        let is_correct = match eval_method {
                            "contains" => generated.contains(expected_trimmed),
                            "semantic" => {
                                let judge_prompt = format!(
                                    "Judge whether the following response correctly answers the question.\n\n\
                                     QUESTION:\n{input}\n\n\
                                     EXPECTED ANSWER:\n{expected_trimmed}\n\n\
                                     GENERATED ANSWER:\n{generated}\n\n\
                                     Reply with ONLY 'CORRECT' or 'INCORRECT'."
                                );
                                match router
                                    .generate_with_model(&judge_prompt, &params, Some(&model), None)
                                    .await
                                {
                                    Ok(judge) => judge.text.trim().to_uppercase().contains("CORRECT"),
                                    Err(_) => false,
                                }
                            }
                            _ => generated == expected_trimmed,
                        };
                        if is_correct {
                            correct += 1;
                        }
                        per_example.push(json!({
                            "index": i, "input": input, "expected": expected_trimmed,
                            "generated": generated, "correct": is_correct,
                            "tokens": response.usage.total_tokens,
                        }));
                    }
                    Err(e) => {
                        errors += 1;
                        tracing::warn!(target: "hkask.training.evaluate", example = i, error = %e, "Inference failed");
                        per_example.push(json!({"index": i, "input": input, "expected": expected.trim(), "error": e.to_string()}));
                    }
                }
            }

            let total = correct + errors;
            let accuracy = if total > 0 { correct as f64 / total as f64 } else { 0.0 };
            Ok(json!({
                "adapter_id": adapter_id, "model": model, "method": eval_method,
                "total_examples": total, "correct": correct, "errors": errors,
                "accuracy": accuracy, "total_tokens_used": total_tokens,
                "per_example": per_example,
            }))
        })
        .await
    }

    /// MMLU-style benchmark evaluation. Each line in the dataset has:
    /// `question` (string), `choices` (array of strings), `answer` (letter: A/B/C/D),
    /// `category` (optional string for per-category scoring).
    async fn eval_benchmark(
        &self,
        raw: &str,
        adapter_id: &str,
        model: &str,
        max_examples: Option<usize>,
    ) -> Result<serde_json::Value, McpToolError> {
        let mut questions: Vec<(String, Vec<String>, String, String)> = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let record: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(target: "hkask.training.evaluate.benchmark", line = i + 1, error = %e, "Skipping unparseable line");
                    continue;
                }
            };
            let question = record
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let choices: Vec<String> = record
                .get("choices")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| c.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let answer = record.get("answer").and_then(|v| v.as_str()).unwrap_or("");
            let category = record
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            if question.is_empty() || choices.is_empty() || answer.is_empty() {
                continue;
            }
            questions.push((
                question.to_string(),
                choices,
                answer.to_uppercase(),
                category,
            ));
        }

        if questions.is_empty() {
            return Err(McpToolError::invalid_argument(
                "No valid benchmark questions found. Each line must have 'question', 'choices' (array), and 'answer' (letter A/B/C/D).",
            ));
        }

        let limit = max_examples.unwrap_or(questions.len()).min(questions.len());
        questions.truncate(limit);

        let router = InferenceRouter::new(self.inference_config.clone());
        let mut correct = 0;
        let mut errors = 0;
        let mut total_tokens = 0u64;
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
            prompt.push_str("\nAnswer with just the letter (A, B, C, or D).");

            let params = LLMParameters {
                temperature: 1.0,
                max_tokens: 16,
                ..Default::default()
            };

            match router
                .generate_with_model(&prompt, &params, Some(model), None)
                .await
            {
                Ok(response) => {
                    total_tokens += response.usage.total_tokens as u64;
                    let generated = response.text.trim();
                    // Extract answer letter: first A-D character in the response
                    let predicted = generated
                        .chars()
                        .find(|c| c.is_ascii_uppercase())
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    let is_correct = predicted == *expected_answer;
                    if is_correct {
                        correct += 1;
                    }
                    let entry = per_category.entry(category.clone()).or_insert((0, 0));
                    entry.0 += if is_correct { 1 } else { 0 };
                    entry.1 += 1;
                    per_example.push(json!({
                        "index": i, "category": category,
                        "question": question, "expected": expected_answer,
                        "predicted": predicted, "generated": generated,
                        "correct": is_correct,
                    }));
                }
                Err(e) => {
                    errors += 1;
                    let entry = per_category.entry(category.clone()).or_insert((0, 0));
                    entry.1 += 1;
                    per_example.push(json!({
                        "index": i, "category": category, "error": e.to_string(),
                    }));
                }
            }
        }

        let total = correct + errors;
        let accuracy = if total > 0 {
            correct as f64 / total as f64
        } else {
            0.0
        };
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

        Ok(json!({
            "adapter_id": adapter_id, "model": model, "method": "benchmark",
            "total_examples": total, "correct": correct, "errors": errors,
            "accuracy": accuracy, "total_tokens_used": total_tokens,
            "per_category": category_results, "per_example": per_example,
        }))
    }
}
