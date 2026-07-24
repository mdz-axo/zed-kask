use super::{ProviderSearchOutput, WebError, WebSearchProvider};
use crate::research::types::*;
use async_trait::async_trait;

/// SerpAPI provider — Google web/news search + YouTube transcript extraction.
///
/// Uses the same API key for all engines. When the query is a YouTube video ID
/// (11-character alphanumeric) or a youtube.com/watch?v= URL, routes to the
/// `youtube_video_transcript` engine. Otherwise uses Google search.
pub struct SerapiProvider {
    client: reqwest::Client,
    api_key: String,
}

impl SerapiProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: super::provider_http_client(),
            api_key,
        }
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
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => WebError::ProviderUnavailable(format!("SerpAPI auth error: {status}")),
                429 => WebError::RateLimited(format!("SerpAPI rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "SerpAPI transcript error {status}: {}",
                    body.chars().take(200).collect::<String>()
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
        "serpapi"
    }
    fn capabilities(&self) -> Vec<SearchCapability> {
        vec![
            SearchCapability::Keyword,
            SearchCapability::News,
            SearchCapability::Freshness,
            SearchCapability::Transcript,
        ]
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        // Route YouTube video IDs to transcript extraction
        if let Some(video_id) = Self::extract_video_id(&query.query) {
            return self.fetch_transcript(&video_id).await;
        }

        let mut params: Vec<(&str, String)> = vec![
            ("q", query.query.clone()),
            ("api_key", self.api_key.clone()),
            ("engine", "google".to_string()),
            ("num", query.num_results.to_string()),
            ("output", "json".to_string()),
        ];
        if !query.include_domains.is_empty() {
            params.push(("as_sitesearch", query.include_domains.join(",")));
        }
        if let Some(ref freshness) = query.freshness {
            let tbs = freshness_serpapi(freshness);
            if !tbs.is_empty() {
                params.push(("tbs", tbs));
            }
        }

        let resp = self
            .client
            .get(SERPAPI_BASE)
            .query(&params)
            .send()
            .await
            .map_err(|e| WebError::ProviderUnavailable(format!("SerpAPI request failed: {e}")))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => WebError::ProviderUnavailable(format!("SerpAPI auth error: {status}")),
                429 => WebError::RateLimited(format!("SerpAPI rate limited: {status}")),
                _ => WebError::ProviderError(format!(
                    "SerpAPI error {status}: {}",
                    body.chars().take(200).collect::<String>()
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
                    .filter_map(|item| {
                        Some(SearchResult {
                            title: item["title"].as_str()?.to_string(),
                            url: item["link"].as_str()?.to_string(),
                            description: item["snippet"].as_str().map(|s| s.to_string()),
                            source: Some("google".to_string()),
                            published: None,
                            provider: None,
                        })
                    })
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
