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
    /// The API base; a test points it at a fixture.
    base: String,
}

impl OpenAlexProvider {
    pub fn new() -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
            base: OPENALEX_API_BASE.to_string(),
        })
    }

    /// A provider that sends its requests to `base` (a test fixture).
    #[cfg(test)]
    pub(crate) fn at(base: &str) -> Self {
        let mut provider = Self::new().expect("provider");
        provider.base = base.to_string();
        provider
    }

    /// The direct-lookup path for an identifier.
    ///
    /// An arXiv identifier is looked up by the DOI arXiv registers for it,
    /// `10.48550/arxiv.<id>`, without the version suffix: OpenAlex holds
    /// the unversioned DOI and answers 404 for a versioned one (measured
    /// 2026-11-12: `1706.03762` found, `1706.03762v5` not found).
    fn work_path(&self, id: &PaperId) -> String {
        let base = &self.base;
        match id {
            PaperId::Doi(doi) => format!("{base}/works/doi:{doi}"),
            PaperId::OpenAlex(openalex_id) => format!("{base}/works/{openalex_id}"),
            PaperId::Pmid(pmid) => format!("{base}/works/pmid:{pmid}"),
            PaperId::Pmcid(pmcid) => format!("{base}/works/pmcid:{pmcid}"),
            PaperId::Arxiv(arxiv_id) => format!(
                "{base}/works/doi:10.48550/arxiv.{}",
                arxiv_id_without_version(arxiv_id)
            ),
        }
    }

    /// Send a GET and read the body — the shared transport spine of the
    /// resolve and search surfaces (transport failures name the surface).
    /// The status classification stays per-caller: resolve intercepts 404
    /// as its no-record case before classifying, search classifies straight.
    async fn send_read(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<(reqwest::StatusCode, String), WebError> {
        let response = request.send().await.map_err(|error| {
            WebError::ProviderUnavailable(format!("OpenAlex request failed: {error}"))
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            WebError::ProviderUnavailable(format!("OpenAlex body read failed: {error}"))
        })?;
        Ok((status, body))
    }

    /// Resolve a typed identifier to its OpenAlex work record. Ok(None)
    /// when no record exists (404).
    pub(crate) async fn resolve(&self, id: &PaperId) -> Result<Option<PaperMetadata>, WebError> {
        let path = self.work_path(id);
        let (status, body) = self.send_read(self.client.get(&path)).await?;
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

/// An arXiv identifier with its `vN` version suffix removed. The suffix is
/// recognised the way `parse_paper_id` recognises it: a `v` followed only
/// by digits, at the end.
fn arxiv_id_without_version(arxiv_id: &str) -> &str {
    match arxiv_id.rsplit_once('v') {
        Some((base, version))
            if !base.is_empty()
                && !version.is_empty()
                && version.chars().all(|digit| digit.is_ascii_digit()) =>
        {
            base
        }
        _ => arxiv_id,
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
    let oa_pdf_url = work_oa_pdf_url(&value);
    Some(PaperMetadata {
        openalex_id,
        doi,
        title,
        publication_year,
        authors,
        venue,
        oa_pdf_url,
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
                oa_pdf_url: work_oa_pdf_url(work),
                provider: Some("openalex".to_string()),
            })
        })
        .collect()
}

/// Candidate open-access copy URL from an OpenAlex work record:
/// `best_oa_location.pdf_url` (the direct PDF) falling back to
/// `open_access.oa_url` (a landing page for the OA copy). A candidate, not
/// a verified document — the copy can be a different version than the
/// cited work, so the consumer verifies identity before use.
fn work_oa_pdf_url(work: &serde_json::Value) -> Option<String> {
    work["best_oa_location"]["pdf_url"]
        .as_str()
        .or_else(|| work["open_access"]["oa_url"].as_str())
        .map(str::to_string)
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
        let (status, body) = self
            .send_read(
                self.client
                    .get(format!("{}/works", self.base))
                    .query(&params),
            )
            .await?;
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
            .get(format!("{}/works", self.base))
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
    fn parse_work_reads_candidate_oa_pdf_url() {
        // best_oa_location.pdf_url is the primary candidate (the direct
        // PDF); open_access.oa_url is the fallback (a landing page for the
        // OA copy); absent → None. The field is a CANDIDATE copy, not a
        // verified document — the consumer verifies identity (2026-09-30
        // zk-reference lesson L2: the provider holds the OA copy while the
        // result URL is the often-paywalled DOI landing page).
        let work = parse_openalex_work(
            r#"{"id": "https://openalex.org/W1", "title": "T",
                 "best_oa_location": {"pdf_url": "https://repo.example.org/paper.pdf"}}"#,
        )
        .expect("work with best_oa_location parses");
        assert_eq!(
            work.oa_pdf_url.as_deref(),
            Some("https://repo.example.org/paper.pdf")
        );

        let work = parse_openalex_work(
            r#"{"id": "https://openalex.org/W1", "title": "T",
                 "open_access": {"oa_url": "https://repo.example.org/landing"}}"#,
        )
        .expect("work with open_access fallback parses");
        assert_eq!(
            work.oa_pdf_url.as_deref(),
            Some("https://repo.example.org/landing")
        );

        let work = parse_openalex_work(WORK_FIXTURE).expect("fixture work parses");
        assert_eq!(work.oa_pdf_url, None, "no OA fields → no candidate URL");
    }

    #[test]
    fn parse_works_maps_candidate_oa_pdf_url() {
        let results = parse_openalex_works(
            r#"{"results": [{
                "id": "https://openalex.org/W1",
                "doi": "https://doi.org/10.1/first",
                "title": "First paper",
                "best_oa_location": {"pdf_url": "https://repo.example.org/first.pdf"}
            }, {
                "id": "https://openalex.org/W2", "title": "Second paper"
            }]}"#,
        );
        assert_eq!(
            results[0].oa_pdf_url.as_deref(),
            Some("https://repo.example.org/first.pdf")
        );
        assert_eq!(results[1].oa_pdf_url, None);
    }

    /// expect: an arXiv identifier is looked up by arXiv's DOI for it,
    /// without the version suffix. Before, no request was made for an
    /// arXiv identifier and the caller was told OpenAlex has no record.
    #[tokio::test]
    async fn an_arxiv_identifier_is_looked_up_by_its_unversioned_arxiv_doi() {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{WORK_FIXTURE}",
            WORK_FIXTURE.len()
        )
        .into_bytes();
        let (base, requests) = crate::test_support::http_fixture_capturing_requests(response).await;
        let provider = OpenAlexProvider::at(&base);
        for arxiv_id in ["1706.03762v5", "1706.03762"] {
            let work = provider
                .resolve(&PaperId::Arxiv(arxiv_id.to_string()))
                .await
                .expect("lookup succeeds")
                .expect("a record");
            assert_eq!(work.openalex_id, "W2741809807", "{arxiv_id}");
        }
        let targets: Vec<String> = requests
            .lock()
            .expect("requests")
            .iter()
            .map(|request| request.target.clone())
            .collect();
        assert_eq!(
            targets,
            [
                "/works/doi:10.48550/arxiv.1706.03762",
                "/works/doi:10.48550/arxiv.1706.03762"
            ]
        );
    }

    /// expect: only a trailing `v` followed by digits is a version.
    #[test]
    fn only_a_trailing_version_suffix_is_removed() {
        assert_eq!(arxiv_id_without_version("2401.12345v12"), "2401.12345");
        assert_eq!(arxiv_id_without_version("2401.12345"), "2401.12345");
        assert_eq!(arxiv_id_without_version("2401.12345v"), "2401.12345v");
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
