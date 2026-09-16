//! YouTube discovery and metadata via SerpApi.
//!
//! Search metadata belongs here, not in `YtDlpRunner`: SerpApi provides stable,
//! structured discovery fields without depending on a local extractor binary.
//! `yt-dlp` remains intentionally limited to `video_fetch`, where it retrieves
//! the selected video's media bytes for durable local publication.

use crate::types::YoutubeSearchRequest;
use crate::*;
use serde::Deserialize;

const SERPAPI_SEARCH_URL: &str = "https://serpapi.com/search";
const DEFAULT_MAX_RESULTS: usize = 20;
const MAX_RESULTS: usize = 50;

#[derive(Debug, Deserialize)]
struct SerpApiSearchResponse {
    #[serde(default)]
    video_results: Vec<SerpApiVideoResult>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SerpApiVideoResult {
    position_on_page: Option<u32>,
    title: String,
    link: String,
    video_id: Option<String>,
    channel: Option<SerpApiChannel>,
    published_date: Option<String>,
    views: Option<u64>,
    length: Option<String>,
    description: Option<String>,
    #[serde(default)]
    extensions: Vec<String>,
    thumbnail: Option<SerpApiThumbnail>,
}

#[derive(Debug, Deserialize)]
struct SerpApiChannel {
    name: String,
    verified: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SerpApiThumbnail {
    Url(String),
    Variants {
        #[serde(rename = "static")]
        static_url: Option<String>,
        rich: Option<String>,
    },
}

impl SerpApiThumbnail {
    fn url(self) -> Option<String> {
        match self {
            Self::Url(url) => Some(url),
            Self::Variants { static_url, rich } => static_url.or(rich),
        }
    }
}

fn duration_seconds(duration: &str) -> Option<u64> {
    let mut total = 0_u64;
    for part in duration.split(':') {
        total = total.checked_mul(60)?;
        total = total.checked_add(part.parse::<u64>().ok()?)?;
    }
    Some(total)
}

fn serpapi_error(message: String) -> McpToolError {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("api key") || normalized.contains("unauthorized") {
        McpToolError::permission_denied(format!(
            "SerpApi YouTube search authorization failed; check HKASK_SERPAPI_API_KEY: {message}"
        ))
    } else {
        McpToolError::unavailable(format!("SerpApi YouTube search failed: {message}"))
    }
}

fn parse_search_response(
    body: &str,
    max_results: usize,
) -> Result<serde_json::Value, McpToolError> {
    let response: SerpApiSearchResponse = serde_json::from_str(body).map_err(|error| {
        McpToolError::unavailable(format!(
            "SerpApi YouTube search returned malformed JSON: {error}"
        ))
    })?;
    if let Some(error) = response.error {
        return Err(serpapi_error(error));
    }

    let videos = response
        .video_results
        .into_iter()
        .take(max_results)
        .map(|video| {
            let duration_seconds = video.length.as_deref().and_then(duration_seconds);
            let (channel, channel_verified) = video
                .channel
                .map(|channel| (Some(channel.name), channel.verified.unwrap_or(false)))
                .unwrap_or((None, false));
            serde_json::json!({
                "position": video.position_on_page,
                "title": video.title,
                "url": video.link,
                "video_id": video.video_id,
                "channel": channel,
                "channel_verified": channel_verified,
                "published_date": video.published_date,
                "views": video.views,
                "duration": video.length,
                "duration_seconds": duration_seconds,
                "description": video.description,
                "extensions": video.extensions,
                "thumbnail_url": video.thumbnail.and_then(SerpApiThumbnail::url),
            })
        })
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "provider": "serpapi",
        "count": videos.len(),
        "videos": videos,
    }))
}

