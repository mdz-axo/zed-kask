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
    /// Optional: filter by tag(s), semicolon-delimited (e.g., "employment;monthly").
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
    /// Optional: aggregate to a lower frequency. One of the FRED codes d, w,
    /// bw, m, q, sa, a (the names daily, weekly, biweekly, monthly,
    /// quarterly, semiannual, annual are accepted and mapped), or a
    /// week-ending code (wef, weth, wew, wetu, wem, wesu, wesa, bwew, bwem).
    /// Default: native frequency.
    pub frequency: Option<String>,
    /// Optional: units transformation. One of: lin (levels), chg (change),
    /// ch1 (change from year ago), pch (percent change), pc1 (percent change
    /// from year ago), pca (compounded annual rate of change), cch
    /// (continuously compounded rate of change), cca (continuously compounded
    /// annual rate of change), log (natural log). Default: lin.
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

/// Build a FRED API URL with the API key and `file_type=json` prepended.
/// Values are encoded by the shared builder, so a caller value cannot add
/// or override a query parameter (mcp-tool-review PM2-03).
fn fred_url(endpoint: &str, api_key: &str, params: &[(&str, &str)]) -> String {
    let mut all: Vec<(&str, &str)> = vec![("api_key", api_key), ("file_type", "json")];
    all.extend_from_slice(params);
    EconomicDataClient::build_url(FRED_API_BASE, endpoint, &all)
}

/// FRED answers an id it does not have with HTTP 400 "The … does not
/// exist." rather than an empty list, so the shared status map would call a
/// missing caller-named resource a bad argument (mcp-tool-review PM2-15,
/// live 2026-11-11). `not_found` is the message naming the missing thing.
fn missing_as_not_found(error: EconomicDataError, not_found: String) -> EconomicDataError {
    match error {
        EconomicDataError::HttpError {
            status: 400,
            ref body,
            ..
        } if body.contains("does not exist") => EconomicDataError::NotFound(not_found),
        other => other,
    }
}

/// FRED's aggregation-frequency codes with the names this tool also accepts.
const FRED_FREQUENCIES: &[(&str, &str)] = &[
    ("d", "daily"),
    ("w", "weekly"),
    ("bw", "biweekly"),
    ("m", "monthly"),
    ("q", "quarterly"),
    ("sa", "semiannual"),
    ("a", "annual"),
];

/// FRED's week-ending aggregation codes (no long names).
const FRED_WEEK_ENDING_FREQUENCIES: &[&str] = &[
    "wef", "weth", "wew", "wetu", "wem", "wesu", "wesa", "bwew", "bwem",
];

/// FRED's units-transformation codes.
const FRED_UNITS: &[&str] = &[
    "lin", "chg", "ch1", "pch", "pc1", "pca", "cch", "cca", "log",
];

/// Resolve a caller's frequency (code or name) to the FRED code.
fn fred_frequency_code(raw: &str) -> Result<&'static str, EconomicDataError> {
    let wanted = raw.trim().to_ascii_lowercase();
    FRED_FREQUENCIES
        .iter()
        .find(|(code, name)| wanted == *code || wanted == *name)
        .map(|(code, _)| *code)
        .or_else(|| {
            FRED_WEEK_ENDING_FREQUENCIES
                .iter()
                .find(|code| wanted == **code)
                .copied()
        })
        .ok_or_else(|| {
            EconomicDataError::InvalidParam(format!(
                "frequency {raw:?} is not one of: d (daily), w (weekly), bw (biweekly), m (monthly), q (quarterly), sa (semiannual), a (annual), {}",
                FRED_WEEK_ENDING_FREQUENCIES.join(", ")
            ))
        })
}

/// Resolve a caller's units transformation to the FRED code.
fn fred_units_code(raw: &str) -> Result<&'static str, EconomicDataError> {
    let wanted = raw.trim().to_ascii_lowercase();
    FRED_UNITS
        .iter()
        .find(|code| wanted == **code)
        .copied()
        .ok_or_else(|| {
            EconomicDataError::InvalidParam(format!(
                "units {raw:?} is not one of: {}",
                FRED_UNITS.join(", ")
            ))
        })
}

