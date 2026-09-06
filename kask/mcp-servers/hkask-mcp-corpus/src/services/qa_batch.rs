//! Prepared QA generation with AIMD-gated synchronous inference or provider batches.

use std::collections::HashMap;
use std::sync::Arc;

use hkask_mcp_server::server::McpToolError;
use hkask_types::{ChatMessage, InferencePort};

use crate::batch::{ADAPTIVE_CONCURRENCY_FLOOR, AdaptiveLimiter, MAX_RETRIES, retry_with_backoff};
use crate::helpers::map_corpus_io_error;
use crate::services::qa_pipeline::{QaCompletion, QaOutput, qa_llm_parameters, read_prompts};
use crate::tools::semantic::batch_api::generate_qa_via_batch_api;
use crate::tools::semantic::qa::configured_qa_model;

pub(crate) struct QaBatchRequest {
    pub prompts_jsonl: String,
    pub output: String,
    pub concurrency: usize,
    pub model: Option<String>,
}

pub struct QaBatchService {
    inference_router: Arc<dyn InferencePort>,
}

impl QaBatchService {
    pub fn new(inference_router: Arc<dyn InferencePort>) -> Self {
        Self { inference_router }
    }

    /// Validate the entire input and open output before inference. Prepared
    /// messages are forwarded unchanged; completion accounting is transport-independent.
    #[must_use = "result must be used"]
    pub async fn generate_qa_batch(
        &self,
        request: QaBatchRequest,
    ) -> Result<serde_json::Value, McpToolError> {
        let QaBatchRequest {
            prompts_jsonl,
            output,
            concurrency,
            model,
        } = request;
        let prompts = read_prompts(&prompts_jsonl)?;
        let selected_model = configured_qa_model(model);
        let output_path = crate::path_safety::contain_for_write(&output)?;
        let file = std::fs::File::create(&output_path).map_err(|error| {
            map_corpus_io_error(error, &format!("Cannot create output file '{output}'"))
        })?;
        let mut completions = QaOutput::new(std::io::BufWriter::new(file), prompts.len());

        if let Some(model) = selected_model.as_deref() {
            if hkask_inference::batch::detect_batch_provider(model).is_some() {
                // Keep the original routing prefix/suffix for bridge-side detection.
                generate_qa_via_batch_api(
                    &self.inference_router,
                    &prompts,
                    model,
                    &mut completions,
                )
                .await?;
                return completions.finish(&output, true);
            }
        }

        let limiter = AdaptiveLimiter::new(concurrency, ADAPTIVE_CONCURRENCY_FLOOR);
        let mut tasks = tokio::task::JoinSet::new();
        let mut pending = HashMap::with_capacity(prompts.len());
        for prompt in prompts {
            let router = Arc::clone(&self.inference_router);
            let limiter = limiter.clone();
            let selected_model = selected_model.clone();
            let messages = [
                ChatMessage {
                    role: "system".into(),
                    content: prompt.system.clone(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: prompt.user.clone(),
                },
            ];
            let prompt_id = prompt.prompt_id.clone();
            let task = tasks.spawn(async move {
                let slot = limiter.acquire().await;
                let parameters = qa_llm_parameters();
                let response = retry_with_backoff(
                    MAX_RETRIES,
                    "hkask.mcp.docproc.qa_batch",
                    &prompt_id,
                    || {
                        router.generate_with_messages(
                            &messages,
                            &parameters,
                            selected_model.as_deref(),
                            None,
                        )
                    },
                )
                .await;
                match response {
                    Ok(response) => {
                        slot.report_success();
                        Ok(QaCompletion {
                            text: response.text,
                            tokens_used: u64::from(response.usage.total_tokens),
                        })
                    }
                    Err(error) => {
                        slot.report_failure();
                        Err(format!("LLM failed after {MAX_RETRIES} retries: {error}"))
                    }
                }
            });
            pending.insert(task.id(), prompt);
        }

        // JoinSet yields completion order and aborts remaining tasks if output
        // fails or the tool is cancelled. Keep metadata outside tasks so panics
        // still produce an identified failed-prompt record.
        while let Some(result) = tasks.join_next_with_id().await {
            let (identity, completion) = match result {
                Ok((identity, completion)) => (identity, completion),
                Err(error) => (error.id(), Err(format!("QA task join failed: {error}"))),
            };
            let prompt = pending.remove(&identity).ok_or_else(|| {
                McpToolError::internal("QA task completed without prompt metadata")
            })?;
            completions.complete(
                &prompt,
                completion,
                selected_model.as_deref().unwrap_or("router_default"),
            )?;
        }
        completions.finish(&output, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::qa_pipeline::PreparedQaPrompt;
    use hkask_types::inference_ipc::{BatchPromptEntry, BatchResultEntry};
    use hkask_types::template::LLMParameters;
    use hkask_types::{ChatToolDefinition, InferenceError, InferenceResult};
    use serde_json::json;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;

    type InferenceFuture<'a> =
        Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>>;

    #[derive(Default)]
    struct RecordingPort {
        messages: Mutex<Vec<(String, Vec<ChatMessage>)>>,
        batches: Mutex<Vec<(String, Vec<BatchPromptEntry>)>>,
        results: Option<Vec<BatchResultEntry>>,
    }

    fn response_text(question: &str) -> String {
        json!({"qa_pairs": [
            {"question": question, "answer": "Grounded answer one.", "bloom_level": "factual"},
            {"question": "Second question?", "answer": "Grounded answer two.", "bloom_level": "factual"}
        ]}).to_string()
    }

    impl InferencePort for RecordingPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> InferenceFuture<'_> {
            Box::pin(async {
                Err(InferenceError::Generation(
                    "Prepared QA must use role-aware messages".into(),
                ))
            })
        }

        fn generate_with_messages(
            &self,
            messages: &[ChatMessage],
            _parameters: &LLMParameters,
            model: Option<&str>,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> InferenceFuture<'_> {
            self.messages
                .lock()
                .expect("record messages")
                .push((model.unwrap_or("none").to_string(), messages.to_vec()));
            let user = messages
                .iter()
                .find(|message| message.role == "user")
                .expect("user message")
                .content
                .clone();
            Box::pin(async move {
                assert_ne!(user, "panic", "injected task panic");
                if user == "provider-error" {
                    return Err(InferenceError::Generation(
                        "injected provider outage".into(),
                    ));
                }
                Ok(InferenceResult {
                    text: if user == "malformed" {
                        "not JSON".into()
                    } else {
                        response_text(&user)
                    },
                    model: "offline-model".into(),
                    usage: hkask_types::InferenceUsage {
                        prompt_tokens: 4,
                        completion_tokens: 6,
                        total_tokens: 10,
                    },
                    finish_reason: "stop".into(),
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }

        fn generate_batch<'a>(
            &'a self,
            model: &str,
            prompts: &[BatchPromptEntry],
            _max_tokens: u32,
            _temperature: f32,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<BatchResultEntry>, InferenceError>> + Send + 'a>>
        {
            self.batches
                .lock()
                .expect("record batch")
                .push((model.to_string(), prompts.to_vec()));
            let results = self.results.clone().unwrap_or_else(|| {
                prompts
                    .iter()
                    .rev()
                    .map(|prompt| BatchResultEntry {
                        custom_id: prompt.custom_id.clone(),
                        text: Some(response_text(&prompt.user)),
                        total_tokens: 10,
                        error: None,
                    })
                    .collect()
            });
            Box::pin(async move { Ok(results) })
        }
    }

    fn prepared(identity: &str, user: &str) -> PreparedQaPrompt {
        PreparedQaPrompt {
            prompt_id: identity.into(),
            chunk_ref: "shared-chunk".into(),
            source: "source.txt".into(),
            concepts: vec!["concept".into()],
            salience: 0.5,
            qa_type: "factual".into(),
            system: format!("Exact prepared instructions for {identity}\nDo not rewrap."),
            user: user.into(),
        }
    }

    fn fixture(
        directory: &tempfile::TempDir,
        prompts: &[PreparedQaPrompt],
        model: &str,
    ) -> Result<QaBatchRequest, Box<dyn std::error::Error>> {
        let path = directory.path().join("prompts.jsonl");
        let content = prompts
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        std::fs::write(&path, content)?;
        Ok(QaBatchRequest {
            prompts_jsonl: path.to_string_lossy().into(),
            output: directory
                .path()
                .join("output.jsonl")
                .to_string_lossy()
                .into(),
            concurrency: 2,
            model: Some(model.into()),
        })
    }

    fn records(path: &str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
        Ok(std::fs::read_to_string(path)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?)
    }

    /// expect: [P8] Every prompt survives shared chunk references, with identical instructions on either transport.
    #[tokio::test]
    async fn transports_preserve_prepared_messages_and_match_out_of_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let prompts = vec![
            prepared("qa-1", "first question"),
            prepared("qa-2", "second question"),
        ];
        for model in ["offline-model", "offline-model:batch"] {
            let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
            let request = fixture(&directory, &prompts, model)?;
            let output = request.output.clone();
            let router = Arc::new(RecordingPort::default());
            let summary = QaBatchService::new(router.clone())
                .generate_qa_batch(request)
                .await?;
            assert_eq!(summary["prompts_total"], 2);
            assert_eq!(summary["prompts_succeeded"], 2);
            assert_eq!(summary["prompts_failed"], 0);
            assert_eq!(summary["qa_rows_written"], 4);
            assert_eq!(summary["degraded"], false);
            let rows = records(&output)?;
            assert_eq!(rows.len(), 4);
            for prompt in &prompts {
                let matching: Vec<_> = rows
                    .iter()
                    .filter(|row| row["prompt_id"] == prompt.prompt_id)
                    .collect();
                assert_eq!(matching.len(), 2);
                assert_eq!(matching[0]["response"]["instruction"], prompt.user);
                for row in matching {
                    assert_eq!(row["chunk_ref"], prompt.chunk_ref);
                    assert_eq!(row["source"], prompt.source);
                    assert_eq!(row["response"]["concepts"], json!(prompt.concepts));
                    assert_eq!(row["qa_type"], prompt.qa_type);
                    assert_eq!(row["salience"], prompt.salience);
                    assert_eq!(row["provenance"]["prompt_id"], prompt.prompt_id);
                    assert_eq!(row["provenance"]["generator_model"], model);
                }
            }
            if model.ends_with(":batch") {
                assert!(router.messages.lock().expect("messages").is_empty());
                let batches = router.batches.lock().expect("batches");
                let (called_model, entries) = batches.first().expect("one batch");
                assert_eq!(called_model, model);
                for (entry, prompt) in entries.iter().zip(&prompts) {
                    assert_eq!(entry.custom_id, prompt.prompt_id);
                    assert_eq!(entry.system, prompt.system);
                    assert_eq!(entry.user, prompt.user);
                }
            } else {
                assert!(router.batches.lock().expect("batches").is_empty());
                let calls = router.messages.lock().expect("messages");
                assert_eq!(calls.len(), 2);
                for (called_model, messages) in calls.iter() {
                    assert_eq!(called_model, model);
                    let [system, user] = messages.as_slice() else {
                        panic!("Expected two role-separated messages")
                    };
                    assert_eq!(system.role, "system");
                    assert_eq!(user.role, "user");
                    let prompt = prompts
                        .iter()
                        .find(|prompt| prompt.user == user.content)
                        .expect("known prompt");
                    assert_eq!(system.content, prompt.system);
                }
            }
        }
        Ok(())
    }

    /// expect: [P4] Invalid records anywhere in the file reject the whole request before inference or output truncation.
    #[tokio::test]
    async fn all_records_validated_before_calls() -> Result<(), Box<dyn std::error::Error>> {
        let good = serde_json::to_value(prepared("qa-1", "user"))?;
        let mut invalid_records = vec![
            json!({}),
            json!({"chunk_id":"legacy", "text":"legacy", "bloom_levels":["factual"]}),
            good.clone(),
        ];
        for field in [
            "prompt_id",
            "chunk_ref",
            "source",
            "qa_type",
            "system",
            "user",
        ] {
            let mut record = serde_json::to_value(prepared("qa-2", "user"))?;
            record[field] = json!("  ");
            invalid_records.push(record);
        }
        for field in [
            "prompt_id",
            "chunk_ref",
            "source",
            "qa_type",
            "system",
            "user",
            "salience",
            "concepts",
        ] {
            let mut record = serde_json::to_value(prepared("qa-2", "user"))?;
            record.as_object_mut().expect("object").remove(field);
            invalid_records.push(record);
        }
        for (field, value) in [
            ("concepts", json!([""])),
            ("concepts", json!([1])),
            ("salience", json!("bad")),
            ("prompt_id", json!("unsafe/id")),
            ("prompt_id", json!("x".repeat(65))),
            ("text", json!("legacy alias")),
        ] {
            let mut record = serde_json::to_value(prepared("qa-2", "user"))?;
            record[field] = value;
            invalid_records.push(record);
        }
        for model in ["offline-model", "offline-model:batch"] {
            for bad in &invalid_records {
                let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
                let request = fixture(&directory, &[], model)?;
                std::fs::write(&request.prompts_jsonl, format!("{good}\n{bad}\n"))?;
                std::fs::write(&request.output, "unchanged")?;
                let output = request.output.clone();
                let router = Arc::new(RecordingPort::default());
                assert!(
                    QaBatchService::new(router.clone())
                        .generate_qa_batch(request)
                        .await
                        .is_err()
                );
                assert!(router.messages.lock().expect("messages").is_empty());
                assert!(router.batches.lock().expect("batches").is_empty());
                assert_eq!(std::fs::read_to_string(output)?, "unchanged");
            }
        }
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
        let request = fixture(&directory, &[], "offline-model")?;
        assert!(read_prompts(&request.prompts_jsonl).is_err());
        std::fs::write(&request.prompts_jsonl, format!("{good}\nnot JSON\n"))?;
        assert!(read_prompts(&request.prompts_jsonl).is_err());
        Ok(())
    }

    /// expect: [P9] Batch missing, duplicate, malformed, and provider-error results each count as one failed prompt.
    #[tokio::test]
    async fn batch_response_failures_have_truthful_totals() -> Result<(), Box<dyn std::error::Error>>
    {
        let prompts: Vec<_> = (1..=7)
            .map(|index| prepared(&format!("qa-{index}"), "user"))
            .collect();
        let result = |identity: &str, text: Option<&str>, error: Option<&str>| BatchResultEntry {
            custom_id: identity.into(),
            text: text.map(String::from),
            error: error.map(String::from),
            total_tokens: 10,
        };
        let valid = response_text("valid");
        let router = Arc::new(RecordingPort {
            results: Some(vec![
                result("qa-7", Some(&valid), None),
                result("qa-2", Some(&valid), None),
                result("qa-2", Some(&valid), None),
                result("qa-3", None, Some("provider refused")),
                result("qa-4", None, None),
                result("qa-5", Some(&valid), Some("conflicting error")),
                result("qa-6", Some("bad JSON"), None),
            ]),
            ..Default::default()
        });
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
        let request = fixture(&directory, &prompts, "offline-model:batch")?;
        let output = request.output.clone();
        let summary = QaBatchService::new(router)
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_total"], 7);
        assert_eq!(summary["prompts_succeeded"], 1);
        assert_eq!(summary["prompts_failed"], 6);
        assert_eq!(summary["qa_rows_written"], 2);
        assert_eq!(summary["degraded"], true);
        let rows = records(&output)?;
        assert_eq!(rows.len(), 8);
        for (index, reason) in [
            "no result",
            "duplicate",
            "provider refused",
            "Malformed",
            "Malformed",
            "rejected",
        ]
        .iter()
        .enumerate()
        {
            let identity = format!("qa-{}", index + 1);
            let row = rows
                .iter()
                .find(|row| row["prompt_id"] == identity)
                .expect("failure row");
            assert!(row["error"].as_str().expect("error").contains(reason));
            assert_eq!(row["chunk_ref"], "shared-chunk");
            assert!(row.get("response").is_none());
        }
        Ok(())
    }

    /// expect: [P9] Unsolicited provider identities are visible protocol errors, not silently discarded results.
    #[tokio::test]
    async fn unknown_batch_identity_is_a_tool_error() -> Result<(), Box<dyn std::error::Error>> {
        let router = Arc::new(RecordingPort {
            results: Some(vec![BatchResultEntry {
                custom_id: "unknown".into(),
                text: Some(response_text("question")),
                error: None,
                total_tokens: 10,
            }]),
            ..Default::default()
        });
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
        let request = fixture(
            &directory,
            &[prepared("qa-1", "user")],
            "offline-model:batch",
        )?;
        let error = QaBatchService::new(router)
            .generate_qa_batch(request)
            .await
            .expect_err("unknown ID must fail");
        assert!(error.to_string().contains("unknown prompt_id 'unknown'"));
        Ok(())
    }

    /// expect: [P9] Panics, exhausted retries and parse rejection cannot masquerade as successful prompts.
    #[tokio::test]
    async fn synchronous_failures_and_join_errors_are_counted()
    -> Result<(), Box<dyn std::error::Error>> {
        let prompts = vec![
            prepared("qa-1", "panic"),
            prepared("qa-2", "malformed"),
            prepared("qa-3", "provider-error"),
            prepared("qa-4", "valid"),
        ];
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
        let request = fixture(&directory, &prompts, "offline-model")?;
        let output = request.output.clone();
        let router = Arc::new(RecordingPort::default());
        let summary = QaBatchService::new(router.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_total"], 4);
        assert_eq!(summary["prompts_succeeded"], 1);
        assert_eq!(summary["prompts_failed"], 3);
        assert_eq!(summary["qa_rows_written"], 2);
        let rows = records(&output)?;
        for (identity, reason) in [
            ("qa-1", "join failed"),
            ("qa-2", "rejected"),
            ("qa-3", "injected provider outage"),
        ] {
            let row = rows
                .iter()
                .find(|row| row["prompt_id"] == identity)
                .expect("failure row");
            assert!(row["error"].as_str().expect("error").contains(reason));
        }
        let calls = router.messages.lock().expect("calls");
        assert_eq!(
            calls
                .iter()
                .filter(|(_, messages)| messages
                    .iter()
                    .any(|message| message.content == "provider-error"))
                .count(),
            MAX_RETRIES as usize
        );
        Ok(())
    }

    /// expect: [P8] The real builder emits one canonical identity per prompt, with a response contract the transports preserve.
    #[tokio::test]
    async fn builder_records_round_trip_through_both_transports()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::prompt_builder::{BuildPromptsRequest, PromptBuilderService};
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
        let tagged = directory.path().join("tagged.jsonl");
        std::fs::write(&tagged, json!({"entity_ref":"shared-chunk", "source":"source.txt", "text":"The passage supplies a verifiable fact.", "concepts":["fact"], "salience":0.5}).to_string())?;
        for max_prompts in [0, 2, 4] {
            let path = directory.path().join("built.jsonl");
            let result = PromptBuilderService::new()
                .build_prompts(BuildPromptsRequest {
                    tagged_jsonl: tagged.to_string_lossy().into(),
                    output: path.to_string_lossy().into(),
                    db_path: directory.path().join("memory.db").to_string_lossy().into(),
                    passphrase: "offline-test-passphrase".into(),
                    prefix: Some("shared-".into()),
                    context_k: 0,
                    prompts_per_chunk: 3,
                    type_distribution: "1,0,0,0,0".into(),
                    max_prompts,
                    ontology_bloom_overrides: None,
                })
                .await?;
            let expected = if max_prompts == 2 { 2 } else { 3 };
            assert_eq!(result["prompts_written"], expected);
            let prompts = read_prompts(&path.to_string_lossy())?;
            assert_eq!(prompts.len(), expected);
            assert_eq!(
                prompts
                    .iter()
                    .map(|prompt| &prompt.prompt_id)
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                expected
            );
            for prompt in &prompts {
                assert_eq!(prompt.chunk_ref, "shared-chunk");
                assert!(prompt.system.contains("qa_pairs"));
                assert!(prompt.system.contains("bloom_level"));
                assert!(
                    prompt
                        .user
                        .contains("The passage supplies a verifiable fact.")
                );
            }
            for model in ["offline-model", "offline-model:batch"] {
                let router = Arc::new(RecordingPort::default());
                let summary = QaBatchService::new(router.clone())
                    .generate_qa_batch(QaBatchRequest {
                        prompts_jsonl: path.to_string_lossy().into(),
                        output: directory
                            .path()
                            .join("generated.jsonl")
                            .to_string_lossy()
                            .into(),
                        concurrency: 2,
                        model: Some(model.into()),
                    })
                    .await?;
                assert_eq!(summary["prompts_succeeded"], expected);
                if model.ends_with(":batch") {
                    let calls = router.batches.lock().expect("batch calls");
                    let (_, entries) = calls.first().expect("batch call");
                    for (entry, prompt) in entries.iter().zip(&prompts) {
                        assert_eq!(entry.system, prompt.system);
                        assert_eq!(entry.user, prompt.user);
                    }
                } else {
                    let calls = router.messages.lock().expect("calls");
                    for (_, messages) in calls.iter() {
                        let [system, user] = messages.as_slice() else {
                            panic!("Expected system and user")
                        };
                        assert!(
                            prompts.iter().any(|prompt| prompt.system == system.content
                                && prompt.user == user.content)
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// expect: [P1] An unusable output path fails before either transport spends inference.
    #[tokio::test]
    async fn output_is_preflighted_for_both_transports() -> Result<(), Box<dyn std::error::Error>> {
        for model in ["offline-model", "offline-model:batch"] {
            let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
            let mut request = fixture(&directory, &[prepared("qa-1", "user")], model)?;
            request.output = directory.path().to_string_lossy().into();
            let router = Arc::new(RecordingPort::default());
            assert!(
                QaBatchService::new(router.clone())
                    .generate_qa_batch(request)
                    .await
                    .is_err()
            );
            assert!(router.messages.lock().expect("messages").is_empty());
            assert!(router.batches.lock().expect("batches").is_empty());
        }
        Ok(())
    }

    /// expect: [P8] Duplicate prompt identities must fail before generation, even for a shared chunk.
    #[test]
    fn duplicate_prompt_ids_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
        let path = directory.path().join("prompts.jsonl");
        let record = json!({
            "prompt_id": "qa-1", "chunk_ref": "chunk-1", "source": "source.txt",
            "system": "Prepared system", "user": "Prepared user", "qa_type": "factual",
            "concepts": [], "salience": 0.5
        });
        std::fs::write(&path, format!("{record}\n{record}\n"))?;
        let result = read_prompts(&path.to_string_lossy());
        assert!(result.is_err(), "duplicate prompt IDs were accepted");
        Ok(())
    }
}
