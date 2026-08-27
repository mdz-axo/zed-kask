//! QA batch service — concurrent QA generation from a prompts JSONL file.
//!
//! Extracted from `CorpusServer::corpus_generate_qa_batch` in
//! `tools/semantic.rs`. The `#[tool]` method becomes thin I/O framing:
//! deserialize params, construct the service, delegate.
//!
//! Two execution paths, selected by model eligibility:
//! - Batch API (OpenRouter `:batch` suffix / DeepInfra prefix) — 20–50% cost
//!   discount, no rate limits. Delegates to `tools/semantic/batch_api.rs`.
//! - Concurrent synchronous IPC with semaphore-gated `tokio::spawn` per
//!   prompt, retry with backoff, incremental JSONL output.

use std::io::Write;
use std::sync::Arc;

use hkask_mcp_server::server::McpToolError;
use hkask_types::InferencePort;
use serde_json::json;

use crate::batch::{BatchOutcome, MAX_RETRIES, retry_with_backoff};
use crate::helpers::{map_corpus_io_error, read_jsonl};
use crate::services::qa_pipeline;
use crate::tools::semantic::batch_api::generate_qa_via_batch_api;
use crate::tools::semantic::qa::{
    BatchQaPrompt, configured_qa_model, parse_qa_response, write_qa_result,
};
use crate::{Mutex, extract_json_from_response};

/// Input for [`QaBatchService::generate_qa_batch`].
pub(crate) struct QaBatchRequest {
    pub prompts_jsonl: String,
    pub output: String,
    pub concurrency: usize,
    pub model: Option<String>,
}

/// Concurrent QA generation from a prompts JSONL file.
pub struct QaBatchService {
    inference_router: Arc<dyn InferencePort>,
}

impl QaBatchService {
    pub fn new(inference_router: Arc<dyn InferencePort>) -> Self {
        Self { inference_router }
    }

    /// Generate QA pairs for every prompt in the JSONL file.
    ///
    /// Routes through the provider Batch API when the selected model is
    /// batch-eligible; otherwise fans out concurrent synchronous calls with
    /// retry and incremental output. Returns a summary envelope.
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

        let prompts_vec = read_prompts(&prompts_jsonl)?;
        let total = prompts_vec.len();
        let selected_model = configured_qa_model(model);

        // When the model is batch-eligible (OpenRouter `:batch` suffix or
        // DeepInfra prefix), route through the shared batch API in
        // `hkask-inference::batch` instead of N concurrent synchronous
        // IPC calls. This gives a 20–50% cost discount and no rate limits.
        //
        // Pass the ORIGINAL model string (with `:batch` suffix or
        // `DeepInfra/` prefix) to `generate_batch` — the bridge calls
        // `detect_batch_provider` again to strip the prefix and select
        // the provider. Stripping here would cause the bridge's
        // `detect_batch_provider` to return `None` (no `:batch` suffix,
        // no `DeepInfra/` prefix) and fail with "not batch-eligible".
        if let Some(ref model_str) = selected_model {
            if hkask_inference::batch::detect_batch_provider(model_str).is_some() {
                return generate_qa_via_batch_api(
                    &self.inference_router,
                    prompts_vec,
                    model_str,
                    &output,
                    total,
                )
                .await;
            }
        }

