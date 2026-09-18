//! Prepared QA candidate generation with AIMD-gated synchronous inference.
//!
//! Generation executes reviewed adjudication mandates only: reviewed passage
//! skips are terminal and zero-inference, and reviewed admits plan, write, and
//! validate against the ordered level mandates. It does not verify its own
//! claims: the external grounding manifest at ingestion owns acceptance.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use hkask_mcp_server::server::McpToolError;
use hkask_types::{ChatMessage, InferencePort, InferenceResult};

use crate::batch::{
    ADAPTIVE_CONCURRENCY_FLOOR, AdaptiveLimiter, MAX_RETRIES, inference_error_is_transient,
    retry_with_backoff,
};
use crate::helpers::map_corpus_io_error;
use crate::services::qa_adjudication::{
    QA_ADJUDICATION_PROTOCOL, ReviewedPassageDecision, ReviewedQaAdjudications,
    read_complete_adjudications,
};
use crate::services::qa_pipeline::{
    PreparedQaPrompt, QaCompletion, QaCompletionError, QaEnvelopeError, QaOutput,
    QaResponseMetadata, enforce_reviewed_adjudication, merge_disposition_plans,
    parse_disposition_plan_response, qa_llm_parameters, read_prompts,
    render_disposition_plan_messages, render_planned_qa_messages,
};

use crate::tools::semantic::qa::map_qa_inference_error;

// One registry across service instances and transports, not one mutex per call.
static QA_OUTPUTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

fn response_metadata(response: &InferenceResult) -> QaResponseMetadata {
    QaResponseMetadata {
        tokens_used: u64::from(response.usage.total_tokens),
        completion_tokens: Some(u64::from(response.usage.completion_tokens)),
        finish_reason: Some(response.finish_reason.clone()),
        cost_usd: response.cost_usd,
    }
}

fn qa_completion(
    response: InferenceResult,
    completed: Result<String, QaEnvelopeError>,
) -> QaCompletion {
    let (text, rejection) = match completed {
        Ok(text) => (text, None),
        // Rejections are recorded verbatim in output rows via Display.
        Err(error) => (String::new(), Some(error.to_string())),
    };
    QaCompletion {
        text,
        rejection,
        tokens_used: u64::from(response.usage.total_tokens),
        completion_tokens: Some(u64::from(response.usage.completion_tokens)),
        finish_reason: Some(response.finish_reason),
        cost_usd: response.cost_usd,
    }
}

async fn infer_with_retry_using(
    router: &Arc<dyn InferencePort>,
    limiter: &AdaptiveLimiter,
    selected_model: &str,
    parameters: hkask_types::template::LLMParameters,
    messages: &[ChatMessage],
    prompt_id: &str,
    phase: &str,
) -> Result<InferenceResult, QaCompletionError> {
    let mut attempts = 0;
    let retry_identity = format!("{prompt_id}:{phase}");
    retry_with_backoff(
        MAX_RETRIES,
        "hkask.mcp.docproc.qa_batch",
        &retry_identity,
        || {
            attempts += 1;
            async {
                let slot = limiter.acquire().await;
                let response = router
                    .generate_with_messages(messages, &parameters, Some(selected_model), None)
                    .await;
                match &response {
                    Ok(_) => slot.report_success(),
                    Err(error) if inference_error_is_transient(error) => slot.report_failure(),
                    Err(_) => {}
                }
                response
            }
        },
    )
    .await
    .map_err(|error| {
        QaCompletionError::LlmFailed(attempts, format!("{phase} inference failed: {error}"))
    })
}

