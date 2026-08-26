//! Batch inference API router for OpenRouter and DeepInfra.
//!
//! Both providers implement the OpenAI Batch API format: upload a JSONL file
//! of requests, create a batch, poll for completion, download results. The
//! batch API trades immediate responses for lower cost (OpenRouter: 50%
//! discount, DeepInfra: 20% discount) and no rate limits.
//!
//! ## When to use
//!
//! Any MCP server that needs to run a large number of non-urgent LLM calls
//! (QA generation, evals, classification, embeddings) should check
//! [`detect_batch_provider`] on the configured model. When it returns
//! `Some`, route through [`submit_batch`] instead of the synchronous
//! `InferencePort::generate_with_model` path.
//!
//! ## Provider detection
//!
//! - **OpenRouter**: model name ends with `:batch` (e.g. `z-ai/glm-5.2:batch`)
//! - **DeepInfra**: model name starts with `DeepInfra/` — all DeepInfra models
//!   support batch, no suffix needed. The caller can also force DeepInfra
//!   batch by setting `HKASK_BATCH_PROVIDER=deepinfra`.
//!
//! ## Credentials
//!
//! API keys are read from env vars (injected by `build_mcp_server_env`):
//! - OpenRouter: `OPENROUTER_API_KEY`
//! - DeepInfra: `DEEPINFRA_TOKEN` (or `DEEPINFRA_API_KEY` as fallback)
//!
//! ## Endpoints
//!
//! | Step | OpenRouter | DeepInfra |
//! |------|-----------|-----------|
//! | Upload | `POST /api/beta/batches/files` | `POST /v1/openai/files` (purpose=batch) |
//! | Create | `POST /api/beta/batches` | `POST /v1/openai/batches` |
//! | Status | `GET /api/beta/batches/{id}` | `GET /v1/openai/batches/{id}` |
//! | Download | `GET /api/beta/batches/files/{id}/content` | `GET /v1/openai/files/{id}/content` |

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::sleep;

/// Maximum time to wait for a batch to complete (6 hours).
const MAX_BATCH_WAIT: Duration = Duration::from_secs(6 * 60 * 60);

/// Polling interval for batch status checks.
const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Which batch API provider to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchProvider {
    OpenRouter,
    DeepInfra,
}

impl BatchProvider {
    fn base_url(&self) -> &str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/beta",
            Self::DeepInfra => "https://api.deepinfra.com/v1/openai",
        }
    }

    fn auth_header(&self) -> &'static str {
        match self {
            Self::OpenRouter => "Authorization",
            Self::DeepInfra => "Authorization",
        }
    }

    fn auth_value(&self, key: &str) -> String {
        match self {
            Self::OpenRouter => format!("Bearer {key}"),
            Self::DeepInfra => format!("Bearer {key}"),
        }
    }
}

/// Detect whether a model name is batch-eligible and which provider to use.
///
/// - `z-ai/glm-5.2:batch` → OpenRouter (the `:batch` suffix is OpenRouter's
///   convention for batch-eligible models)
/// - `DeepInfra/Qwen/Qwen3-Embedding-0.6B` → DeepInfra (all DeepInfra models
///   support batch)
/// - `HKASK_BATCH_PROVIDER=deepinfra` env var forces DeepInfra for any model
///   (the model name is passed through without the `:batch` suffix)
/// - `HKASK_BATCH_PROVIDER=openrouter` env var forces OpenRouter (strips
///   `:batch` from the model name before submission)
///
/// Returns `None` when the model is not batch-eligible and no provider override
/// is set.
pub fn detect_batch_provider(model: &str) -> Option<(BatchProvider, String)> {
    // Env override takes precedence
    if let Ok(provider) = std::env::var("HKASK_BATCH_PROVIDER") {
        let provider = provider.trim().to_lowercase();
        match provider.as_str() {
            "openrouter" => {
                let clean_model = model.strip_suffix(":batch").unwrap_or(model);
                return Some((BatchProvider::OpenRouter, clean_model.to_string()));
            }
            "deepinfra" => {
                let clean_model = model.strip_prefix("DeepInfra/").unwrap_or(model);
                return Some((BatchProvider::DeepInfra, clean_model.to_string()));
            }
            _ => {}
        }
    }

    // OpenRouter: `:batch` suffix
    if let Some(clean_model) = model.strip_suffix(":batch") {
        return Some((BatchProvider::OpenRouter, clean_model.to_string()));
    }

    // DeepInfra: provider prefix
    if model.starts_with("DeepInfra/") {
        let clean_model = model.strip_prefix("DeepInfra/").unwrap_or(model);
        return Some((BatchProvider::DeepInfra, clean_model.to_string()));
    }

    None
}

/// A single inference result from the batch API.
#[derive(Debug, Clone)]
pub struct BatchInferenceResult {
    /// The generated text.
    pub text: String,
    /// Total tokens used (prompt + completion).
    pub total_tokens: u64,
}

