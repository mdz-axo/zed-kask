//! Provider-batch transport for canonical prepared QA, through the IPC bridge.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use hkask_types::InferencePort;
use hkask_types::inference_ipc::BatchPromptEntry;

use crate::McpToolError;
use crate::services::qa_pipeline::{
    PreparedQaPrompt, QaCompletion, QaCompletionError, QaOutput, qa_llm_parameters,
    render_prepared_messages,
};

/// Render the compact prepared contract identically to the synchronous path.
/// The bridge holds credentials and preserves the model routing prefix/suffix.
pub(crate) async fn generate_qa_via_batch_api<W: Write>(
    inference_router: &Arc<dyn InferencePort>,
    prompts: &[PreparedQaPrompt],
    model: &str,
    output: &mut QaOutput<W>,
) -> Result<(), McpToolError> {
    let batch_prompts: Vec<BatchPromptEntry> = prompts
        .iter()
        .map(|prompt| {
            let [system, user] = render_prepared_messages(prompt)?;
            Ok(BatchPromptEntry {
                custom_id: prompt.prompt_id.clone(),
                system: system.content,
                user: user.content,
            })
        })
        .collect::<Result<_, McpToolError>>()?;
    let batch_results = inference_router
        .generate_batch(model, &batch_prompts, 2000, qa_llm_parameters().temperature)
        .await
        // Submission is not idempotent: retain the typed failure without
        // retrying a batch whose remote acceptance may be unknown.
        .map_err(crate::tools::semantic::qa::map_qa_inference_error)?;

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
            [] => Err(QaCompletionError::BatchNoResult),
            [result] => match (&result.text, &result.error) {
                (Some(text), None) => Ok(QaCompletion {
                    text: text.clone(),
                    tokens_used: result.total_tokens,
                    cost_usd: None,
                }),
                (None, Some(error)) => Err(QaCompletionError::BatchProvider(error.clone())),
                _ => Err(QaCompletionError::BatchMalformed),
            },
            _ => Err(QaCompletionError::BatchDuplicates(entries.len())),
        };
        output.complete(prompt, completion, model)?;
    }
    Ok(())
}
