//! DBnomics API client and MCP tool implementations.
//!
//! DBnomics is the world's largest open economic time-series aggregator:
//! 1.7B+ series across 47K+ datasets from 700+ providers (IMF, OECD, ECB,
//! INSEE, World Bank, FRED mirrors, etc.). It is the global superset of
//! both FRED (US-centric) and the World Bank Indicators API.
//!
//! API docs: https://db.nomics.world/api/v22/swagger
//!
//! No API key required — DBnomics is fully anonymous. There is no
//! `HKASK_DBNOMICS_API_KEY` credential and no entry in the server's
//! credential allowlist.

use serde::Deserialize;
use serde_json::Value;

// ── Constants ──────────────────────────────────────────────────────────────

const DBNOMICS_API_BASE: &str = "https://api.db.nomics.world/v22/";

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum DbnomicsError {
    #[error("invalid parameter: {0}")]
    InvalidParam(String),
    #[error("DBnomics API request failed: {0}")]
    RequestFailed(String),
    #[error("DBnomics API returned HTTP {status}: {body}")]
    HttpError { status: u16, body: String },
    #[error("DBnomics API response parse error: {0}")]
    ParseError(String),
}

impl From<DbnomicsError> for hkask_mcp_server::server::McpToolError {
    fn from(error: DbnomicsError) -> Self {
        use hkask_mcp_server::server::McpToolError;
        match error {
            DbnomicsError::InvalidParam(_) => McpToolError::invalid_argument(error.to_string()),
            DbnomicsError::HttpError { .. } | DbnomicsError::RequestFailed(_) => {
                McpToolError::unavailable(error.to_string())
            }
            DbnomicsError::ParseError(_) => McpToolError::internal(error.to_string()),
        }
    }
}

// ── Request types (MCP tool parameters) ─────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DbnomicsSearchRequest {
    /// Full-text query (e.g., "GDP", "unemployment rate", "consumer prices").
    pub query: String,
    /// Max results to return (default 10, capped at 100).
    pub limit: Option<u32>,
    /// Offset for pagination (default 0).
    pub offset: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DbnomicsListProvidersRequest {
    /// Max results to return (default 20, capped at 100).
    pub limit: Option<u32>,
    /// Offset for pagination (default 0).
    pub offset: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DbnomicsGetDatasetRequest {
    /// Provider code (e.g., "IMF", "OECD", "ECB", "INSEE", "FRED").
    pub provider_code: String,
    /// Dataset code. Supports the `:latest` release alias (e.g., "WEO:latest"),
    /// which the API resolves via HTTP 302 to the actual release code. The
    /// reqwest default redirect policy (up to 10 hops) follows this.
    pub dataset_code: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DbnomicsGetSeriesRequest {
    /// Provider code (e.g., "IMF", "OECD", "ECB").
    pub provider_code: String,
    /// Dataset code (supports `:latest` release alias).
    pub dataset_code: String,
    /// Series code within the dataset.
    pub series_code: String,
    /// Whether to include observations (default true).
    pub observations: Option<bool>,
    /// Max observations to return (default 1000). Caps the observation
    /// array length; the series metadata is always returned in full.
    pub limit: Option<u32>,
}

// ── DBnomics API client ────────────────────────────────────────────────────

fn dbnomics_url(endpoint: &str, params: &[(&str, &str)]) -> String {
    let mut url = format!("{DBNOMICS_API_BASE}{endpoint}");
    if !params.is_empty() {
        url.push('?');
        let mut first = true;
        for (key, value) in params {
            if !first {
                url.push('&');
            }
            first = false;
            url.push_str(key);
            url.push('=');
            url.push_str(value);
        }
    }
    url
}

async fn dbnomics_fetch(
    http: &reqwest::Client,
    endpoint: &str,
    params: &[(&str, &str)],
) -> Result<Value, DbnomicsError> {
    let url = dbnomics_url(endpoint, params);
    let response = http
        .get(&url)
        .send()
        .await
        .map_err(|error| DbnomicsError::RequestFailed(error.to_string()))?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DbnomicsError::HttpError { status, body });
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| DbnomicsError::ParseError(error.to_string()))?;
    Ok(body)
}

// ── Tool implementations ────────────────────────────────────────────────────

