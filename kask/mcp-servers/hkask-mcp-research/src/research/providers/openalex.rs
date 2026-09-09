//! OpenAlex provider — free scholarly metadata, no API key.
//!
//! Two surfaces: a `WebSearchProvider` for the search pool (works search),
//! and identifier resolution (`resolve_paper_id`) — direct lookups by DOI,
//! OpenAlex ID, PMID, and PMCID. arXiv IDs have no direct OpenAlex lookup
//! key; resolution returns `None` for them (surfaced by the caller, never
//! silent). The client is the provider-API client (like Semantic
//! Scholar/arXiv), not the strict raw-fetch transport — OpenAlex is a
//! provider API, not an arbitrary user URL.

use super::{ProviderSearchOutput, WebError, WebSearchProvider};
use crate::research::paper_id::PaperId;
use crate::research::types::*;
use async_trait::async_trait;

const OPENALEX_API_BASE: &str = "https://api.openalex.org";

/// OpenAlex work metadata for a resolved paper.
#[derive(Clone)]
pub(crate) struct OpenAlexProvider {
    client: reqwest::Client,
}

impl OpenAlexProvider {
    pub fn new() -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
        })
    }

    /// The direct-lookup path for an identifier, or None when the kind has
    /// no OpenAlex lookup key (arXiv).
    fn work_path(id: &PaperId) -> Option<String> {
        match id {
            PaperId::Doi(doi) => Some(format!("{OPENALEX_API_BASE}/works/doi:{doi}")),
            PaperId::OpenAlex(openalex_id) => {
                Some(format!("{OPENALEX_API_BASE}/works/{openalex_id}"))
            }
            PaperId::Pmid(pmid) => Some(format!("{OPENALEX_API_BASE}/works/pmid:{pmid}")),
            PaperId::Pmcid(pmcid) => Some(format!("{OPENALEX_API_BASE}/works/pmcid:{pmcid}")),
            PaperId::Arxiv(_) => None,
        }
    }

    /// Resolve a typed identifier to its OpenAlex work record. Ok(None)
    /// when there is no lookup path for the kind or no record exists (404).
    pub(crate) async fn resolve(&self, id: &PaperId) -> Result<Option<PaperMetadata>, WebError> {
        let Some(path) = Self::work_path(id) else {
            return Ok(None);
        };
        let response = self.client.get(&path).send().await.map_err(|error| {
            WebError::ProviderUnavailable(format!("OpenAlex request failed: {error}"))
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            WebError::ProviderUnavailable(format!("OpenAlex body read failed: {error}"))
        })?;
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(match status.as_u16() {
                429 => WebError::RateLimited(format!("OpenAlex rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "OpenAlex error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }
        Ok(parse_openalex_work(&body))
    }
}

/// Parse an OpenAlex work record into `PaperMetadata`. None when the record
/// lacks an id or title, or the body is not JSON — a record without those is
/// not a work.
fn parse_openalex_work(body: &str) -> Option<PaperMetadata> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let openalex_id = value["id"].as_str()?.rsplit('/').next()?.to_string();
    let title = value["title"]
        .as_str()
        .or_else(|| value["display_name"].as_str())?
        .to_string();
    let doi = value["doi"]
        .as_str()
        .map(|doi| doi.trim_start_matches("https://doi.org/").to_lowercase());
    let publication_year = value["publication_year"].as_u64();
    let authors = value["authorships"]
        .as_array()
        .map(|authorships| {
            authorships
                .iter()
                .filter_map(|authorship| {
                    authorship["author"]["display_name"]
                        .as_str()
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    let venue = value["primary_location"]["source"]["display_name"]
        .as_str()
        .map(str::to_string);
    Some(PaperMetadata {
        openalex_id,
        doi,
        title,
        publication_year,
        authors,
        venue,
    })
}

/// Parse an OpenAlex works-search response into search results. Entries
/// missing a title are filtered; a missing or malformed results array is an
/// empty result, never an error.
fn parse_openalex_works(body: &str) -> Vec<SearchResult> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(works) = value["results"].as_array() else {
        return Vec::new();
    };
    works
        .iter()
        .filter_map(|work| {
            let title = work["title"].as_str()?.to_string();
            let openalex_id = work["id"].as_str()?.rsplit('/').next()?.to_string();
            // Prefer the DOI landing page; fall back to the OpenAlex page.
            let url = work["doi"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("https://openalex.org/{openalex_id}"));
            let authors: Vec<String> = work["authorships"]
                .as_array()
                .map(|authorships| {
                    authorships
                        .iter()
                        .filter_map(|authorship| {
                            authorship["author"]["display_name"]
                                .as_str()
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let publication_year = work["publication_year"].as_u64();
            let venue = value_venue(work);
            let mut description_parts: Vec<String> = Vec::new();
            if !authors.is_empty() {
                description_parts.push(authors.join(", "));
            }
            if let Some(year) = publication_year {
                description_parts.push(format!("({year})"));
            }
            if let Some(venue) = venue.as_deref() {
                description_parts.push(venue.to_string());
            }
            Some(SearchResult {
                title,
                url,
                description: (!description_parts.is_empty()).then(|| description_parts.join(" — ")),
                source: venue,
                published: publication_year.map(|year| year.to_string()),
                provider: Some("openalex".to_string()),
            })
        })
        .collect()
}

fn value_venue(work: &serde_json::Value) -> Option<String> {
    work["primary_location"]["source"]["display_name"]
        .as_str()
        .map(str::to_string)
}

#[async_trait]
impl WebSearchProvider for OpenAlexProvider {
    fn kind(&self) -> &str {
        "openalex"
    }

    fn capabilities(&self) -> Vec<SearchCapability> {
        vec![SearchCapability::Semantic, SearchCapability::Keyword]
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        let params: Vec<(&str, String)> = vec![
            ("search", query.query.clone()),
            ("per_page", query.num_results.to_string()),
        ];
        let response = self
            .client
            .get(format!("{OPENALEX_API_BASE}/works"))
            .query(&params)
            .send()
            .await
            .map_err(|error| {
                WebError::ProviderUnavailable(format!("OpenAlex request failed: {error}"))
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            WebError::ProviderUnavailable(format!("OpenAlex body read failed: {error}"))
        })?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                429 => WebError::RateLimited(format!("OpenAlex rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "OpenAlex error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }
        Ok(ProviderSearchOutput {
            results: parse_openalex_works(&body),
            ..Default::default()
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        let response = self
            .client
            .get(format!("{OPENALEX_API_BASE}/works"))
            .query(&[("search", "test"), ("per_page", "1")])
            .send()
            .await
            .map_err(|error| {
                WebError::ProviderUnavailable(format!("OpenAlex health check failed: {error}"))
            })?;
        if response.status().is_success() || response.status().as_u16() == 429 {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "OpenAlex health check returned {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod openalex_tests {
    use super::*;

    const WORK_FIXTURE: &str = r#"{
        "id": "https://openalex.org/W2741809807",
        "doi": "https://doi.org/10.1038/S41586-024-00000-X",
        "title": "A typed identity for papers",
        "publication_year": 2024,
        "authorships": [
            {"author": {"display_name": "Ada Lovelace"}},
            {"author": {"display_name": "Grace Hopper"}}
        ],
        "primary_location": {"source": {"display_name": "Nature"}}
    }"#;

    const SEARCH_FIXTURE: &str = r#"{
        "meta": {"count": 2},
        "results": [
            {
                "id": "https://openalex.org/W1",
                "doi": "https://doi.org/10.1/first",
                "title": "First paper",
                "publication_year": 2023,
                "authorships": [{"author": {"display_name": "A. Author"}}],
                "primary_location": {"source": {"display_name": "Venue One"}}
            },
            {
                "id": "https://openalex.org/W2",
                "title": "Second paper"
            }
        ]
    }"#;

    #[test]
    fn parse_work_reads_metadata() {
        let work = parse_openalex_work(WORK_FIXTURE).expect("work parses");
        assert_eq!(work.openalex_id, "W2741809807");
        assert_eq!(work.doi.as_deref(), Some("10.1038/s41586-024-00000-x"));
        assert_eq!(work.title, "A typed identity for papers");
        assert_eq!(work.publication_year, Some(2024));
        assert_eq!(work.authors, vec!["Ada Lovelace", "Grace Hopper"]);
        assert_eq!(work.venue.as_deref(), Some("Nature"));
    }

    #[test]
    fn parse_work_requires_id_and_title() {
        // No id → None (a work record without an OpenAlex ID is not a work).
        assert!(parse_openalex_work(r#"{"title": "No id"}"#).is_none());
        // No title → None.
        assert!(parse_openalex_work(r#"{"id": "https://openalex.org/W1"}"#).is_none());
        // Malformed JSON → None.
        assert!(parse_openalex_work("not json").is_none());
        // Missing optional fields default, not fail.
        let work = parse_openalex_work(r#"{"id": "https://openalex.org/W2", "title": "T"}"#)
            .expect("minimal work parses");
        assert_eq!(work.doi, None);
        assert_eq!(work.publication_year, None);
        assert!(work.authors.is_empty());
        assert_eq!(work.venue, None);
    }

    #[test]
    fn parse_works_maps_search_results() {
        let results = parse_openalex_works(SEARCH_FIXTURE);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First paper");
        assert_eq!(results[0].url, "https://doi.org/10.1/first");
        assert_eq!(results[0].source.as_deref(), Some("Venue One"));
        assert_eq!(results[0].published.as_deref(), Some("2023"));
        assert_eq!(results[0].provider.as_deref(), Some("openalex"));
        // A work without a DOI falls back to its OpenAlex landing page.
        assert_eq!(results[1].url, "https://openalex.org/W2");
    }

    #[test]
    fn parse_works_tolerates_missing_and_malformed_arrays() {
        assert!(parse_openalex_works(r#"{"meta": {"count": 0}}"#).is_empty());
        assert!(parse_openalex_works("not json").is_empty());
        // Entries missing a title are filtered, not fatal.
        let results = parse_openalex_works(
            r#"{"results": [{"id": "https://openalex.org/W1"}, {"id": "https://openalex.org/W2", "title": "Ok"}]}"#,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Ok");
    }
}