/// Extract a string field from a FRED series JSON object, defaulting to `""`
/// when absent or non-string — the per-field shape shared by the series
/// listings (`series/search`, `/fred/series`, release series). The field
/// lists stay per site: each tool's output shape is its own contract, and a
/// remapped source key (e.g. `seasonal_adjustment` read from
/// `seasonal_adjustment_short`) rides the explicit key parameter.
fn series_str_field(series: &Value, key: &str) -> String {
    series
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
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

    // `null` when the provider omits it — never a fabricated 0.
    let count = body.get("count").and_then(|v| v.as_u64());
    let series_list = body
        .get("seriess")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = series_list
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": series_str_field(s, "id"),
                "title": series_str_field(s, "title"),
                "units": series_str_field(s, "units"),
                "frequency": series_str_field(s, "frequency"),
                "seasonal_adjustment": series_str_field(s, "seasonal_adjustment_short"),
                "observation_start": series_str_field(s, "observation_start"),
                "observation_end": series_str_field(s, "observation_end"),
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
    let frequency = req
        .frequency
        .as_deref()
        .map(fred_frequency_code)
        .transpose()?;
    let units = req.units.as_deref().map(fred_units_code).transpose()?;

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
    if let Some(code) = frequency {
        params.push(("frequency", code.to_string()));
    }
    if let Some(code) = units {
        params.push(("units", code.to_string()));
    }
    let params_ref: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let url = fred_url("series/observations", key, &params_ref);

    let body = client
        .fetch(FRED_PROVIDER, &url)
        .await
        .map_err(|e| missing_as_not_found(e, format!("FRED series {} not found", req.series_id)))?;

    // `null` when the provider omits it — never a fabricated 0.
    let count = body.get("count").and_then(|v| v.as_u64());
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

    // The observations response carries the units TRANSFORMATION applied
    // (e.g. "lin"), not the series' unit of measure, and no frequency —
    // `frequency` echoes the aggregation requested (null = native).
    let units = body.get("units").and_then(|v| v.as_str()).unwrap_or("lin");

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
    let body = client
        .fetch(FRED_PROVIDER, &url)
        .await
        .map_err(|e| missing_as_not_found(e, format!("FRED series {} not found", req.series_id)))?;
    series_info_from_body(&body, &req.series_id)
}