/// Result of a batch submission.
pub struct BatchResult {
    /// Results keyed by `custom_id`. `Ok` for successes, `Err(message)` for
    /// failures. Every prompt in the input batch has an entry here — failures
    /// are NOT dropped, so the caller can report accurate failure counts.
    pub results: std::collections::HashMap<String, Result<BatchInferenceResult, String>>,
    /// Number of prompts that succeeded.
    pub succeeded: usize,
    /// Number of prompts that failed.
    pub failed: usize,
}

/// Shared HTTP client for batch API calls. Reusing a single client avoids
/// creating a new connection pool per batch submission.
static BATCH_HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn batch_http_client() -> &'static reqwest::Client {
    BATCH_HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

/// Submit a batch of prompts to the provider's Batch API and wait for results.
///
/// `api_key` is the provider's API key, resolved by the caller (the zed-side
/// bridge reads it from the keychain). `model` is the raw model name (without
/// provider prefix or `:batch` suffix). `max_tokens` controls the output
/// length per prompt. `temperature` controls sampling.
///
/// Uses `BatchPromptEntry` from `hkask-types::inference_ipc` directly — no
/// duplicate `BatchPrompt` type. The bridge converts between the IPC protocol
/// type and this function without an intermediate struct.
pub async fn submit_batch(
    provider: BatchProvider,
    api_key: &str,
    model: &str,
    prompts: &[hkask_types::inference_ipc::BatchPromptEntry],
    max_tokens: u32,
    temperature: f32,
) -> Result<BatchResult, String> {
    let client = batch_http_client();
    let base = provider.base_url();

    // 1. Format prompts as OpenAI Batch API JSONL
    let jsonl = format_batch_jsonl(model, prompts, max_tokens, temperature);

    // 2. Upload the file
    tracing::info!(
        target: "hkask.inference.batch",
        provider = ?provider,
        prompt_count = prompts.len(),
        model = %model,
        "Uploading batch file"
    );
    let file_id = upload_batch_file(&client, provider, &api_key, base, &jsonl).await?;

    // 3. Create the batch
    let batch_id = create_batch(&client, provider, &api_key, base, &file_id).await?;
    tracing::info!(
        target: "hkask.inference.batch",
        provider = ?provider,
        batch_id = %batch_id,
        prompt_count = prompts.len(),
        "Batch created — polling for completion"
    );

    // 4. Poll until completed
    let output_file_id =
        poll_batch_completion(&client, provider, &api_key, base, &batch_id).await?;

    // 5. Download results
    tracing::info!(
        target: "hkask.inference.batch",
        batch_id = %batch_id,
        "Downloading batch results"
    );
    let results_content =
        download_batch_results(&client, provider, &api_key, base, &output_file_id).await?;

    // 6. Parse results
    parse_batch_results(&results_content)
}

// ── Internal helpers ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct BatchRequestLine {
    custom_id: String,
    method: String,
    url: String,
    body: BatchRequestBody,
}

#[derive(Debug, Serialize)]
struct BatchRequestBody {
    model: String,
    messages: Vec<BatchMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct BatchMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct FileUploadResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct BatchCreateResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct BatchStatusResponse {
    status: String,
    output_file_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BatchResultLine {
    custom_id: String,
    response: Option<BatchResultResponse>,
    error: Option<BatchResultError>,
}

#[derive(Debug, Deserialize)]
struct BatchResultResponse {
    body: BatchResultBody,
}

#[derive(Debug, Deserialize)]
struct BatchResultBody {
    choices: Vec<BatchResultChoice>,
    usage: Option<BatchResultUsage>,
}

#[derive(Debug, Deserialize)]
struct BatchResultChoice {
    message: BatchResultChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct BatchResultChoiceMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct BatchResultUsage {
    total_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct BatchResultError {
    message: String,
}

fn format_batch_jsonl(
    model: &str,
    prompts: &[hkask_types::inference_ipc::BatchPromptEntry],
    max_tokens: u32,
    temperature: f32,
) -> String {
    let mut lines = Vec::with_capacity(prompts.len());
    for p in prompts {
        let line = BatchRequestLine {
            custom_id: p.custom_id.clone(),
            method: "post".to_string(),
            url: "/v1/chat/completions".to_string(),
            body: BatchRequestBody {
                model: model.to_string(),
                messages: vec![
                    BatchMessage {
                        role: "system".to_string(),
                        content: p.system.clone(),
                    },
                    BatchMessage {
                        role: "user".to_string(),
                        content: p.user.clone(),
                    },
                ],
                max_tokens,
                temperature,
            },
        };
        lines.push(serde_json::to_string(&line).unwrap_or_default());
    }
    lines.join("\n") + "\n"
}

async fn upload_batch_file(
    client: &reqwest::Client,
    provider: BatchProvider,
    api_key: &str,
    base: &str,
    jsonl: &str,
) -> Result<String, String> {
    let url = match provider {
        BatchProvider::OpenRouter => format!("{base}/batches/files"),
        BatchProvider::DeepInfra => format!("{base}/files"),
    };

    let resp = client
        .post(&url)
        .header(provider.auth_header(), provider.auth_value(api_key))
        .header("Content-Type", "application/jsonl")
        .body(jsonl.to_string())
        .send()
        .await
        .map_err(|e| format!("Batch file upload failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Batch file upload failed ({status}): {body}"));
    }

    let upload_resp: FileUploadResponse = resp
        .json()
        .await
        .map_err(|e| format!("Batch file upload response parse failed: {e}"))?;

    Ok(upload_resp.id)
}

async fn create_batch(
    client: &reqwest::Client,
    provider: BatchProvider,
    api_key: &str,
    base: &str,
    file_id: &str,
) -> Result<String, String> {
    let url = format!("{base}/batches");
    let body = serde_json::json!({
        "input_file_id": file_id,
        "endpoint": "/v1/chat/completions",
        "completion_window": "24h",
    });

    let resp = client
        .post(&url)
        .header(provider.auth_header(), provider.auth_value(api_key))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("Batch creation failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Batch creation failed ({status}): {body}"));
    }

    let create_resp: BatchCreateResponse = resp
        .json()
        .await
        .map_err(|e| format!("Batch creation response parse failed: {e}"))?;

    Ok(create_resp.id)
}

async fn poll_batch_completion(
    client: &reqwest::Client,
    provider: BatchProvider,
    api_key: &str,
    base: &str,
    batch_id: &str,
) -> Result<String, String> {
    let url = format!("{base}/batches/{batch_id}");
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > MAX_BATCH_WAIT {
            return Err(format!(
                "Batch {batch_id} did not complete within {} seconds",
                MAX_BATCH_WAIT.as_secs()
            ));
        }

        let resp = client
            .get(&url)
            .header(provider.auth_header(), provider.auth_value(api_key))
            .send()
            .await
            .map_err(|e| format!("Batch status poll failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Batch status poll failed ({status}): {body}"));
        }

