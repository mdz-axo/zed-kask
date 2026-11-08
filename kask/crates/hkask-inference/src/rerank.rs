//! Rerank — direct provider rerank calls for the zed side of the inference
//! IPC bridge.
//!
//! Mirrors `batch.rs`: the zed side holds the API key (read from the keychain
//! via the GPUI credential channel) and calls the provider's rerank endpoint
//! directly. The MCP server never sees the credential — it sends
//! `InferenceMethod::Rerank` over the IPC bridge and receives
//! `InferenceOutcome::RerankScores`.
//!
//! Two serving arms, keyed on the model string's provider prefix
//! (`detect_rerank_provider` — the model string IS the routing signal):
//!
//! - **Cloud (`OpenRouter/`)** — OpenRouter's rerank router
//!   (`POST /api/v1/rerank`) takes the full document list in one request and
//!   returns each document's native `relevance_score` — the dedicated
//!   reranker's own judgment, not a parsed LLM generation. One call replaces
//!   the per-candidate fanout this stage previously used. The OpenRouter key
//!   is read from the keychain on the zed side.
//! - **Local (`ollama/`)** — ollama has no rerank endpoint (`/v1/rerank` and
//!   `/api/rerank` both 404, probe-verified 2026-11-06), so the local arm
//!   speaks the Qwen3-Reranker judge protocol (Zhang et al.,
//!   arXiv:2506.05176) over ollama's chat API: one deterministic
//!   (temperature 0, generation-bounded) call per document, the first
//!   sampled token IS the verdict — `yes` → 1.0, `no` → 0.0. No credential:
//!   the loopback ollama instance is keyless by construction. The score is
//!   deliberately BINARY — a single sampled token's logprob is not a graded
//!   probability (casing variants fragment the mass and the sampler breaks
//!   argmax), so yes-scored documents tie above no-scored ones and each
//!   group keeps its heuristic relative order in the consumer's stable
//!   sort.

use futures_util::StreamExt;
use hkask_types::inference_ipc::RerankScoreEntry;

static RERANK_HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn rerank_http_client() -> &'static reqwest::Client {
    RERANK_HTTP_CLIENT.get_or_init(crate::provider_http_client)
}

/// Provider-prefixed model → (provider, clean model id). `OpenRouter/` serves
/// the cloud rerank router; `ollama/` (case-insensitive, the kask
/// provider-qualification norm) serves the local judge-protocol arm — the
/// private leg's rerank models are `ollama/`-qualified. A model without a
/// recognized prefix is a configuration error, not a transient outage — the
/// caller surfaces it as such.
pub fn detect_rerank_provider(model: &str) -> Option<(&'static str, String)> {
    if let Some(clean) = model.strip_prefix("OpenRouter/") {
        return Some(("openrouter", clean.to_string()));
    }
    if let Some(clean) = strip_prefix_ascii_ci(model, "ollama/") {
        return Some(("ollama", clean.to_string()));
    }
    None
}

/// Case-insensitive ASCII prefix strip. `str::get` guards the UTF-8 boundary,
/// so a multibyte character at the prefix edge can never panic the slice.
fn strip_prefix_ascii_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

#[derive(serde::Deserialize)]
struct OpenRouterRerankResponse {
    results: Vec<OpenRouterRerankResult>,
}

#[derive(serde::Deserialize)]
struct OpenRouterRerankResult {
    index: usize,
    relevance_score: f64,
}

/// A rerank-pipeline failure, structured by kind. The IPC server surfaces
/// the Display message verbatim in `InferenceOutcome::Error`.
#[derive(Debug, thiserror::Error)]
pub enum RerankError {
    #[error("rerank request failed: {source}")]
    Request {
        #[source]
        source: reqwest::Error,
    },
    #[error("rerank endpoint returned {status}: {body}")]
    Status {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("rerank response unparseable: {source}")]
    Parse {
        #[source]
        source: reqwest::Error,
    },
}

