//! World Bank Indicators API client and MCP tool implementations.
//!
//! The World Bank Indicators API provides access to ~29,500 time series
//! indicators across 45+ databases covering development, poverty, health,
//! education, trade, gender, climate, and governance for all countries,
//! regional aggregates, and income groups.
//!
//! This is the global complement to FRED (which is US-centric). Together they
//! give the data radar US depth (FRED) + global breadth (World Bank).
//!
//! API docs: https://datahelpdesk.worldbank.org/knowledgebase/articles/889392
//!
//! No API key required — the World Bank API is fully open.

use serde::Deserialize;
use serde_json::Value;

// ── Constants ──────────────────────────────────────────────────────────────

const WB_API_BASE: &str = "https://api.worldbank.org/v2";

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum WbError {
    #[error("World Bank API request failed: {0}")]
    RequestFailed(String),
    #[error("World Bank API returned HTTP {status}: {body}")]
    HttpError { status: u16, body: String },
    #[error("World Bank API response parse error: {0}")]
    ParseError(String),
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
    #[error("World Bank API returned an error message: {0}")]
    ApiError(String),
}

impl From<WbError> for hkask_mcp_server::server::McpToolError {
    fn from(e: WbError) -> Self {
        use hkask_mcp_server::server::McpToolError;
        match e {
            WbError::InvalidParam(_) => McpToolError::invalid_argument(e.to_string()),
            WbError::RequestFailed(_) | WbError::HttpError { .. } => {
                McpToolError::unavailable(e.to_string())
            }
            WbError::ParseError(_) | WbError::ApiError(_) => McpToolError::internal(e.to_string()),
        }
    }
}

// ── Request types (MCP tool parameters) ─────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WbSearchIndicatorsRequest {
    /// Search text (e.g., "employment", "GDP per capita", "poverty headcount").
    pub query: String,
    /// Optional: filter by topic ID (e.g., 11 for Poverty).
    pub topic_id: Option<u32>,
    /// Max results to return (default 10, capped at 100).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WbGetObservationsRequest {
    /// World Bank indicator ID (e.g., "SP.POP.TOTL", "NY.GDP.PCAP.PP.KD").
    pub indicator_id: String,
    /// ISO 3-letter country code (e.g., "USA", "CHN", "DEU"), or "all" for
    /// all countries/regions.
    pub country_code: String,
    /// Optional: start year (e.g., "2000"). Defaults to earliest available.
    pub date_start: Option<String>,
    /// Optional: end year (e.g., "2024"). Defaults to latest available.
    pub date_end: Option<String>,
    /// Max observations to return (default 100, capped at 1000).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WbListCountriesRequest {
    /// Optional: filter by income group or region. Values: "all" (default),
    /// "hic" (high income), "mic" (middle income), "lic" (low income).
    pub income_group: Option<String>,
    /// Max results (default 50, capped at 300 — the WB API max per_page).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WbListTopicsRequest {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WbGetIndicatorInfoRequest {
    /// World Bank indicator ID (e.g., "SP.POP.TOTL").
    pub indicator_id: String,
}

// ── World Bank API client ───────────────────────────────────────────────────

/// Build a World Bank API URL with common parameters.
fn wb_url(endpoint: &str, params: &[(&str, &str)]) -> String {
    let mut url = format!("{WB_API_BASE}/{endpoint}?format=json");
    for (k, v) in params {
        url.push_str(&format!("&{k}={v}"));
    }
    url
}

/// Fetch JSON from a World Bank API endpoint.
///
/// The WB API returns a 2-element JSON array: [metadata_object, data_array].
/// This function returns the raw `serde_json::Value` (the full array).
async fn wb_fetch(
    http: &reqwest::Client,
    endpoint: &str,
    params: &[(&str, &str)],
) -> Result<Value, WbError> {
    let url = wb_url(endpoint, params);
    let resp = http
        .get(&url)
        .send()
        .await
        .map_err(|e| WbError::RequestFailed(e.to_string()))?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(WbError::HttpError { status, body });
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|e| WbError::ParseError(e.to_string()))?;
    Ok(body)
}

