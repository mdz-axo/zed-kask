use super::{ProviderSearchOutput, WebError, WebSearchProvider};
use crate::research::types::*;
use async_trait::async_trait;

/// Semantic Scholar academic paper search provider.
///
/// Free tier: no API key required, rate-limited to ~1 request/second.
/// Searches papers by title/author/keyword and returns structured results
/// with abstracts, authors, year, citation count, and external IDs (DOI, ArXiv).
pub(crate) struct SemanticScholarProvider {
    client: reqwest::Client,
}

impl SemanticScholarProvider {
    pub fn new() -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
        })
    }
}

const SEMANTIC_SCHOLAR_API_BASE: &str = "https://api.semanticscholar.org/graph/v1";
#[async_trait]
impl WebSearchProvider for SemanticScholarProvider {
    fn kind(&self) -> &str {
        "semantic_scholar"
    }
    fn capabilities(&self) -> Vec<SearchCapability> {
        vec![SearchCapability::Semantic, SearchCapability::Keyword]
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        // Build query params: search by keyword, request paper details
        let params: Vec<(&str, String)> = vec![
            ("query", query.query.clone()),
            ("limit", query.num_results.to_string()),
            (
                "fields",
                "title,authors,year,abstract,externalIds,citationCount,url,publicationTypes,openAccessPdf".to_string(),
            ),
        ];

        // 429 retry with backoff. Semantic Scholar's free tier rate-limits
        // under bulk use (observed 2026-09-30: ~half of a retrieval sweep's
        // fusion calls 429'd, and the agent routed around the provider with
        // direct curl — a retry-policy gap, not agent ingenuity). Two retries
        // with 1s/2s backoff absorb the transient bursts; a persistent 429
        // surfaces with the attempt count so the caller knows it was not a
        // single-shot failure.
        const MAX_429_RETRIES: u32 = 2;
        let (status, body) = {
            let mut attempt: u32 = 0;
            loop {
                let resp = self
                    .client
                    .get(format!("{SEMANTIC_SCHOLAR_API_BASE}/paper/search"))
                    .query(&params)
                    .send()
                    .await
                    .map_err(|e| {
                        WebError::ProviderUnavailable(format!(
                            "Semantic Scholar request failed: {e}"
                        ))
                    })?;
                let status = resp.status();
                let body = resp.text().await.map_err(|e| {
                    WebError::ProviderUnavailable(format!("Semantic Scholar body read failed: {e}"))
                })?;
                if status.as_u16() == 429 && attempt < MAX_429_RETRIES {
                    attempt += 1;
                    let backoff_secs = u64::from(attempt);
                    tracing::warn!(
                        provider = "semantic_scholar",
                        attempt,
                        backoff_secs,
                        "Semantic Scholar rate limited; retrying with backoff"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                    continue;
                }
                if status.as_u16() == 429 {
                    return Err(WebError::RateLimited(format!(
                        "Semantic Scholar rate limited after {attempt} backoff retries: {status}"
                    )));
                }
                break (status, body);
            }
        };

        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "Semantic Scholar error {status}: {}",
                hkask_inference::openai_compat::sanitize_error_body(&body)
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Semantic Scholar response: {e}"))
        })?;

        let results = parsed["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|paper| {
                        let title = paper["title"].as_str()?.to_string();

                        // Prefer openAccessPdf URL, fall back to Semantic Scholar page
                        let url = paper["openAccessPdf"]["url"]
                            .as_str()
                            .or_else(|| paper["url"].as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| {
                                paper["paperId"]
                                    .as_str()
                                    .map(|id| {
                                        format!("https://api.semanticscholar.org/CorpusID:{id}")
                                    })
                                    .unwrap_or_default()
                            });

                        // Build description from authors + year + abstract snippet
                        let authors: Vec<String> = paper["authors"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|a| a["name"].as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let year = paper["year"].as_u64().map(|y| y.to_string());
                        let citations = paper["citationCount"]
                            .as_u64()
                            .map(|c| format!("{c} citations"));

                        let mut desc_parts: Vec<String> = Vec::new();
                        if !authors.is_empty() {
                            desc_parts.push(authors.join(", "));
                        }
                        if let Some(ref y) = year {
                            desc_parts.push(format!("({y})"));
                        }
                        if let Some(ref c) = citations {
                            desc_parts.push(c.clone());
                        }
                        if let Some(ab) = paper["abstract"].as_str() {
                            let short_abstract: String = ab.chars().take(300).collect();
                            desc_parts.push(short_abstract);
                        }

                        let description = if desc_parts.is_empty() {
                            None
                        } else {
                            Some(desc_parts.join(" — "))
                        };

                        // Source: journal name or publication type
                        let source = paper["publicationTypes"]
                            .as_array()
                            .and_then(|arr| arr.first())
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                            .or_else(|| paper["journal"]["name"].as_str().map(|s| s.to_string()));

                        Some(SearchResult {
                            title,
                            url,
                            description,
                            source,
                            published: year,
                            oa_pdf_url: paper["openAccessPdf"]["url"].as_str().map(str::to_string),
                            provider: Some("semantic_scholar".to_string()),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(ProviderSearchOutput {
            results,
            ..Default::default()
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Quick liveness check with a minimal query
        let resp = self
            .client
            .get(format!("{SEMANTIC_SCHOLAR_API_BASE}/paper/search"))
            .query(&[("query", "test"), ("limit", "1")])
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("Semantic Scholar health check failed: {e}"))
            })?;
        if resp.status().is_success() || resp.status().as_u16() == 429 {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "Semantic Scholar health check returned {}",
                resp.status()
            )))
        }
    }
}
