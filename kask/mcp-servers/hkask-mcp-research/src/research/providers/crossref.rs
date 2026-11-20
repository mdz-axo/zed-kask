//! Crossref provider — bibliographic title resolution.
//!
//! One surface: `search_bibliographic` resolves a free-text title to
//! candidate works (DOI, title, year, venue) via Crossref's
//! `query.bibliographic` search. This is the exact-title lookup the keyword
//! search providers cannot serve (observed 2026-09-30: arXiv keyword search
//! for an exact title returned five unrelated papers; OpenAlex fuzzy-matched
//! a different field entirely) — the retrieval path's most common query is
//! "resolve this exact title to a verified paper", and `resolve_paper`
//! consumes these candidates to return the typed identity.
//!
//! Crossref's polite pool needs no API key; a `mailto` contact improves
//! routing but is optional and omitted (no configured contact to send).
//! Every candidate is surfaced to the caller — a title search can hit a
//! different work than intended, so the consumer verifies the match (the
//! no-substitution rule) before trusting the resolution.

use crate::research::types::WebError;
use serde::{Deserialize, Serialize};

const CROSSREF_API_BASE: &str = "https://api.crossref.org";

/// A candidate work from a Crossref bibliographic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossrefCandidate {
    /// The DOI, normalized lowercase — the resolution handle.
    pub doi: String,
    /// The work's title as Crossref records it — the match-verification
    /// field: the caller compares it against the queried title before
    /// trusting the resolution.
    pub title: String,
    pub publication_year: Option<u64>,
    /// Container title (journal/venue) when present.
    pub venue: Option<String>,
    /// First author's family name when present — a secondary
    /// match-verification field.
    pub first_author: Option<String>,
}

