//! FRED (Federal Reserve Economic Data) API client and MCP tool implementations.
//!
//! FRED is the St. Louis Fed's economic data API, offering ~800,000 economic
//! time series from 80+ sources (BEA, BLS, Census, Fed, etc.). This module
//! exposes the FRED API as MCP tools within the prediction-markets server,
//! complementing the existing `base_event.rs` FRED integration (which fetches
//! only 4 hardcoded series for reference-level pricing).
//!
//! API docs: https://fred.stlouisfed.org/docs/api/fred/
//!
//! All tools require `HKASK_FRED_API_KEY` (already in the server's credential
//! allowlist). The key is stored as `self.fred_api_key` on the server struct.

use serde::Deserialize;
use serde_json::Value;

// ── Constants ──────────────────────────────────────────────────────────────

const FRED_API_BASE: &str = "https://api.stlouisfed.org/fred";

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum FredError {
    #[error("FRED API key not configured (set HKASK_FRED_API_KEY)")]
    MissingApiKey,
    #[error("FRED API request failed: {0}")]
    RequestFailed(String),
    #[error("FRED API returned HTTP {status}: {body}")]
    HttpError { status: u16, body: String },
    #[error("FRED API response parse error: {0}")]
    ParseError(String),
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
}

impl From<FredError> for hkask_mcp_server::server::McpToolError {
    fn from(e: FredError) -> Self {
        use hkask_mcp_server::server::McpToolError;
        match e {
            FredError::MissingApiKey | FredError::InvalidParam(_) => {
                McpToolError::invalid_argument(e.to_string())
            }
            FredError::HttpError { .. } | FredError::RequestFailed(_) => {
                McpToolError::unavailable(e.to_string())
            }
            FredError::ParseError(_) => McpToolError::internal(e.to_string()),
        }
    }
}

// ── Request types (MCP tool parameters) ─────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FredSearchSeriesRequest {
    /// Search text (e.g., "nonfarm payrolls", "GDP", "unemployment rate").
    pub search_text: String,
    /// Optional: filter by category ID.
    pub category_id: Option<u32>,
    /// Optional: filter by tag(s), comma-separated (e.g., "employment;monthly").
    pub tag_names: Option<String>,
    /// Max results to return (default 10, capped at 100).
    pub limit: Option<u32>,
    /// Optional: order results by (popularity, series_id, title, units, frequency,
    /// seasonal_adjustment, realtime_start, realtime_end). Default: "popularity".
    pub order_by: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FredGetObservationsRequest {
    /// FRED series ID (e.g., "FEDFUNDS", "CPIAUCSL", "PAYEMS", "GDPC1").
    pub series_id: String,
    /// Optional: start date (YYYY-MM-DD). Defaults to series start.
    pub observation_start: Option<String>,
    /// Optional: end date (YYYY-MM-DD). Defaults to latest.
    pub observation_end: Option<String>,
    /// Optional: frequency transformation. One of: daily, weekly, biweekly,
    /// monthly, quarterly, semianual, annual. Default: native frequency.
    pub frequency: Option<String>,
    /// Optional: units transformation. One of: lin, chg, chg1d, chg1w, chg1m,
    /// chg1q, chg1a, pct, pct1d, pct1w, pct1m, pct1q, pct1a, log, pc1, pc1ch.
    pub units: Option<String>,
    /// Max observations to return (default 100, capped at 1000).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FredGetSeriesInfoRequest {
    /// FRED series ID (e.g., "FEDFUNDS", "CPIAUCSL").
    pub series_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FredListCategoriesRequest {
    /// Optional: parent category ID. If omitted, returns root categories.
    /// Use 0 for the root, or a specific category ID to get its children.
    pub category_id: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FredGetReleaseRequest {
    /// Release ID (e.g., 50 for "Employment Situation Summary").
    pub release_id: u32,
}

// ── FRED API client ────────────────────────────────────────────────────────

/// Build a FRED API URL with the API key and common parameters.
fn fred_url(
    endpoint: &str,
    api_key: &str,
    params: &[(&str, &str)],
) -> String {
    let mut url = format!("{FRED_API_BASE}/{endpoint}?api_key={api_key}&file_type=json");
    for (k, v) in params {
        url.push_str(&format!("&{k}={v}"));
    }
    url
}

/// Fetch JSON from a FRED API endpoint.
async fn fred_fetch(
    http: &reqwest::Client,
    api_key: &str,
    endpoint: &str,
    params: &[(&str, &str)],
) -> Result<Value, FredError> {
    let url = fred_url(endpoint, api_key, params);
    let resp = http
        .get(&url)
        .send()
        .await
        .map_err(|e| FredError::RequestFailed(e.to_string()))?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(FredError::HttpError { status, body });
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| FredError::ParseError(e.to_string()))?;
    Ok(body)
}

/// Require the FRED API key, returning an error if absent.
fn require_api_key(key: Option<&str>) -> Result<&str, FredError> {
    key.filter(|k| !k.is_empty())
        .ok_or(FredError::MissingApiKey)
}

// ── Tool implementations ────────────────────────────────────────────────────

/// `fred_search_series`: Search FRED series by text.
pub async fn search_series(
    http: &reqwest::Client,
    api_key: Option<&str>,
    req: &FredSearchSeriesRequest,
) -> Result<Value, FredError> {
    let key = require_api_key(api_key)?;
    let limit = req.limit.unwrap_or(10).min(100);
    let order_by = req.order_by.as_deref().unwrap_or("popularity");

    let mut params: Vec<(&str, String)> = vec![
        ("search_text", req.search_text.clone()),
        ("limit", limit.to_string()),
        ("order_by", order_by.to_string()),
    ];
    if let Some(cat_id) = req.category_id {
        params.push(("category_id", cat_id.to_string()));
    }
    if let Some(ref tags) = req.tag_names {
        params.push(("tag_names", tags.clone()));
    }
    let params_ref: Vec<(&str, &str)> =
        params.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let body = fred_fetch(http, key, "series/search", &params_ref).await?;

    // Extract the series array and simplify.
    let count = body
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let series_list = body
        .get("seriess")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = series_list
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                "title": s.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                "units": s.get("units").and_then(|v| v.as_str()).unwrap_or(""),
                "frequency": s.get("frequency").and_then(|v| v.as_str()).unwrap_or(""),
                "seasonal_adjustment": s.get("seasonal_adjustment_short").and_then(|v| v.as_str()).unwrap_or(""),
                "observation_start": s.get("observation_start").and_then(|v| v.as_str()).unwrap_or(""),
                "observation_end": s.get("observation_end").and_then(|v| v.as_str()).unwrap_or(""),
                "popularity": s.get("popularity").and_then(|v| v.as_i64()).unwrap_or(0),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "count": count,
        "results": results,
        "returned": results.len(),
    }))
}

