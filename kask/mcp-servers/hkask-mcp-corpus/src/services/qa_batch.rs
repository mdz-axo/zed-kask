//! Prepared QA generation with AIMD-gated synchronous inference or provider batches.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use hkask_mcp_server::server::McpToolError;
use hkask_types::InferencePort;

use crate::batch::{
    ADAPTIVE_CONCURRENCY_FLOOR, AdaptiveLimiter, MAX_RETRIES, inference_error_is_transient,
    retry_with_backoff,
};
use crate::helpers::map_corpus_io_error;
use crate::services::qa_pipeline::{
    PreparedQaPrompt, QaCompletion, QaCompletionError, QaOutput, qa_llm_parameters, read_prompts,
    render_prepared_messages,
};
use crate::tools::semantic::batch_api::generate_qa_via_batch_api;
use crate::tools::semantic::qa::map_qa_inference_error;

// One registry across service instances and transports, not one mutex per call.
static QA_OUTPUTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

struct QaOutputLease {
    path: PathBuf,
    opened: AtomicBool,
    finished: AtomicBool,
}

impl QaOutputLease {
    fn acquire(path: PathBuf) -> Result<Arc<Self>, McpToolError> {
        let mut outputs = QA_OUTPUTS
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| McpToolError::internal("QA output ownership registry is poisoned"))?;
        if !outputs.insert(path.clone()) {
            return Err(McpToolError::failed_precondition(format!(
                "QA output '{}' already has an active owner; partial output is preserved. Do not retry while that call or its workers are active",
                path.display()
            )));
        }
        Ok(Arc::new(Self {
            path,
            opened: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        }))
    }
}

