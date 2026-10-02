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
}

impl CrossrefProvider {
    pub fn new() -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
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
        let resp = self
            .client
            .get(format!("{CROSSREF_API_BASE}/works"))
            .query(&[
                ("query.bibliographic", title.to_string()),
                ("rows", rows.to_string()),
            ])
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Crossref request failed: {e}")))?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| {
            WebError::ProviderUnavailable(format!("Crossref body read failed: {e}"))
        })?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                429 => WebError::RateLimited(format!("Crossref rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "Crossref error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
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
