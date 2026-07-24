//! Search helpers for the discovery pipeline: MCP web_search and YouTube transcripts.

use super::types::{DiscoveredWork, USER_AGENT};
use super::utils::slugify;
use hkask_capability::DelegationToken;
use hkask_capability::ToolPort;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

// ── MCP web_search ─────────────────────────────────────────────────────────

/// Call the MCP server's web_search tool and parse results into DiscoveredWork structs.
pub(crate) async fn mcp_search(
    mcp: &dyn ToolPort,
    token: &DelegationToken,
    query: &str,
    num_results: usize,
    strategy: &str,
) -> Result<Vec<DiscoveredWork>, ServiceError> {
    let input = serde_json::json!({
        "query": query,
        "strategy": strategy,
        "num_results": num_results,
    });

    let server_id = mcp
        .get_tool_info("web_search")
        .await
        .map(|info| info.server_id)
        .ok_or_else(|| ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: "MCP web_search tool is not registered".to_string(),
        })?;
    let result = mcp
        .invoke(&server_id, "web_search", input, token)
        .await
        .map_err(|e| {
            let msg = format!("MCP web_search failed: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    tracing::debug!(target: "hkask.discover", query = %query, has_results = result.get("results").is_some(), result_keys = ?result.as_object().map(|o| o.keys().collect::<Vec<_>>()), "MCP search response");

    let payload = result.get("content").unwrap_or(&result);

    let results = payload["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let title = item["title"].as_str()?.to_string();
                    let url = item["url"].as_str()?.to_string();
                    let mut source = item["source"].as_str().unwrap_or("web").to_lowercase();

                    if source != "arxiv"
                        && source != "semantic_scholar"
                        && let Some(providers) = item["providers"].as_array()
                    {
                        let provider_strs: Vec<&str> =
                            providers.iter().filter_map(|p| p.as_str()).collect();
                        if provider_strs.contains(&"arxiv") {
                            source = "arxiv".to_string();
                        } else if provider_strs.contains(&"semantic_scholar") {
                            source = "semantic_scholar".to_string();
                        }
                    }
                    let published = item["published"].as_str().map(|s| s.to_string());
                    let year = published.as_ref().and_then(|d| d[..4].parse::<u16>().ok());

                    if title.is_empty() || url.is_empty() {
                        return None;
                    }

                    let work_type = match source.as_str() {
                        "semantic_scholar" => "journal_article",
                        "arxiv" => "preprint",
                        _ => "web_page",
                    };

                    let abstract_text = item["abstract"]
                        .as_str()
                        .or_else(|| item["snippet"].as_str())
                        .or_else(|| item["description"].as_str())
                        .map(|s| s.to_string());

                    Some(DiscoveredWork {
                        slug: slugify(&title),
                        title,
                        url,
                        year,
                        source,
                        work_type: work_type.to_string(),
                        abstract_text,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(results)
}

// ── YouTube transcript search (SerpAPI) ─────────────────────────────────────

const SERPAPI_BASE: &str = "https://serpapi.com/search";

/// Search YouTube for videos matching the query and fetch their transcripts.
pub(crate) async fn search_youtube_transcripts(
    query: &str,
    api_key: &str,
    limit: usize,
) -> Result<Vec<DiscoveredWork>, ServiceError> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| {
            let msg = format!("HTTP client build failed: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    // Step 1: Search YouTube for videos
    let params: Vec<(&str, String)> = vec![
        ("q", query.to_string()),
        ("api_key", api_key.to_string()),
        ("engine", "youtube".to_string()),
        ("num", limit.to_string()),
    ];

    let resp = client
        .get(SERPAPI_BASE)
        .query(&params)
        .send()
        .await
        .map_err(|e| {
            let msg = format!("SerpAPI YouTube search failed: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    let body = resp.text().await.unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);

    let video_results = parsed["video_results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|video| {
                    let title = video["title"].as_str()?.to_string();
                    let link = video["link"].as_str()?.to_string();
                    let video_id = extract_youtube_id(&link)?;
                    Some((title, video_id))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if video_results.is_empty() {
        return Ok(vec![]);
    }

    // Step 2: Fetch transcript for each video in parallel
    let mut handles = Vec::new();
    for (title, video_id) in video_results {
        let client = client.clone();
        let api_key = api_key.to_string();
        handles.push(tokio::spawn(async move {
            match fetch_youtube_transcript(&client, &api_key, &video_id, &title).await {
                Ok(Some(work)) => Some(work),
                Ok(None) => {
                    tracing::info!(target: "hkask.discover", video_id = %video_id, title = %title, "No transcript available for video — skipping");
                    None
                }
                Err(e) => {
                    tracing::warn!(target: "hkask.discover", video_id = %video_id, error = %e, "Failed to fetch transcript — skipping");
                    None
                }
            }
        }));
    }

    let mut transcripts: Vec<DiscoveredWork> = Vec::new();
    for handle in handles {
        if let Ok(Some(work)) = handle.await {
            transcripts.push(work);
        }
    }

    Ok(transcripts)
}

fn extract_youtube_id(url: &str) -> Option<String> {
    if let Some(pos) = url.find("v=") {
        let after = &url[pos + 2..];
        let id: String = after.chars().take(11).collect();
        if id.len() == 11
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Some(id);
        }
    }
    if let Some(pos) = url.find("youtu.be/") {
        let after = &url[pos + 9..];
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

async fn fetch_youtube_transcript(
    client: &reqwest::Client,
    api_key: &str,
    video_id: &str,
    title: &str,
) -> Result<Option<DiscoveredWork>, ServiceError> {
    let params: Vec<(&str, String)> = vec![
        ("v", video_id.to_string()),
        ("api_key", api_key.to_string()),
        ("engine", "youtube_video_transcript".to_string()),
    ];

    let resp = client
        .get(SERPAPI_BASE)
        .query(&params)
        .send()
        .await
        .map_err(|e| {
            let msg = format!("SerpAPI transcript request failed: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: format!("SerpAPI transcript error {status} for video '{video_id}'"),
        });
    }

    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);

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

    if transcript_text.is_empty() {
        return Ok(None);
    }

    let video_url = format!("https://www.youtube.com/watch?v={video_id}");
    let video_title = parsed["title"].as_str().unwrap_or(title).to_string();

    Ok(Some(DiscoveredWork {
        slug: slugify(&video_title),
        title: video_title,
        url: video_url,
        year: None,
        source: "youtube_transcript".to_string(),
        work_type: "transcript".to_string(),
        abstract_text: Some(String::new()),
    }))
}