/// Rerank `documents` against `query` via OpenRouter's rerank router.
///
/// `api_key` is the OpenRouter key read from the keychain on the zed side.
/// `model` is the clean (prefix-stripped) model id, e.g.
/// `qwen/qwen3-reranker-8b`. Returns one entry per scored document, in the
/// provider's relevance order.
pub async fn rerank_documents(
    api_key: &str,
    model: &str,
    query: &str,
    documents: &[String],
) -> Result<Vec<RerankScoreEntry>, RerankError> {
    let response = rerank_http_client()
        .post("https://openrouter.ai/api/v1/rerank")
        .bearer_auth(api_key)
        .json(&serde_json::json!({
            "model": model,
            "query": query,
            "documents": documents,
        }))
        .send()
        .await
        .map_err(|error| RerankError::Request { source: error })?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<body read failed>".to_string());
        return Err(RerankError::Status { status, body });
    }

    let parsed: OpenRouterRerankResponse = response
        .json()
        .await
        .map_err(|error| RerankError::Parse { source: error })?;

    Ok(parsed
        .results
        .into_iter()
        .map(|result| RerankScoreEntry {
            index: result.index,
            relevance_score: result.relevance_score,
        })
        .collect())
}

// ── The local arm: the Qwen3-Reranker judge protocol over ollama ──────────

/// The judge system prompt from the Qwen3-Reranker usage card (Zhang et
/// al., arXiv:2506.05176): the model answers "yes" or "no" — a reranker's
/// relevance judgment, not a generation.
const RERANK_JUDGE_SYSTEM_PROMPT: &str = "Judge whether the Document meets the requirements based on the Query and the Instruct provided. Note that the answer can only be \"yes\" or \"no\".";
/// The retrieval instruction the deep-search rerank stage judges under (the
/// paper's default retrieval instruction).
const RERANK_JUDGE_INSTRUCTION: &str =
    "Given a web search query, retrieve relevant passages that answer the query";
/// Bounded per-document judge concurrency — one local ollama instance serves
/// one model; a small bound keeps the per-search judge burst polite.
const LOCAL_RERANK_CONCURRENCY: usize = 8;
/// The judgment token cap: under `think: false` the verdict is the FIRST
/// sampled token, so a longer tail (template drift) costs latency for
/// nothing — the response is cut off.
const LOCAL_RERANK_MAX_TOKENS: u32 = 16;

#[derive(serde::Deserialize)]
struct OllamaChatResponse {
    #[serde(default)]
    message: Option<OllamaChatMessage>,
    #[serde(default)]
    logprobs: Option<Vec<OllamaLogprob>>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(serde::Deserialize)]
struct OllamaChatMessage {
    content: String,
}

#[derive(serde::Deserialize)]
struct OllamaLogprob {
    token: String,
}

/// The binary verdict score from a judge response: the first sampled token
/// (the logprobs entry when ollama returns them, else the first word of the
/// content) is the model's relevance judgment — `yes` → 1.0, `no` → 0.0.
/// Any other first token is template drift: `None`, and the consumer keeps
/// that document at its heuristic position.
fn judge_verdict_score(response: &OllamaChatResponse) -> Option<f64> {
    let token = response
        .logprobs
        .as_deref()
        .and_then(|logprobs| logprobs.first())
        .map(|entry| entry.token.clone())
        .or_else(|| {
            response
                .message
                .as_ref()
                .and_then(|message| message.content.trim().split_whitespace().next())
                .map(str::to_string)
        })?;
    let verdict = token
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_ascii_lowercase();
    match verdict.as_str() {
        "yes" => Some(1.0),
        "no" => Some(0.0),
        _ => None,
    }
}