#[tool_router(router = youtube_router, vis = "pub")]
impl MediaServer {
    /// Search YouTube and return structured metadata from SerpApi.
    ///
    /// This path deliberately does not invoke `yt-dlp`. Use `video_fetch` only
    /// after selecting a result when a durable local media Asset is required.
    #[tool(
        description = "Search YouTube through SerpApi and return structured video metadata including views, duration, channel, publication date, provider extensions such as 4K or CC, and URL. Does not download video media; use video_fetch for that."
    )]
    pub async fn youtube_search(
        &self,
        Parameters(YoutubeSearchRequest { query, max_results }): Parameters<YoutubeSearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "youtube_search", async {
            if query.trim().is_empty() {
                return Err(McpToolError::invalid_argument("query must not be empty"));
            }
            let max_results = max_results.unwrap_or(DEFAULT_MAX_RESULTS as u32) as usize;
            if !(1..=MAX_RESULTS).contains(&max_results) {
                return Err(McpToolError::invalid_argument(format!(
                    "max_results must be between 1 and {MAX_RESULTS}"
                )));
            }
            let api_key = self.serpapi_key.as_deref().ok_or_else(|| {
                McpToolError::permission_denied("youtube_search requires HKASK_SERPAPI_API_KEY")
            })?;

            let response = reqwest::Client::new()
                .get(SERPAPI_SEARCH_URL)
                .query(&[
                    ("engine", "youtube"),
                    ("search_query", query.trim()),
                    ("api_key", api_key),
                ])
                .send()
                .await
                .map_err(|error| {
                    McpToolError::unavailable(format!(
                        "SerpApi YouTube search request failed: {}",
                        error.without_url()
                    ))
                })?;
            let status = response.status();
            let body = response.text().await.map_err(|error| {
                McpToolError::unavailable(format!(
                    "SerpApi YouTube search body read failed: {error}"
                ))
            })?;
            if !status.is_success() {
                let message = format!("HTTP {status}: {}", body.trim());
                if matches!(status.as_u16(), 401 | 403) {
                    return Err(McpToolError::permission_denied(format!(
                        "SerpApi YouTube search authorization failed; check HKASK_SERPAPI_API_KEY: {message}"
                    )));
                }
                return Err(McpToolError::unavailable(format!(
                    "SerpApi YouTube search failed: {message}"
                )));
            }

            parse_search_response(&body, max_results)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranking_metadata_without_downloader_fields() -> Result<(), Box<dyn std::error::Error>>
    {
        let body = r#"{
            "video_results": [{
                "position_on_page": 3,
                "title": "Kurt Vonnegut on the Shapes of Stories",
                "link": "https://www.youtube.com/watch?v=example",
                "video_id": "example",
                "channel": {"name": "Archive", "verified": true},
                "published_date": "10 years ago",
                "views": 2199896,
                "length": "7:49",
                "description": "Vonnegut explains story graphs.",
                "extensions": ["HD", "CC"],
                "thumbnail": {"static": "https://example.test/thumb.jpg"}
            }]
        }"#;

        let parsed = parse_search_response(body, 20)?;
        assert_eq!(parsed["provider"], "serpapi");
        assert_eq!(parsed["videos"][0]["views"], 2_199_896);
        assert_eq!(parsed["videos"][0]["duration_seconds"], 469);
        assert_eq!(parsed["videos"][0]["channel_verified"], true);
        assert_eq!(parsed["videos"][0]["extensions"][0], "HD");
        Ok(())
    }

    #[test]
    fn surfaces_provider_error_instead_of_empty_results() {
        let error = parse_search_response(r#"{"error":"Invalid API key"}"#, 20)
            .expect_err("provider error must surface");
        assert!(error.to_string().contains("permission_denied"));
        assert!(error.to_string().contains("HKASK_SERPAPI_API_KEY"));
    }

    #[test]
    fn parses_hour_and_minute_durations() {
        assert_eq!(duration_seconds("7:49"), Some(469));
        assert_eq!(duration_seconds("1:02:03"), Some(3723));
        assert_eq!(duration_seconds("live"), None);
    }
}
