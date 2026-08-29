//! Rerank — direct provider rerank-endpoint calls for the zed side of the
//! inference IPC bridge.
//!
//! Mirrors `batch.rs`: the zed side holds the API key (read from the keychain
//! via the GPUI credential channel) and calls the provider's rerank endpoint
//! directly. The MCP server never sees the credential — it sends
//! `InferenceMethod::Rerank` over the IPC bridge and receives
//! `InferenceOutcome::RerankScores`.
//!
//! OpenRouter's rerank router (`POST /api/v1/rerank`) takes the full
//! document list in one request and returns each document's native
//! `relevance_score` — the dedicated reranker's own judgment, not a parsed
//! LLM generation. One call replaces the per-candidate fanout this stage
//! previously used.

use hkask_types::inference_ipc::RerankScoreEntry;

static RERANK_HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn rerank_http_client() -> &'static reqwest::Client {
    RERANK_HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

/// Provider-prefixed model → (provider, clean model id). Only OpenRouter has
/// a rerank endpoint among the registered providers; a model without the
/// `OpenRouter/` prefix is a configuration error, not a transient outage —
/// the caller surfaces it as such.
pub fn detect_rerank_provider(model: &str) -> Option<(&'static str, String)> {
    let clean = model.strip_prefix("OpenRouter/")?;
    Some(("openrouter", clean.to_string()))
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
) -> Result<Vec<RerankScoreEntry>, String> {
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
        .map_err(|error| format!("rerank request failed: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<body read failed>".to_string());
        return Err(format!("rerank endpoint returned {status}: {body}"));
    }

    let parsed: OpenRouterRerankResponse = response
        .json()
        .await
        .map_err(|error| format!("rerank response unparseable: {error}"))?;

    Ok(parsed
        .results
        .into_iter()
        .map(|result| RerankScoreEntry {
            index: result.index,
            relevance_score: result.relevance_score,
        })
        .collect())
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

    #[test]
    fn detect_rerank_provider_rejects_unprefixed_model() {
        assert!(detect_rerank_provider("qwen/qwen3-reranker-8b").is_none());
        assert!(detect_rerank_provider("DeepInfra/Qwen/Qwen3-Reranker-8B").is_none());
    }
}