/// One judge call: score a single (query, document) pair against the local
/// ollama instance. Deterministic (temperature 0) and generation-bounded:
/// under `think: false` the first sampled token is the verdict, so a
/// rambling tail (observed on template-drifted GGUFs) is cut off at
/// [`LOCAL_RERANK_MAX_TOKENS`]. Returns `(verdict, prompt_tokens,
/// completion_tokens)` — ollama reports honest eval counts.
async fn judge_document(
    base_url: &str,
    model: &str,
    query: &str,
    document: &str,
) -> Result<(Option<f64>, u32, u32), RerankError> {
    let body = serde_json::json!({
        "model": model,
        "stream": false,
        "think": false,
        "logprobs": true,
        "messages": [
            {"role": "system", "content": RERANK_JUDGE_SYSTEM_PROMPT},
            {"role": "user", "content": format!(
                "<Instruct>: {RERANK_JUDGE_INSTRUCTION}\n<Query>: {query}\n<Document>: {document}"
            )}
        ],
        "options": {"temperature": 0.0, "num_predict": LOCAL_RERANK_MAX_TOKENS}
    });
    let response = rerank_http_client()
        .post(format!("{base_url}/api/chat"))
        .json(&body)
        .send()
        .await
        .map_err(|source| RerankError::Request { source })?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<body read failed>".to_string());
        return Err(RerankError::Status { status, body });
    }
    let parsed: OllamaChatResponse = response
        .json()
        .await
        .map_err(|source| RerankError::Parse { source })?;
    Ok((
        judge_verdict_score(&parsed),
        parsed.prompt_eval_count.unwrap_or(0),
        parsed.eval_count.unwrap_or(0),
    ))
}