/// `fred_get_observations`: Fetch time series observations.
pub async fn get_observations(
    http: &reqwest::Client,
    api_key: Option<&str>,
    req: &FredGetObservationsRequest,
) -> Result<Value, FredError> {
    let key = require_api_key(api_key)?;
    let limit = req.limit.unwrap_or(100).min(1000);

    let mut params: Vec<(&str, String)> = vec![
        ("series_id", req.series_id.clone()),
        ("limit", limit.to_string()),
        ("sort_order", "desc".to_string()),
    ];
    if let Some(ref start) = req.observation_start {
        params.push(("observation_start", start.clone()));
    }
    if let Some(ref end) = req.observation_end {
        params.push(("observation_end", end.clone()));
    }
    if let Some(ref freq) = req.frequency {
        params.push(("frequency", freq.clone()));
    }
    if let Some(ref units) = req.units {
        params.push(("units", units.clone()));
    }
    let params_ref: Vec<(&str, &str)> =
        params.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let body = fred_fetch(http, key, "series/observations", &params_ref).await?;

    let count = body
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let observations = body
        .get("observations")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Simplify: extract date + value pairs, skip "." (FRED's missing value).
    let obs: Vec<Value> = observations
        .iter()
        .filter_map(|o| {
            let date = o.get("date").and_then(|v| v.as_str())?;
            let value_str = o.get("value").and_then(|v| v.as_str())?;
            if value_str == "." {
                return None; // FRED uses "." for missing values
            }
            let value: f64 = value_str.parse().ok()?;
            Some(serde_json::json!({
                "date": date,
                "value": value,
            }))
        })
        .collect();

    // Get series metadata from the response.
    let units = body
        .get("units")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let frequency = body
        .get("frequency")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(serde_json::json!({
        "series_id": req.series_id,
        "units": units,
        "frequency": frequency,
        "count": count,
        "observations": obs,
        "returned": obs.len(),
    }))
}

/// `fred_get_series_info`: Get metadata for a single series.
pub async fn get_series_info(
    http: &reqwest::Client,
    api_key: Option<&str>,
    req: &FredGetSeriesInfoRequest,
) -> Result<Value, FredError> {
    let key = require_api_key(api_key)?;
    let params = vec![("series_id", req.series_id.as_str())];
    let body = fred_fetch(http, key, "series", &params).await?;

    let s = &body;
    Ok(serde_json::json!({
        "id": s.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "title": s.get("title").and_then(|v| v.as_str()).unwrap_or(""),
        "units": s.get("units").and_then(|v| v.as_str()).unwrap_or(""),
        "frequency": s.get("frequency").and_then(|v| v.as_str()).unwrap_or(""),
        "frequency_short": s.get("frequency_short").and_then(|v| v.as_str()).unwrap_or(""),
        "seasonal_adjustment": s.get("seasonal_adjustment").and_then(|v| v.as_str()).unwrap_or(""),
        "seasonal_adjustment_short": s.get("seasonal_adjustment_short").and_then(|v| v.as_str()).unwrap_or(""),
        "observation_start": s.get("observation_start").and_then(|v| v.as_str()).unwrap_or(""),
        "observation_end": s.get("observation_end").and_then(|v| v.as_str()).unwrap_or(""),
        "last_updated": s.get("last_updated").and_then(|v| v.as_str()).unwrap_or(""),
        "popularity": s.get("popularity").and_then(|v| v.as_i64()).unwrap_or(0),
        "notes": s.get("notes").and_then(|v| v.as_str()).unwrap_or(""),
    }))
}

/// `fred_list_categories`: Browse the FRED category tree.
pub async fn list_categories(
    http: &reqwest::Client,
    api_key: Option<&str>,
    req: &FredListCategoriesRequest,
) -> Result<Value, FredError> {
    let key = require_api_key(api_key)?;
    let cat_id = req.category_id.unwrap_or(0);
    let cat_id_str = cat_id.to_string();
    let params = vec![("category_id", cat_id_str.as_str())];
    let body = fred_fetch(http, key, "category/children", &params).await?;

    let categories = body
        .get("categories")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = categories
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                "name": c.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "parent_id": c.get("parent_id").and_then(|v| v.as_i64()).unwrap_or(0),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "parent_category_id": cat_id,
        "categories": results,
        "count": results.len(),
    }))
}

