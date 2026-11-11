//! Shared economic-data service layer.
//!
//! Three economic-data providers (FRED, World Bank, DBnomics) share an
//! identical fetch shape: build a URL, GET it, classify the HTTP error or
//! parse the JSON body. This module owns that shared shape once; each
//! provider module (`fred`, `worldbank`, `dbnomics`) owns only its
//! per-provider response shaping.
//!
//! The deletion test justifies the collapse: the three `*_fetch` helpers
//! and three `From<Error> for McpToolError` impls were identical
//! (modulo one error variant). A 4th provider would copy-paste the same
//! boilerplate — the fetch logic earns its keep as a shared client, not 3×.

use hkask_mcp_server::server::McpToolError;
use serde_json::Value;

// ── Shared error type ──────────────────────────────────────────────────────

/// One error type for all economic-data providers. Per-variant
/// classification into `McpToolError` lives here, not in each provider.
#[derive(Debug, thiserror::Error)]
pub enum EconomicDataError {
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
    /// A caller-named resource (series, dataset, indicator, release) the
    /// provider does not have. The message names the missing thing.
    #[error("{0}")]
    NotFound(String),
    #[error(
        "HKASK_FRED_API_KEY not configured. Set HKASK_FRED_API_KEY via the keychain or environment variable."
    )]
    MissingApiKey,
    #[error("{provider} API request failed: {detail}")]
    RequestFailed {
        provider: &'static str,
        detail: String,
    },
    #[error("{provider} API returned HTTP {status}: {body}")]
    HttpError {
        provider: &'static str,
        status: u16,
        body: String,
    },
    #[error("{provider} API response parse error: {detail}")]
    ParseError {
        provider: &'static str,
        detail: String,
    },
}

impl From<EconomicDataError> for McpToolError {
    fn from(error: EconomicDataError) -> Self {
        use EconomicDataError::*;
        let message = error.to_string();
        match error {
            InvalidParam(_) => McpToolError::invalid_argument(message),
            NotFound(_) => McpToolError::not_found(message),
            MissingApiKey => McpToolError::permission_denied(message),
            RequestFailed { .. } => McpToolError::unavailable(message),
            // The provider's status decides the class (mcp-tool-review
            // PM2-05): a rejected caller value is not a transient outage.
            // FRED answers a bad or unknown id with HTTP 400 — and an
            // unregistered key with HTTP 400 naming `api_key`, which is a
            // credential fault, not a bad argument.
            HttpError {
                status: 400, body, ..
            } if body.contains("api_key") => {
                McpToolError::permission_denied(format!("{message} — check HKASK_FRED_API_KEY"))
            }
            HttpError { status, .. } => match status {
                400 | 422 => McpToolError::invalid_argument(message),
                401 | 403 => McpToolError::permission_denied(message),
                404 => McpToolError::not_found(message),
                429 => McpToolError::rate_limited(message),
                500..=599 => McpToolError::unavailable(message),
                _ => McpToolError::internal(message),
            },
            ParseError { .. } => McpToolError::internal(message),
        }
    }
}

// ── Shared client ─────────────────────────────────────────────────────────

/// A minimal HTTP client for economic-data APIs: builds a URL, GETs it,
/// classifies the HTTP error or parses the JSON body. Stateless — the
/// `reqwest::Client` is the only state, and it's cheap to share.
pub struct EconomicDataClient<'a> {
    http: &'a reqwest::Client,
}

impl<'a> EconomicDataClient<'a> {
    pub fn new(http: &'a reqwest::Client) -> Self {
        Self { http }
    }

    /// Build a URL from a base, endpoint, and query params. The caller is
    /// responsible for any required API-key query param (FRED appends it
    /// in its adapter; WB/DBnomics are keyless). If the base ends with `/`, no
    /// extra slash is inserted between base and endpoint.
    ///
    /// Query param values are URL-encoded so LLM-controlled values (e.g.
    /// `series_code`, `query`) cannot inject extra params (`&limit=...`) or
    /// truncate the URL (`#`). All three providers build their URLs here.
    /// Path segments are NOT encoded here — callers pass caller-supplied
    /// segments through [`path_segment`] before interpolating them into
    /// `endpoint`.
    pub fn build_url(base: &str, endpoint: &str, params: &[(&str, &str)]) -> String {
        let mut url = if endpoint.is_empty() {
            base.trim_end_matches('/').to_string()
        } else if base.ends_with('/') {
            format!("{base}{endpoint}")
        } else {
            format!("{base}/{endpoint}")
        };
        if !params.is_empty() {
            url.push('?');
            let mut first = true;
            for (k, v) in params {
                if !first {
                    url.push('&');
                }
                first = false;
                url.push_str(k);
                url.push('=');
                url.push_str(&hkask_mcp_server::url_encode_query_value(v));
            }
        }
        url
    }