impl Drop for QaOutputLease {
    fn drop(&mut self) {
        if self.opened.load(Ordering::Relaxed) && !self.finished.load(Ordering::Relaxed) {
            tracing::warn!(
                target: "hkask.mcp.docproc.qa_batch",
                output = %self.path.display(),
                "QA stopped without completion; partial output is preserved. No automatic retry or resume; a new call will overwrite this file"
            );
        }
        // Recover only for cleanup; acquisition fails closed on a poisoned registry.
        let mut outputs = QA_OUTPUTS.get_or_init(Mutex::default).lock().unwrap_or_else(|error| {
            tracing::warn!(target: "hkask.mcp.docproc.qa_batch", "QA output ownership registry poisoned during release");
            error.into_inner()
        });
        outputs.remove(&self.path);
    }
}

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
        let selected_model =
            hkask_inference::model_constants::resolve_qa_generation_model(model.as_deref())
                .map_err(map_qa_inference_error)?;
        let output_path = crate::path_safety::distinct_output_path(&prompts_jsonl, &output)?;
        let lease = QaOutputLease::acquire(output_path)?;
        let file = std::fs::File::create(&lease.path).map_err(|error| {
            map_corpus_io_error(error, &format!("Cannot create output file '{output}'"))
        })?;
        lease.opened.store(true, Ordering::Relaxed);
        // No BufWriter: cancellation must not depend on an unchecked drop-time
        // flush to preserve completed rows. File errors surface at the write.
        let completions = QaOutput::new(file, prompts.len());
        self.generate_prepared(
            prompts,
            &selected_model,
            AdaptiveLimiter::new(concurrency, ADAPTIVE_CONCURRENCY_FLOOR),
            completions,
            lease,
            &output,
        )
        .await
    }

    async fn generate_prepared<W: Write>(
        &self,
        prompts: Vec<PreparedQaPrompt>,
        selected_model: &str,
        limiter: AdaptiveLimiter,
        mut completions: QaOutput<W>,
        lease: Arc<QaOutputLease>,
        output: &str,
    ) -> Result<serde_json::Value, McpToolError> {
        let result = async {
            if hkask_inference::batch::detect_batch_provider(selected_model).is_some() {
                // Keep the original routing prefix/suffix for bridge-side detection.
                generate_qa_via_batch_api(
                    &self.inference_router,
                    &prompts,
                    selected_model,
                    &mut completions,
                )
                .await?;
                return completions.finish(output, true);
            }

            let mut tasks = tokio::task::JoinSet::new();
            let mut pending = HashMap::with_capacity(prompts.len());
            for prompt in prompts {
                let router = Arc::clone(&self.inference_router);
                let limiter = limiter.clone();
                let selected_model = selected_model.to_owned();
                let task_lease = Arc::clone(&lease);
                let messages = render_prepared_messages(&prompt)?;
                let prompt_id = prompt.prompt_id.clone();
                let task = tasks.spawn(async move {
                    // JoinSet abort is asynchronous: retain ownership until this
                    // worker actually drops, not merely until its abort is requested.
                    let _lease = task_lease;
                    let parameters = qa_llm_parameters();
                    let mut attempts = 0;
                    let response = retry_with_backoff(
                        MAX_RETRIES,
                        "hkask.mcp.docproc.qa_batch",
                        &prompt_id,
                        || {
                            attempts += 1;
                            async {
                                let slot = limiter.acquire().await;
                                let response = router
                                    .generate_with_messages(
                                        &messages,
                                        &parameters,
                                        Some(&selected_model),
                                        None,
                                    )
                                    .await;
                                match &response {
                                    Ok(_) => slot.report_success(),
                                    Err(error) if inference_error_is_transient(error) => {
                                        slot.report_failure()
                                    }
                                    Err(_) => {}
                                }
                                response
                            }
                        },
                    )
                    .await;
                    match response {
                        Ok(response) => Ok(QaCompletion {
                            text: response.text,
                            tokens_used: u64::from(response.usage.total_tokens),
                        }),
                        Err(error) => {
                            Err(QaCompletionError::LlmFailed(attempts, error.to_string()))
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
                    Err(error) => (
                        error.id(),
                        Err(QaCompletionError::JoinFailed(error.to_string())),
                    ),
                };
                let prompt = pending.remove(&identity).ok_or_else(|| {
                    McpToolError::internal("QA task completed without prompt metadata")
                })?;
                completions.complete(&prompt, completion, selected_model)?;
            }
            completions.finish(output, false)
        }
        .await;
        let result = result.map_err(|mut error: McpToolError| {
            error.message.push_str(&format!(
                "; partial QA output is preserved at '{}'. No automatic retry or resume; wait for active workers to stop before overwriting",
                lease.path.display()
            ));
            error
        });
        if result.is_ok() {
            lease.finished.store(true, Ordering::Relaxed);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::qa_pipeline::PreparedQaPrompt;
    use hkask_types::inference_ipc::{BatchPromptEntry, BatchResultEntry};
    use hkask_types::template::LLMParameters;
    use hkask_types::{ChatMessage, ChatToolDefinition, InferenceError, InferenceResult};
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

    struct ControlledPort {
        error: Option<fn() -> InferenceError>,
        ready_calls: usize,
        ready_after_calls: usize,
        calls: std::sync::atomic::AtomicUsize,
        active: std::sync::atomic::AtomicUsize,
    }

    impl ControlledPort {
        fn new(error: Option<fn() -> InferenceError>, ready_calls: usize) -> Self {
            Self {
                error,
                ready_calls,
                ready_after_calls: 0,
                calls: std::sync::atomic::AtomicUsize::new(0),
                active: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn respond(&self) -> InferenceFuture<'_> {
            Box::pin(async move {
                use std::sync::atomic::Ordering::SeqCst;
                struct Active<'a>(&'a std::sync::atomic::AtomicUsize);
                impl Drop for Active<'_> {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, SeqCst);
                    }
                }
                self.active.fetch_add(1, SeqCst);
                let _active = Active(&self.active);
                let call = self.calls.fetch_add(1, SeqCst) + 1;
                if call > self.ready_calls {
                    return std::future::pending().await;
                }
                while self.calls.load(SeqCst) < self.ready_after_calls {
                    tokio::task::yield_now().await;
                }
                if let Some(error) = self.error {
                    return Err(error());
                }
                Ok(InferenceResult {
                    text: response_text("completed before cancellation"),
                    model: "OpenRouter/offline-model".into(),
                    usage: Default::default(),
                    finish_reason: "stop".into(),
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    impl InferencePort for ControlledPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> InferenceFuture<'_> {
            panic!("QA must not fall back to generate")
        }

        fn generate_with_messages(
            &self,
            _messages: &[ChatMessage],
            _parameters: &LLMParameters,
            _model: Option<&str>,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> InferenceFuture<'_> {
            self.respond()
        }

        fn generate_batch<'a>(
            &'a self,
            _model: &str,
            _prompts: &[BatchPromptEntry],
            _max_tokens: u32,
            _temperature: f32,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<BatchResultEntry>, InferenceError>> + Send + 'a>>
        {
            Box::pin(async move {
                self.respond().await?;
                Ok(Vec::new())
            })
        }
    }

    async fn wait_until(mut ready: impl FnMut() -> bool) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("mock operation did not reach expected state");
    }

    fn response_text(question: &str) -> String {
        json!([
            [
                "factual",
                question,
                "Grounded answer one.",
                [["p0", "Grounded answer one."]]
            ],
            [
                "conceptual",
                "Second question?",
                "Grounded answer two.",
                [["p0", "Grounded answer two."]]
            ]
        ])
        .to_string()
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
                assert!(!user.contains("panic"), "injected task panic");
                if user.contains("provider-error") {
                    return Err(InferenceError::Connection(
                        "injected provider outage".into(),
                    ));
                }
                Ok(InferenceResult {
                    text: if user.contains("malformed") {
                        "not JSON".into()
                    } else {
                        response_text(&user)
                    },
                    model: "OpenRouter/offline-model".into(),
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

    fn prepared(identity: &str, marker: &str) -> PreparedQaPrompt {
        PreparedQaPrompt {
            prompt_id: identity.into(),
            protocol: crate::services::qa_pipeline::PREPARED_QA_PROTOCOL.into(),
            passages: vec![crate::services::qa_pipeline::PreparedQaPassage {
                local_id: "p0".into(),
                chunk_ref: "shared-chunk".into(),
                source: "source.txt".into(),
                text: format!("Grounded answer one. Grounded answer two. {marker}"),
            }],
            candidate_terms: vec!["grounded answer".into()],
            qa_types: vec![
                crate::tools::corpus::QaType::Factual,
                crate::tools::corpus::QaType::Conceptual,
            ],
        }
    }

    fn fixture_directory() -> std::io::Result<tempfile::TempDir> {
        // Stay within corpus path containment without exposing transient files to Git.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-batch-test");
        std::fs::create_dir_all(&root)?;
        tempfile::tempdir_in(root)
    }

    /// expect: QA tests leave no untracked fixtures, even if a run is interrupted.
    /// [P5] Motivating: Test artifacts belong under the ignored build directory.
    #[test]
    fn fixture_directory_is_contained_and_cleaned() -> std::io::Result<()> {
        let directory = fixture_directory()?;
        let path = directory.path().to_path_buf();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-batch-test");
        assert!(
            path.starts_with(root),
            "fixture escaped the ignored test directory"
        );
        std::fs::write(path.join("fixture.jsonl"), "{}\n")?;
        drop(directory);
        assert!(!path.exists(), "fixture was not removed on drop");
        Ok(())
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
        for model in ["OpenRouter/offline-model", "OpenRouter/offline-model:batch"] {
            let directory = fixture_directory()?;
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
                assert_eq!(matching[0]["qa_type"], "factual");
                assert_eq!(matching[1]["qa_type"], "conceptual");
                for row in matching {
                    assert_eq!(row["chunk_ref"], prompt.primary().chunk_ref);
                    assert_eq!(row["source"], prompt.primary().source);
                    assert_eq!(row["response"]["concepts"], json!(prompt.candidate_terms));
                    assert!(row.get("salience").is_none());
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
                    let [system, user] = render_prepared_messages(prompt)?;
                    assert_eq!(entry.custom_id, prompt.prompt_id);
                    assert_eq!(entry.system, system.content);
                    assert_eq!(entry.user, user.content);
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
                        .find(|prompt| {
                            render_prepared_messages(prompt)
                                .is_ok_and(|rendered| rendered[1].content == user.content)
                        })
                        .expect("known prompt");
                    assert_eq!(system.content, render_prepared_messages(prompt)?[0].content);
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
            "protocol",
            "passages",
            "candidate_terms",
            "qa_types",
        ] {
            let mut record = serde_json::to_value(prepared("qa-2", "user"))?;
            record.as_object_mut().expect("object").remove(field);
            invalid_records.push(record);
        }
        for (field, value) in [
            ("protocol", json!("old-protocol")),
            ("passages", json!([])),
            ("candidate_terms", json!([""])),
            ("qa_types", json!([])),
            ("prompt_id", json!("unsafe/id")),
            ("prompt_id", json!("x".repeat(65))),
            ("text", json!("legacy alias")),
        ] {
            let mut record = serde_json::to_value(prepared("qa-2", "user"))?;
            record[field] = value;
            invalid_records.push(record);
        }
        for (field, value) in [
            ("local_id", json!("wrong")),
            ("chunk_ref", json!("")),
            ("source", json!("")),
            ("text", json!("")),
        ] {
            let mut record = serde_json::to_value(prepared("qa-2", "user"))?;
            record["passages"][0][field] = value;
            invalid_records.push(record);
        }
        for model in ["OpenRouter/offline-model", "OpenRouter/offline-model:batch"] {
            for bad in &invalid_records {
                let directory = fixture_directory()?;
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
        let directory = fixture_directory()?;
        let request = fixture(&directory, &[], "OpenRouter/offline-model")?;
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
        let directory = fixture_directory()?;
        let request = fixture(&directory, &prompts, "OpenRouter/offline-model:batch")?;
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
        let directory = fixture_directory()?;
        let request = fixture(
            &directory,
            &[prepared("qa-1", "user")],
            "OpenRouter/offline-model:batch",
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
        let directory = fixture_directory()?;
        let request = fixture(&directory, &prompts, "OpenRouter/offline-model")?;
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
                    .any(|message| message.content.contains("provider-error")))
                .count(),
            MAX_RETRIES as usize
        );
        Ok(())
    }

    /// expect: The real builder emits one compact two-pair request per chunk,
    /// and both transports render the same local-identity messages.
    #[tokio::test]
    async fn builder_records_round_trip_through_both_transports()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::prompt_builder::{BuildPromptsRequest, PromptBuilderService};
        let directory = fixture_directory()?;
        let tagged = directory.path().join("tagged.jsonl");
        std::fs::write(&tagged, json!({"entity_ref":"shared-chunk", "classification":{"status":"classified", "ontology_protocol":hkask_bridge_ontology::term_resolution::TERM_RESOLUTION_PROTOCOL}, "source":"source.txt", "text":"Grounded answer one. Grounded answer two.", "candidate_terms":["quantity"], "ontology_tags":{"sumo":[hkask_bridge_ontology::sumo::QUANTITY]}, "concepts":[hkask_bridge_ontology::sumo::QUANTITY], "salience":0.5}).to_string())?;
        let path = directory.path().join("built.jsonl");
        let result = PromptBuilderService::new()
            .build_prompts(BuildPromptsRequest {
                tagged_jsonl: tagged.to_string_lossy().into(),
                output: path.to_string_lossy().into(),
                db_path: None,
                passphrase: None,
                prefix: Some("shared-".into()),
                context_k: 0,
                qa_pairs_per_chunk: 2,
                type_distribution: "1,1,1,1,1".into(),
                max_pairs: 0,
            })
            .await?;
        assert_eq!(result["prompts_written"], 1);
        assert_eq!(result["pairs_requested"], 2);
        let prompts = read_prompts(&path.to_string_lossy())?;
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].primary().chunk_ref, "shared-chunk");
        assert_eq!(prompts[0].qa_types.len(), 2);
        let expected_messages = render_prepared_messages(&prompts[0])?;
        let rendered = expected_messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Grounded answer one."));
        assert!(!rendered.contains("shared-chunk"));
        assert!(!rendered.contains("source.txt"));

        for model in ["OpenRouter/offline-model", "OpenRouter/offline-model:batch"] {
            let router = Arc::new(RecordingPort::default());
            let summary = QaBatchService::new(router.clone())
                .generate_qa_batch(QaBatchRequest {
                    prompts_jsonl: path.to_string_lossy().into(),
                    output: directory
                        .path()
                        .join(format!("generated-{}.jsonl", model.ends_with(":batch")))
                        .to_string_lossy()
                        .into(),
                    concurrency: 2,
                    model: Some(model.into()),
                })
                .await?;
            assert_eq!(summary["prompts_succeeded"], 1);
            assert_eq!(summary["qa_rows_written"], 2);
            if model.ends_with(":batch") {
                let calls = router.batches.lock().expect("batch calls");
                let (_, entries) = calls.first().expect("batch call");
                assert_eq!(entries[0].system, expected_messages[0].content);
                assert_eq!(entries[0].user, expected_messages[1].content);
            } else {
                let calls = router.messages.lock().expect("calls");
                assert_eq!(calls[0].1.len(), expected_messages.len());
                for (actual, expected) in calls[0].1.iter().zip(&expected_messages) {
                    assert_eq!(actual.role, expected.role);
                    assert_eq!(actual.content, expected.content);
                }
            }
        }

        let capped = directory.path().join("capped.jsonl");
        let result = PromptBuilderService::new()
            .build_prompts(BuildPromptsRequest {
                tagged_jsonl: tagged.to_string_lossy().into(),
                output: capped.to_string_lossy().into(),
                db_path: None,
                passphrase: None,
                prefix: Some("shared-".into()),
                context_k: 0,
                qa_pairs_per_chunk: 2,
                type_distribution: "1,1,1,1,1".into(),
                max_pairs: 1,
            })
            .await?;
        assert_eq!(result["prompts_written"], 1);
        assert_eq!(result["pairs_requested"], 1);
        assert_eq!(
            read_prompts(&capped.to_string_lossy())?[0].qa_types.len(),
            1
        );
        Ok(())
    }

    /// expect: [P1] An unusable output path fails before either transport spends inference.
    #[tokio::test]
    async fn output_is_preflighted_for_both_transports() -> Result<(), Box<dyn std::error::Error>> {
        for model in ["OpenRouter/offline-model", "OpenRouter/offline-model:batch"] {
            let directory = fixture_directory()?;
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

    /// expect: Slice5 rejects input aliases before truncation or inference on either transport.
    #[cfg(unix)]
    #[tokio::test]
    async fn slice5_input_aliases_preserve_input() -> Result<(), Box<dyn std::error::Error>> {
        for model in ["OpenRouter/offline-model", "OpenRouter/offline-model:batch"] {
            for alias in ["identical", "symlink-output", "symlink-input", "hardlink"] {
                let directory = fixture_directory()?;
                let mut request = fixture(&directory, &[prepared("qa-1", "user")], model)?;
                let original = std::fs::read(&request.prompts_jsonl)?;
                match alias {
                    "identical" => request.output = request.prompts_jsonl.clone(),
                    "symlink-output" => {
                        std::os::unix::fs::symlink(&request.prompts_jsonl, &request.output)?
                    }
                    "symlink-input" => {
                        std::fs::rename(&request.prompts_jsonl, &request.output)?;
                        std::os::unix::fs::symlink(&request.output, &request.prompts_jsonl)?;
                    }
                    "hardlink" => std::fs::hard_link(&request.prompts_jsonl, &request.output)?,
                    _ => unreachable!(),
                }
                let input = request.prompts_jsonl.clone();
                let router = Arc::new(RecordingPort::default());
                let error = QaBatchService::new(router.clone())
                    .generate_qa_batch(request)
                    .await
                    .expect_err("input alias must fail");
                assert!(error.to_string().contains("alias"), "{error}");
                assert_eq!(std::fs::read(input)?, original);
                assert!(router.messages.lock().expect("messages").is_empty());
                assert!(router.batches.lock().expect("batches").is_empty());
            }
        }
        Ok(())
    }

    /// expect: Slice5 owns canonical destinations across instances and transports, including dangling symlinks.
    #[cfg(unix)]
    #[tokio::test]
    async fn slice5_duplicate_destination_and_cancellation()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::Ordering::SeqCst;
        for model in ["OpenRouter/offline-model", "OpenRouter/offline-model:batch"] {
            for alias in ["same", "dangling-symlink", "symlink-parent", "dot"] {
                let directory = fixture_directory()?;
                let mut request = fixture(
                    &directory,
                    &[prepared("qa-1", "first"), prepared("qa-2", "second")],
                    model,
                )?;
                let output = request.output.clone();
                let input = request.prompts_jsonl.clone();
                let competing_output = match alias {
                    "dangling-symlink" => {
                        let link = directory.path().join("alias.jsonl");
                        std::os::unix::fs::symlink("output.jsonl", &link)?;
                        request.output = link.to_string_lossy().into();
                        output.clone()
                    }
                    "symlink-parent" => {
                        let link = directory.path().join("alias-dir");
                        std::os::unix::fs::symlink(directory.path(), &link)?;
                        link.join("output.jsonl").to_string_lossy().into()
                    }
                    "dot" => directory
                        .path()
                        .join("./output.jsonl")
                        .to_string_lossy()
                        .into(),
                    _ => output.clone(),
                };
                let ready_calls = usize::from(!model.ends_with(":batch"));
                let router = Arc::new(ControlledPort::new(None, ready_calls));
                let service = QaBatchService::new(router.clone());
                let running = tokio::spawn(async move { service.generate_qa_batch(request).await });
                wait_until(|| router.calls.load(SeqCst) == ready_calls + 1).await;
                if ready_calls > 0 {
                    wait_until(|| {
                        std::fs::metadata(&output).is_ok_and(|metadata| metadata.len() > 0)
                    })
                    .await;
                    assert_eq!(
                        records(&output)?.len(),
                        2,
                        "completed rows must be visible before cancellation"
                    );
                }
                let partial = std::fs::read(&output)?;
                let competing_port = Arc::new(RecordingPort::default());
                let competing_model = if alias == "same" {
                    model
                } else if model.ends_with(":batch") {
                    "OpenRouter/offline-model"
                } else {
                    "OpenRouter/offline-model:batch"
                };
                let competing_request = || QaBatchRequest {
                    prompts_jsonl: input.clone(),
                    output: competing_output.clone(),
                    concurrency: 2,
                    model: Some(competing_model.into()),
                };
                let competing_service = QaBatchService::new(competing_port.clone());
                let error = competing_service
                    .generate_qa_batch(competing_request())
                    .await
                    .expect_err("destination is active");
                assert!(error.to_string().contains("active owner"), "{error}");
                assert!(error.to_string().contains("Do not retry"), "{error}");
                assert!(competing_port.messages.lock().expect("messages").is_empty());
                assert!(competing_port.batches.lock().expect("batches").is_empty());
                assert_eq!(std::fs::read(&output)?, partial);
                let mut independent = competing_request();
                independent.output = directory
                    .path()
                    .join("independent.jsonl")
                    .to_string_lossy()
                    .into();
                assert_eq!(
                    competing_service.generate_qa_batch(independent).await?["prompts_succeeded"],
                    2,
                    "an active output must not block unrelated destinations"
                );
                running.abort();
                assert!(running.await.expect_err("cancelled owner").is_cancelled());
                wait_until(|| router.active.load(SeqCst) == 0).await;
                let canonical = std::fs::canonicalize(&output)?;
                wait_until(|| {
                    !QA_OUTPUTS
                        .get_or_init(Mutex::default)
                        .lock()
                        .expect("registry")
                        .contains(&canonical)
                })
                .await;
                assert_eq!(
                    std::fs::read(&output)?,
                    partial,
                    "cancellation must preserve output"
                );
                let summary = competing_service
                    .generate_qa_batch(competing_request())
                    .await?;
                assert_eq!(summary["prompts_succeeded"], 2);
                assert_eq!(
                    records(&output)?.len(),
                    4,
                    "explicit retry replaces, not appends or forks"
                );
            }
        }
        Ok(())
    }

    /// expect: Slice5 keeps ownership between JoinSet abort request and actual worker destruction.
    #[tokio::test]
    async fn slice5_cancel_does_not_release_before_workers_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::Ordering::SeqCst;
        let directory = fixture_directory()?;
        let request = fixture(
            &directory,
            &[prepared("qa-1", "pending")],
            "OpenRouter/offline-model",
        )?;
        let retry = QaBatchRequest {
            prompts_jsonl: request.prompts_jsonl.clone(),
            output: request.output.clone(),
            concurrency: 2,
            model: request.model.clone(),
        };
        let output = request.output.clone();
        let router = Arc::new(ControlledPort::new(None, 0));
        let service = QaBatchService::new(router.clone());
        let mut running = Box::pin(service.generate_qa_batch(request));
        tokio::select! {
            result = &mut running => panic!("pending inference unexpectedly completed: {result:?}"),
            () = wait_until(|| router.active.load(SeqCst) == 1) => {}
        }
        // On this current-thread runtime the aborted worker cannot be polled
        // between dropping the owner and the competing call's synchronous gate.
        drop(running);
        assert_eq!(router.active.load(SeqCst), 1);
        let competing_port = Arc::new(RecordingPort::default());
        let error = QaBatchService::new(competing_port.clone())
            .generate_qa_batch(retry)
            .await
            .expect_err("worker still holds output");
        assert!(error.to_string().contains("active owner"), "{error}");
        assert!(competing_port.messages.lock().expect("messages").is_empty());
        assert_eq!(std::fs::metadata(&output)?.len(), 0);
        let canonical = std::fs::canonicalize(output)?;
        wait_until(|| {
            router.active.load(SeqCst) == 0
                && !QA_OUTPUTS
                    .get_or_init(Mutex::default)
                    .lock()
                    .expect("registry")
                    .contains(&canonical)
        })
        .await;
        Ok(())
    }

    /// expect: Slice5 aborts retry backoff with its owner and permits a later explicit call.
    #[tokio::test]
    async fn slice5_cancel_during_retry() -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::Ordering::SeqCst;
        let directory = fixture_directory()?;
        let request = fixture(
            &directory,
            &[prepared("qa-1", "user")],
            "OpenRouter/offline-model",
        )?;
        let input = request.prompts_jsonl.clone();
        let output = request.output.clone();
        let router = Arc::new(ControlledPort::new(
            Some(|| InferenceError::Connection("retry me".into())),
            usize::MAX,
        ));
        let service = QaBatchService::new(router.clone());
        let running = tokio::spawn(async move { service.generate_qa_batch(request).await });
        wait_until(|| router.calls.load(SeqCst) == 1).await;
        let retry = || QaBatchRequest {
            prompts_jsonl: input.clone(),
            output: output.clone(),
            concurrency: 2,
            model: Some("OpenRouter/offline-model".into()),
        };
        let service = QaBatchService::new(Arc::new(RecordingPort::default()));
        assert!(
            service
                .generate_qa_batch(retry())
                .await
                .expect_err("backoff still owns destination")
                .to_string()
                .contains("active owner")
        );
        running.abort();
        assert!(running.await.expect_err("cancelled owner").is_cancelled());
        let canonical = std::fs::canonicalize(&output)?;
        wait_until(|| {
            !QA_OUTPUTS
                .get_or_init(Mutex::default)
                .lock()
                .expect("registry")
                .contains(&canonical)
        })
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
        assert_eq!(router.calls.load(SeqCst), 1, "aborted worker retried");
        assert_eq!(
            service.generate_qa_batch(retry()).await?["prompts_succeeded"],
            1
        );
        Ok(())
    }

    /// expect: Slice5 feeds actual attempts into AIMD and releases the slot during retry backoff.
    #[tokio::test]
    async fn slice5_first_transient_attempt_backs_off_immediately()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::Ordering::SeqCst;
        let directory = fixture_directory()?;
        let request = fixture(
            &directory,
            &[prepared("qa-1", "user")],
            "OpenRouter/offline-model",
        )?;
        let path =
            crate::path_safety::distinct_output_path(&request.prompts_jsonl, &request.output)?;
        let lease = QaOutputLease::acquire(path.clone())?;
        let file = std::fs::File::create(&path)?;
        lease.opened.store(true, Ordering::Relaxed);
        let prompts = read_prompts(&request.prompts_jsonl)?;
        let limiter = AdaptiveLimiter::new(8, ADAPTIVE_CONCURRENCY_FLOOR);
        for _ in 0..6 {
            limiter.acquire().await.report_success();
        }
        assert_eq!(limiter.current(), 8);
        let router = Arc::new(ControlledPort::new(
            Some(|| InferenceError::Overloaded("busy".into())),
            1,
        ));
        let service = QaBatchService::new(router.clone());
        let task_limiter = limiter.clone();
        let running = tokio::spawn(async move {
            service
                .generate_prepared(
                    prompts,
                    "OpenRouter/offline-model",
                    task_limiter,
                    QaOutput::new(file, 1),
                    lease,
                    &request.output,
                )
                .await
        });
        wait_until(|| router.calls.load(SeqCst) == 1).await;
        assert_eq!(
            limiter.current(),
            4,
            "first transient failure must reduce the allowance before retry sleep"
        );
        let mut slots = Vec::new();
        for _ in 0..4 {
            slots.push(
                tokio::time::timeout(std::time::Duration::from_millis(100), limiter.acquire())
                    .await?,
            );
        }
        assert_eq!(router.calls.load(SeqCst), 1);
        running.abort();
        assert!(running.await.expect_err("cancelled owner").is_cancelled());
        wait_until(|| {
            !QA_OUTPUTS
                .get_or_init(Mutex::default)
                .lock()
                .expect("registry")
                .contains(&path)
        })
        .await;
        drop(slots);
        Ok(())
    }

    /// expect: Slice5 propagates real file-write errors, preserves partial bytes and cancels owned workers.
    #[tokio::test]
    async fn slice5_file_write_failure_releases_workers_and_output()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::Ordering::SeqCst;
        let directory = fixture_directory()?;
        let request = fixture(
            &directory,
            &[prepared("qa-1", "first"), prepared("qa-2", "pending")],
            "OpenRouter/offline-model",
        )?;
        std::fs::write(&request.output, "previous partial output\n")?;
        let path =
            crate::path_safety::distinct_output_path(&request.prompts_jsonl, &request.output)?;
        let lease = QaOutputLease::acquire(path.clone())?;
        lease.opened.store(true, Ordering::Relaxed);
        let weak = Arc::downgrade(&lease);
        // A real read-only File produces an OS write error; this drives the same
        // writer-generic scheduling path as File::create, without /dev or live I/O.
        let file = std::fs::File::open(&path)?;
        let prompts = read_prompts(&request.prompts_jsonl)?;
        let mut controlled = ControlledPort::new(None, 1);
        controlled.ready_after_calls = 2;
        let router = Arc::new(controlled);
        let service = QaBatchService::new(router.clone());
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            service.generate_prepared(
                prompts,
                "OpenRouter/offline-model",
                AdaptiveLimiter::new(2, ADAPTIVE_CONCURRENCY_FLOOR),
                QaOutput::new(file, 2),
                lease,
                &request.output,
            ),
        )
        .await?;
        let error = result.expect_err("file write must fail");
        assert!(
            error.to_string().contains("Cannot write QA output"),
            "{error}"
        );
        assert!(
            error.to_string().contains("partial QA output is preserved"),
            "{error}"
        );
        wait_until(|| router.active.load(SeqCst) == 0 && weak.upgrade().is_none()).await;
        assert_eq!(router.calls.load(SeqCst), 2);
        assert_eq!(std::fs::read_to_string(&path)?, "previous partial output\n");
        assert!(
            !QA_OUTPUTS
                .get_or_init(Mutex::default)
                .lock()
                .expect("registry")
                .contains(&path)
        );
        let summary = QaBatchService::new(Arc::new(RecordingPort::default()))
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_succeeded"], 2);
        Ok(())
    }

    /// expect: Slice5 releases a lease even when opening the output fails before any inference.
    #[tokio::test]
    async fn slice5_create_failure_releases_output() -> Result<(), Box<dyn std::error::Error>> {
        let directory = fixture_directory()?;
        let mut request = fixture(
            &directory,
            &[prepared("qa-1", "user")],
            "OpenRouter/offline-model",
        )?;
        let output = directory.path().join("blocked");
        std::fs::create_dir(&output)?;
        request.output = output.to_string_lossy().into();
        let service = QaBatchService::new(Arc::new(RecordingPort::default()));
        assert!(
            service
                .generate_qa_batch(QaBatchRequest {
                    prompts_jsonl: request.prompts_jsonl.clone(),
                    output: request.output.clone(),
                    concurrency: 2,
                    model: request.model.clone(),
                })
                .await
                .is_err()
        );
        std::fs::remove_dir(&output)?;
        assert_eq!(
            service.generate_qa_batch(request).await?["prompts_succeeded"],
            1
        );
        Ok(())
    }

    /// expect: Slice5 never resubmits a provider batch and preserves authorization error classification.
    #[tokio::test]
    async fn slice5_batch_failure_is_typed_and_single_shot()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::Ordering::SeqCst;
        let cases: [fn() -> InferenceError; 7] = [
            || InferenceError::Auth("bad key".into()),
            || InferenceError::NotConfigured("missing key".into()),
            || InferenceError::Model("missing model".into()),
            || InferenceError::VisionUnsupported("no vision".into()),
            || InferenceError::Connection("offline".into()),
            || InferenceError::Overloaded("busy".into()),
            || InferenceError::Timeout("late".into()),
        ];
        for make_error in cases {
            let directory = fixture_directory()?;
            let request = fixture(
                &directory,
                &[prepared("qa-1", "user")],
                "OpenRouter/offline-model:batch",
            )?;
            let router = Arc::new(ControlledPort::new(Some(make_error), usize::MAX));
            let expected = map_qa_inference_error(make_error());
            let error = QaBatchService::new(router.clone())
                .generate_qa_batch(request)
                .await
                .expect_err("provider failure");
            assert_eq!(error.kind, expected.kind);
            assert!(error.message.contains(&expected.message));
            assert!(error.message.contains("partial QA output is preserved"));
            assert_eq!(router.calls.load(SeqCst), 1);
        }
        Ok(())
    }

    /// expect: Slice5 permanent inference errors get one call; typed transients get at most three.
    #[tokio::test]
    async fn slice5_typed_attempt_counts() -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::Ordering::SeqCst;
        let cases: [(fn() -> InferenceError, usize); 7] = [
            (|| InferenceError::Auth("bad key".into()), 1),
            (|| InferenceError::NotConfigured("missing key".into()), 1),
            (|| InferenceError::Model("missing model".into()), 1),
            (|| InferenceError::VisionUnsupported("no vision".into()), 1),
            (|| InferenceError::Connection("offline".into()), 3),
            (|| InferenceError::Overloaded("busy".into()), 3),
            (|| InferenceError::Timeout("late".into()), 3),
        ];
        for (error, expected) in cases {
            let directory = fixture_directory()?;
            let request = fixture(
                &directory,
                &[prepared("qa-1", "user")],
                "OpenRouter/offline-model",
            )?;
            let output = request.output.clone();
            let router = Arc::new(ControlledPort::new(Some(error), usize::MAX));
            let summary = QaBatchService::new(router.clone())
                .generate_qa_batch(request)
                .await?;
            assert_eq!(router.calls.load(SeqCst), expected, "{}", error());
            assert_eq!(summary["prompts_failed"], 1);
            let rows = records(&output)?;
            let message = rows.first().expect("failure row")["error"]
                .as_str()
                .expect("error");
            assert!(message.contains(&format!("after {expected}")), "{message}");
            assert!(message.contains(&error().to_string()), "{message}");
        }
        Ok(())
    }

    /// expect: [P8] Duplicate prompt identities must fail before generation, even for a shared chunk.
    #[test]
    fn duplicate_prompt_ids_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let directory = fixture_directory()?;
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