        let status_resp: BatchStatusResponse = resp
            .json()
            .await
            .map_err(|e| format!("Batch status response parse failed: {e}"))?;

        tracing::info!(
            target: "hkask.inference.batch",
            batch_id = %batch_id,
            status = %status_resp.status,
            elapsed_secs = start.elapsed().as_secs(),
            "Batch status"
        );

        match status_resp.status.as_str() {
            "completed" => {
                return status_resp.output_file_id.ok_or_else(|| {
                    format!("Batch {batch_id} completed but no output file id")
                });
            }
            "failed" | "cancelled" | "expired" => {
                return Err(format!(
                    "Batch {batch_id} ended with status: {}",
                    status_resp.status
                ));
            }
            _ => {
                sleep(POLL_INTERVAL).await;
            }
        }
    }
}

async fn download_batch_results(
    client: &reqwest::Client,
    provider: BatchProvider,
    api_key: &str,
    base: &str,
    file_id: &str,
) -> Result<String, String> {
    let url = match provider {
        BatchProvider::OpenRouter => format!("{base}/batches/files/{file_id}/content"),
        BatchProvider::DeepInfra => format!("{base}/files/{file_id}/content"),
    };

    let resp = client
        .get(&url)
        .header(provider.auth_header(), provider.auth_value(api_key))
        .send()
        .await
        .map_err(|e| format!("Batch results download failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Batch results download failed ({status}): {body}"));
    }

    resp.text()
        .await
        .map_err(|e| format!("Batch results read failed: {e}"))
}

fn parse_batch_results(content: &str) -> Result<BatchResult, String> {
    let mut results = std::collections::HashMap::new();
    let mut succeeded = 0;
    let mut failed = 0;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let result_line: BatchResultLine = serde_json::from_str(line)
            .map_err(|e| format!("Failed to parse batch result line: {e}"))?;

        if let Some(resp) = result_line.response {
            if let Some(choice) = resp.body.choices.first() {
                let text = choice.message.content.clone();
                let total_tokens = resp.body.usage.map(|u| u.total_tokens).unwrap_or(0);
                results.insert(
                    result_line.custom_id,
                    Ok(BatchInferenceResult { text, total_tokens }),
                );
                succeeded += 1;
            } else {
                results.insert(
                    result_line.custom_id.clone(),
                    Err("batch result has no choices".to_string()),
                );
                failed += 1;
                tracing::warn!(
                    target: "hkask.inference.batch",
                    custom_id = %result_line.custom_id,
                    "Batch result has no choices"
                );
            }
        } else if let Some(err) = result_line.error {
            let err_msg = err.message;
            results.insert(
                result_line.custom_id.clone(),
                Err(err_msg.clone()),
            );
            failed += 1;
            tracing::warn!(
                target: "hkask.inference.batch",
                custom_id = %result_line.custom_id,
                error = %err_msg,
                "Batch result error"
            );
        } else {
            results.insert(
                result_line.custom_id.clone(),
                Err("unknown batch result format".to_string()),
            );
            failed += 1;
        }
    }

    tracing::info!(
        target: "hkask.inference.batch",
        succeeded,
        failed,
        "Batch results parsed"
    );

    Ok(BatchResult {
        results,
        succeeded,
        failed,
    })
}