    /// GET the URL and return the parsed JSON body, or a typed error.
    /// `provider` tags the error so the message identifies the source.
    pub async fn fetch(
        &self,
        provider: &'static str,
        url: &str,
    ) -> Result<Value, EconomicDataError> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            // `without_url`: reqwest appends the request URL to its
            // error text, and FRED's key rides the query string.
            .map_err(|e| EconomicDataError::RequestFailed {
                provider,
                detail: e.without_url().to_string(),
            })?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(EconomicDataError::HttpError {
                provider,
                status,
                body,
            });
        }
        response
            .json::<Value>()
            .await
            .map_err(|e| EconomicDataError::ParseError {
                provider,
                detail: e.without_url().to_string(),
            })
    }
}

/// Admit a caller-supplied value as one URL path segment. Provider codes
/// legitimately carry `.`, `:` and `-` (`SP.POP.TOTL`, `WEO:latest`), so
/// this rejects only what would restructure the URL: separators, query and
/// fragment markers, escapes, whitespace and dot-segments.
pub(crate) fn path_segment<'a>(field: &str, value: &'a str) -> Result<&'a str, EconomicDataError> {
    let value = value.trim();
    let structural = |c: char| {
        matches!(c, '/' | '\\' | '?' | '#' | '&' | '%') || c.is_whitespace() || c.is_control()
    };
    if value.is_empty() || value == "." || value == ".." || value.chars().any(structural) {
        return Err(EconomicDataError::InvalidParam(format!(
            "{field} must be one non-empty path segment without '/', '\\', '?', '#', '&', '%' or whitespace, got {value:?}"
        )));
    }
    Ok(value)
}

// The per-provider modules (fred, worldbank, dbnomics) live as sibling
// files in `src/economic_data/` and are declared here. Each owns its
// request types and response shaping; the shared client/error live above.

pub mod dbnomics;
pub mod fred;
pub mod worldbank;

#[cfg(test)]
mod tests {
    use super::*;

    /// mcp-tool-review PM2-04 pin: a transport failure must never carry the
    /// request URL into the tool error. FRED's key rides the query string,
    /// and reqwest's error `Display` appends `for url (...)` — so an
    /// un-stripped error hands the credential to the calling model.
    #[tokio::test]
    async fn request_failure_message_never_carries_the_url_or_api_key() {
        let http = reqwest::Client::new();
        let client = EconomicDataClient::new(&http);
        // Port 1 on loopback refuses the connection — no network leaves the host.
        let error = client
            .fetch(
                "FRED",
                "http://127.0.0.1:1/fred/series?api_key=PROBESECRETKEY123&file_type=json",
            )
            .await
            .expect_err("a refused connection must error");
        assert!(
            matches!(error, EconomicDataError::RequestFailed { .. }),
            "a refused connection is a request failure, got: {error:?}"
        );
        let message = McpToolError::from(error).message;
        assert!(
            !message.contains("PROBESECRETKEY123") && !message.contains("api_key"),
            "the tool error must not carry the URL or its key, got: {message}"
        );
    }

    /// mcp-tool-review PM2-05 pin: the provider's HTTP status decides the
    /// error class. Live 2026-11-10, FRED's HTTP 400 for a bad series id
    /// reached the caller as `unavailable` (retry later).
    #[test]
    fn http_status_decides_the_error_class() {
        use hkask_types::McpErrorKind;
        let kind = |status: u16| {
            McpToolError::from(EconomicDataError::HttpError {
                provider: "FRED",
                status,
                body: String::new(),
            })
            .kind
        };
        assert_eq!(kind(400), McpErrorKind::InvalidArgument);
        assert_eq!(kind(403), McpErrorKind::PermissionDenied);
        assert_eq!(kind(404), McpErrorKind::NotFound);
        assert_eq!(kind(429), McpErrorKind::RateLimited);
        assert_eq!(kind(503), McpErrorKind::Unavailable);

        // FRED's live body for an unregistered key (captured 2026-11-11).
        let bad_key = McpToolError::from(EconomicDataError::HttpError {
            provider: "FRED",
            status: 400,
            body: r#"{"error_code":400,"error_message":"Bad Request.  The value for variable api_key is not registered."}"#.to_string(),
        });
        assert_eq!(bad_key.kind, McpErrorKind::PermissionDenied);
        assert!(
            bad_key.message.contains("HKASK_FRED_API_KEY"),
            "{}",
            bad_key.message
        );
        assert_eq!(
            McpToolError::from(EconomicDataError::NotFound("series X not found".into())).kind,
            McpErrorKind::NotFound
        );
    }

    /// mcp-tool-review PM2-03 pin: a caller value can be one path segment,
    /// never a path, a query or a fragment.
    #[test]
    fn path_segment_admits_provider_codes_and_rejects_url_structure() {
        for ok in ["SP.POP.TOTL", "WEO:latest", "ABW.BCA.us_dollars", " USA "] {
            assert!(path_segment("code", ok).is_ok(), "{ok:?} is a valid code");
        }
        assert_eq!(path_segment("code", " USA ").ok(), Some("USA"));
        for bad in ["", "..", "a/b", "a?b=1", "a#b", "a&b", "a%2Fb", "a b"] {
            let error = path_segment("code", bad).expect_err("must reject");
            assert!(
                matches!(error, EconomicDataError::InvalidParam(_)),
                "{bad:?}"
            );
        }
    }
}
