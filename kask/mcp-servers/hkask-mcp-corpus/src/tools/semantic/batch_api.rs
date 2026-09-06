//! Provider-batch transport for canonical prepared QA, through the IPC bridge.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use hkask_types::InferencePort;
use hkask_types::inference_ipc::BatchPromptEntry;

use crate::McpToolError;
use crate::services::qa_pipeline::{PreparedQaPrompt, QaCompletion, QaOutput, qa_llm_parameters};

/// Forward prepared instructions unchanged. The bridge holds credentials and
/// resolves the original model routing prefix/suffix, never the corpus server.
pub(crate) async fn generate_qa_via_batch_api<W: Write>(
    inference_router: &Arc<dyn InferencePort>,
    prompts: &[PreparedQaPrompt],
    model: &str,
    output: &mut QaOutput<W>,
) -> Result<(), McpToolError> {
    let batch_prompts: Vec<BatchPromptEntry> = prompts
        .iter()
        .map(|prompt| BatchPromptEntry {
            custom_id: prompt.prompt_id.clone(),
            system: prompt.system.clone(),
            user: prompt.user.clone(),
        })
        .collect();
    let batch_results = inference_router
        .generate_batch(model, &batch_prompts, 2000, qa_llm_parameters().temperature)
        .await
        .map_err(|error| McpToolError::unavailable(format!("Batch API IPC failed: {error}")))?;

    // Retain duplicates as errors rather than last-write-wins. Prepopulate the
    // map to distinguish missing expected results from unsolicited identities.
    let mut results: HashMap<&str, Vec<_>> = prompts
        .iter()
        .map(|prompt| (prompt.prompt_id.as_str(), Vec::new()))
        .collect();
    for result in batch_results {
        let entries = results.get_mut(result.custom_id.as_str()).ok_or_else(|| {
            McpToolError::internal(format!(
                "Batch API returned unknown prompt_id '{}'",
                result.custom_id
            ))
        })?;
        entries.push(result);
    }
    for prompt in prompts {
        let entries = results
            .remove(prompt.prompt_id.as_str())
            .ok_or_else(|| McpToolError::internal("Batch prompt identity disappeared"))?;
        let completion = match entries.as_slice() {
            [] => Err("Batch API returned no result for prompt".to_string()),
            [result] => match (&result.text, &result.error) {
                (Some(text), None) => Ok(QaCompletion {
                    text: text.clone(),
                    tokens_used: result.total_tokens,
                }),
                (None, Some(error)) => Err(format!("Batch provider error: {error}")),
                _ => {
                    Err("Malformed batch result: expected exactly one of text or error".to_string())
                }
            },
            _ => Err(format!(
                "Batch API returned {} duplicate results for prompt",
                entries.len()
            )),
        };
        output.complete(prompt, completion, model)?;
    }
    Ok(())
}
