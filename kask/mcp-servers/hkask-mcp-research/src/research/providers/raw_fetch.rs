use super::{WebBrowseProvider, WebError, WebExtractProvider};
use crate::research::strip_html;
use crate::research::types::*;
use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hkask_mcp_server::server::{
    McpToolError, validate_resolved_addresses, validate_tool_url_literal,
};

/// Redirect hops raw fetch will follow before surfacing an error. Matches the
/// reqwest default (`redirect::Policy::limited(10)`); a custom policy must
/// enforce its own bound, the fork does not add one for custom closures.
const MAX_REDIRECTS: usize = 10;

pub(crate) struct RawFetchProvider {
    client: reqwest::Client,
}

/// Build the validated fetch HTTP client — the RawFetch transport.
///
/// Unlike the provider-API client (`provider_http_client`), every connection
/// this client makes is gated at the transport itself:
/// - a custom redirect policy re-runs the strict literal destination checks
///   on every redirect hop and bounds hop count and cycles;
/// - a validating DNS resolver rejects any resolution that lands on a
///   loopback/private/unspecified address, at the exact moment of connection
///   (a hostname cannot re-bind between validation and connect);
/// - proxies are disabled, so the connected destination is always the
///   validated one (a proxy would connect on our behalf and silently bypass
///   the gate).
///
/// Callers using this client for user-supplied URLs validate the initial URL
/// upstream (tool layer + pool boundary); this client is the inner, per-hop
/// and per-connection gate. Third-party provider APIs (Firecrawl/Tavily/Exa)
/// fetch target URLs server-side from their own network position; that fetch
/// is not this transport and is not gated here.
pub(crate) fn validated_fetch_client() -> Result<reqwest::Client, WebError> {
    warn_if_proxy_configured();
    reqwest::Client::builder()
        .user_agent(format!("hkask-mcp-web/{SERVER_VERSION}"))
        .timeout(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
        .redirect_policy(reqwest::redirect::Policy::custom(redirect_policy))
        .dns_resolver(Arc::new(ValidatingResolver))
        .no_proxy()
        .build()
        .map_err(|e| WebError::ProviderError(format!("failed to build HTTP client: {e}")))
}

/// Raw fetches deliberately bypass proxy configuration: a proxy would connect
/// on our behalf, so the validated destination could not be the connected one.
/// The bypass is surfaced so a proxied environment is visible, not silent;
/// provider-API clients keep their own (unchanged) proxy support.
fn warn_if_proxy_configured() {
    const PROXY_ENV_VARS: [&str; 6] = [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ];
    let configured: Vec<&str> = PROXY_ENV_VARS
        .into_iter()
        .filter(|name| std::env::var_os(name).is_some())
        .collect();
    if !configured.is_empty() {
        tracing::warn!(
            target: "hkask.web",
            proxies = ?configured,
            "validated fetch client ignores proxy environment variables: a proxied destination cannot be validated, so gated fetches connect directly; provider APIs keep their own proxy support"
        );
    }
}

/// Per-redirect-hop policy: bound the chain, refuse cycles, and re-run the
/// strict literal checks on the next destination before any request to it.
/// DNS-named redirect targets are re-validated at connect time by
/// `ValidatingResolver`.
fn redirect_policy(attempt: reqwest::redirect::Attempt) -> reqwest::redirect::Action {
    match check_redirect_target(attempt.url(), attempt.previous()) {
        Ok(()) => attempt.follow(),
        Err(reason) => attempt.error(reason),
    }
}

/// The redirect decision for one hop, extracted so it is testable without
/// reqwest's `Attempt` (which cannot be constructed outside the crate).
/// `previous` holds the already-visited URLs of this chain, target excluded.
fn check_redirect_target(
    target: &reqwest::Url,
    previous: &[reqwest::Url],
) -> Result<(), RedirectPolicyError> {
    if previous.len() >= MAX_REDIRECTS {
        return Err(RedirectPolicyError::TooManyRedirects(MAX_REDIRECTS));
    }
    if previous.contains(target) {
        return Err(RedirectPolicyError::Loop(target.clone()));
    }
    if let Err(error) = validate_tool_url_literal(target.as_str()) {
        return Err(RedirectPolicyError::Rejected(target.clone(), error));
    }
    Ok(())
}

/// Per-hop redirect policy refusals: the chain bound, a cycle back to a
/// visited URL, or a target that fails the strict literal checks. Feeds
/// reqwest's `Attempt::error`, so it implements `std::error::Error`.
#[derive(Debug, thiserror::Error)]
enum RedirectPolicyError {
    #[error("too many redirects (limit {0})")]
    TooManyRedirects(usize),
    #[error("redirect loop back to {0}")]
    Loop(reqwest::Url),
    #[error("redirect to {0} rejected: {1}")]
    Rejected(reqwest::Url, #[source] McpToolError),
}

/// DNS resolver that gates every connect-time resolution: no hostname may
/// re-bind to a forbidden destination between the upstream URL validation and
/// the actual connection. Resolutions happen here, inside reqwest's connect
/// path, so the validated addresses ARE the connected addresses.
struct ValidatingResolver;

impl reqwest::dns::Resolve for ValidatingResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|error| {
                    Box::new(std::io::Error::other(format!(
                        "DNS resolution failed for {host}: {error}"
                    ))) as Box<dyn std::error::Error + Send + Sync>
                })?
                .collect();
            validate_resolved_addresses(&host, &addresses).map_err(|error| {
                Box::new(std::io::Error::other(error.message))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

impl RawFetchProvider {
    pub fn new() -> Result<Self, WebError> {
        Ok(Self {
            client: validated_fetch_client()?,
        })
    }
}

/// Extract a PDF's text layer with `pdf-extract` (text layer only, no OCR).
/// This replaces the former refusal "RawFetch cannot extract PDF content;
/// use a PDF extraction provider" — that message advertised a PDF
/// extraction provider that did not exist; this is it (2026-11-01: a Locke
/// & Latham PDF produced a 153-char scraper shell and a typed refusal from
/// this leg — the chain had no working PDF route at all).
fn extract_pdf_content(
    final_url: &reqwest::Url,
    bytes: &[u8],
) -> Result<ExtractedContent, WebError> {
    let text = pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| WebError::ProviderError(format!("PDF text extraction failed: {e}")))?;
    // No text layer: a scanned PDF. OCR lives in the corpus server
    // (`corpus_ocr`) — name the route instead of returning an empty
    // "success" (the empty-success defect class this server forbids).
    if text.trim().is_empty() {
        return Err(WebError::ProviderUnavailable(
            "PDF has no text layer (scanned images) — download the file and run it \
             through the corpus server's corpus_ocr pipeline, which handles \
             scanned PDFs"
                .to_string(),
        ));
    }
    Ok(ExtractedContent {
        url: final_url.to_string(),
        content: text,
        format: "markdown".to_string(),
        metadata: None,
    })
}

#[async_trait]
impl WebExtractProvider for RawFetchProvider {
    fn kind(&self) -> &str {
        "rawfetch"
    }

    async fn extract(
        &self,
        url: &str,
        opts: &ExtractOptions,
    ) -> Result<ExtractedContent, WebError> {
        if opts.format == "json" {
            return Err(WebError::ProviderError(
                "RawFetch cannot produce structured JSON; a structured extraction provider is required"
                    .to_string(),
            ));
        }
        // SSRF validation is at the pool boundary (extract_with_fallback) for
        // the initial URL, and at this transport for every redirect hop and
        // every connect-time DNS resolution — see `raw_fetch_http_client`.
        let resp = self.client.get(url).send().await.map_err(|e| {
            WebError::ProviderUnavailable(format!(
                "RawFetch request failed: {}",
                super::reqwest_error_detail(&e)
            ))
        })?;
        let status = resp.status();
        // Report where the content actually came from: with a permitted
        // (validated) redirect, the final URL is the truthful provenance.
        let final_url = resp.url().clone();
        let pdf_content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.eq_ignore_ascii_case("application/pdf"))
            });
        if status.is_success() && pdf_content_type {
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| WebError::ProviderError(format!("RawFetch read error: {e}")))?;
            return extract_pdf_content(&final_url, &bytes);
        }
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderError(format!("RawFetch read error: {e}")))?;
        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "RawFetch error {status}: {}",
                hkask_inference::openai_compat::sanitize_error_body(&body)
            )));
        }
        if body.starts_with("%PDF-") {
            // Magic fallback: the origin served PDF bytes without the
            // application/pdf content type. `body` went through lossy text
            // decode, so binary streams may be mangled — extraction can then
            // fail honestly rather than return a shell, and the chain falls
            // through to the next provider.
            return extract_pdf_content(&final_url, body.as_bytes());
        }
        Ok(ExtractedContent {
            url: final_url.to_string(),
            content: strip_html::strip_html(&body),
            format: "markdown".to_string(),
            metadata: None,
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Task 1: Liveness check — fetch example.com
        let resp = self
            .client
            .get("https://example.com")
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!("RawFetch health check failed: {e}"))
            })?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(WebError::ProviderUnavailable(format!(
                "RawFetch health check returned {}",
                resp.status()
            )))
        }
    }
}
#[async_trait]
impl WebBrowseProvider for RawFetchProvider {
    fn kind(&self) -> &str {
        "rawfetch"
    }