/// Exact-title comparison for candidate ranking (P1, 2026-10-01):
/// normalized (lowercase, whitespace-collapsed) string equality.
/// Crossref's bibliographic relevance can rank a superstring title
/// above the exact work (observed live 2026-10-01: "Computing
/// Machinery and Intelligence Amplification" outranked Turing's
/// "Computing Machinery and Intelligence"), so `resolve_paper`'s title
/// mode sorts candidates exact-first. The sort is stable — Crossref's
/// relevance order stands within each group, so equally-exact
/// candidates (e.g. a preprint and the published version of the same
/// work) keep their relative order and stay verifiable via the
/// surfaced candidates.
pub(crate) fn title_matches_exactly(query: &str, candidate: &str) -> bool {
    fn normalized(title: &str) -> String {
        title
            .trim()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    normalized(query) == normalized(candidate)
}

/// Crossref bibliographic search client.
#[derive(Clone)]
pub(crate) struct CrossrefProvider {
    client: reqwest::Client,
    /// Request base; `CROSSREF_API_BASE` except in retry-path tests, where
    /// it points at the loopback HTTP fixture (the serapi `base` pattern).
    base: String,
}

impl CrossrefProvider {
    pub fn new() -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
            base: CROSSREF_API_BASE.to_string(),
        })
    }

    /// Resolve a free-text title to up to `rows` candidate works, most
    /// relevant first. An empty result is a legitimate outcome (no
    /// registered work matches the title) and returns `Ok(vec![])`.
    pub(crate) async fn search_bibliographic(
        &self,
        title: &str,
        rows: u32,
    ) -> Result<Vec<CrossrefCandidate>, WebError> {
        // 429 retry with backoff (the semantic_scholar pattern): Crossref's
        // polite pool throttles parallel title resolutions (observed
        // 2026-11-02: 5 parallel run-scoped resolve_paper calls all 429'd
        // on their first response and each failed the whole call). Two
        // retries with 1s/2s backoff absorb the transient burst; a
        // persistent 429 surfaces with the attempt count as `RateLimited`
        // — the honest classification (a2eb0ff1fc): back off, don't
        // diagnose the tool.
        const MAX_429_RETRIES: u32 = 2;
        let (status, body) = {
            let mut attempt: u32 = 0;
            loop {
                let resp = self
                    .client
                    .get(format!("{}/works", self.base))
                    .query(&[
                        ("query.bibliographic", title.to_string()),
                        ("rows", rows.to_string()),
                    ])
                    .send()
                    .await
                    .map_err(|e| {
                        WebError::ProviderUnavailable(format!("Crossref request failed: {e}"))
                    })?;
                let status = resp.status();
                let body = resp.text().await.map_err(|e| {
                    WebError::ProviderUnavailable(format!("Crossref body read failed: {e}"))
                })?;
                if status.as_u16() == 429 && attempt < MAX_429_RETRIES {
                    attempt += 1;
                    let backoff_secs = u64::from(attempt);
                    tracing::warn!(
                        provider = "crossref",
                        attempt,
                        backoff_secs,
                        "Crossref rate limited; retrying with backoff"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                    continue;
                }
                if status.as_u16() == 429 {
                    return Err(WebError::RateLimited(format!(
                        "Crossref rate limited after {attempt} backoff retries: {status}"
                    )));
                }
                break (status, body);
            }
        };
        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "Crossref error {status}: {}",
                hkask_inference::openai_compat::sanitize_error_body(&body)
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Crossref response: {e}"))
        })?;
        let items = parsed["message"]["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        Ok(items
            .iter()
            .filter_map(|item| {
                let doi = item["DOI"].as_str()?.to_lowercase();
                // Crossref titles are arrays; the first element is the title.
                let title = item["title"]
                    .as_array()
                    .and_then(|titles| titles.first())
                    .and_then(|t| t.as_str())?
                    .to_string();
                let publication_year = item["issued"]["date-parts"]
                    .as_array()
                    .and_then(|parts| parts.first())
                    .and_then(|part| part.as_array())
                    .and_then(|years| years.first())
                    .and_then(|year| year.as_u64());
                let venue = item["container-title"]
                    .as_array()
                    .and_then(|venues| venues.first())
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let first_author = item["author"]
                    .as_array()
                    .and_then(|authors| authors.first())
                    .and_then(|author| author["family"].as_str())
                    .map(str::to_string);
                Some(CrossrefCandidate {
                    doi,
                    title,
                    publication_year,
                    venue,
                    first_author,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::http_fixture_sequence;
    use std::sync::atomic::Ordering;

    /// Raw HTTP response bytes for the fixture: a status line plus a
    /// close-delimited body. `Connection: close` makes every retry a new
    /// connection, so the fixture's hit count is the attempt-count oracle.
    fn status_response(status: u16, reason: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn rate_limited_response() -> Vec<u8> {
        status_response(429, "Too Many Requests", "slow down")
    }

    fn candidates_response() -> Vec<u8> {
        let body = serde_json::json!({
            "message": {
                "items": [{
                    "DOI": "10.1234/retry-resolved",
                    "title": ["A Retried Title"],
                    "issued": {"date-parts": [[2026]]},
                    "container-title": ["Journal of Retries"],
                    "author": [{"family": "Backoff"}]
                }]
            }
        });
        status_response(200, "OK", &body.to_string())
    }

    /// The 2026-11-02 incident shape, resolved: a transient Crossref 429
    /// is absorbed by the backoff retry, so a throttled first response no
    /// longer fails the whole resolve_paper call.
    #[tokio::test]
    async fn transient_429_retries_and_resolves() {
        let (url, hits) =
            http_fixture_sequence(vec![rate_limited_response(), candidates_response()]).await;
        let mut provider = CrossrefProvider::new().expect("provider");
        provider.base = url;
        let candidates = provider
            .search_bibliographic("A Retried Title", 3)
            .await
            .expect("429-then-200 must resolve via backoff retry");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "one throttled attempt plus one retry"
        );
        assert_eq!(candidates.len(), 1, "candidates: {candidates:?}");
        assert_eq!(candidates[0].doi, "10.1234/retry-resolved");
    }

    /// A persistent 429 surfaces as `RateLimited` only after
    /// MAX_429_RETRIES backoff retries — the honest "back off, don't
    /// diagnose the tool" classification, with the attempt count in the
    /// message so the caller knows it was not a single-shot failure.
    #[tokio::test]
    async fn persistent_429_surfaces_rate_limited_after_max_retries() {
        let (url, hits) = http_fixture_sequence(vec![
            rate_limited_response(),
            rate_limited_response(),
            rate_limited_response(),
        ])
        .await;
        let mut provider = CrossrefProvider::new().expect("provider");
        provider.base = url;
        let error = provider
            .search_bibliographic("A Retried Title", 3)
            .await
            .expect_err("a persistent 429 must not resolve");
        assert!(matches!(error, WebError::RateLimited(_)), "kind: {error:?}");
        let message = error.to_string();
        assert!(
            message.contains("after 2 backoff retries"),
            "attempt count in message: {message}"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            3,
            "the initial attempt plus MAX_429_RETRIES retries"
        );
    }
}
