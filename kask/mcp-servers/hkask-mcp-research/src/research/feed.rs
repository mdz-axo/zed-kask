//! Feed fetching and autodiscovery utilities.

use reqwest::Client;

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

/// THROWAWAY RED-evidence probe — deleted after evidence capture. Demonstrates
/// that `rss_discover_feeds`'s current client (a plain default reqwest client)
/// silently follows a redirect into a loopback destination.
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::discover_feeds;
    use crate::research::validated_fetch_client;

    async fn http_fixture(response: Arc<Vec<u8>>) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let address = listener.local_addr().expect("fixture address");
        let hits = Arc::new(AtomicUsize::new(0));
        let server_hits = hits.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                server_hits.fetch_add(1, Ordering::SeqCst);
                let response = response.clone();
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0u8; 512];
                    loop {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                buffer.extend_from_slice(&chunk[..n]);
                                if buffer.ends_with(b"\r\n\r\n") {
                                    break;
                                }
                            }
                        }
                    }
                    let _ = socket.write_all(&response).await;
                });
            }
        });
        (format!("http://{address}"), hits)
    }

    /// expect: "Feed discovery must not follow a redirect into loopback
    /// services." [P4]
    /// pre: the origin serves a redirect to a loopback sentinel; the initial
    /// URL is assumed validated upstream (the tool layer's strict gate).
    /// post: the validated client rejects the hop before any request reaches
    /// the sentinel, and the error names the rejection.
    #[tokio::test]
    async fn discover_feeds_rejects_redirect_to_loopback() {
        let sentinel_body: Arc<Vec<u8>> = Arc::new(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{}",
                "sentinel-body"
            )
            .into_bytes(),
        );
        let (sentinel_url, sentinel_hits) = http_fixture(sentinel_body).await;
        let redirect: Arc<Vec<u8>> = Arc::new(
            format!(
                "HTTP/1.1 302 Found\r\nLocation: {sentinel_url}/sentinel\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .into_bytes(),
        );
        let (origin_url, origin_hits) = http_fixture(redirect).await;

        let client = validated_fetch_client().expect("validated client");
        let error = discover_feeds(&client, &format!("{origin_url}/page"))
            .await
            .expect_err("a redirect into a loopback destination must be rejected");

        assert!(
            error.to_string().contains("Loopback"),
            "error must name the loopback rejection: {error}"
        );
        assert_eq!(
            sentinel_hits.load(Ordering::SeqCst),
            0,
            "the forbidden sentinel must not receive a request"
        );
        assert_eq!(
            origin_hits.load(Ordering::SeqCst),
            1,
            "exactly the initial request is made"
        );
    }

    /// expect: "The validated client still fetches ordinary pages for feed
    /// discovery." [P4]
    /// post: a plain 200 HTML page with a feed link is parsed normally — the
    /// redirect gate changes nothing for direct (non-redirect) fetches.
    #[tokio::test]
    async fn discover_feeds_direct_fetch_still_works() {
        let body =
            "<html><link rel=\"alternate\" type=\"application/rss+xml\" href=\"/feed.xml\"></html>";
        let page: Arc<Vec<u8>> = Arc::new(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .into_bytes(),
        );
        let (origin_url, _origin_hits) = http_fixture(page).await;

        let client = validated_fetch_client().expect("validated client");
        let feeds = discover_feeds(&client, &format!("{origin_url}/page"))
            .await
            .expect("a direct fetch must succeed through the validated client");
        assert_eq!(
            feeds.len(),
            1,
            "the feed link must be discovered: {feeds:?}"
        );
        assert_eq!(feeds[0]["url"], format!("{origin_url}/feed.xml"));
    }
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
    // SSRF validation is at the tool layer (rss_discover_feeds calls
    // validate_tool_url_with_dns) and per-hop/per-connection at the transport:
    // callers pass the validated fetch client (`validated_fetch_client`) for
    // arbitrary user URLs. The permissive rss_client must NOT be passed here.
    // The reqwest cause chain is rendered in full — a rejected redirect hop
    // must name its reason, not collapse to "error following redirect".
    let response =
        client.get(url).send().await.map_err(|e| {
            anyhow::anyhow!("{}", crate::research::providers::reqwest_error_detail(&e))
        })?;
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
