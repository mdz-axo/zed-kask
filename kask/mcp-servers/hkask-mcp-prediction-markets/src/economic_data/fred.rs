//! FRED (Federal Reserve Economic Data) provider adapter.
//!
//! FRED is the St. Louis Fed's economic data API, offering ~800,000 economic
//! time series from 80+ sources (BEA, BLS, Census, Fed, etc.). This module
//! owns only FRED's request types and response shaping; the shared HTTP
//! fetch/error shape lives in `super::EconomicDataClient` / `EconomicDataError`.
//!
//! API docs: https://fred.stlouisfed.org/docs/api/fred/
//!
//! All tools require `HKASK_FRED_API_KEY` (in the server's credential
//! allowlist). The key is passed through to each tool function.

use super::{EconomicDataClient, EconomicDataError};
use serde::Deserialize;
use serde_json::Value;

const FRED_API_BASE: &str = "https://api.stlouisfed.org/fred";
const FRED_PROVIDER: &str = "FRED";

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

// ── Helpers ────────────────────────────────────────────────────────────────

/// Require the FRED API key, returning an error if absent or empty.
fn require_api_key(key: Option<&str>) -> Result<&str, EconomicDataError> {
    key.filter(|k| !k.is_empty())
        .ok_or(EconomicDataError::MissingApiKey)
}

/// Build a FRED API URL with the API key and `file_type=json` appended.
fn fred_url(endpoint: &str, api_key: &str, params: &[(&str, &str)]) -> String {
    let mut url = format!("{FRED_API_BASE}/{endpoint}?api_key={api_key}&file_type=json");
    for (k, v) in params {
        url.push_str(&format!("&{k}={v}"));
    }
    url
}

// ── Tool implementations ────────────────────────────────────────────────────

/// `fred_search_series`: Search FRED series by text.
pub async fn search_series(
    client: &EconomicDataClient<'_>,
    api_key: Option<&str>,
    req: &FredSearchSeriesRequest,
) -> Result<Value, EconomicDataError> {
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
    let params_ref: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let url = fred_url("series/search", key, &params_ref);

    let body = client.fetch(FRED_PROVIDER, &url).await?;

    let count = body.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
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
    client: &EconomicDataClient<'_>,
    api_key: Option<&str>,
    req: &FredGetObservationsRequest,
) -> Result<Value, EconomicDataError> {
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
    let params_ref: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let url = fred_url("series/observations", key, &params_ref);

    let body = client.fetch(FRED_PROVIDER, &url).await?;

    let count = body.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
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

    let units = body.get("units").and_then(|v| v.as_str()).unwrap_or("");
    let frequency = body.get("frequency").and_then(|v| v.as_str()).unwrap_or("");

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
    client: &EconomicDataClient<'_>,
    api_key: Option<&str>,
    req: &FredGetSeriesInfoRequest,
) -> Result<Value, EconomicDataError> {
    let key = require_api_key(api_key)?;
    let url = fred_url("series", key, &[("series_id", req.series_id.as_str())]);
    let body = client.fetch(FRED_PROVIDER, &url).await?;

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
    client: &EconomicDataClient<'_>,
    api_key: Option<&str>,
    req: &FredListCategoriesRequest,
) -> Result<Value, EconomicDataError> {
    let key = require_api_key(api_key)?;
    let cat_id = req.category_id.unwrap_or(0);
    let cat_id_str = cat_id.to_string();
    let url = fred_url(
        "category/children",
        key,
        &[("category_id", cat_id_str.as_str())],
    );
    let body = client.fetch(FRED_PROVIDER, &url).await?;

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
    client: &EconomicDataClient<'_>,
    api_key: Option<&str>,
    req: &FredGetReleaseRequest,
) -> Result<Value, EconomicDataError> {
    let key = require_api_key(api_key)?;
    let release_id_str = req.release_id.to_string();

    let release_url = fred_url("release", key, &[("release_id", release_id_str.as_str())]);
    let release_body = client.fetch(FRED_PROVIDER, &release_url).await?;

    let release = &release_body
        .get("releases")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .cloned()
        .unwrap_or(Value::Null);

    let series_url = fred_url(
        "release/series",
        key,
        &[("release_id", release_id_str.as_str()), ("limit", "50")],
    );
    let series_body = client.fetch(FRED_PROVIDER, &series_url).await?;

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
