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
    /// Max observations to return (default 1000) — the most recent ones
    /// are kept; `total_observations` reports the full count. The series
    /// metadata is always returned in full.
    pub limit: Option<u32>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn dbnomics_url(endpoint: &str, params: &[(&str, &str)]) -> String {
    EconomicDataClient::build_url(DBNOMICS_API_BASE, endpoint, params)
}

/// The DBnomics series' normalized JSON — the content-field mapping search
/// results and series info share. The code fields are parameters: search
/// reads them from the series document, series info falls back to the
/// request's own codes.
fn dbnomics_series_json(
    series: &Value,
    provider_code: &str,
    dataset_code: &str,
    series_code: &str,
) -> Value {
    serde_json::json!({
        "provider_code": provider_code,
        "dataset_code": dataset_code,
        "series_code": series_code,
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

    let results: Vec<Value> = docs.iter().map(dbnomics_dataset_hit).collect();

    Ok(serde_json::json!({
        "query": request.query,
        "num_found": num_found,
        "results": results,
        "returned": results.len(),
        "offset": offset,
    }))
}

/// One `/search` hit. The endpoint returns DATASETS that contain matching
/// series (live shape 2026-11-10: `code`, `name`, `provider_code`,
/// `nb_series`, `nb_matching_series`) — not series. The pre-fix shaping
/// read series fields the hit does not carry, so every result came back
/// with an empty dataset and series code (mcp-tool-review PM2-09).
fn dbnomics_dataset_hit(dataset: &Value) -> Value {
    let text = |key: &str| dataset.get(key).and_then(|v| v.as_str()).unwrap_or("");
    serde_json::json!({
        "provider_code": text("provider_code"),
        "provider_name": text("provider_name"),
        "dataset_code": text("code"),
        "dataset_name": text("name"),
        "nb_series": dataset.get("nb_series").and_then(|v| v.as_u64()),
        "nb_matching_series": dataset.get("nb_matching_series").and_then(|v| v.as_u64()),
    })
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
    let url = dbnomics_url("providers", &params);
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
    let provider_code = super::path_segment("provider_code", &request.provider_code)?;
    let dataset_code = super::path_segment("dataset_code", &request.dataset_code)?;

    let endpoint = format!("datasets/{provider_code}/{dataset_code}");
    let url = dbnomics_url(&endpoint, &[]);
    let body = client.fetch(DBNOMICS_PROVIDER, &url).await?;
    dataset_from_body(&body, provider_code, dataset_code)
}

/// Shape the `/datasets/{provider}/{dataset}` response. DBnomics answers an
/// unknown dataset with HTTP 200 and an empty `datasets.docs` — that is
/// the caller's reference not existing, never an all-empty success.
fn dataset_from_body(
    body: &Value,
    provider_code: &str,
    dataset_code: &str,
) -> Result<Value, EconomicDataError> {
    let dataset = body.pointer("/datasets/docs/0").ok_or_else(|| {
        EconomicDataError::NotFound(format!(
            "DBnomics dataset {provider_code}/{dataset_code} not found"
        ))
    })?;

    // The dimension codes live in `dimensions_codes_order` (there is no
    // `dimensions` key on the dataset document).
    let dimensions = dataset
        .get("dimensions_codes_order")
        .cloned()
        .unwrap_or(Value::Null);
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
    let provider_code = super::path_segment("provider_code", &request.provider_code)?;
    let dataset_code = super::path_segment("dataset_code", &request.dataset_code)?;
    let series_code = super::path_segment("series_code", &request.series_code)?;

    let include_observations = request.observations.unwrap_or(true);
    let observation_limit = request.limit.unwrap_or(1000) as usize;

    let endpoint = format!("series/{provider_code}/{dataset_code}/{series_code}");

    // The API's own `limit` caps the number of SERIES, not observations, so
    // it is not sent: the observation cap is applied in `series_from_body`.
    let params: &[(&str, &str)] = if include_observations {
        &[("observations", "1")]
    } else {
        &[]
    };
    let url = dbnomics_url(&endpoint, params);
    let body = client.fetch(DBNOMICS_PROVIDER, &url).await?;
    series_from_body(
        &body,
        (provider_code, dataset_code, series_code),
        include_observations,
        observation_limit,
    )
}

/// Shape the `/series/{provider}/{dataset}/{series}` response. The series
/// document is `series.docs[0]` — `series` itself is the paging envelope
/// (`docs`, `limit`, `num_found`, `offset`). The pre-fix shaping read the
/// envelope as the document, so every fetch returned an empty name and no
/// observations (mcp-tool-review PM2-09, live 2026-11-10).
fn series_from_body(
    body: &Value,
    (provider_code, dataset_code, series_code): (&str, &str, &str),
    include_observations: bool,
    observation_limit: usize,
) -> Result<Value, EconomicDataError> {
    let series = body.pointer("/series/docs/0").ok_or_else(|| {
        EconomicDataError::NotFound(format!(
            "DBnomics series {provider_code}/{dataset_code}/{series_code} not found"
        ))
    })?;

    let num_found = body
        .pointer("/series/num_found")
        .and_then(|value| value.as_u64());

    let all_observations: Vec<Value> = if include_observations {
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
    // Periods ascend, so the tail is the most recent `observation_limit`.
    let total_observations = all_observations.len();
    let skipped = total_observations.saturating_sub(observation_limit);
    let observations: Vec<Value> = all_observations.into_iter().skip(skipped).collect();

    let mut info = dbnomics_series_json(
        series,
        provider_code,
        series
            .get("dataset_code")
            .and_then(|v| v.as_str())
            .unwrap_or(dataset_code),
        series
            .get("series_code")
            .and_then(|v| v.as_str())
            .unwrap_or(series_code),
    );
    info["num_found"] = serde_json::json!(num_found);
    info["returned"] = serde_json::json!(observations.len());
    info["total_observations"] = serde_json::json!(total_observations);
    info["observations"] = serde_json::json!(observations);
    Ok(info)
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// PM-04 regression pin (mcp-tool-review): the providers endpoint is the
    /// PLURAL path. The singular form 404'd live (2026-10-02 tool probe)
    /// while the plural path returned 94 providers — a one-token drift that
    /// left the tool permanently broken.
    #[test]
    fn providers_endpoint_is_the_plural_path() {
        let url = dbnomics_url("providers", &[("limit", "5"), ("offset", "0")]);
        assert!(
            url.starts_with("https://api.db.nomics.world/v22/providers"),
            "the providers endpoint must be the plural path, got: {url}"
        );
    }

    /// mcp-tool-review PM2-09 pins, over the live response shapes captured
    /// 2026-11-10: the series document is `series.docs[0]`, the observation
    /// cap keeps the most recent, and an absent document is `not_found`.
    #[test]
    fn series_reads_the_document_inside_the_paging_envelope() {
        let body = serde_json::json!({"series": {"limit": 1000, "num_found": 1, "offset": 0,
            "docs": [{"@frequency": "annual", "dataset_code": "WEO:2025-04",
                "series_code": "ABW.BCA.us_dollars", "series_name": "Aruba – Current account",
                "period": ["2028", "2029", "2030"], "value": [0.1, 0.2, 0.368]}]}});
        let codes = ("IMF", "WEO:latest", "ABW.BCA.us_dollars");
        let out = series_from_body(&body, codes, true, 2).expect("series shapes");
        assert_eq!(out["series_name"], "Aruba – Current account");
        assert_eq!(out["dataset_code"], "WEO:2025-04", "the resolved release");
        assert_eq!(out["frequency"], "annual");
        assert_eq!(out["total_observations"], 3);
        assert_eq!(out["returned"], 2);
        assert_eq!(out["observations"][1]["period"], "2030", "most recent kept");
        assert_eq!(out["observations"][1]["value"], 0.368);

        let empty = serde_json::json!({"series": {"docs": [], "num_found": 0}});
        let error = series_from_body(&empty, codes, true, 2).expect_err("no document");
        assert!(matches!(error, EconomicDataError::NotFound(_)), "{error:?}");
    }

    /// An unknown dataset is HTTP 200 with empty `datasets.docs` (live
    /// 2026-11-10: the tool returned an all-empty success for it).
    #[test]
    fn unknown_dataset_is_not_found_and_dimensions_are_read() {
        let empty = serde_json::json!({"datasets": {"docs": [], "num_found": 0}});
        let error = dataset_from_body(&empty, "IMF", "ZZ").expect_err("no dataset");
        assert!(matches!(error, EconomicDataError::NotFound(_)), "{error:?}");
        assert!(error.to_string().contains("IMF/ZZ"), "{error}");

        let body = serde_json::json!({"datasets": {"docs": [{"code": "WEO:2025-04",
            "name": "World Economic Outlook by countries",
            "dimensions_codes_order": ["weo-country", "weo-subject", "unit"],
            "indexed_at": "2025-05-15T11:13:27.379Z"}]}});
        let out = dataset_from_body(&body, "IMF", "WEO:latest").expect("dataset shapes");
        assert_eq!(out["dataset_code"], "WEO:2025-04");
        assert_eq!(out["dimensions"][0], "weo-country");
    }

    /// A `/search` hit is a dataset: its code is `code`, its name `name`.
    #[test]
    fn search_hit_is_shaped_as_the_dataset_it_is() {
        let hit = serde_json::json!({"code": "CHELEM-TRADE-INDIC", "provider_code": "CEPII",
            "name": "CHELEM - International Trade indicators",
            "nb_series": 797349, "nb_matching_series": 683775});
        let out = dbnomics_dataset_hit(&hit);
        assert_eq!(out["dataset_code"], "CHELEM-TRADE-INDIC");
        assert_eq!(out["provider_code"], "CEPII");
        assert_eq!(out["nb_matching_series"], 683775);
    }

    /// The other three endpoints match the live v22 API — the fix touched
    /// exactly one token.
    #[test]
    fn search_dataset_and_series_endpoints_match_the_live_api() {
        assert!(
            dbnomics_url("search", &[("q", "GDP")])
                .starts_with("https://api.db.nomics.world/v22/search")
        );
        assert!(
            dbnomics_url("datasets/IMF/WEO", &[])
                .starts_with("https://api.db.nomics.world/v22/datasets/IMF/")
        );
        assert!(
            dbnomics_url("series/IMF/WEO/NGDP", &[])
                .starts_with("https://api.db.nomics.world/v22/series/IMF/")
        );
    }
}