/// `fred_get_release`: Get release metadata (data release schedule + series list).
pub async fn get_release(
    http: &reqwest::Client,
    api_key: Option<&str>,
    req: &FredGetReleaseRequest,
) -> Result<Value, FredError> {
    let key = require_api_key(api_key)?;
    let release_id_str = req.release_id.to_string();

    // Fetch release metadata.
    let release_params = vec![("release_id", release_id_str.as_str())];
    let release_body = fred_fetch(http, key, "release", &release_params).await?;

    let release = &release_body
        .get("releases")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .cloned()
        .unwrap_or(Value::Null);

    // Fetch series in this release.
    let series_body = fred_fetch(
        http,
        key,
        "release/series",
        &[
            ("release_id", release_id_str.as_str()),
            ("limit", "50"),
        ],
    )
    .await?;

    let series_list = series_body
        .get("seriess")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let series: Vec<Value> = series_list
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                "title": s.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                "units": s.get("units").and_then(|v| v.as_str()).unwrap_or(""),
                "frequency": s.get("frequency").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "release_id": req.release_id,
        "name": release.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        "description": release.get("description").and_then(|v| v.as_str()).unwrap_or(""),
        "last_updated": release.get("last_updated").and_then(|v| v.as_str()).unwrap_or(""),
        "next_release": release.get("next_release").and_then(|v| v.as_str()).unwrap_or(""),
        "series": series,
        "series_count": series.len(),
    }))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fred_url_builds_correctly() {
        let url = fred_url("series/observations", "testkey", &[("series_id", "FEDFUNDS")]);
        assert!(url.contains("api.stlouisfed.org/fred/series/observations"));
        assert!(url.contains("api_key=testkey"));
        assert!(url.contains("file_type=json"));
        assert!(url.contains("series_id=FEDFUNDS"));
    }

    #[test]
    fn require_api_key_returns_err_when_missing() {
        assert!(require_api_key(None).is_err());
        assert!(require_api_key(Some("")).is_err());
        assert!(require_api_key(Some("key")).is_ok());
    }

    #[test]
    fn fred_error_classifies_correctly() {
        use hkask_types::McpErrorKind;
        let e: hkask_mcp_server::server::McpToolError =
            FredError::MissingApiKey.into();
        assert_eq!(e.kind, McpErrorKind::InvalidArgument);

        let e: hkask_mcp_server::server::McpToolError =
            FredError::RequestFailed("timeout".into()).into();
        assert_eq!(e.kind, McpErrorKind::Unavailable);

        let e: hkask_mcp_server::server::McpToolError =
            FredError::ParseError("bad json".into()).into();
        assert_eq!(e.kind, McpErrorKind::Internal);
    }
}
