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

    /// Validate the entire compact input and open output before inference. Both
    /// transports render identical local-identity messages; accounting is shared.
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
                            completion_tokens: Some(u64::from(response.usage.completion_tokens)),
                            finish_reason: Some(response.finish_reason),
                            cost_usd: response.cost_usd,
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
            completions.finish(output)
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
    use crate::services::qa_pipeline::{PREPARED_QA_PROTOCOL, PreparedQaPassage};
    use crate::tools::corpus::QaType;
    use hkask_types::template::LLMParameters;
    use hkask_types::{
        ChatMessage, ChatToolDefinition, InferenceError, InferencePort, InferenceResult,
        InferenceUsage,
    };
    use serde_json::json;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};

    type Reply<'a> =
        Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>>;

    #[derive(Clone, Copy)]
    enum Mode {
        Success,
        Malformed,
        Pending,
    }

    struct StubPort {
        mode: Mode,
        calls: AtomicUsize,
    }

    impl StubPort {
        fn new(mode: Mode) -> Self {
            Self {
                mode,
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl InferencePort for StubPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Reply<'_> {
            panic!("prepared QA must use role-aware messages")
        }

        fn generate_with_messages(
            &self,
            messages: &[ChatMessage],
            parameters: &LLMParameters,
            model: Option<&str>,
            tools: Option<&[ChatToolDefinition]>,
        ) -> Reply<'_> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(messages.len(), 2);
            assert!(!parameters.thinking_allowed);
            assert_eq!(model, Some("OpenRouter/offline-model"));
            assert!(tools.is_none());
            let mode = self.mode;
            Box::pin(async move {
                if matches!(mode, Mode::Pending) {
                    return std::future::pending().await;
                }
                let text = if matches!(mode, Mode::Malformed) {
                    "[".to_string()
                } else {
                    json!([
                        [
                            "factual",
                            "What is grounded?",
                            "Grounded answer one.",
                            [["p0", "Grounded answer one."]]
                        ],
                        [
                            "conceptual",
                            "Why is it grounded?",
                            "Grounded answer two.",
                            [["p0", "Grounded answer two."]]
                        ]
                    ])
                    .to_string()
                };
                Ok(InferenceResult {
                    text,
                    model: "OpenRouter/offline-model".into(),
                    usage: InferenceUsage {
                        prompt_tokens: 4,
                        completion_tokens: 6,
                        total_tokens: 10,
                    },
                    finish_reason: "stop".into(),
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: Some(0.01),
                })
            })
        }
    }

    fn prompt(id: &str) -> PreparedQaPrompt {
        PreparedQaPrompt {
            prompt_id: id.into(),
            protocol: PREPARED_QA_PROTOCOL.into(),
            passages: vec![PreparedQaPassage {
                local_id: "p0".into(),
                chunk_ref: format!("chunk-{id}"),
                source: "source.txt".into(),
                text: "Grounded answer one. Grounded answer two.".into(),
            }],
            candidate_terms: vec!["grounded answer".into()],
            qa_types: vec![QaType::Factual, QaType::Conceptual],
        }
    }

    fn fixture(
        prompts: &[PreparedQaPrompt],
    ) -> Result<(tempfile::TempDir, QaBatchRequest), Box<dyn std::error::Error>> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-batch-test");
        std::fs::create_dir_all(&root)?;
        let directory = tempfile::Builder::new().prefix("case-").tempdir_in(root)?;
        let input = directory.path().join("prompts.jsonl");
        let mut body = String::new();
        for prompt in prompts {
            body.push_str(&serde_json::to_string(prompt)?);
            body.push('\n');
        }
        std::fs::write(&input, body)?;
        Ok((
            directory,
            QaBatchRequest {
                prompts_jsonl: input.to_string_lossy().into_owned(),
                output: input
                    .with_file_name("generated.jsonl")
                    .to_string_lossy()
                    .into_owned(),
                concurrency: 2,
                model: Some("OpenRouter/offline-model".into()),
            },
        ))
    }

    fn records(path: &str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
        std::fs::read_to_string(path)?
            .lines()
            .map(|line| serde_json::from_str(line).map_err(Into::into))
            .collect()
    }

    /// expect: Prepared generation restores canonical evidence and reports every prompt once.
    #[tokio::test]
    async fn generates_verified_rows_and_truthful_summary() -> Result<(), Box<dyn std::error::Error>>
    {
        let prompts = [prompt("qa-1"), prompt("qa-2")];
        let (_directory, request) = fixture(&prompts)?;
        let output = request.output.clone();
        let port = Arc::new(StubPort::new(Mode::Success));
        let summary = QaBatchService::new(port.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_total"], 2);
        assert_eq!(summary["prompts_succeeded"], 2);
        assert_eq!(summary["prompts_failed"], 0);
        assert_eq!(summary["qa_rows_written"], 4);
        assert_eq!(summary["provider_responses"], 2);
        assert_eq!(summary["reported_cost_usd"], 0.02);
        assert_eq!(port.calls.load(Ordering::SeqCst), 2);
        let rows = records(&output)?;
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row["source"] == "source.txt"));
        assert!(rows.iter().all(|row| {
            row["response"]["evidence_quotes"][0]["chunk_ref"]
                .as_str()
                .is_some_and(|chunk| chunk.starts_with("chunk-qa-"))
        }));
        Ok(())
    }

    /// expect: Invalid prepared input and input/output aliases fail before inference or truncation.
    #[tokio::test]
    async fn preflight_preserves_inputs_and_spends_no_inference()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, mut request) = fixture(&[prompt("qa-1"), prompt("qa-1")])?;
        let port = Arc::new(StubPort::new(Mode::Success));
        assert!(
            QaBatchService::new(port.clone())
                .generate_qa_batch(request)
                .await
                .is_err()
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);

        let (_directory, alias_request) = fixture(&[prompt("qa-1")])?;
        request = QaBatchRequest {
            output: alias_request.prompts_jsonl.clone(),
            ..alias_request
        };
        let original = std::fs::read(&request.prompts_jsonl)?;
        assert!(
            QaBatchService::new(port.clone())
                .generate_qa_batch(request)
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(&_directory.path().join("prompts.jsonl"))?,
            original
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
        Ok(())
    }

    /// expect: A malformed provider response becomes one identified failure row, never partial QA.
    #[tokio::test]
    async fn malformed_response_is_an_explicit_prompt_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, request) = fixture(&[prompt("qa-1")])?;
        let output = request.output.clone();
        let summary = QaBatchService::new(Arc::new(StubPort::new(Mode::Malformed)))
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_succeeded"], 0);
        assert_eq!(summary["prompts_failed"], 1);
        assert_eq!(summary["qa_rows_written"], 0);
        let rows = records(&output)?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["prompt_id"], "qa-1");
        assert!(rows[0].get("response").is_none());
        Ok(())
    }

    /// expect: One canonical output has one live owner until its worker is destroyed.
    #[tokio::test]
    async fn active_output_rejects_a_competing_writer() -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, request) = fixture(&[prompt("qa-1")])?;
        let competing = QaBatchRequest {
            prompts_jsonl: request.prompts_jsonl.clone(),
            output: request.output.clone(),
            concurrency: 1,
            model: request.model.clone(),
        };
        let pending = Arc::new(StubPort::new(Mode::Pending));
        let running = tokio::spawn({
            let pending = pending.clone();
            async move {
                QaBatchService::new(pending)
                    .generate_qa_batch(request)
                    .await
            }
        });
        while pending.calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        let error = QaBatchService::new(Arc::new(StubPort::new(Mode::Success)))
            .generate_qa_batch(competing)
            .await
            .expect_err("competing writer");
        assert_eq!(error.kind, hkask_types::McpErrorKind::FailedPrecondition);
        running.abort();
        let error = running.await.expect_err("aborted owner task");
        assert!(error.is_cancelled());
        Ok(())
    }
}