/// Shape FRED's `/fred/series` response into the tool output. The API wraps
/// the metadata in `{"seriess": [ {...} ]}` — the fields are NOT at the top
/// level. An empty `seriess` array (unknown series id) or a missing key is
/// surfaced as an error, never shaped into an all-empty success.
fn series_info_from_body(body: &Value, series_id: &str) -> Result<Value, EconomicDataError> {
    let s = body
        .get("seriess")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| EconomicDataError::NotFound(format!("FRED series {series_id} not found")))?;
    Ok(serde_json::json!({
        "id": series_str_field(s, "id"),
        "title": series_str_field(s, "title"),
        "units": series_str_field(s, "units"),
        "frequency": series_str_field(s, "frequency"),
        "frequency_short": series_str_field(s, "frequency_short"),
        "seasonal_adjustment": series_str_field(s, "seasonal_adjustment"),
        "seasonal_adjustment_short": series_str_field(s, "seasonal_adjustment_short"),
        "observation_start": series_str_field(s, "observation_start"),
        "observation_end": series_str_field(s, "observation_end"),
        "last_updated": series_str_field(s, "last_updated"),
        "popularity": s.get("popularity").and_then(|v| v.as_i64()).unwrap_or(0),
        "notes": series_str_field(s, "notes"),
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
    let body = client
        .fetch(FRED_PROVIDER, &url)
        .await
        .map_err(|e| missing_as_not_found(e, format!("FRED category {cat_id} not found")))?;

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
    let release_body = client
        .fetch(FRED_PROVIDER, &release_url)
        .await
        .map_err(|e| {
            missing_as_not_found(e, format!("FRED release {} not found", req.release_id))
        })?;

    // A release id FRED does not have is surfaced, never shaped into an
    // all-empty success.
    let release = release_body
        .get("releases")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| {
            EconomicDataError::NotFound(format!("FRED release {} not found", req.release_id))
        })?;

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
                "id": series_str_field(s, "id"),
                "title": series_str_field(s, "title"),
                "units": series_str_field(s, "units"),
                "frequency": series_str_field(s, "frequency"),
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

    /// Regression pin: the fields live under `seriess[0]`, not the top-level
    /// body. The pre-fix shaping read the top level and emitted all-empty
    /// fields on every successful fetch.
    #[test]
    fn series_info_reads_seriess_array() {
        let body = serde_json::json!({
            "seriess": [{
                "id": "CPIAUCSL",
                "title": "Consumer Price Index for All Urban Consumers: All Items in U.S. City Average",
                "units": "Index 1982-1984=100",
                "frequency": "Monthly",
                "frequency_short": "M",
                "seasonal_adjustment": "Seasonally Adjusted",
                "seasonal_adjustment_short": "SA",
                "observation_start": "1947-01-01",
                "observation_end": "2026-08-01",
                "last_updated": "2026-09-11 07:37:15-05",
                "popularity": 90,
                "notes": "The Consumer Price Index..."
            }]
        });
        let out =
            series_info_from_body(&body, "CPIAUCSL").expect("shaping must succeed on real shape");
        assert_eq!(out["id"], "CPIAUCSL");
        assert_eq!(out["units"], "Index 1982-1984=100");
        assert_eq!(out["frequency"], "Monthly");
        assert_eq!(out["popularity"], 90);
        assert!(
            out["title"]
                .as_str()
                .expect("title is a string")
                .contains("Consumer Price Index")
        );
    }

    /// An unknown series id makes FRED return `{"seriess": []}` — that must
    /// surface as an error naming the id, not an all-empty success.
    #[test]
    fn unknown_series_id_is_surfaced_not_empty_success() {
        let body = serde_json::json!({ "seriess": [] });
        let err = series_info_from_body(&body, "NOT_A_SERIES")
            .err()
            .expect("empty seriess must error");
        assert!(
            err.to_string().contains("NOT_A_SERIES"),
            "error must name the series id, got: {err}"
        );
    }

    /// A body without the `seriess` key (shape drift, upstream error page
    /// parsed as JSON) must error — the pre-fix code returned an all-empty
    /// success for it.
    #[test]
    fn missing_seriess_key_is_surfaced_not_empty_success() {
        let body = serde_json::json!({ "some_other_shape": true });
        assert!(series_info_from_body(&body, "X").is_err());
    }

    /// mcp-tool-review PM2-03 pin: a caller value cannot add or override a
    /// query parameter. Live 2026-11-10, an `observation_start` of
    /// `2024-01-01&observation_end=2024-03-01` narrowed the window.
    #[test]
    fn caller_values_cannot_inject_query_parameters() {
        let url = fred_url(
            "series/observations",
            "KEY",
            &[("observation_start", "2024-01-01&observation_end=2024-03-01")],
        );
        assert_eq!(
            url,
            "https://api.stlouisfed.org/fred/series/observations?api_key=KEY&file_type=json&observation_start=2024-01-01%26observation_end%3D2024-03-01"
        );
    }

    /// mcp-tool-review PM2-07 pin: the frequency and units closed sets are
    /// FRED's. Live 2026-11-10, the previously documented `monthly` and
    /// `pct` were both rejected by FRED with HTTP 400.
    #[test]
    fn frequency_and_units_resolve_to_fred_codes_or_name_the_valid_set() {
        assert_eq!(fred_frequency_code("monthly").ok(), Some("m"));
        assert_eq!(fred_frequency_code(" Q ").ok(), Some("q"));
        assert_eq!(fred_frequency_code("wef").ok(), Some("wef"));
        let error = fred_frequency_code("fortnightly").expect_err("unknown frequency");
        assert!(error.to_string().contains("bw (biweekly)"), "{error}");
        assert_eq!(fred_units_code("pch").ok(), Some("pch"));
        let error = fred_units_code("pct").expect_err("pct is not a FRED code");
        assert!(error.to_string().contains("pch"), "{error}");
    }

    /// mcp-tool-review PM2-15 pin: an id FRED does not have is `not_found`
    /// naming it. Live 2026-11-11, an unknown series, release and category
    /// each came back HTTP 400 "… does not exist." and reached the caller as
    /// `invalid_argument`. Every other 400 stays the caller's bad argument.
    #[test]
    fn an_id_fred_does_not_have_is_not_found_naming_it() {
        use hkask_mcp_server::server::McpToolError;
        use hkask_types::McpErrorKind;
        let http_400 = |message: &str| EconomicDataError::HttpError {
            provider: FRED_PROVIDER,
            status: 400,
            body: format!(r#"{{"error_code":400,"error_message":"{message}"}}"#),
        };
        for missing in [
            "Bad Request.  The series does not exist.",
            "Bad Request.  The release does not exist.",
            "Bad Request.  The category does not exist.",
        ] {
            let error = McpToolError::from(missing_as_not_found(
                http_400(missing),
                "FRED series ZZPROBEUNKNOWN not found".to_string(),
            ));
            assert_eq!(error.kind, McpErrorKind::NotFound, "{missing}");
            assert_eq!(error.message, "FRED series ZZPROBEUNKNOWN not found");
        }

        let bad_date = McpToolError::from(missing_as_not_found(
            http_400(
                "Bad Request.  Variable observation_start is not a YYYY-MM-DD formatted date (e.g. 2000-02-24).",
            ),
            "FRED series FEDFUNDS not found".to_string(),
        ));
        assert_eq!(bad_date.kind, McpErrorKind::InvalidArgument);
    }
}