async fn infer_with_retry(
    router: &Arc<dyn InferencePort>,
    limiter: &AdaptiveLimiter,
    selected_model: &str,
    messages: &[ChatMessage],
    prompt_id: &str,
    phase: &str,
) -> Result<InferenceResult, QaCompletionError> {
    infer_with_retry_using(
        router,
        limiter,
        selected_model,
        qa_llm_parameters(),
        messages,
        prompt_id,
        phase,
    )
    .await
}

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
    pub quality_adjudications_jsonl: String,
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
            quality_adjudications_jsonl,
            output,
            concurrency,
            model,
        } = request;
        let prompts = read_prompts(&prompts_jsonl)?;
        let adjudications = read_complete_adjudications(&quality_adjudications_jsonl, &prompts)?;
        let selected_model =
            hkask_inference::model_constants::resolve_qa_generation_model(model.as_deref())
                .map_err(map_qa_inference_error)?;

        let output_path = crate::path_safety::distinct_output_path(&prompts_jsonl, &output)?;
        crate::path_safety::distinct_output_path(&quality_adjudications_jsonl, &output)?;
        let lease = QaOutputLease::acquire(output_path)?;
        let file = std::fs::File::create(&lease.path).map_err(|error| {
            map_corpus_io_error(error, &format!("Cannot create output file '{output}'"))
        })?;
        lease.opened.store(true, Ordering::Relaxed);
        // No BufWriter: cancellation must not depend on an unchecked drop-time
        // flush to preserve completed rows. File errors surface at the write.
        let mut completions = QaOutput::new(file, prompts.len());
        completions.set_reviewed_adjudications(
            QA_ADJUDICATION_PROTOCOL,
            adjudications.passage_admits(),
            adjudications.passage_skips(),
            adjudications.level_generates(),
            adjudications.level_skips(),
        );
        self.generate_prepared(
            prompts,
            adjudications,
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
        adjudications: ReviewedQaAdjudications,
        selected_model: &str,
        limiter: AdaptiveLimiter,
        mut completions: QaOutput<W>,
        lease: Arc<QaOutputLease>,
        output: &str,
    ) -> Result<serde_json::Value, McpToolError> {
        let result = async {
            let adjudications = Arc::new(adjudications);
            let mut tasks = tokio::task::JoinSet::new();
            let mut pending = HashMap::with_capacity(prompts.len());
            for prompt in prompts {
                let reviewed_adjudication = adjudications
                    .decision(&prompt.prompt_id)
                    .cloned()
                    .ok_or_else(|| {
                        McpToolError::internal(format!(
                            "Reviewed adjudication manifest does not cover prompt '{}'",
                            prompt.prompt_id
                        ))
                    })?;
                if let ReviewedPassageDecision::Skip(reason) = reviewed_adjudication.passage() {
                    completions.complete_reviewed_skip(&prompt, reason, selected_model)?;
                    continue;
                }
                let router = Arc::clone(&self.inference_router);
                let limiter = limiter.clone();
                let selected_model = selected_model.to_owned();
                let task_lease = Arc::clone(&lease);
                let planning_messages = render_disposition_plan_messages(
                    &prompt,
                    &reviewed_adjudication,
                )?;
                let worker_prompt = prompt.clone();
                let prompt_id = prompt.prompt_id.clone();
                let task = tasks.spawn(async move {
                    // JoinSet abort is asynchronous: retain ownership until this
                    // worker actually drops, not merely until its abort is requested.
                    let _lease = task_lease;
                    let mut prior_responses = Vec::new();
                    let mut planning_response = match infer_with_retry(
                        &router,
                        &limiter,
                        &selected_model,
                        &planning_messages,
                        &prompt_id,
                        "QA disposition planning",
                    )
                    .await
                    {
                        Ok(response) => response,
                        Err(error) => return (prior_responses, Err(error)),
                    };
                    let proposed_response =
                        crate::extract_json_from_response(&planning_response.text);
                    let reviewed = &reviewed_adjudication;
                    let mut plan = parse_disposition_plan_response(
                        &proposed_response,
                        &worker_prompt,
                    )
                    .and_then(|plan| enforce_reviewed_adjudication(plan, reviewed));
                    if let Err(error) = &plan {
                        let exact_error = error.to_string();
                        prior_responses.push(response_metadata(&planning_response));
                        let mut correction_messages = planning_messages.clone();
                        correction_messages[0].content.push_str(&format!(
                            " Your plan violated the reviewed mandate: {exact_error}. Return one corrected plan only."
                        ));
                        planning_response = match infer_with_retry(
                            &router,
                            &limiter,
                            &selected_model,
                            &correction_messages,
                            &prompt_id,
                            "QA disposition mandate correction",
                        )
                        .await
                        {
                            Ok(response) => response,
                            Err(error) => return (prior_responses, Err(error)),
                        };
                        plan = parse_disposition_plan_response(
                            &crate::extract_json_from_response(&planning_response.text),
                            &worker_prompt,
                        )
                        .and_then(|plan| enforce_reviewed_adjudication(plan, reviewed));
                    }
                    let plan = match plan {
                        Ok(plan) => plan,
                        Err(error) => {
                            return (
                                prior_responses,
                                Ok(qa_completion(
                                    planning_response,
                                    Err(QaEnvelopeError::MandateRejected(error)),
                                )),
                            );
                        }
                    };
                    let writer_messages = match render_planned_qa_messages(&worker_prompt, &plan) {
                        Ok(messages) => messages,
                        Err(error) => {
                            prior_responses.push(response_metadata(&planning_response));
                            return (
                                prior_responses,
                                Err(QaCompletionError::Rejected(error.to_string())),
                            );
                        }
                    };
                    let Some(writer_messages) = writer_messages else {
                        return (
                            prior_responses,
                            Ok(qa_completion(
                                planning_response,
                                merge_disposition_plans(&plan, None),
                            )),
                        );
                    };
                    prior_responses.push(response_metadata(&planning_response));
                    let mut writer_response = match infer_with_retry(
                        &router,
                        &limiter,
                        &selected_model,
                        &writer_messages,
                        &prompt_id,
                        "QA writing",
                    )
                    .await
                    {
                        Ok(response) => response,
                        Err(error) => return (prior_responses, Err(error)),
                    };
                    let mut writer_draft =
                        crate::extract_json_from_response(&writer_response.text);
                    let mut completed = merge_disposition_plans(&plan, Some(&writer_draft));
                    if let Err(error) = &completed {
                        prior_responses.push(response_metadata(&writer_response));
                        let mut correction_messages = writer_messages.clone();
                        correction_messages[0].content.push_str(&format!(
                            " Your previous response failed the typed writer schema: {error}. Return one corrected outer array only."
                        ));
                        writer_response = match infer_with_retry(
                            &router,
                            &limiter,
                            &selected_model,
                            &correction_messages,
                            &prompt_id,
                            "QA writer schema correction",
                        )
                        .await
                        {
                            Ok(response) => response,
                            Err(error) => return (prior_responses, Err(error)),
                        };
                        writer_draft = crate::extract_json_from_response(&writer_response.text);
                        completed = merge_disposition_plans(&plan, Some(&writer_draft));
                    }
                    let writer_completed = match completed {
                        Ok(completed) => completed,
                        Err(error) => {
                            return (
                                prior_responses,
                                Ok(qa_completion(writer_response, Err(error))),
                            );
                        }
                    };
                    (
                        prior_responses,
                        Ok(qa_completion(writer_response, Ok(writer_completed))),
                    )
                });
                pending.insert(task.id(), prompt);
            }

            // JoinSet yields completion order and aborts remaining tasks if output
            // fails or the tool is cancelled. Keep metadata outside tasks so panics
            // still produce an identified failed-prompt record.
            while let Some(result) = tasks.join_next_with_id().await {
                let (identity, prior_responses, completion) = match result {
                    Ok((identity, (prior_responses, completion))) => {
                        (identity, prior_responses, completion)
                    }
                    Err(error) => (
                        error.id(),
                        Vec::new(),
                        Err(QaCompletionError::JoinFailed(error.to_string())),
                    ),
                };
                let prompt = pending.remove(&identity).ok_or_else(|| {
                    McpToolError::internal("QA task completed without prompt metadata")
                })?;
                completions.complete_with_prior(
                    &prompt,
                    completion,
                    &prior_responses,
                    selected_model,
                )?;
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
        SkipConceptual,
        Malformed,
        WriterMalformed,
        WriterMalformedOnce,
        ReviewedGenerateMismatchOnce,
        ReviewedGenerateMismatchAlways,
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
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(messages.len(), 2);
            let is_planning = messages[0].content.contains("disposition plan");
            let is_mandate_correction = messages[0]
                .content
                .contains("Your plan violated the reviewed mandate:");
            assert!(!parameters.thinking_allowed);
            assert_eq!(model, Some("OpenRouter/offline-model"));
            assert!(tools.is_none());
            let mode = self.mode;
            if is_mandate_correction
                && matches!(
                    mode,
                    Mode::ReviewedGenerateMismatchOnce | Mode::ReviewedGenerateMismatchAlways
                )
            {
                assert!(messages[0].content.contains(
                    "reviewed mandate for level 1 requires generate relation Some(\"mechanism\"), generator returned skip reason 'conceptual_support_absent'"
                ));
            }
            if matches!(mode, Mode::WriterMalformedOnce) && !is_planning && call == 2 {
                assert!(
                    messages[0]
                        .content
                        .contains("failed the typed writer schema")
                );
            }
            Box::pin(async move {
                if matches!(mode, Mode::Pending) {
                    return std::future::pending().await;
                }
                let malformed_writer = !is_planning;
                let text = if matches!(mode, Mode::Malformed)
                    || matches!(mode, Mode::WriterMalformed) && malformed_writer
                    || matches!(mode, Mode::WriterMalformedOnce) && malformed_writer && call == 1
                {
                    "[".to_string()
                } else if is_planning {
                    let skip_conceptual = matches!(mode, Mode::SkipConceptual)
                        || matches!(mode, Mode::ReviewedGenerateMismatchAlways)
                        || matches!(mode, Mode::ReviewedGenerateMismatchOnce)
                            && !is_mandate_correction;
                    let conceptual = if skip_conceptual {
                        json!({"level":"conceptual","disposition":"skip","relation":null,"reason":"conceptual_support_absent","evidence_ids":[]})
                    } else {
                        json!({"level":"conceptual","disposition":"generate","relation":"mechanism","reason":null,"evidence_ids":["e0"]})
                    };
                    json!([
                        "clean",
                        [
                            {"level":"factual","disposition":"generate","relation":null,"reason":null,"evidence_ids":["e0"]},
                            conceptual
                        ]
                    ])
                    .to_string()
                } else if matches!(mode, Mode::SkipConceptual) {
                    json!([{"level":"factual","question":"What is grounded?","answer":"Grounded answer one."}]).to_string()
                } else {
                    json!([
                        {"level":"factual","question":"What is grounded?","answer":"Grounded answer one."},
                        {"level":"conceptual","question":"Why is it grounded?","answer":"Grounded answer two."}
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
                        reported: true,
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
        // Generation requires a complete manifest: the default fixture admits
        // every prompt and mandates generation for both ordered levels.
        let adjudications = directory.path().join("adjudications.jsonl");
        let mut manifest = String::new();
        for prompt in prompts {
            manifest.push_str(
                &serde_json::to_string(&json!({
                    "protocol": QA_ADJUDICATION_PROTOCOL,
                    "prompt_id": prompt.prompt_id,
                    "chunk_ref": prompt.primary().chunk_ref,
                    "source": prompt.primary().source,
                    "passage": {"decision":"admit","reason":null},
                    "levels": [
                        {"level":"factual","decision":"generate","relation":null,"reason":null},
                        {"level":"conceptual","decision":"generate","relation":"mechanism","reason":null}
                    ],
                }))?,
            );
            manifest.push('\n');
        }
        std::fs::write(&adjudications, manifest)?;
        Ok((
            directory,
            QaBatchRequest {
                prompts_jsonl: input.to_string_lossy().into_owned(),
                quality_adjudications_jsonl: adjudications.to_string_lossy().into_owned(),
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

    fn attach_adjudication_row(
        directory: &tempfile::TempDir,
        request: &mut QaBatchRequest,
        row: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path = directory.path().join("adjudications.jsonl");
        std::fs::write(&path, format!("{}\n", serde_json::to_string(&row)?))?;
        request.quality_adjudications_jsonl = path.to_string_lossy().into_owned();
        Ok(())
    }

    fn attach_adjudication(
        directory: &tempfile::TempDir,
        request: &mut QaBatchRequest,
        decision: &str,
        reason: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let levels = if decision == "skip" {
            json!([
                {"level":"factual","decision":"skip","relation":null,"reason":reason},
                {"level":"conceptual","decision":"skip","relation":null,"reason":reason}
            ])
        } else {
            json!([
                {"level":"factual","decision":"generate","relation":null,"reason":null},
                {"level":"conceptual","decision":"generate","relation":"mechanism","reason":null}
            ])
        };
        attach_adjudication_row(
            directory,
            request,
            json!({
                "protocol": QA_ADJUDICATION_PROTOCOL,
                "prompt_id": "qa-1",
                "chunk_ref": "chunk-qa-1",
                "source": "source.txt",
                "passage": {"decision":decision,"reason":reason},
                "levels": levels,
            }),
        )
    }

    fn attach_conceptual_skip_adjudication(
        directory: &tempfile::TempDir,
        request: &mut QaBatchRequest,
    ) -> Result<(), Box<dyn std::error::Error>> {
        attach_adjudication_row(
            directory,
            request,
            json!({
                "protocol": QA_ADJUDICATION_PROTOCOL,
                "prompt_id": "qa-1",
                "chunk_ref": "chunk-qa-1",
                "source": "source.txt",
                "passage": {"decision":"admit","reason":null},
                "levels": [
                    {"level":"factual","decision":"generate","relation":null,"reason":null},
                    {"level":"conceptual","decision":"skip","relation":null,"reason":"conceptual_support_absent"}
                ],
            }),
        )
    }

    /// expect: Generation admits the generator's named-object drafts as candidates
    /// with canonical evidence, marks them pending external verification, and
    /// reports every prompt once.
    #[tokio::test]
    async fn generates_candidate_rows_and_truthful_summary()
    -> Result<(), Box<dyn std::error::Error>> {
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
        assert_eq!(summary["grounding_status"], "pending_external_verification");
        assert_eq!(summary["provider_responses"], 4);
        assert_eq!(summary["tokens_used"], 40);
        assert!(
            (summary["reported_cost_usd"]
                .as_f64()
                .expect("reported cost")
                - 0.04)
                .abs()
                < 1e-12
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 4);
        let rows = records(&output)?;
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row["source"] == "source.txt"));
        assert!(rows.iter().all(|row| {
            row["provenance"]["grounding_status"] == "pending_external_verification"
        }));
        assert!(rows.iter().all(|row| {
            row["response"]["evidence_quotes"][0]["chunk_ref"]
                .as_str()
                .is_some_and(|chunk| chunk.starts_with("chunk-qa-"))
        }));
        Ok(())
    }

    /// expect: A reviewed skip completes without spending any inference.
    #[tokio::test]
    async fn reviewed_skip_is_enforced_before_inference() -> Result<(), Box<dyn std::error::Error>>
    {
        let (directory, mut request) = fixture(&[prompt("qa-1")])?;
        attach_adjudication(
            &directory,
            &mut request,
            "skip",
            Some("contaminated_or_garbled"),
        )?;
        let output = request.output.clone();
        let port = Arc::new(StubPort::new(Mode::Pending));
        let summary = QaBatchService::new(port.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["qa_rows_written"], 0);
        assert_eq!(summary["qa_levels_skipped"], 2);
        assert_eq!(summary["provider_responses"], 0);
        assert_eq!(summary["reviewed_passage_skips"], 1);
        assert_eq!(summary["reviewed_level_generates"], 0);
        assert_eq!(summary["reviewed_level_skips"], 2);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
        let rows = records(&output)?;
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter().all(|row| {
                row["provenance"]["adjudication_protocol"] == QA_ADJUDICATION_PROTOCOL
            })
        );
        Ok(())
    }

    /// expect: A reviewed admit generates candidates per the reviewed level
    /// mandates and the summary accounts every reviewed decision.
    #[tokio::test]
    async fn reviewed_admit_generates_under_reviewed_mandates()
    -> Result<(), Box<dyn std::error::Error>> {
        let (directory, mut request) = fixture(&[prompt("qa-1")])?;
        attach_adjudication(&directory, &mut request, "admit", None)?;
        let output = request.output.clone();
        let port = Arc::new(StubPort::new(Mode::Success));
        let summary = QaBatchService::new(port.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["qa_rows_written"], 2);
        assert_eq!(summary["qa_levels_skipped"], 0);
        assert_eq!(summary["reviewed_passage_admits"], 1);
        assert_eq!(summary["reviewed_level_generates"], 2);
        assert_eq!(summary["reviewed_level_skips"], 0);
        assert_eq!(summary["provider_responses"], 2);
        assert_eq!(port.calls.load(Ordering::SeqCst), 2);
        let rows = records(&output)?;
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.get("response").is_some()));
        Ok(())
    }

    /// expect: A generator plan cannot turn a reviewed conceptual generate mandate into a skip;
    /// the exact deterministic mismatch is returned once for generator-owned correction.
    #[tokio::test]
    async fn reviewed_conceptual_generate_mismatch_is_corrected_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let (directory, mut request) = fixture(&[prompt("qa-1")])?;
        attach_adjudication(&directory, &mut request, "admit", None)?;
        let output = request.output.clone();
        let port = Arc::new(StubPort::new(Mode::ReviewedGenerateMismatchOnce));
        let summary = QaBatchService::new(port.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_succeeded"], 1);
        assert_eq!(summary["qa_rows_written"], 2);
        assert_eq!(summary["qa_levels_skipped"], 0);
        assert_eq!(summary["provider_responses"], 3);
        assert_eq!(port.calls.load(Ordering::SeqCst), 3);
        let rows = records(&output)?;
        assert!(rows.iter().all(|row| row.get("response").is_some()));
        Ok(())
    }

    /// expect: Repeating the same reviewed-mandate mismatch exhausts the single
    /// generator correction and fails the prompt without terminal skip rows.
    #[tokio::test]
    async fn second_reviewed_generate_mismatch_fails_prompt()
    -> Result<(), Box<dyn std::error::Error>> {
        let (directory, mut request) = fixture(&[prompt("qa-1")])?;
        attach_adjudication(&directory, &mut request, "admit", None)?;
        let output = request.output.clone();
        let port = Arc::new(StubPort::new(Mode::ReviewedGenerateMismatchAlways));
        let summary = QaBatchService::new(port.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_failed"], 1);
        assert_eq!(summary["qa_rows_written"], 0);
        assert_eq!(summary["qa_levels_skipped"], 0);
        assert_eq!(summary["provider_responses"], 2);
        assert_eq!(port.calls.load(Ordering::SeqCst), 2);
        let rows = records(&output)?;
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0]["error"]
                .as_str()
                .is_some_and(|error| error.contains("QA disposition mandate rejected"))
        );
        Ok(())
    }

    /// expect: A reviewed conceptual skip mandate cannot be converted into generation;
    /// repeated generator disagreement fails before writer or QA-verifier calls.
    #[tokio::test]
    async fn reviewed_conceptual_skip_cannot_generate() -> Result<(), Box<dyn std::error::Error>> {
        let (directory, mut request) = fixture(&[prompt("qa-1")])?;
        attach_conceptual_skip_adjudication(&directory, &mut request)?;
        let output = request.output.clone();
        let port = Arc::new(StubPort::new(Mode::Success));
        let summary = QaBatchService::new(port.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_failed"], 1);
        assert_eq!(summary["qa_rows_written"], 0);
        assert_eq!(summary["qa_levels_skipped"], 0);
        assert_eq!(summary["reviewed_level_generates"], 1);
        assert_eq!(summary["reviewed_level_skips"], 1);
        assert_eq!(summary["provider_responses"], 2);
        assert_eq!(port.calls.load(Ordering::SeqCst), 2);
        let rows = records(&output)?;
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["error"].as_str().is_some_and(|error| {
            error.contains("requires skip reason 'conceptual_support_absent'")
        }));
        Ok(())
    }

    /// expect: A reviewed conceptual skip mandate retains factual QA while the
    /// unsupported conceptual level becomes a terminal skip before writing.
    #[tokio::test]
    async fn level_plan_skips_unsupported_conceptual_before_writing()
    -> Result<(), Box<dyn std::error::Error>> {
        let (directory, mut request) = fixture(&[prompt("qa-1")])?;
        attach_conceptual_skip_adjudication(&directory, &mut request)?;
        let output = request.output.clone();
        let port = Arc::new(StubPort::new(Mode::SkipConceptual));
        let summary = QaBatchService::new(port.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_succeeded"], 1);
        assert_eq!(summary["qa_rows_written"], 1);
        assert_eq!(summary["qa_levels_skipped"], 1);
        assert_eq!(summary["provider_responses"], 2);
        assert_eq!(port.calls.load(Ordering::SeqCst), 2);
        let rows = records(&output)?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["qa_type"], "factual");
        assert_eq!(
            rows[0]["response"]["evidence_quotes"][0]["chunk_ref"],
            "chunk-qa-1"
        );
        assert_eq!(rows[1]["qa_type"], "conceptual");
        assert_eq!(rows[1]["status"], "skipped");
        assert_eq!(rows[1]["reason"], "conceptual_support_absent");
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

    /// expect: One malformed writer response gets one fully metered schema correction
    /// and recovers without partial rows.
    #[tokio::test]
    async fn writer_schema_correction_recovers_without_partial_rows()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, request) = fixture(&[prompt("qa-1")])?;
        let output = request.output.clone();
        let port = Arc::new(StubPort::new(Mode::WriterMalformedOnce));
        let summary = QaBatchService::new(port.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_succeeded"], 1);
        assert_eq!(summary["prompts_failed"], 0);
        assert_eq!(summary["qa_rows_written"], 2);
        assert_eq!(summary["provider_responses"], 3);
        assert_eq!(port.calls.load(Ordering::SeqCst), 3);
        assert_eq!(records(&output)?.len(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn malformed_writer_is_an_explicit_prompt_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, request) = fixture(&[prompt("qa-1")])?;
        let output = request.output.clone();
        let port = Arc::new(StubPort::new(Mode::WriterMalformed));
        let summary = QaBatchService::new(port.clone())
            .generate_qa_batch(request)
            .await?;
        assert_eq!(summary["prompts_succeeded"], 0);
        assert_eq!(summary["prompts_failed"], 1);
        assert_eq!(summary["qa_rows_written"], 0);
        assert_eq!(summary["provider_responses"], 3);
        assert_eq!(port.calls.load(Ordering::SeqCst), 3);
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
            quality_adjudications_jsonl: request.quality_adjudications_jsonl.clone(),
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
