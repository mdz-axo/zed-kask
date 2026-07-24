//! Feed fetching and autodiscovery utilities.

use reqwest::Client;

use crate::research::providers::validate_provider_url;
use crate::research::rss_types::FetchResult;

/// Fetch an RSS/Atom feed with conditional GET (ETag/Last-Modified).
///
/// Returns a `FetchResult` with the parsed feed and updated cache headers.
/// A 304 Not Modified response returns an empty feed with `status: 304`.
pub async fn fetch_feed(
    client: &Client,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<FetchResult, anyhow::Error> {
    let mut request = client.get(url);
    if let Some(e) = etag {
        request = request.header("If-None-Match", e);
    }
    if let Some(lm) = last_modified {
        request = request.header("If-Modified-Since", lm);
    }

    let response = request.send().await?;
    let status = response.status().as_u16();

    if status == 304 {
        let empty_feed = feed_rs::parser::parse(std::io::empty())?;
        return Ok(FetchResult {
            feed: empty_feed,
            etag: None,
            last_modified: None,
            status,
        });
    }

    if !response.status().is_success() {
        anyhow::bail!("HTTP {} fetching {}", response.status(), url);
    }

    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let last_modified = response
        .headers()
        .get("last-modified")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = response.bytes().await?;
    let feed = feed_rs::parser::parse(body.as_ref())?;

    Ok(FetchResult {
        feed,
        etag,
        last_modified,
        status,
    })
}

/// Discover RSS/Atom feeds from a URL via HTML link autodiscovery.
///
/// If the URL itself serves a feed (content-type indicates RSS/Atom),
/// returns it directly. Otherwise parses the HTML for `<link rel="alternate">`
/// tags with `application/rss+xml` or `application/atom+xml` types.
pub async fn discover_feeds(
    client: &Client,
    url: &str,
) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    // SSRF defense: validate the URL before fetching. discover_feeds is
    // reachable from the rss_discover_feeds tool, so user-supplied URLs
    // must pass the same validation as web_extract/web_browse.
    validate_provider_url(url).map_err(|e| anyhow::anyhow!("URL validation failed: {e}"))?;
    let response = client.get(url).send().await?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    if content_type.contains("rss")
        || content_type.contains("atom")
        || content_type.contains("feed")
    {
        return Ok(vec![serde_json::json!({
            "url": url,
            "type": "feed",
            "content_type": content_type,
        })]);
    }

    let body = response.text().await?;
    let mut feeds = Vec::new();

    let re1 = regex::Regex::new(
        r#"<link[^>]+rel\s*=\s*["']alternate["'][^>]+type\s*=\s*["']application/(rss|atom)\+xml["'][^>]+href\s*=\s*["']([^"']+)["']"#,
    )?;
    let re2 = regex::Regex::new(
        r#"<link[^>]+type\s*=\s*["']application/(rss|atom)\+xml["'][^>]+href\s*=\s*["']([^"']+)["']"#,
    )?;

    for re in [&re1, &re2] {
        for cap in re.captures_iter(&body) {
            let feed_type = cap.get(1).map(|m| m.as_str()).unwrap_or("rss");
            let href = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let feed_url = if href.starts_with("http") {
                href.to_string()
            } else {
                let base = reqwest::Url::parse(url)?;
                base.join(href)
                    .map(|u| u.to_string())
                    .unwrap_or_else(|_| href.to_string())
            };
            if !feeds
                .iter()
                .any(|f: &serde_json::Value| f["url"].as_str() == Some(feed_url.as_str()))
            {
                feeds.push(serde_json::json!({
                    "url": feed_url,
                    "type": feed_type,
                }));
            }
        }
    }

    Ok(feeds)
}