        // Concurrent processing with configurable semaphore
        let sem = Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));
        let router = Arc::clone(&self.inference_router);

        // Output file writer (with incremental flush every 10 completions)
        let output_path = crate::path_safety::contain_for_write(&output)?;
        let file = std::fs::File::create(&output_path).map_err(|e| {
            map_corpus_io_error(e, &format!("Cannot create output file '{output}'"))
        })?;
        let output_writer = Arc::new(Mutex::new(std::io::BufWriter::new(file)));
        let write_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // B5 fix: track failed prompts so the outcome can be classified as
        // degraded when the failure rate exceeds the threshold.
        let failed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let mut handles = Vec::with_capacity(total);
        for prompt in prompts_vec {
            let router = Arc::clone(&router);
            let sem = Arc::clone(&sem);
            let selected_model = selected_model.clone();
            let output_writer = Arc::clone(&output_writer);
            let write_count = Arc::clone(&write_count);
            let failed_count = Arc::clone(&failed_count);

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await;

                let params = qa_pipeline::qa_llm_parameters();
                let levels = prompt
                    .bloom_levels
                    .clone()
                    .unwrap_or_else(qa_pipeline::default_bloom_levels);
                let levels_str = levels.join(", ");
                let formatted = qa_pipeline::format_single_chunk_prompt(
                    &levels_str,
                    &prompt.chunk_id,
                    &prompt.text,
                );
                let (prompt_text, template_source) = (formatted.text, formatted.template_source);
                let response = match retry_with_backoff(
                    MAX_RETRIES,
                    "hkask.mcp.docproc.qa_batch",
                    &prompt.chunk_id,
                    || {
                        router.generate_with_model(
                            &prompt_text,
                            &params,
                            selected_model.as_deref(),
                            None,
                        )
                    },
                )
                .await
                {
                    Ok(resp) => resp,
                    Err(e) => {
                        let result = json!({"chunk_id": prompt.chunk_id, "error": format!("LLM failed after {} retries: {}", MAX_RETRIES, e)});
                        failed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        write_qa_result(&result, &output_writer, &write_count);
                        return;
                    }
                };
                let content = &response.text;
                match parse_qa_response(&extract_json_from_response(content), &levels, None) {
                    Ok(qa_response) => {
                        // Write one JSONL line per QA pair in envelope format
                        // (matches what corpus_ingest_qa's parse_qa_record expects)
                        for pair in qa_response.qa_pairs {
                            let result = qa_pipeline::qa_result_envelope(
                                &prompt,
                                pair,
                                selected_model.as_deref().unwrap_or("router_default"),
                                template_source,
                                response.usage.total_tokens,
                            );
                            write_qa_result(&result, &output_writer, &write_count);
                        }
                    }
                    Err(e) => {
                        failed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let result = qa_pipeline::qa_error_envelope(
                            &prompt.chunk_id,
                            &format!("QA response rejected: {e}"),
                        );
                        write_qa_result(&result, &output_writer, &write_count);
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            if let Err(join_err) = handle.await {
                tracing::warn!(
                    target: "hkask.mcp.docproc.qa_batch",
                    error = %join_err,
                    "QA batch task join failed"
                );
            }
        }

        {
            let mut w = output_writer.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = w.flush() {
                tracing::warn!(
                    target: "hkask.mcp.docproc.qa_batch",
                    error = %e,
                    "failed to flush QA batch output writer"
                );
            }
        }
        let written = write_count.load(std::sync::atomic::Ordering::Relaxed);
        let failed = failed_count.load(std::sync::atomic::Ordering::Relaxed);
        let result = json!({
            "total": total,
            "written": written,
            "failed": failed,
            "output": output,
        });
        // B5 fix: report degraded outcome when failure rate exceeds threshold.
        let outcome = BatchOutcome::from_counts(failed, total);
        outcome.log_if_degraded("hkask.mcp.docproc.qa_batch", "QA batch");
        Ok(result)
    }
}

