//! Batch API QA generation — routes through the IPC bridge to the provider's
//! Batch API (OpenRouter or DeepInfra) for a 20–50% cost discount.
//!
//! Extracted from `tools/semantic.rs` to isolate the batch API concern from
//! the synchronous QA generation paths. The method is a free function that
//! takes the inference router and the prompt/output parameters.

use std::io::Write;
use std::sync::Arc;

use hkask_types::InferencePort;
use hkask_types::inference_ipc::BatchPromptEntry;
use serde_json::json;

use crate::batch::BatchOutcome;
use crate::helpers::map_corpus_io_error;
use crate::services::qa_pipeline;
use crate::tools::semantic::qa::{BatchQaPrompt, parse_qa_response, write_qa_result};
use crate::{McpToolError, extract_json_from_response};

/// Generate QA pairs via the batch API through the IPC bridge.
///
/// Sends all prompts as a single `GenerateBatch` IPC request to the zed
/// process, which holds the API keys and handles submission to the
/// provider's Batch API (OpenRouter or DeepInfra). The corpus server
/// never sees the credentials.
///
/// `model` is the ORIGINAL model string (e.g. `z-ai/glm-5.2:batch` or
/// `DeepInfra/Qwen/Qwen3-Embedding-0.6B`) — the bridge calls
/// `detect_batch_provider` to strip the prefix and select the provider.
pub(crate) async fn generate_qa_via_batch_api(
    inference_router: &Arc<dyn InferencePort>,
    prompts_vec: Vec<BatchQaPrompt>,
    model: &str,
    output: &str,
    total: usize,
) -> Result<serde_json::Value, McpToolError> {
    // Format prompts as IPC batch entries
    let batch_prompts: Vec<BatchPromptEntry> = prompts_vec
        .iter()
        .map(|p| {
            let levels = p
                .bloom_levels
                .clone()
                .unwrap_or_else(qa_pipeline::default_bloom_levels);
            let levels_str = levels.join(", ");
            let user_text = qa_pipeline::format_batch_user_text(&levels_str, &p.chunk_id, &p.text);
            BatchPromptEntry {
                custom_id: p.chunk_id.clone(),
                system: qa_pipeline::BATCH_SYSTEM_PROMPT.to_string(),
                user: user_text,
            }
        })
        .collect();

    // Send through the IPC bridge — zed holds the API keys
    let batch_results = inference_router
        .generate_batch(model, &batch_prompts, 2000, 0.3)
        .await
        .map_err(|e| McpToolError::internal(format!("Batch API IPC failed: {e}")))?;

    // Write results to the output file in the same format as the
    // synchronous path.
    let output_path = crate::path_safety::contain_for_write(output)?;
    let file = std::fs::File::create(&output_path)
        .map_err(|e| map_corpus_io_error(e, &format!("Cannot create output file '{}'", output)))?;
    let output_writer = Arc::new(std::sync::Mutex::new(std::io::BufWriter::new(file)));
    let write_count = std::sync::atomic::AtomicUsize::new(0);

    // Build a lookup map from custom_id to result
    let result_map: std::collections::HashMap<
        String,
        &hkask_types::inference_ipc::BatchResultEntry,
    > = batch_results
        .iter()
        .map(|r| (r.custom_id.clone(), r))
        .collect();

    for prompt in &prompts_vec {
        if let Some(result) = result_map.get(&prompt.chunk_id) {
            if let Some(ref text) = result.text {
                let levels = prompt
                    .bloom_levels
                    .clone()
                    .unwrap_or_else(qa_pipeline::default_bloom_levels);
                match parse_qa_response(&extract_json_from_response(text), &levels, None) {
                    Ok(qa_response) => {
                        for pair in qa_response.qa_pairs {
                            let result_json = qa_pipeline::qa_result_envelope(
                                prompt,
                                pair,
                                model,
                                "batch-api",
                                result.total_tokens,
                            );
                            write_qa_result(&result_json, &output_writer, &write_count);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "hkask.mcp.docproc.batch_api",
                            chunk_id = %prompt.chunk_id,
                            error = %e,
                            "QA response rejected"
                        );
                    }
                }
            } else if let Some(ref err) = result.error {
                tracing::warn!(
                    target: "hkask.mcp.docproc.batch_api",
                    chunk_id = %prompt.chunk_id,
                    error = %err,
                    "Batch result error"
                );
            }
        }
    }

    // Flush
    {
        let mut w = output_writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = w.flush() {
            tracing::warn!(
                target: "hkask.mcp.docproc.batch_api",
                error = %e,
                "Failed to flush batch API output writer"
            );
        }
    }

    let written = write_count.load(std::sync::atomic::Ordering::Relaxed);
    let failed = batch_results.iter().filter(|r| r.error.is_some()).count();
    let result = json!({
        "total": total,
        "written": written,
        "failed": failed,
        "output": output,
        "batch_api": true,
    });
    let outcome = BatchOutcome::from_counts(failed, total);
    outcome.log_if_degraded("hkask.mcp.docproc.batch_api", "QA batch API");
    Ok(result)
}
