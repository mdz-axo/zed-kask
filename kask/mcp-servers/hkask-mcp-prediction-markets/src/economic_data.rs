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
    #[error("API key not configured")]
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
    #[error("{provider} API error: {detail}")]
    ApiError {
        provider: &'static str,
        detail: String,
    },
}

impl From<EconomicDataError> for McpToolError {
    fn from(error: EconomicDataError) -> Self {
        use EconomicDataError::*;
        match error {
            InvalidParam(_) | MissingApiKey => McpToolError::invalid_argument(error.to_string()),
            HttpError { .. } | RequestFailed { .. } => McpToolError::unavailable(error.to_string()),
            ParseError { .. } | ApiError { .. } => McpToolError::internal(error.to_string()), // rr0044-ok: mapper-internal-arm
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
    /// truncate the URL (`#`). Path segments are NOT encoded here — callers
    /// must validate path-segment inputs against `^[A-Za-z0-9_-]+$` before
    /// interpolating them into `endpoint` (RR-0052).
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
                url.push_str(&url_encode_value(v));
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
        let response =
            self.http
                .get(url)
                .send()
                .await
                .map_err(|e| EconomicDataError::RequestFailed {
                    provider,
                    detail: e.to_string(),
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
                detail: e.to_string(),
            })
    }
}

/// Percent-encode a query-parameter value per RFC 3986 (unreserved + `+` for
/// space). Prevents LLM-controlled values from injecting extra query params
/// (`&limit=...`) or truncating the URL (`#`) (RR-0052).
fn url_encode_value(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push('%');
                encoded.push(hex_digit(byte >> 4));
                encoded.push(hex_digit(byte & 0x0f));
            }
        }
    }
    encoded
}

fn hex_digit(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'A' + (n - 10)) as char,
    }
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
    use proptest::prelude::*;

    proptest! {
        /// P1 invariant: url_encode_value output never contains raw `&`, `#`,
        /// or `=` from the input — these are the injection characters that let
        /// an LLM-controlled value add extra query params or truncate the URL
        /// (RR-0052).
        #[test]
        fn url_encode_value_no_injection_chars(
            input in "[\x20-\x7e]{0,100}"
        ) {
            let encoded = url_encode_value(&input);
            for ch in input.chars() {
                if ch == '&' || ch == '#' {
                    let hex = format!("%{:02X}", ch as u8);
                    prop_assert!(
                        encoded.contains(&hex),
                        "'{}' was not encoded in output: {:?}",
                        ch, encoded
                    );
                }
            }
            prop_assert!(
                !encoded.contains('&') && !encoded.contains('#'),
                "encoded output contains injection chars: {:?}",
                encoded
            );
        }

        /// P1 round-trip: for unreserved characters (A-Za-z0-9-_.~),
        /// url_encode_value is the identity function.
        #[test]
        fn url_encode_value_unreserved_round_trips(
            input in "[A-Za-z0-9_.~-]{0,100}"
        ) {
            let encoded = url_encode_value(&input);
            let (input_dbg, encoded_dbg) = (format!("{:?}", input), format!("{:?}", encoded));
            prop_assert!(
                encoded == input,
                "unreserved chars should not be encoded: input={} encoded={}",
                input_dbg, encoded_dbg
            );
        }

        /// P1 invariant: build_url with a single key-value pair produces a URL
        /// with exactly one `key=` occurrence and the value is fully encoded.
        #[test]
        fn build_url_single_param_no_injection(
            value in "[\x20-\x7e]{0,100}"
        ) {
            let url = EconomicDataClient::build_url(
                "https://api.example.com",
                "endpoint",
                &[("key", &value)],
            );
            let key_count = url.matches("key=").count();
            prop_assert_eq!(
                key_count, 1,
                "expected exactly 1 'key=' in URL, got {}: {:?}",
                key_count, url
            );
            let q_count = url.matches('?').count();
            prop_assert_eq!(q_count, 1, "expected 1 '?', got {}: {:?}", q_count, url);
        }
    }
}
