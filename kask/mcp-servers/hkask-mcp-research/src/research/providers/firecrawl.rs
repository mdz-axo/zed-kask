use super::{
    ProviderSearchOutput, WebError, WebExtractProvider, WebSearchProvider, reqwest_error_detail,
    search_results_from,
};
use crate::research::types::*;
use async_trait::async_trait;

#[derive(Clone)]
pub(crate) struct FirecrawlProvider {
    client: reqwest::Client,
    api_key: Option<String>,
    /// Request base; `FIRECRAWL_API_BASE` except in the extract-path
    /// tests, where it points at the loopback HTTP fixture (the crossref
    /// `base` pattern).
    base: String,
}

impl FirecrawlProvider {
    pub fn new(api_key: Option<String>) -> Result<Self, WebError> {
        Ok(Self {
            client: super::provider_http_client()?,
            api_key,
            base: FIRECRAWL_API_BASE.to_string(),
        })
    }

    fn auth_header(&self) -> Result<reqwest::header::HeaderValue, WebError> {
        let key = self.api_key.as_ref().ok_or(WebError::NoProvider)?;
        let value = format!("Bearer {key}");
        // Pre-validate instead of letting reqwest defer the failure to send()
        // as a bare "builder error". If the key ever holds non-header bytes
        // (e.g. a pasted trailing newline), this names the key's shape so the
        // operator can distinguish a dirty credential from a broken request.
        reqwest::header::HeaderValue::from_str(&value).map_err(|_| {
            WebError::ProviderError(format!(
                "firecrawl api key is not valid HTTP header bytes \
                 (length {}, ascii: {}) — re-set the key via the kask settings UI",
                key.len(),
                key.is_ascii()
            ))
        })
    }
}

/// The Firecrawl v2 `/scrape` request body for one extraction.
///
/// JSON mode in v2 is a format OBJECT: `formats: [{type: "json", schema,
/// prompt}]` (docs.firecrawl.dev/features/llm-extract, read 2026-11-12).
/// The v1 form — `formats: ["json"]` with a top-level `jsonOptions` — is
/// refused by v2 with 400 "Unrecognized key in body … jsonOptions"
/// (measured 2026-11-12), which made every `format: "json"` extraction
/// fail. The caller's schema is sent; before this it was accepted,
/// size-checked and dropped.
fn scrape_payload(url: &str, opts: &ExtractOptions) -> serde_json::Value {
    let mut payload = serde_json::json!({ "url": url });
    payload["formats"] = if opts.format == "json" {
        let mut format = serde_json::json!({ "type": "json" });
        if let Some(schema) = &opts.json_schema {
            format["schema"] = schema.clone();
        }
        if let Some(prompt) = &opts.json_prompt {
            format["prompt"] = serde_json::json!(prompt);
        }
        serde_json::json!([format])
    } else {
        serde_json::json!(["markdown"])
    };
    if opts.main_content_only {
        payload["onlyMainContent"] = serde_json::json!(true);
    }
    if opts.wait_for_ms > 0 {
        payload["waitFor"] = serde_json::json!(opts.wait_for_ms);
    }
    payload
}

/// The extracted content from a v2 `/scrape` response body.
///
/// A response without the requested field is an error, never an empty
/// success: JSON mode with no `data.json` used to return the text `null`
/// as valid JSON content, and markdown mode with no `data.markdown` an
/// empty string.
fn scrape_content(parsed: &serde_json::Value, format: &str) -> Result<String, WebError> {
    if format == "json" {
        match &parsed["data"]["json"] {
            serde_json::Value::Null => Err(WebError::ProviderError(
                "Firecrawl returned no structured data (data.json is absent or null)".to_string(),
            )),
            value => Ok(value.to_string()),
        }
    } else {
        parsed["data"]["markdown"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                WebError::ProviderError(
                    "Firecrawl returned no markdown (data.markdown is absent)".to_string(),
                )
            })
    }
}