/// Rerank `documents` against `query` via the local ollama chat API (the
/// Qwen3-Reranker judge protocol — ollama has no rerank endpoint). One
/// judge call per document at bounded concurrency; each document's first
/// sampled token is the binary verdict (1.0/0.0), and template drift leaves
/// the document unscored (no entry — the consumer keeps heuristic order).
/// No credential: the local ollama descriptor is keyless by construction.
///
/// Failure semantics: per-document errors degrade that document (surfaced by
/// the consumer's missing-count reason); when EVERY document fails the call
/// is an error carrying the first failure (ollama down, a missing model, a
/// template rejection — the real reason, never a vacuous "no scores").
/// Usage is the honestly summed ollama eval counts (`reported: true`).
pub async fn rerank_documents_local(
    base_url: &str,
    model: &str,
    query: &str,
    documents: &[String],
) -> Result<(Vec<RerankScoreEntry>, hkask_types::InferenceUsage), RerankError> {
    // Own the documents up front — the judge futures are built from owned
    // items, so the map closure is not higher-ranked over a borrowed item
    // (the `FnOnce`-not-general-enough class).
    let owned: Vec<String> = documents.to_vec();
    let outcomes = futures_util::stream::iter(owned.into_iter().enumerate())
        .map(|(index, document)| async move {
            (
                index,
                judge_document(&base_url, &model, &query, &document).await,
            )
        })
        .buffer_unordered(LOCAL_RERANK_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    let mut entries = Vec::new();
    let mut prompt_tokens = 0u32;
    let mut completion_tokens = 0u32;
    let mut first_error: Option<RerankError> = None;
    let mut errored = 0usize;
    for (index, outcome) in outcomes {
        match outcome {
            Ok((verdict, prompt, completion)) => {
                prompt_tokens = prompt_tokens.saturating_add(prompt);
                completion_tokens = completion_tokens.saturating_add(completion);
                if let Some(relevance_score) = verdict {
                    entries.push(RerankScoreEntry {
                        index,
                        relevance_score,
                    });
                } else {
                    // Template drift — the document stays unscored; its
                    // usage still counts (the call really ran).
                    tracing::warn!(
                        target: "hkask.inference.rerank",
                        index,
                        "judge response carried no yes/no verdict — document left unscored"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(
                    target: "hkask.inference.rerank",
                    index,
                    error = %error,
                    "judge call failed — document left unscored"
                );
                errored += 1;
                first_error.get_or_insert(error);
            }
        }
    }
    if errored > 0 && errored == documents.len() {
        // Every judge call failed — the whole-call failure semantics carry
        // the real reason (transport/model), not a vacuous "no scores".
        return Err(first_error.expect("errored > 0 implies first_error"));
    }
    Ok((
        entries,
        hkask_types::InferenceUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens.saturating_add(completion_tokens),
            reported: true,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rerank_provider_strips_openrouter_prefix() {
        let (provider, clean) =
            detect_rerank_provider("OpenRouter/qwen/qwen3-reranker-8b").expect("detects");
        assert_eq!(provider, "openrouter");
        assert_eq!(clean, "qwen/qwen3-reranker-8b");
    }

    /// The local arm: `ollama/`-qualified models (case-insensitive, the kask
    /// provider-qualification norm) route to the judge-protocol serving.
    #[test]
    fn detect_rerank_provider_serves_the_local_ollama_arm() {
        let (provider, clean) =
            detect_rerank_provider("ollama/qwen3-reranker-4b:latest").expect("detects");
        assert_eq!(provider, "ollama");
        assert_eq!(clean, "qwen3-reranker-4b:latest");
        let (provider, _) =
            detect_rerank_provider("Ollama/qwen3-reranker-0.6b").expect("ci prefix detects");
        assert_eq!(provider, "ollama");
    }

    #[test]
    fn detect_rerank_provider_rejects_unprefixed_model() {
        assert!(detect_rerank_provider("qwen/qwen3-reranker-8b").is_none());
        assert!(detect_rerank_provider("DeepInfra/Qwen/Qwen3-Reranker-8B").is_none());
        // The UTF-8 boundary guard: a multibyte character where the prefix
        // edge would sit must not panic the slice.
        assert!(detect_rerank_provider("日本語/model").is_none());
    }

    /// The repaired shipped default MUST be routable — the 2026-09-11
    /// `deepinfra/…` literal was unroutable by construction (the native path
    /// serves only `OpenRouter/`- and `ollama/`-prefixed models), so every
    /// default-configured deep search silently degraded to heuristic rerank
    /// (the operator-reported bug, repaired 2026-11-06).
    #[test]
    fn default_rerank_model_is_routable_by_the_native_path() {
        assert!(
            detect_rerank_provider(crate::model_constants::DEFAULT_RERANK_MODEL).is_some(),
            "the shipped default must name a servable provider prefix"
        );
    }

    /// The private leg's default names the LOCAL arm — a cloud-prefixed
    /// private default would be the crossing D100 forbids.
    #[test]
    fn default_private_rerank_model_routes_to_the_local_arm() {
        let (provider, clean) =
            detect_rerank_provider(crate::model_constants::DEFAULT_PRIVATE_RERANK_MODEL)
                .expect("the private default is ollama-qualified");
        assert_eq!(provider, "ollama");
        assert!(!clean.is_empty());
    }

    #[test]
    fn judge_verdict_score_reads_the_first_token_binary() {
        // The probed live shapes (ollama 0.40.12, think:false, logprobs):
        // the first sampled token is the verdict.
        let yes = serde_json::from_str::<OllamaChatResponse>(
            r#"{"message":{"content":"yes."},"logprobs":[{"token":"yes","logprob":-0.3}]}"#,
        )
        .expect("parse");
        assert_eq!(judge_verdict_score(&yes), Some(1.0));

        let no = serde_json::from_str::<OllamaChatResponse>(
            r#"{"message":{"content":"No."},"logprobs":[{"token":"No","logprob":-0.8}]}"#,
        )
        .expect("parse");
        assert_eq!(judge_verdict_score(&no), Some(0.0));

        // No logprobs (an older ollama): the content's first word is the
        // verdict.
        let content_only =
            serde_json::from_str::<OllamaChatResponse>(r#"{"message":{"content":"yes"}}"#)
                .expect("parse");
        assert_eq!(judge_verdict_score(&content_only), Some(1.0));

        // Template drift — neither yes nor no: unscored, never a guess.
        let drift = serde_json::from_str::<OllamaChatResponse>(
            r#"{"message":{"content":"<Query>: maybe"},"logprobs":[{"token":"<Query>","logprob":-0.1}]}"#,
        )
        .expect("parse");
        assert_eq!(judge_verdict_score(&drift), None);
    }
}
