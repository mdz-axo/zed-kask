//! DBnomics API provider adapter.
//!
//! DBnomics is the world's largest open economic time-series aggregator:
//! 1.7B+ series across 47K+ datasets from 700+ providers (IMF, OECD, ECB,
//! INSEE, World Bank, FRED mirrors, etc.). It is the global superset of
//! both FRED (US-centric) and the World Bank Indicators API.
//!
//! API docs: https://db.nomics.world/api/v22/swagger
//!
//! No API key required — DBnomics is fully anonymous. The shared HTTP
//! fetch/error shape lives in `super::EconomicDataClient` / `EconomicDataError`.

use super::{EconomicDataClient, EconomicDataError};
use serde::Deserialize;
use serde_json::Value;

const DBNOMICS_API_BASE: &str = "https://api.db.nomics.world/v22/";
const DBNOMICS_PROVIDER: &str = "DBnomics";

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

// ── Helpers ────────────────────────────────────────────────────────────────

fn dbnomics_url(endpoint: &str, params: &[(&str, &str)]) -> String {
    EconomicDataClient::build_url(DBNOMICS_API_BASE, endpoint, params)
}

// ── Tool implementations ────────────────────────────────────────────────────

/// `dbnomics_search`: Full-text search across all DBnomics series.
pub async fn search(
    client: &EconomicDataClient<'_>,
    request: &DbnomicsSearchRequest,
) -> Result<Value, EconomicDataError> {
    let query = request.query.trim();
    if query.is_empty() {
        return Err(EconomicDataError::InvalidParam(
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
    let url = dbnomics_url("search", &params);
    let body = client.fetch(DBNOMICS_PROVIDER, &url).await?;

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
    client: &EconomicDataClient<'_>,
    request: &DbnomicsListProvidersRequest,
) -> Result<Value, EconomicDataError> {
    let limit = request.limit.unwrap_or(20).min(100);
    let offset = request.offset.unwrap_or(0);

    let limit_str = limit.to_string();
    let offset_str = offset.to_string();
    let params: Vec<(&str, &str)> = vec![
        ("limit", limit_str.as_str()),
        ("offset", offset_str.as_str()),
    ];
    let url = dbnomics_url("provider", &params);
    let body = client.fetch(DBNOMICS_PROVIDER, &url).await?;

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
    client: &EconomicDataClient<'_>,
    request: &DbnomicsGetDatasetRequest,
) -> Result<Value, EconomicDataError> {
    let provider_code = request.provider_code.trim();
    let dataset_code = request.dataset_code.trim();
    if provider_code.is_empty() {
        return Err(EconomicDataError::InvalidParam(
            "provider_code must not be empty".to_string(),
        ));
    }
    if dataset_code.is_empty() {
        return Err(EconomicDataError::InvalidParam(
            "dataset_code must not be empty".to_string(),
        ));
    }

    let endpoint = format!("datasets/{provider_code}/{dataset_code}");
    let url = dbnomics_url(&endpoint, &[]);
    let body = client.fetch(DBNOMICS_PROVIDER, &url).await?;

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
    client: &EconomicDataClient<'_>,
    request: &DbnomicsGetSeriesRequest,
) -> Result<Value, EconomicDataError> {
    let provider_code = request.provider_code.trim();
    let dataset_code = request.dataset_code.trim();
    let series_code = request.series_code.trim();
    if provider_code.is_empty() {
        return Err(EconomicDataError::InvalidParam(
            "provider_code must not be empty".to_string(),
        ));
    }
    if dataset_code.is_empty() {
        return Err(EconomicDataError::InvalidParam(
            "dataset_code must not be empty".to_string(),
        ));
    }
    if series_code.is_empty() {
        return Err(EconomicDataError::InvalidParam(
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
    let url = dbnomics_url(&endpoint, &params_ref);
    let body = client.fetch(DBNOMICS_PROVIDER, &url).await?;

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
            EconomicDataError::InvalidParam("bad".into()).into();
        assert_eq!(error.kind, McpErrorKind::InvalidArgument);

        let error: hkask_mcp_server::server::McpToolError = EconomicDataError::RequestFailed {
            provider: DBNOMICS_PROVIDER,
            detail: "timeout".into(),
        }
        .into();
        assert_eq!(error.kind, McpErrorKind::Unavailable);

        let error: hkask_mcp_server::server::McpToolError = EconomicDataError::HttpError {
            provider: DBNOMICS_PROVIDER,
            status: 500,
            body: "err".into(),
        }
        .into();
        assert_eq!(error.kind, McpErrorKind::Unavailable);

        let error: hkask_mcp_server::server::McpToolError = EconomicDataError::ParseError {
            provider: DBNOMICS_PROVIDER,
            detail: "bad json".into(),
        }
        .into();
        assert_eq!(error.kind, McpErrorKind::Internal);
    }
}