    async fn browse(
        &self,
        url: &str,
        instruction: &str,
        timeout: Duration,
    ) -> Result<BrowseResult, WebError> {
        // SSRF validation is at the pool boundary (browse_with_fallback) for
        // the initial URL, and at this transport for every redirect hop.
        let resp = self
            .client
            .get(url)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                WebError::ProviderUnavailable(format!(
                    "RawFetch browse request failed: {}",
                    super::reqwest_error_detail(&e)
                ))
            })?;
        let status = resp.status();
        let final_url = resp.url().clone();
        let body = resp
            .text()
            .await
            .map_err(|e| WebError::ProviderError(format!("RawFetch browse read error: {e}")))?;
        if !status.is_success() {
            return Err(WebError::ProviderError(format!(
                "RawFetch browse error {status}: {}",
                hkask_inference::openai_compat::sanitize_error_body(&body)
            )));
        }
        Ok(BrowseResult {
            url: final_url.to_string(),
            content: strip_html::strip_html(&body),
            instruction: Some(instruction.to_string()),
            actions_taken: vec!["raw_fetch".to_string()],
        })
    }

    async fn health(&self) -> Result<(), WebError> {
        // Delegate to the extract health check (same logic)
        WebExtractProvider::health(self).await
    }
}

pub(crate) fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    use reqwest::dns::Resolve;

    use crate::test_support::http_fixture;

    fn extract_options() -> ExtractOptions {
        ExtractOptions {
            format: "markdown".to_string(),
            json_prompt: None,
            json_schema: None,
            main_content_only: true,
            wait_for_ms: 0,
        }
    }

    fn redirect_response(location: &str) -> Arc<Vec<u8>> {
        Arc::new(
            format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .into_bytes(),
        )
    }

    fn content_response(body: &str) -> Arc<Vec<u8>> {
        Arc::new(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_bytes(),
        )
    }

    /// expect: "A JSON extraction request must not succeed with Markdown or binary data."
    #[tokio::test]
    async fn raw_fetch_rejects_structured_json_request() {
        let (url, hits) = http_fixture(content_response("<p>not JSON</p>")).await;
        let provider = RawFetchProvider::new().expect("provider");
        let mut opts = extract_options();
        opts.format = "json".to_string();
        let error = provider
            .extract(&url, &opts)
            .await
            .expect_err("JSON unsupported");
        assert!(error.to_string().contains("cannot produce structured JSON"));
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    /// Build a minimal, structurally valid single-page PDF whose content
    /// stream is `content_stream` — enough for pdf-extract's text-layer
    /// extraction (correct xref offsets computed at build time).
    fn minimal_pdf(content_stream: &str) -> Vec<u8> {
        let objects: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
                .to_string(),
            format!(
                "<< /Length {} >>\nstream\n{content_stream}endstream",
                content_stream.len()
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets: Vec<usize> = Vec::with_capacity(objects.len());
        for (idx, obj) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{obj}\nendobj\n", idx + 1).as_bytes());
        }
        let xref_start = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_start}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn pdf_response(pdf: &[u8]) -> Arc<Vec<u8>> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            pdf.len()
        )
        .into_bytes();
        response.extend_from_slice(pdf);
        Arc::new(response)
    }

    /// expect: "A PDF's text layer is extracted and returned as Markdown —
    /// never raw PDF bytes, and never the former typed refusal." (Replaces
    /// the 2026-10 refusal pin: RawFetch IS the PDF extraction provider the
    /// old message advertised — the local leg of the extract chain.)
    #[tokio::test]
    async fn raw_fetch_extracts_pdf_text_layer() {
        let pdf = minimal_pdf("BT /F1 12 Tf 72 720 Td (Hello Extracted World) Tj ET\n");
        let (url, _) = http_fixture(pdf_response(&pdf)).await;
        let provider = RawFetchProvider::new().expect("provider");
        let extracted = provider
            .extract(&url, &extract_options())
            .await
            .expect("PDF text layer extracts");
        assert!(
            extracted.content.contains("Hello Extracted World"),
            "content: {}",
            extracted.content
        );
        assert!(!extracted.content.starts_with("%PDF-"));
    }

    /// expect: "A scanned PDF (no text layer) surfaces the corpus OCR route,
    /// not an empty success" — the empty-success defect class this server
    /// forbids.
    #[tokio::test]
    async fn raw_fetch_pdf_without_text_layer_names_the_ocr_route() {
        let pdf = minimal_pdf("BT /F1 12 Tf 72 720 Td ET\n");
        let (url, _) = http_fixture(pdf_response(&pdf)).await;
        let provider = RawFetchProvider::new().expect("provider");
        let error = provider
            .extract(&url, &extract_options())
            .await
            .expect_err("no text layer must not be a bare success");
        let message = error.to_string();
        assert!(
            message.contains("corpus_ocr"),
            "names the OCR route: {message}"
        );
        assert!(message.contains("no text layer"), "{message}");
    }

    /// expect: "An unparseable PDF surfaces a typed extraction error, not a
    /// hang, a shell, or raw bytes."
    #[tokio::test]
    async fn raw_fetch_pdf_parse_failure_surfaces_typed_error() {
        let (url, _) = http_fixture(content_response("%PDF-1.6\nbinary data")).await;
        let provider = RawFetchProvider::new().expect("provider");
        let error = provider
            .extract(&url, &extract_options())
            .await
            .expect_err("garbage PDF must error");
        assert!(error.to_string().contains("PDF text extraction failed"));
    }

    /// expect: "A redirect must not carry my fetch into loopback services." [P4]
    /// pre: the origin serves a redirect to a loopback literal; the initial URL
    /// is assumed validated upstream (the pool boundary's job).
    /// post: the hop is rejected before any request reaches the sentinel, and
    /// the surfaced error names the loopback rejection.
    #[tokio::test]
    async fn redirect_to_loopback_literal_is_rejected_before_request() {
        let (sentinel_url, sentinel_hits) = http_fixture(content_response("sentinel-body")).await;
        let (origin_url, origin_hits) =
            http_fixture(redirect_response(&format!("{sentinel_url}/sentinel"))).await;

        let provider = RawFetchProvider::new().expect("provider");
        let error = provider
            .extract(&format!("{origin_url}/page"), &extract_options())
            .await
            .expect_err("redirect to a loopback literal must be rejected");

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

    /// expect: "A redirect must not reach the cloud metadata endpoint." [P4]
    /// pre: the origin redirects to the 169.254.169.254 metadata address.
    /// post: the hop is rejected with the private-IP reason before connecting.
    #[tokio::test]
    async fn redirect_to_private_literal_is_rejected() {
        let (origin_url, _origin_hits) = http_fixture(redirect_response(
            "http://169.254.169.254/latest/meta-data/",
        ))
        .await;

        let provider = RawFetchProvider::new().expect("provider");
        let error = provider
            .extract(&format!("{origin_url}/page"), &extract_options())
            .await
            .expect_err("redirect to the metadata endpoint must be rejected");

        assert!(
            error.to_string().contains("Private IP"),
            "error must name the private-IP rejection: {error}"
        );
    }

    /// expect: "A hostname redirect cannot resolve my fetch into loopback." [P4]
    /// pre: the origin redirects to `localhost` (a DNS name, so the sync policy
    /// cannot judge it); the validating resolver runs at connect time.
    /// post: the resolution is rejected before any connection, and the error
    /// names the host and its loopback address.
    #[tokio::test]
    async fn redirect_to_localhost_name_is_rejected_by_connect_time_resolver() {
        let (sentinel_url, sentinel_hits) = http_fixture(content_response("sentinel-body")).await;
        let sentinel_port = sentinel_url.rsplit(':').next().expect("port");
        let (origin_url, origin_hits) = http_fixture(redirect_response(&format!(
            "http://localhost:{sentinel_port}/sentinel"
        )))
        .await;

        let provider = RawFetchProvider::new().expect("provider");
        let error = provider
            .extract(&format!("{origin_url}/page"), &extract_options())
            .await
            .expect_err("a redirect resolving to loopback must be rejected");

        assert!(
            error.to_string().contains("resolves to loopback"),
            "error must name the hostname resolution: {error}"
        );
        assert_eq!(
            sentinel_hits.load(Ordering::SeqCst),
            0,
            "the resolver must reject before connecting"
        );
        assert_eq!(
            origin_hits.load(Ordering::SeqCst),
            1,
            "exactly the initial request is made"
        );
    }

    /// expect: "The same gate applies when browsing, not only extracting." [P4]
    #[tokio::test]
    async fn browse_redirect_to_loopback_literal_is_rejected() {
        let (sentinel_url, sentinel_hits) = http_fixture(content_response("sentinel-body")).await;
        let (origin_url, _origin_hits) =
            http_fixture(redirect_response(&format!("{sentinel_url}/sentinel"))).await;

        let provider = RawFetchProvider::new().expect("provider");
        let error = provider
            .browse(
                &format!("{origin_url}/page"),
                "extract",
                Duration::from_secs(5),
            )
            .await
            .expect_err("browse redirect to loopback must be rejected");

        assert!(error.to_string().contains("Loopback"), "{error}");
        assert_eq!(
            sentinel_hits.load(Ordering::SeqCst),
            0,
            "no sentinel request"
        );
    }

    /// expect: "Redirect chains are bounded, and public destinations stay
    /// reachable." [P4]
    #[test]
    fn redirect_decision_bounds_cycles_and_forbidden_targets() {
        let public = reqwest::Url::parse("http://203.0.113.7/page").expect("public target");
        let second = reqwest::Url::parse("http://203.0.113.8/page").expect("public target");

        // Permitted hop within the bound is followed.
        assert!(check_redirect_target(&public, &[]).is_ok());
        assert!(check_redirect_target(&second, std::slice::from_ref(&public)).is_ok());

        // Hop bound: a chain already MAX_REDIRECTS long is refused.
        let full_chain: Vec<reqwest::Url> = (0..MAX_REDIRECTS)
            .map(|index| {
                reqwest::Url::parse(&format!("http://203.0.113.{index}/page"))
                    .expect("chain target")
            })
            .collect();
        let bound = check_redirect_target(&public, &full_chain).unwrap_err();
        assert!(bound.to_string().contains("too many redirects"), "{bound}");

        // Cycle: returning to a visited URL is refused.
        let cycle = check_redirect_target(&public, &[second, public.clone()]).unwrap_err();
        assert!(cycle.to_string().contains("redirect loop"), "{cycle}");

        // Forbidden literals are refused regardless of chain depth.
        for forbidden in [
            "http://127.0.0.1:9/x",
            "http://169.254.169.254/latest/",
            "http://0.0.0.0/",
            "http://[::1]/",
            "http://[64:ff9b::7f00:1]/",
        ] {
            let target = reqwest::Url::parse(forbidden).expect("forbidden target");
            let error = check_redirect_target(&target, std::slice::from_ref(&public)).unwrap_err();
            assert!(
                error.to_string().contains("rejected"),
                "forbidden redirect target {forbidden} must be refused: {error}"
            );
        }

        // Scheme and credential redirects are refused too.
        assert!(
            check_redirect_target(
                &reqwest::Url::parse("ftp://203.0.113.7/x").expect("scheme target"),
                &[]
            )
            .is_err()
        );
        assert!(
            check_redirect_target(
                &reqwest::Url::parse("http://user:pass@203.0.113.7/x").expect("cred target"),
                &[]
            )
            .is_err()
        );
    }

    /// expect: "The resolver itself refuses loopback resolutions." [P4]
    #[tokio::test]
    async fn validating_resolver_rejects_loopback_resolution() {
        let name = "localhost".parse::<reqwest::dns::Name>().expect("name");
        let error = match ValidatingResolver.resolve(name).await {
            Err(error) => error,
            Ok(_) => panic!("localhost resolves to loopback and must be refused"),
        };
        assert!(
            error.to_string().contains("resolves to loopback"),
            "error must name the offending resolution: {error}"
        );
    }
}