/// Parse a Firecrawl v2 search response. v2 nests the web results under
/// `data.web` (`{"success":true,"data":{"web":[…],"news":[…]}}`); the
/// former `data`-as-array read was the v1 shape and silently returned zero
/// results on every successful v2 call.
fn parse_v2_search_results(parsed: &serde_json::Value) -> Vec<SearchResult> {
    search_results_from(
        &parsed["data"]["web"],
        |item| {
            item["description"]
                .as_str()
                .or_else(|| item["snippet"].as_str())
                .map(str::to_string)
        },
        |_| None,
        |_| None,
    )
}
#[async_trait]
impl WebSearchProvider for FirecrawlProvider {
    fn kind(&self) -> &str {
        "firecrawl"
    }
    fn capabilities(&self) -> Vec<SearchCapability> {
        vec![SearchCapability::Keyword, SearchCapability::Semantic]
    }

    async fn search(&self, query: &SearchQuery) -> Result<ProviderSearchOutput, WebError> {
        let auth = self.auth_header()?;
        let payload = serde_json::json!({ "query": query.query, "limit": query.num_results });
        let resp = self
            .client
            .post(format!("{}/search", self.base))
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!(
                    "Firecrawl request failed: {}",
                    reqwest_error_detail(&e)
                ))
            })?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| {
            WebError::ProviderUnavailable(format!("Firecrawl body read failed: {e}"))
        })?;
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => {
                    WebError::ProviderUnavailable(format!("Firecrawl auth error: {status}"))
                }
                429 => WebError::RateLimited(format!("Firecrawl rate limited: {status}")),
                _ => WebError::ProviderError(format!("Firecrawl API error {status}")),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Firecrawl response: {e}"))
        })?;

        let results = parse_v2_search_results(&parsed);

        Ok(ProviderSearchOutput {
            results,
            ..Default::default()
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        if self.api_key.is_none() {
            return Err(WebError::NoProvider);
        }
        Ok(())
    }
}
#[async_trait]
impl WebExtractProvider for FirecrawlProvider {
    fn kind(&self) -> &str {
        "firecrawl"
    }