/// Read prompts from a JSONL file, mapping `build_prompts` output fields to
/// `BatchQaPrompt` (chunk_ref → chunk_id, system+user → text, qa_type →
/// bloom_levels). Fails when no valid prompts are found.
fn read_prompts(path: &str) -> Result<Vec<BatchQaPrompt>, McpToolError> {
    let prompts_values = read_jsonl::<serde_json::Value>(path, "prompts_jsonl")?;
    let mut prompts_vec: Vec<BatchQaPrompt> = Vec::new();
    for v in prompts_values {
        let chunk_id = v
            .get("chunk_ref")
            .and_then(|v| v.as_str())
            .or_else(|| v.get("chunk_id").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();
        let system = v.get("system").and_then(|v| v.as_str()).unwrap_or("");
        let user = v.get("user").and_then(|v| v.as_str()).unwrap_or("");
        let text = if system.is_empty() && user.is_empty() {
            v.get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            format!("{system}\n\n{user}")
        };
        let bloom_levels = v
            .get("qa_type")
            .and_then(|v| v.as_str())
            .map(|qt| vec![qt.to_string()])
            .or_else(|| {
                v.get("bloom_levels").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
            });
        let source = v
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let concepts = v
            .get("concepts")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        prompts_vec.push(BatchQaPrompt {
            text,
            chunk_id,
            bloom_levels,
            source,
            concepts,
        });
    }

    if prompts_vec.is_empty() {
        return Err(McpToolError::invalid_argument(
            "prompts_jsonl contains no valid prompts",
        ));
    }

    Ok(prompts_vec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp_jsonl(name: &str, lines: &[serde_json::Value]) -> String {
        // Path containment (path_safety) rejects /tmp — fixtures must live
        // under the crate root. Use a scratch dir inside target/ so cargo
        // gitignores it.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("qa-batch-test");
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join(format!("{name}.jsonl"));
        let body: String = lines
            .iter()
            .map(|v| serde_json::to_string(v).expect("serialize"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, body).expect("write temp file");
        path.to_string_lossy().to_string()
    }

    #[test]
    fn read_prompts_maps_build_prompts_fields() {
        // The canonical build_prompts output shape: chunk_ref + system/user +
        // qa_type. All three must be aliased into BatchQaPrompt.
        let line = json!({
            "chunk_ref": "corpus:doc:1",
            "system": "You generate QA pairs.",
            "user": "Chunk text here.",
            "qa_type": "analyze",
            "source": "doc.pdf.txt",
            "concepts": ["ROIC", "moat"],
        });
        let path = write_temp_jsonl("canonical", &[line]);
        let prompts = read_prompts(&path).expect("parse");
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].chunk_id, "corpus:doc:1");
        assert_eq!(
            prompts[0].text,
            "You generate QA pairs.\n\nChunk text here."
        );
        assert_eq!(
            prompts[0].bloom_levels.as_deref(),
            Some(&["analyze".to_string()][..])
        );
        assert_eq!(prompts[0].source, "doc.pdf.txt");
        assert_eq!(prompts[0].concepts, vec!["ROIC", "moat"]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_prompts_accepts_preformatted_text_and_bloom_levels() {
        // The alternative shape: chunk_id + combined text + bloom_levels array.
        let line = json!({
            "chunk_id": "c2",
            "text": "Pre-formatted prompt.",
            "bloom_levels": ["remember", "apply"],
        });
        let path = write_temp_jsonl("preformatted", &[line]);
        let prompts = read_prompts(&path).expect("parse");
        assert_eq!(prompts[0].chunk_id, "c2");
        assert_eq!(prompts[0].text, "Pre-formatted prompt.");
        assert_eq!(
            prompts[0].bloom_levels,
            Some(vec!["remember".to_string(), "apply".to_string()])
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_prompts_empty_file_is_invalid_argument() {
        let path = write_temp_jsonl("empty", &[]);
        let err = read_prompts(&path).expect_err("must fail on empty");
        assert!(format!("{err}").contains("no valid prompts"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_prompts_missing_fields_default() {
        // A line with no recognizable fields still yields a prompt with
        // empty defaults — lenient by design (malformed lines are the
        // caller's data-quality problem, surfaced downstream).
        let line = json!({"unrelated": true});
        let path = write_temp_jsonl("minimal", &[line]);
        let prompts = read_prompts(&path).expect("parse");
        assert_eq!(prompts[0].chunk_id, "");
        assert_eq!(prompts[0].text, "");
        assert!(prompts[0].bloom_levels.is_none());
        let _ = std::fs::remove_file(&path);
    }
}