/// `dbnomics_search`: Full-text search across all DBnomics series.
pub async fn search(
    http: &reqwest::Client,
    request: &DbnomicsSearchRequest,
) -> Result<Value, DbnomicsError> {
    let query = request.query.trim();
    if query.is_empty() {
        return Err(DbnomicsError::InvalidParam(
            "query must not be empty".to_string(),
        ));
    }
    let limit = request.limit.unwrap_or(10).min(100);
    let offset = request.offset.unwrap_or(0);

    let limit_str = limit.to_string();
    let offset_str = offset.to_string();
    let params: Vec<(&str, &str)> = vec![
        ("q", query),
        ("limit", limit_str.as_str()),
        ("offset", offset_str.as_str()),
    ];

    let body = dbnomics_fetch(http, "search", &params).await?;

    let num_found = body
        .pointer("/results/num_found")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let docs = body
        .pointer("/results/docs")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = docs
        .iter()
        .map(|series| {
            serde_json::json!({
                "provider_code": series.get("provider_code").and_then(|v| v.as_str()).unwrap_or(""),
                "dataset_code": series.get("dataset_code").and_then(|v| v.as_str()).unwrap_or(""),
                "series_code": series.get("series_code").and_then(|v| v.as_str()).unwrap_or(""),
                "series_name": series.get("series_name").and_then(|v| v.as_str())
                    .or_else(|| series.get("name").and_then(|v| v.as_str()))
                    .unwrap_or(""),
                "frequency": series.get("@frequency").and_then(|v| v.as_str()).unwrap_or(""),
                "start_period": series.get("period").and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                "end_period": series.get("period").and_then(|v| v.as_array())
                    .and_then(|arr| arr.last())
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "query": request.query,
        "num_found": num_found,
        "results": results,
        "returned": results.len(),
        "offset": offset,
    }))
}

/// `dbnomics_list_providers`: List statistical providers.
pub async fn list_providers(
    http: &reqwest::Client,
    request: &DbnomicsListProvidersRequest,
) -> Result<Value, DbnomicsError> {
    let limit = request.limit.unwrap_or(20).min(100);
    let offset = request.offset.unwrap_or(0);

    let limit_str = limit.to_string();
    let offset_str = offset.to_string();
    let params: Vec<(&str, &str)> = vec![
        ("limit", limit_str.as_str()),
        ("offset", offset_str.as_str()),
    ];

    let body = dbnomics_fetch(http, "provider", &params).await?;

    let num_found = body
        .pointer("/providers/num_found")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let docs = body
        .pointer("/providers/docs")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = docs
        .iter()
        .map(|provider| {
            serde_json::json!({
                "code": provider.get("code").and_then(|v| v.as_str()).unwrap_or(""),
                "name": provider.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "region": provider.get("region").and_then(|v| v.as_str()).unwrap_or(""),
                "website": provider.get("website").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "num_found": num_found,
        "providers": results,
        "returned": results.len(),
        "offset": offset,
    }))
}

/// `dbnomics_get_dataset`: Get dataset metadata.
pub async fn get_dataset(
    http: &reqwest::Client,
    request: &DbnomicsGetDatasetRequest,
) -> Result<Value, DbnomicsError> {
    let provider_code = request.provider_code.trim();
    let dataset_code = request.dataset_code.trim();
    if provider_code.is_empty() {
        return Err(DbnomicsError::InvalidParam(
            "provider_code must not be empty".to_string(),
        ));
    }
    if dataset_code.is_empty() {
        return Err(DbnomicsError::InvalidParam(
            "dataset_code must not be empty".to_string(),
        ));
    }

    let endpoint = format!("datasets/{provider_code}/{dataset_code}");
    let body = dbnomics_fetch(http, &endpoint, &[]).await?;

    let dataset = body
        .get("dataset")
        .cloned()
        .or_else(|| body.pointer("/datasets/docs/0").cloned())
        .unwrap_or(Value::Null);

    let dimensions = dataset.get("dimensions").cloned().unwrap_or(Value::Null);
    let dimensions_labels = dataset
        .get("dimensions_labels")
        .cloned()
        .unwrap_or(Value::Null);

    Ok(serde_json::json!({
        "provider_code": provider_code,
        "dataset_code": dataset.get("code").and_then(|v| v.as_str())
            .unwrap_or(dataset_code),
        "name": dataset.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        "description": dataset.get("description").and_then(|v| v.as_str()).unwrap_or(""),
        "dimensions": dimensions,
        "dimensions_labels": dimensions_labels,
        "last_update": dataset.get("indexed_at").and_then(|v| v.as_str())
            .or_else(|| dataset.get("updated_at").and_then(|v| v.as_str()))
            .unwrap_or(""),
    }))
}

/// `dbnomics_get_series`: Get series observations.
pub async fn get_series(
    http: &reqwest::Client,
    request: &DbnomicsGetSeriesRequest,
) -> Result<Value, DbnomicsError> {
    let provider_code = request.provider_code.trim();
    let dataset_code = request.dataset_code.trim();
    let series_code = request.series_code.trim();
    if provider_code.is_empty() {
        return Err(DbnomicsError::InvalidParam(
            "provider_code must not be empty".to_string(),
        ));
    }
    if dataset_code.is_empty() {
        return Err(DbnomicsError::InvalidParam(
            "dataset_code must not be empty".to_string(),
        ));
    }
    if series_code.is_empty() {
        return Err(DbnomicsError::InvalidParam(
            "series_code must not be empty".to_string(),
        ));
    }

    let include_observations = request.observations.unwrap_or(true);
    let observation_limit = request.limit.unwrap_or(1000);

    let endpoint = format!("series/{provider_code}/{dataset_code}/{series_code}");

    let mut params: Vec<(&str, String)> = Vec::new();
    if include_observations {
        params.push(("observations", "1".to_string()));
        let limit_str = observation_limit.to_string();
        params.push(("limit", limit_str));
    }
    let params_ref: Vec<(&str, &str)> = params
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();

    let body = dbnomics_fetch(http, &endpoint, &params_ref).await?;

    let series = body
        .get("series")
        .cloned()
        .or_else(|| body.pointer("/series/docs/0").cloned())
        .unwrap_or(Value::Null);

    let num_found = body
        .pointer("/series/num_found")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    let observations: Vec<Value> = if include_observations {
        let period_array = series.get("period").and_then(|value| value.as_array());
        let value_array = series.get("value").and_then(|value| value.as_array());
        match (period_array, value_array) {
            (Some(periods), Some(values)) => {
                let pairs = periods.iter().zip(values.iter());
                pairs
                    .filter_map(|(period, value)| {
                        let period_str = period.as_str()?;
                        Some(serde_json::json!({
                            "period": period_str,
                            "value": value,
                        }))
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };

    Ok(serde_json::json!({
        "provider_code": provider_code,
        "dataset_code": series.get("dataset_code").and_then(|v| v.as_str())
            .unwrap_or(dataset_code),
        "series_code": series.get("series_code").and_then(|v| v.as_str())
            .unwrap_or(series_code),
        "series_name": series.get("series_name").and_then(|v| v.as_str())
            .or_else(|| series.get("name").and_then(|v| v.as_str()))
            .unwrap_or(""),
        "frequency": series.get("@frequency").and_then(|v| v.as_str()).unwrap_or(""),
        "start_period": series.get("period").and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        "end_period": series.get("period").and_then(|v| v.as_array())
            .and_then(|arr| arr.last())
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        "num_found": num_found,
        "observations": observations,
        "returned": observations.len(),
    }))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::McpErrorKind;

    #[test]
    fn dbnomics_url_builds_correctly() {
        let url = dbnomics_url("search", &[("q", "GDP"), ("limit", "10")]);
        assert!(url.contains("api.db.nomics.world/v22/search"));
        assert!(url.contains("q=GDP"));
        assert!(url.contains("limit=10"));
    }

    #[test]
    fn dbnomics_url_handles_no_params() {
        let url = dbnomics_url("provider", &[]);
        assert_eq!(url, "https://api.db.nomics.world/v22/provider");
    }

    #[test]
    fn dbnomics_error_classifies_correctly() {
        let error: hkask_mcp_server::server::McpToolError =
            DbnomicsError::InvalidParam("bad".into()).into();
        assert_eq!(error.kind, McpErrorKind::InvalidArgument);

        let error: hkask_mcp_server::server::McpToolError =
            DbnomicsError::RequestFailed("timeout".into()).into();
        assert_eq!(error.kind, McpErrorKind::Unavailable);

        let error: hkask_mcp_server::server::McpToolError =
            DbnomicsError::HttpError { status: 500, body: "err".into() }.into();
        assert_eq!(error.kind, McpErrorKind::Unavailable);

        let error: hkask_mcp_server::server::McpToolError =
            DbnomicsError::ParseError("bad json".into()).into();
        assert_eq!(error.kind, McpErrorKind::Internal);
    }
}