    async fn extract(
        &self,
        url: &str,
        opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        // SSRF validation is at the pool boundary (extract_with_fallback).
        let auth = self.auth_header()?;
        let payload = scrape_payload(url, opts);

        let resp = self
            .client
            .post(format!("{}/scrape", self.base))
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!(
                    "Firecrawl extract failed: {}",
                    reqwest_error_detail(&e)
                ))
            })?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| {
            WebError::ProviderUnavailable(format!("Firecrawl body read failed: {e}"))
        })?;
        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "Firecrawl extract error {status}: {}",
                hkask_inference::openai_compat::sanitize_error_body(&body)
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            WebError::ProviderError(format!("Failed to parse Firecrawl extract response: {e}"))
        })?;

        let content = scrape_content(&parsed, &opts.format)?;
        let metadata = parsed["data"]["metadata"]
            .as_object()
            .map(|m| serde_json::Value::Object(m.clone()));

        Ok(ExtractedContent {
            url: url.to_string(),
            content,
            format: opts.format.clone(),
            metadata,
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        if self.api_key.is_none() {
            return Err(WebError::NoProvider);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Firecrawl v2 nests web results under `data.web`. The former
    /// `data`-as-array read (the v1 shape) silently returned zero results on
    /// every successful v2 call — an empty success, the exact defect class
    /// this server's contracts forbid. Pins the v2 shape.
    #[test]
    fn parse_v2_search_results_reads_data_web_array() {
        let body = serde_json::json!({
            "success": true,
            "data": {
                "web": [
                    {"title": "Speedtest", "url": "https://www.speedtest.net/",
                     "description": "Broadband speed test"},
                    {"title": "No URL", "description": "skipped — url is required"}
                ],
                "news": []
            }
        });
        let results = parse_v2_search_results(&body);
        assert_eq!(results.len(), 1, "only the entry with a url is kept");
        assert_eq!(results[0].title, "Speedtest");
        assert_eq!(results[0].url, "https://www.speedtest.net/");
        assert_eq!(
            results[0].description.as_deref(),
            Some("Broadband speed test")
        );
    }

    /// A v1-shaped body (`data` as a bare array) must NOT parse as success â
    /// v2 is the API version in `FIRECRAWL_API_BASE`; silently accepting both
    /// shapes would re-open the empty-success hole if the v1 read returned.
    #[test]
    fn parse_v2_search_results_rejects_v1_data_array() {
        let v1_body = serde_json::json!({
            "success": true,
            "data": [{"title": "t", "url": "https://example.com"}]
        });
        assert!(
            parse_v2_search_results(&v1_body).is_empty(),
            "v1-shaped data array must yield no results â v2 nests under data.web"
        );
    }

    fn json_options(schema: Option<serde_json::Value>, prompt: Option<&str>) -> ExtractOptions {
        ExtractOptions {
            format: "json".to_string(),
            json_prompt: prompt.map(str::to_string),
            json_schema: schema,
            main_content_only: true,
            wait_for_ms: 0,
        }
    }

    /// JSON mode is a v2 format object carrying the caller's schema and
    /// prompt. The v1 body (`formats: ["json"]` plus a top-level
    /// `jsonOptions`) is refused by v2 with 400, which failed every
    /// `format: "json"` extraction (measured 2026-11-12).
    #[test]
    fn scrape_payload_sends_json_mode_as_a_v2_format_object_with_the_schema() {
        let schema =
            serde_json::json!({"type": "object", "properties": {"title": {"type": "string"}}});
        let payload = scrape_payload(
            "https://example.org/",
            &json_options(Some(schema.clone()), Some("the title")),
        );
        assert_eq!(
            payload["formats"],
            serde_json::json!([{"type": "json", "schema": schema, "prompt": "the title"}])
        );
        assert!(
            payload.get("jsonOptions").is_none(),
            "the v1 key must not be sent: {payload}"
        );
        assert_eq!(payload["onlyMainContent"], true);

        // A prompt alone is valid JSON mode; no schema key is invented.
        let prompt_only = scrape_payload("https://example.org/", &json_options(None, Some("p")));
        assert_eq!(
            prompt_only["formats"],
            serde_json::json!([{"type": "json", "prompt": "p"}])
        );
    }

    #[test]
    fn scrape_payload_sends_markdown_as_a_plain_format_name() {
        let opts = ExtractOptions {
            format: "markdown".to_string(),
            json_prompt: None,
            json_schema: None,
            main_content_only: false,
            wait_for_ms: 1500,
        };
        let payload = scrape_payload("https://example.org/", &opts);
        assert_eq!(payload["formats"], serde_json::json!(["markdown"]));
        assert!(payload.get("onlyMainContent").is_none(), "{payload}");
        assert_eq!(payload["waitFor"], 1500);
    }

    /// A response without the requested field is an error, never an empty
    /// success: JSON mode used to return the text `null`, markdown mode "".
    #[test]
    fn scrape_content_refuses_a_response_without_the_requested_field() {
        let empty = serde_json::json!({"success": true, "data": {"metadata": {}}});
        let json = scrape_content(&empty, "json").expect_err("no data.json");
        assert!(json.to_string().contains("no structured data"), "{json}");
        let null = serde_json::json!({"success": true, "data": {"json": null}});
        assert!(scrape_content(&null, "json").is_err());
        let markdown = scrape_content(&empty, "markdown").expect_err("no data.markdown");
        assert!(markdown.to_string().contains("no markdown"), "{markdown}");

        let ok = serde_json::json!({"data": {"json": {"title": "T"}, "markdown": "# T"}});
        assert_eq!(scrape_content(&ok, "json").unwrap(), r#"{"title":"T"}"#);
        assert_eq!(scrape_content(&ok, "markdown").unwrap(), "# T");
    }

    /// A fixture key: thirty-two zeros in Firecrawl's `fc-` shape. Never a
    /// real key — a real one was committed here until 2026-11-12.
    const FIXTURE_KEY: &str = "fc-00000000000000000000000000000000";

    fn http_json(status: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn provider_at(base: String) -> FirecrawlProvider {
        let mut provider = FirecrawlProvider::new(Some(FIXTURE_KEY.to_string())).expect("provider");
        provider.base = base;
        provider
    }

    /// The request that reaches Firecrawl for a JSON extraction is the v2
    /// body, and a v2 response comes back as JSON content. This is the
    /// pin for the defect measured 2026-11-12: every `format: "json"`
    /// extraction answered 400 "Unrecognized key in body … jsonOptions".
    #[tokio::test]
    async fn a_json_extraction_posts_the_v2_body_and_returns_the_structured_data() {
        let (base, requests) = crate::test_support::http_fixture_capturing_requests(http_json(
            "200 OK",
            r#"{"success":true,"data":{"json":{"title":"Example Domains"},"metadata":{"statusCode":200}}}"#,
        ))
        .await;
        let schema =
            serde_json::json!({"type": "object", "properties": {"title": {"type": "string"}}});
        let extracted = provider_at(base)
            .extract(
                "https://www.iana.org/help/example-domains",
                &json_options(Some(schema.clone()), Some("the page title")),
            )
            .await
            .expect("a v2 JSON response extracts");
        assert_eq!(extracted.format, "json");
        assert_eq!(extracted.content, r#"{"title":"Example Domains"}"#);

        let sent = requests.lock().expect("requests").clone();
        assert_eq!(sent.len(), 1, "one request");
        let body: serde_json::Value =
            serde_json::from_str(&sent[0].body).expect("request body is JSON");
        assert_eq!(body["url"], "https://www.iana.org/help/example-domains");
        assert_eq!(
            body["formats"],
            serde_json::json!([{"type": "json", "schema": schema, "prompt": "the page title"}])
        );
        assert!(body.get("jsonOptions").is_none(), "{body}");
    }

    /// Firecrawl's own refusal reaches the caller with its text, so a
    /// rejected request is not mistaken for a missing provider.
    #[tokio::test]
    async fn a_refused_extraction_carries_firecrawls_reason() {
        let (base, _requests) = crate::test_support::http_fixture_capturing_requests(http_json(
            "400 Bad Request",
            r#"{"success":false,"code":"BAD_REQUEST","error":"Unrecognized key in body"}"#,
        ))
        .await;
        let error = provider_at(base)
            .extract("https://example.org/", &json_options(None, Some("p")))
            .await
            .expect_err("a 400 is an error");
        let message = error.to_string();
        assert!(message.contains("Firecrawl extract error 400"), "{message}");
        assert!(message.contains("Unrecognized key in body"), "{message}");
    }

    /// A clean ASCII key must pre-validate into a HeaderValue — and the
    /// request path must not be able to fail later with reqwest's deferred
    /// "builder error" for this cause.
    #[test]
    fn auth_header_accepts_clean_key() {
        let provider = FirecrawlProvider::new(Some(FIXTURE_KEY.to_string())).expect("provider");
        let header = provider.auth_header().expect("clean key must validate");
        assert_eq!(
            header.as_bytes(),
            format!("Bearer {FIXTURE_KEY}").as_bytes()
        );
    }

    /// A key with non-header bytes (e.g. a pasted trailing newline) must fail
    /// at `auth_header` with an error naming the key's shape — never as a
    /// deferred bare "builder error" from reqwest's send().
    #[test]
    fn auth_header_rejects_dirty_key_with_shape() {
        let provider = FirecrawlProvider::new(Some("fc-dirty\n".to_string())).expect("provider");
        let error = provider.auth_header().expect_err("dirty key must fail");
        let message = error.to_string();
        assert!(message.contains("not valid HTTP header bytes"), "{message}");
        assert!(
            message.contains("length 9"),
            "must name the length: {message}"
        );
        assert!(message.contains("ascii"), "must name ascii-ness: {message}");
    }
}
