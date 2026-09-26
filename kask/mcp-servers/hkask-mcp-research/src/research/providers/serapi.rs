use super::{ProviderSearchOutput, WebError, WebSearchProvider};
use crate::research::types::*;
use async_trait::async_trait;

/// SerpAPI provider — Google web/news search, YouTube transcript extraction,
/// and the Google Scholar and Google Books engines.
///
/// Uses the same API key for all engines. On the Google engine, a query that
/// is a YouTube video ID (11-character alphanumeric) or a youtube.com/watch?v=
/// URL routes to the `youtube_video_transcript` engine. The Scholar and Books
/// instances are separate provider kinds (`google_scholar`, `google_books`)
/// that run only when named explicitly, so fused web searches never call them.
pub(crate) struct SerapiProvider {
    client: reqwest::Client,
    api_key: String,
    engine: SerpEngine,
}

/// Which SerpAPI engine an instance queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SerpEngine {
    Google,
    Scholar,
    Books,
}

impl SerapiProvider {
    pub fn new(api_key: String) -> Result<Self, WebError> {
        Self::with_engine(api_key, SerpEngine::Google)
    }

    pub fn with_engine(api_key: String, engine: SerpEngine) -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
            api_key,
            engine,
        })
    }

    /// Engine-specific request parameters. Google Books is Google search with
    /// `tbm=bks`; Google Scholar is its own SerpAPI engine.
    fn engine_params(&self, query: &SearchQuery) -> Vec<(&'static str, String)> {
        match self.engine {
            SerpEngine::Google => {
                let mut params = vec![("engine", "google".to_string())];
                if !query.include_domains.is_empty() {
                    params.push(("as_sitesearch", query.include_domains.join(",")));
                }
                if let Some(ref freshness) = query.freshness {
                    let tbs = freshness_serpapi(freshness);
                    if !tbs.is_empty() {
                        params.push(("tbs", tbs));
                    }
                }
                params
            }
            SerpEngine::Scholar => vec![("engine", "google_scholar".to_string())],
            SerpEngine::Books => vec![("engine", "google".to_string()), ("tbm", "bks".to_string())],
        }
    }

    /// Map one `organic_results` item. Scholar results carry the publication
    /// summary as `source` and the citation count in `description`, so callers
    /// can weigh scholarly uptake.
    fn organic_result(&self, item: &serde_json::Value) -> Option<SearchResult> {
        let title = item["title"].as_str()?.to_string();
        let url = item["link"].as_str()?.to_string();
        let snippet = item["snippet"].as_str().map(|s| s.to_string());
        let (description, source, published) = match self.engine {
            SerpEngine::Google => (snippet, Some("google".to_string()), None),
            SerpEngine::Scholar => {
                let cited_by = item["inline_links"]["cited_by"]["total"].as_u64();
                let description = match (cited_by, snippet) {
                    (Some(n), Some(s)) => Some(format!("[cited by {n}] {s}")),
                    (Some(n), None) => Some(format!("[cited by {n}]")),
                    (None, s) => s,
                };
                let source = item["publication_info"]["summary"]
                    .as_str()
                    .map(|s| s.to_string());
                (description, source, None)
            }
            SerpEngine::Books => (
                snippet,
                Some("google_books".to_string()),
                item["date"].as_str().map(|s| s.to_string()),
            ),
        };
        Some(SearchResult {
            title,
            url,
            description,
            source,
            published,
            provider: None,
        })
    }

    /// Extract a YouTube video ID from a URL or raw ID string.
    fn extract_video_id(query: &str) -> Option<String> {
        // Direct 11-char video ID (alphanumeric + _ -)
        if query.len() == 11
            && query
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Some(query.to_string());
        }
        // youtube.com/watch?v=VIDEO_ID
        if let Some(pos) = query.find("v=") {
            let after = &query[pos + 2..];
            let id: String = after.chars().take(11).collect();
            if id.len() == 11
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Some(id);
            }
        }
        // youtu.be/VIDEO_ID
        if let Some(pos) = query.find("youtu.be/") {
            let after = &query[pos + 9..];
            let id: String = after.chars().take(11).collect();
            if id.len() == 11
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Some(id);
            }
        }
        None
    }

    /// Fetch a YouTube transcript via SerpAPI's youtube_video_transcript engine.
    async fn fetch_transcript(&self, video_id: &str) -> Result<ProviderSearchOutput, WebError> {
        let params: Vec<(&str, String)> = vec![
            ("v", video_id.to_string()),
            ("api_key", self.api_key.clone()),
            ("engine", "youtube_video_transcript".to_string()),
        ];

        let resp = self
            .client
            .get(SERPAPI_BASE)
            .query(&params)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("SerpAPI transcript request failed: {e}"))
            })?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Serapi body read failed: {e}")))?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => WebError::ProviderUnavailable(format!("SerpAPI auth error: {status}")),
                429 => WebError::RateLimited(format!("SerpAPI rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "SerpAPI transcript error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse SerpAPI transcript response: {e}"))
        })?;

        // Transcript segments: each has "snippet", "start_ms", "end_ms"
        let transcript_text = parsed["transcript"]
            .as_array()
            .map(|segments| {
                segments
                    .iter()
                    .filter_map(|seg| seg["snippet"].as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        let title = parsed["title"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("YouTube transcript: {video_id}"));

        let video_url = format!("https://www.youtube.com/watch?v={video_id}");

        if transcript_text.is_empty() {
            return Ok(ProviderSearchOutput {
                results: vec![SearchResult {
                    title,
                    url: video_url,
                    description: Some("No transcript available for this video".to_string()),
                    source: Some("youtube".to_string()),
                    published: None,
                    provider: Some("serpapi_transcript".to_string()),
                }],
                ..Default::default()
            });
        }

        let word_count = transcript_text.split_whitespace().count();
        let description = Some(format!(
            "[Transcript: {word_count} words] {}",
            transcript_text.chars().take(500).collect::<String>()
        ));

        Ok(ProviderSearchOutput {
            results: vec![SearchResult {
                title,
                url: video_url.clone(),
                description,
                source: Some("youtube".to_string()),
                published: None,
                provider: Some("serpapi_transcript".to_string()),
            }],
            // Store full transcript in content_previews for downstream extraction
            content_previews: {
                let mut map = std::collections::HashMap::new();
                map.insert(video_url.to_lowercase(), transcript_text);
                map
            },
            ..Default::default()
        })
    }
}
#[async_trait]
impl WebSearchProvider for SerapiProvider {
    fn kind(&self) -> &str {
        match self.engine {
            SerpEngine::Google => "serpapi",
            SerpEngine::Scholar => "google_scholar",
            SerpEngine::Books => "google_books",
        }
    }
    fn capabilities(&self) -> Vec<SearchCapability> {
        match self.engine {
            SerpEngine::Google => vec![
                SearchCapability::Keyword,
                SearchCapability::News,
                SearchCapability::Freshness,
                SearchCapability::Transcript,
            ],
            SerpEngine::Scholar | SerpEngine::Books => vec![SearchCapability::Semantic],
        }
    }
    fn explicit_only(&self) -> bool {
        self.engine != SerpEngine::Google
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        // Route YouTube video IDs to transcript extraction (Google engine only)
        if self.engine == SerpEngine::Google
            && let Some(video_id) = Self::extract_video_id(&query.query)
        {
            return self.fetch_transcript(&video_id).await;
        }

        // SerpAPI's Google Scholar engine caps `num` at 20.
        let num = match self.engine {
            SerpEngine::Scholar => query.num_results.min(20),
            SerpEngine::Google | SerpEngine::Books => query.num_results,
        };
        let mut params: Vec<(&str, String)> = vec![
            ("q", query.query.clone()),
            ("api_key", self.api_key.clone()),
            ("num", num.to_string()),
            ("output", "json".to_string()),
        ];
        params.extend(self.engine_params(query));

        let resp = self
            .client
            .get(SERPAPI_BASE)
            .query(&params)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("SerpAPI request failed: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("Serapi body read failed: {e}")))?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => WebError::ProviderUnavailable(format!("SerpAPI auth error: {status}")),
                429 => WebError::RateLimited(format!("SerpAPI rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "SerpAPI error {status}: {}",
                    hkask_inference::openai_compat::sanitize_error_body(&body)
                )),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse SerpAPI response: {e}"))
        })?;

        let organic = parsed["organic_results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| self.organic_result(item))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let news = parsed["news_results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        Some(SearchResult {
                            title: item["title"].as_str()?.to_string(),
                            url: item["link"].as_str()?.to_string(),
                            description: item["snippet"].as_str().map(|s| s.to_string()),
                            source: item["source"].as_str().map(|s| s.to_string()),
                            published: item["date"].as_str().map(|s| s.to_string()),
                            provider: None,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut results = organic;
        results.extend(news);

        let answer_box = parsed["answer_box"].as_object().map(|ab| AnswerBox {
            title: ab
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            snippet: ab
                .get("snippet")
                .or_else(|| ab.get("answer"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            url: ab
                .get("link")
                .or_else(|| ab.get("displayed_link"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        });

        let related_questions: Vec<String> = parsed["related_questions"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item["question"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        Ok(ProviderSearchOutput {
            results,
            answer_box,
            related_questions,
            ..Default::default()
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Lightweight liveness check: send a minimal Google search request.
        // A 429 means the service is alive but rate-limited (healthy).
        let params: Vec<(&str, String)> = vec![
            ("q", "test".to_string()),
            ("api_key", self.api_key.clone()),
            ("engine", "google".to_string()),
            ("num", "1".to_string()),
            ("output", "json".to_string()),
        ];
        let resp = self
            .client
            .get(SERPAPI_BASE)
            .query(&params)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("SerpAPI health check failed: {e}"))
            })?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 429 {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "SerpAPI health check returned {status}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn query(domains: &[&str]) -> SearchQuery {
        SearchQuery {
            query: "oilfield services pricing power".to_string(),
            num_results: 50,
            include_domains: domains.iter().map(|d| d.to_string()).collect(),
            exclude_domains: Vec::new(),
            freshness: None,
        }
    }

    fn provider(engine: SerpEngine) -> Result<SerapiProvider, WebError> {
        SerapiProvider::with_engine("test-key".to_string(), engine)
    }

    /// expect: each engine sends SerpAPI's documented selector — google,
    /// google_scholar, or google with tbm=bks — and only the web engine
    /// forwards the domain allowlist.
    #[test]
    fn engines_send_documented_serpapi_selectors() -> Result<(), WebError> {
        let q = query(&["hbs.edu"]);
        assert_eq!(
            provider(SerpEngine::Google)?.engine_params(&q),
            vec![
                ("engine", "google".to_string()),
                ("as_sitesearch", "hbs.edu".to_string())
            ]
        );
        assert_eq!(
            provider(SerpEngine::Scholar)?.engine_params(&q),
            vec![("engine", "google_scholar".to_string())]
        );
        assert_eq!(
            provider(SerpEngine::Books)?.engine_params(&q),
            vec![
                ("engine", "google".to_string()),
                ("tbm", "bks".to_string())
            ]
        );
        Ok(())
    }

    /// expect: the specialty engines are distinct explicit-only kinds, so a
    /// fused or quick web search can never call (or bill) them.
    #[test]
    fn specialty_engines_are_explicit_only_kinds() -> Result<(), WebError> {
        let google = provider(SerpEngine::Google)?;
        let scholar = provider(SerpEngine::Scholar)?;
        let books = provider(SerpEngine::Books)?;
        assert_eq!(
            (google.kind(), scholar.kind(), books.kind()),
            ("serpapi", "google_scholar", "google_books")
        );
        assert!(!google.explicit_only());
        assert!(scholar.explicit_only() && books.explicit_only());
        assert!(!scholar.capabilities().contains(&SearchCapability::Keyword));
        Ok(())
    }

    /// expect: a Scholar result keeps its publication summary and citation
    /// count (from SerpAPI's documented organic_results shape); a result
    /// without a link is dropped, not invented.
    #[test]
    fn scholar_results_carry_publication_and_citations() -> Result<(), WebError> {
        let scholar = provider(SerpEngine::Scholar)?;
        let item = json!({
            "title": "Population biology of plants.",
            "link": "https://www.cabdirect.org/cabdirect/abstract/19782321379",
            "snippet": "The first chapter is concerned with experiments",
            "publication_info": {"summary": "JL Harper - Population biology of plants., 1977 - cabdirect.org"},
            "inline_links": {"cited_by": {"total": 14003}}
        });
        let result = scholar.organic_result(&item).ok_or(WebError::NoProvider)?;
        assert_eq!(
            result.source.as_deref(),
            Some("JL Harper - Population biology of plants., 1977 - cabdirect.org")
        );
        assert_eq!(
            result.description.as_deref(),
            Some("[cited by 14003] The first chapter is concerned with experiments")
        );
        assert!(
            scholar
                .organic_result(&json!({"title": "Circular statistics in biology."}))
                .is_none()
        );
        let books = provider(SerpEngine::Books)?;
        let book = books
            .organic_result(&json!({"title": "Hidden Champions", "link": "https://books.google.com/books?id=x", "date": "2009"}))
            .ok_or(WebError::NoProvider)?;
        assert_eq!(book.source.as_deref(), Some("google_books"));
        assert_eq!(book.published.as_deref(), Some("2009"));
        Ok(())
    }
}