/// Extract the data array (second element) from a WB API response.
/// The WB API returns `[metadata, data_array]`. Returns the data_array.
fn wb_extract_data(body: &Value) -> Result<&Vec<Value>, WbError> {
    body.as_array()
        .and_then(|arr| arr.get(1))
        .and_then(|v| v.as_array())
        .ok_or_else(|| WbError::ParseError("expected [meta, data] array".into()))
}

/// Extract the metadata object (first element) from a WB API response.
fn wb_extract_meta(body: &Value) -> Result<&Value, WbError> {
    body.as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| WbError::ParseError("expected [meta, data] array".into()))
}

// ── Tool implementations ────────────────────────────────────────────────────

/// `wb_search_indicators`: Search World Bank indicators by text.
pub async fn search_indicators(
    http: &reqwest::Client,
    req: &WbSearchIndicatorsRequest,
) -> Result<Value, WbError> {
    let limit = req.limit.unwrap_or(10).min(100);

    if let Some(topic_id) = req.topic_id {
        // Topic-filtered indicator list.
        let topic_id_str = topic_id.to_string();
        let per_page = limit.to_string();
        let url =
            format!("{WB_API_BASE}/topic/{topic_id_str}/indicator?format=json&per_page={per_page}");
        let resp = http
            .get(&url)
            .send()
            .await
            .map_err(|e| WbError::RequestFailed(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(WbError::HttpError { status, body });
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| WbError::ParseError(e.to_string()))?;

        let data = wb_extract_data(&body)?;
        let query_lower = req.query.to_lowercase();
        let filtered: Vec<&Value> = data
            .iter()
            .filter(|ind| {
                ind.get("name")
                    .and_then(|v| v.as_str())
                    .map(|n| n.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
            })
            .collect();
        let results: Vec<Value> = filtered
            .iter()
            .map(|ind| {
                serde_json::json!({
                    "id": ind.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                    "name": ind.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "unit": ind.get("unit").and_then(|v| v.as_str()).unwrap_or(""),
                    "source": ind.get("source").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or(""),
                    "source_note": ind.get("sourceNote").and_then(|v| v.as_str()).unwrap_or(""),
                    "topics": ind.get("topics").and_then(|v| v.as_array()).map(|t| {
                        t.iter().map(|topic| {
                            serde_json::json!({
                                "id": topic.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                                "value": topic.get("value").and_then(|v| v.as_str()).unwrap_or(""),
                            })
                        }).collect::<Vec<_>>()
                    }).unwrap_or_default(),
                })
            })
            .collect();
        return Ok(serde_json::json!({
            "query": req.query,
            "topic_id": req.topic_id,
            "results": results,
            "returned": results.len(),
        }));
    }

    // No topic filter — list all indicators and filter client-side.
    // Request a larger page to search through.
    let search_params: Vec<(&str, &str)> = vec![("per_page", "1000"), ("format", "json")];
    let body = wb_fetch(http, "indicator", &search_params).await?;

    let data = wb_extract_data(&body)?;
    let query_lower = req.query.to_lowercase();
    let filtered: Vec<&Value> = data
        .iter()
        .filter(|ind| {
            let name = ind.get("name").and_then(|v| v.as_str()).unwrap_or("");
            name.to_lowercase().contains(&query_lower)
        })
        .take(limit as usize)
        .collect();

    let results: Vec<Value> = filtered
        .iter()
        .map(|ind| {
            serde_json::json!({
                "id": ind.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                "name": ind.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "unit": ind.get("unit").and_then(|v| v.as_str()).unwrap_or(""),
                "source": ind.get("source").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or(""),
                "source_note": ind.get("sourceNote").and_then(|v| v.as_str()).unwrap_or(""),
                "topics": ind.get("topics").and_then(|v| v.as_array()).map(|t| {
                    t.iter().map(|topic| {
                        serde_json::json!({
                            "id": topic.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                            "value": topic.get("value").and_then(|v| v.as_str()).unwrap_or(""),
                        })
                    }).collect::<Vec<_>>()
                }).unwrap_or_default(),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "query": req.query,
        "results": results,
        "returned": results.len(),
    }))
}

/// `wb_get_observations`: Fetch time series observations for a country + indicator.
pub async fn get_observations(
    http: &reqwest::Client,
    req: &WbGetObservationsRequest,
) -> Result<Value, WbError> {
    let limit = req.limit.unwrap_or(100).min(1000);
    let limit_str = limit.to_string();

    // Build date range param: "start:end" or just "start" or "end".
    let date_param = match (&req.date_start, &req.date_end) {
        (Some(s), Some(e)) => format!("{s}:{e}"),
        (Some(s), None) => s.clone(),
        (None, Some(e)) => format!(":{e}"),
        (None, None) => String::new(),
    };

    let country = &req.country_code;
    let indicator = &req.indicator_id;
    let endpoint = format!("country/{country}/indicator/{indicator}");

    let mut params: Vec<(&str, String)> = vec![("per_page", limit_str)];
    if !date_param.is_empty() {
        params.push(("date", date_param));
    }
    let params_ref: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let body = wb_fetch(http, &endpoint, &params_ref).await?;

    let meta = wb_extract_meta(&body)?;
    let total = meta.get("total").and_then(|v| v.as_u64()).unwrap_or(0);

    let data = wb_extract_data(&body)?;

    // Extract observations: date + value pairs, skipping null values.
    let obs: Vec<Value> = data
        .iter()
        .filter_map(|o| {
            let date = o.get("date").and_then(|v| v.as_str())?;
            let value = o.get("value").and_then(|v| v.as_f64())?;
            let country = o
                .get("country")
                .and_then(|v| v.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let iso3 = o
                .get("countryiso3code")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Some(serde_json::json!({
                "date": date,
                "value": value,
                "country": country,
                "country_iso3": iso3,
            }))
        })
        .collect();

    // Get indicator name from the first observation.
    let indicator_name = data
        .first()
        .and_then(|o| o.get("indicator"))
        .and_then(|i| i.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(serde_json::json!({
        "indicator_id": req.indicator_id,
        "indicator_name": indicator_name,
        "country_code": req.country_code,
        "total": total,
        "observations": obs,
        "returned": obs.len(),
    }))
}

/// `wb_list_countries`: List all countries/regions with ISO codes.
pub async fn list_countries(
    http: &reqwest::Client,
    req: &WbListCountriesRequest,
) -> Result<Value, WbError> {
    let limit = req.limit.unwrap_or(50).min(300);
    let limit_str = limit.to_string();

    let params: Vec<(&str, &str)> = vec![("per_page", limit_str.as_str())];
    let body = wb_fetch(http, "country", &params).await?;

    let data = wb_extract_data(&body)?;

    // Filter by income group if specified.
    let income_filter = req.income_group.as_deref();
    let filtered: Vec<&Value> = data
        .iter()
        .filter(|c| {
            // Skip aggregates (regions, income groups) — only show countries.
            // WB marks non-countries with capital region id == "NA".
            let is_country = c
                .get("region")
                .and_then(|r| r.get("id"))
                .and_then(|v| v.as_str())
                .map(|id| id != "NA")
                .unwrap_or(false);
            if !is_country {
                return false;
            }
            if let Some(filter) = income_filter {
                let ig = c
                    .get("incomeLevel")
                    .and_then(|il| il.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                return ig.eq_ignore_ascii_case(filter);
            }
            true
        })
        .collect();

    let results: Vec<Value> = filtered
        .iter()
        .map(|c| {
            serde_json::json!({
                "iso3": c.get("iso3").and_then(|v| v.as_str()).unwrap_or(""),
                "name": c.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "region": c.get("region").and_then(|r| r.get("value")).and_then(|v| v.as_str()).unwrap_or(""),
                "income_level": c.get("incomeLevel").and_then(|il| il.get("value")).and_then(|v| v.as_str()).unwrap_or(""),
                "capital_city": c.get("capitalCity").and_then(|v| v.as_str()).unwrap_or(""),
                "longitude": c.get("longitude").and_then(|v| v.as_f64()),
                "latitude": c.get("latitude").and_then(|v| v.as_f64()),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "countries": results,
        "count": results.len(),
        "income_group_filter": req.income_group,
    }))
}

/// `wb_list_topics`: Browse the World Bank topic tree.
pub async fn list_topics(
    _http: &reqwest::Client,
    _req: &WbListTopicsRequest,
) -> Result<Value, WbError> {
    let body = wb_fetch(_http, "topic", &[("per_page", "100")]).await?;
    let data = wb_extract_data(&body)?;

    let results: Vec<Value> = data
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                "value": t.get("value").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "topics": results,
        "count": results.len(),
    }))
}

/// `wb_get_indicator_info`: Get metadata for a single indicator.
pub async fn get_indicator_info(
    http: &reqwest::Client,
    req: &WbGetIndicatorInfoRequest,
) -> Result<Value, WbError> {
    let indicator = &req.indicator_id;
    let endpoint = format!("indicator/{indicator}");
    let body = wb_fetch(http, &endpoint, &[("per_page", "1")]).await?;

    let data = wb_extract_data(&body)?;
    let ind = data
        .first()
        .ok_or_else(|| WbError::ParseError("indicator not found".into()))?;

    Ok(serde_json::json!({
        "id": ind.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "name": ind.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        "unit": ind.get("unit").and_then(|v| v.as_str()).unwrap_or(""),
        "source": ind.get("source").and_then(|s| s.get("value")).and_then(|v| v.as_str()).unwrap_or(""),
        "source_note": ind.get("sourceNote").and_then(|v| v.as_str()).unwrap_or(""),
        "source_organization": ind.get("sourceOrganization").and_then(|v| v.as_str()).unwrap_or(""),
        "topics": ind.get("topics").and_then(|v| v.as_array()).map(|t| {
            t.iter().map(|topic| {
                serde_json::json!({
                    "id": topic.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                    "value": topic.get("value").and_then(|v| v.as_str()).unwrap_or(""),
                })
            }).collect::<Vec<_>>()
        }).unwrap_or_default(),
    }))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wb_url_builds_correctly() {
        let url = wb_url(
            "country/USA/indicator/SP.POP.TOTL",
            &[("date", "2000:2024")],
        );
        assert!(url.contains("api.worldbank.org/v2/country/USA/indicator/SP.POP.TOTL"));
        assert!(url.contains("format=json"));
        assert!(url.contains("date=2000:2024"));
    }

    #[test]
    fn wb_error_classifies_correctly() {
        use hkask_types::McpErrorKind;
        let e: hkask_mcp_server::server::McpToolError = WbError::InvalidParam("bad".into()).into();
        assert_eq!(e.kind, McpErrorKind::InvalidArgument);

        let e: hkask_mcp_server::server::McpToolError =
            WbError::RequestFailed("timeout".into()).into();
        assert_eq!(e.kind, McpErrorKind::Unavailable);

        let e: hkask_mcp_server::server::McpToolError =
            WbError::ParseError("bad json".into()).into();
        assert_eq!(e.kind, McpErrorKind::Internal);
    }

    #[test]
    fn wb_extract_data_parses_2element_array() {
        let body = serde_json::json!([
            {"page": 1, "total": 2},
            [{"id": "SP.POP.TOTL", "name": "Population, total"}]
        ]);
        let data = wb_extract_data(&body).unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(
            data[0].get("name").and_then(|v| v.as_str()).unwrap(),
            "Population, total"
        );
    }

    #[test]
    fn wb_extract_data_returns_err_for_non_array() {
        let body = serde_json::json!({"error": "not found"});
        assert!(wb_extract_data(&body).is_err());
    }
}
