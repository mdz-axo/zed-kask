//! World Bank Indicators API provider adapter.
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
//! No API key required — the World Bank API is fully open. The shared HTTP
//! fetch/error shape lives in `super::EconomicDataClient` / `EconomicDataError`.

use super::{EconomicDataClient, EconomicDataError};
use serde::Deserialize;
use serde_json::Value;

const WB_API_BASE: &str = "https://api.worldbank.org/v2";
const WB_PROVIDER: &str = "World Bank";

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
    /// Optional: start year (e.g., "2000"). Defaults to earliest available;
    /// with no end year the range runs to the latest available.
    pub date_start: Option<String>,
    /// Optional: end year (e.g., "2024"). Defaults to latest available;
    /// with no start year the range runs from the earliest available.
    pub date_end: Option<String>,
    /// Max observations to return (default 100, capped at 1000).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WbListCountriesRequest {
    /// Optional: filter by income group. Values: "all" (default), "hic"
    /// (high income), "mic" (middle income: lower and upper), "lmc" (lower
    /// middle), "umc" (upper middle), "lic" (low income).
    pub income_group: Option<String>,
    /// Max countries to return after filtering (default 50, capped at 300).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WbListTopicsRequest {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WbGetIndicatorInfoRequest {
    /// World Bank indicator ID (e.g., "SP.POP.TOTL").
    pub indicator_id: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Build a World Bank API URL with `format=json` prepended. Values are
/// encoded by the shared builder (mcp-tool-review PM2-03).
fn wb_url(endpoint: &str, params: &[(&str, &str)]) -> String {
    let mut all: Vec<(&str, &str)> = vec![("format", "json")];
    all.extend_from_slice(params);
    EconomicDataClient::build_url(WB_API_BASE, endpoint, &all)
}

/// The World Bank reports a rejected request as HTTP 200 with
/// `[{"message": [{"id", "key", "value"}]}]` in place of `[meta, data]`.
/// Returns that message so the caller can classify it — it is the
/// provider refusing a caller value, not a malformed response.
fn wb_rejection(body: &Value) -> Option<String> {
    let message = body
        .as_array()?
        .first()?
        .get("message")?
        .as_array()?
        .first()?;
    let key = message.get("key").and_then(|v| v.as_str()).unwrap_or("");
    let value = message.get("value").and_then(|v| v.as_str()).unwrap_or("");
    Some(format!("{key}: {value}"))
}

/// Extract the data array (second element) from a WB API response.
/// The WB API returns `[metadata, data_array]`. Returns the data_array.
fn wb_extract_data(body: &Value) -> Result<&Vec<Value>, EconomicDataError> {
    if let Some(rejection) = wb_rejection(body) {
        return Err(EconomicDataError::InvalidParam(format!(
            "World Bank rejected the request ({rejection})"
        )));
    }
    body.as_array()
        .and_then(|arr| arr.get(1))
        .and_then(|v| v.as_array())
        .ok_or_else(|| EconomicDataError::ParseError {
            provider: WB_PROVIDER,
            detail: "expected [meta, data] array".into(),
        })
}

/// The World Bank serves numeric ids, coordinates and page totals as JSON
/// strings (`"id": "3"`, `"latitude": "12.5167"`). Read either form;
/// absent or unparsable is `None`, never a fabricated 0.
fn wb_number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// A World Bank integer id (see [`wb_number`]).
fn wb_id(value: Option<&Value>) -> Option<i64> {
    wb_number(value).map(|number| number as i64)
}

/// Extract the metadata object (first element) from a WB API response.
fn wb_extract_meta(body: &Value) -> Result<&Value, EconomicDataError> {
    body.as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| EconomicDataError::ParseError {
            provider: WB_PROVIDER,
            detail: "expected [meta, data] array".into(),
        })
}

/// The World Bank indicator's normalized JSON — the field mapping every
/// indicator-returning endpoint shares (search's topic-filtered and
/// unfiltered branches, and indicator info).
fn wb_indicator_json(ind: &Value) -> Value {
    serde_json::json!({
        "id": ind.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "name": ind.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        "unit": ind.get("unit").and_then(|v| v.as_str()).unwrap_or(""),
        "source": ind.get("source").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or(""),
        "source_note": ind.get("sourceNote").and_then(|v| v.as_str()).unwrap_or(""),
        "topics": ind.get("topics").and_then(|v| v.as_array()).map(|t| {
            t.iter().map(|topic| {
                serde_json::json!({
                    "id": wb_id(topic.get("id")),
                    "value": topic.get("value").and_then(|v| v.as_str()).unwrap_or(""),
                })
            }).collect::<Vec<_>>()
        }).unwrap_or_default(),
    })
}

// ── Tool implementations ────────────────────────────────────────────────────

/// `wb_search_indicators`: Search World Bank indicators by text.
pub async fn search_indicators(
    client: &EconomicDataClient<'_>,
    req: &WbSearchIndicatorsRequest,
) -> Result<Value, EconomicDataError> {
    let limit = req.limit.unwrap_or(10).min(100);

    // The World Bank has no text-search endpoint: one page of the indicator
    // list is fetched and matched by name here. The page is fetched in full
    // BEFORE the name filter — the caller's `limit` caps the matches, never
    // the rows scanned (the pre-fix topic branch scanned only `limit` rows).
    let endpoint = match req.topic_id {
        Some(topic_id) => format!("topic/{topic_id}/indicator"),
        None => "indicator".to_string(),
    };
    let body = client
        .fetch(
            WB_PROVIDER,
            &wb_url(&endpoint, &[("per_page", WB_SEARCH_PAGE_SIZE)]),
        )
        .await?;
    let data = wb_extract_data(&body)?;
    let catalogue_total = wb_extract_meta(&body)
        .ok()
        .and_then(|meta| wb_id(meta.get("total")));
    Ok(search_page(req, data, catalogue_total, limit as usize))
}

/// Rows fetched per indicator-list request — the scan window of a search.
const WB_SEARCH_PAGE_SIZE: &str = "1000";

/// Match one fetched page of indicators by name and report how much of the
/// catalogue the page covered. A partial scan is surfaced in `note` — a
/// query that finds nothing in the first page has not searched the
/// catalogue (mcp-tool-review PM2-08: the unfiltered list is ~29,500 rows).
fn search_page(
    req: &WbSearchIndicatorsRequest,
    data: &[Value],
    catalogue_total: Option<i64>,
    limit: usize,
) -> Value {
    let query_lower = req.query.to_lowercase();
    let results: Vec<Value> = data
        .iter()
        .filter(|ind| {
            let name = ind.get("name").and_then(|v| v.as_str()).unwrap_or("");
            name.to_lowercase().contains(&query_lower)
        })
        .take(limit)
        .map(wb_indicator_json)
        .collect();
    let scanned = data.len();
    let note = match catalogue_total {
        Some(total) if (scanned as i64) < total => Some(format!(
            "partial scan: matched names in the first {scanned} of {total} indicators only — pass topic_id (see wb_list_topics) to search a whole topic, or use wb_get_indicator_info with a known id"
        )),
        _ => None,
    };
    serde_json::json!({
        "query": req.query,
        "topic_id": req.topic_id,
        "results": results,
        "returned": results.len(),
        "scanned": scanned,
        "catalogue_total": catalogue_total,
        "note": note,
    })
}

/// The World Bank `date` parameter for a start/end pair. The API accepts
/// `YYYY` or `YYYY:YYYY` and rejects an open-ended `:YYYY`, so an omitted
/// bound is filled with a year outside the data's range.
fn wb_date_param(start: Option<&str>, end: Option<&str>) -> Option<String> {
    const EARLIEST: &str = "1900";
    const LATEST: &str = "2100";
    match (start, end) {
        (None, None) => None,
        (start, end) => Some(format!(
            "{}:{}",
            start.unwrap_or(EARLIEST),
            end.unwrap_or(LATEST)
        )),
    }
}

/// The World Bank income-level ids a caller's `income_group` selects, or
/// `None` for no filter.
fn wb_income_levels(
    raw: Option<&str>,
) -> Result<Option<&'static [&'static str]>, EconomicDataError> {
    let Some(raw) = raw else { return Ok(None) };
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "all" => Ok(None),
        "hic" => Ok(Some(&["HIC"])),
        "mic" => Ok(Some(&["LMC", "UMC"])),
        "lmc" => Ok(Some(&["LMC"])),
        "umc" => Ok(Some(&["UMC"])),
        "lic" => Ok(Some(&["LIC"])),
        _ => Err(EconomicDataError::InvalidParam(format!(
            "income_group {raw:?} is not one of: all, hic, mic, lmc, umc, lic"
        ))),
    }
}

/// `wb_get_observations`: Fetch time series observations for a country + indicator.
pub async fn get_observations(
    client: &EconomicDataClient<'_>,
    req: &WbGetObservationsRequest,
) -> Result<Value, EconomicDataError> {
    let limit = req.limit.unwrap_or(100).min(1000);
    let limit_str = limit.to_string();

    let date_param = wb_date_param(req.date_start.as_deref(), req.date_end.as_deref());

    let country = super::path_segment("country_code", &req.country_code)?;
    let indicator = super::path_segment("indicator_id", &req.indicator_id)?;
    let endpoint = format!("country/{country}/indicator/{indicator}");

    let mut params: Vec<(&str, String)> = vec![("per_page", limit_str)];
    if let Some(date_param) = date_param {
        params.push(("date", date_param));
    }
    let params_ref: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let url = wb_url(&endpoint, &params_ref);

    let body = client.fetch(WB_PROVIDER, &url).await?;

    // Data first: a provider rejection is classified there, before the
    // metadata read. `total` is `null` when absent — never a fabricated 0.
    let data = wb_extract_data(&body)?;
    let total = wb_id(wb_extract_meta(&body)?.get("total"));

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
    client: &EconomicDataClient<'_>,
    req: &WbListCountriesRequest,
) -> Result<Value, EconomicDataError> {
    let limit = req.limit.unwrap_or(50).min(300) as usize;
    let income_levels = wb_income_levels(req.income_group.as_deref())?;

    // Fetch the whole list (296 rows, aggregates included) and apply the
    // caller's limit AFTER filtering — a `per_page` of `limit` capped the
    // rows scanned, so most of the page was aggregates and the filter saw
    // a fraction of the countries.
    let url = wb_url("country", &[("per_page", "400")]);
    let body = client.fetch(WB_PROVIDER, &url).await?;

    let data = wb_extract_data(&body)?;
    Ok(countries_page(
        data,
        income_levels,
        req.income_group.as_deref(),
        limit,
    ))
}

/// Shape the country list: countries only (aggregates dropped), the
/// income-level filter, then the caller's limit.
fn countries_page(
    data: &[Value],
    income_levels: Option<&[&str]>,
    income_group: Option<&str>,
    limit: usize,
) -> Value {
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
            if let Some(levels) = income_levels {
                let ig = c
                    .get("incomeLevel")
                    .and_then(|il| il.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                return levels.contains(&ig);
            }
            true
        })
        .collect();
    let matched = filtered.len();

    let results: Vec<Value> = filtered
        .iter()
        .take(limit)
        .map(|c| {
            serde_json::json!({
                // The ISO3 code is the row's `id` (there is no `iso3` key).
                "iso3": c.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                "name": c.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "region": c.get("region").and_then(|r| r.get("value")).and_then(|v| v.as_str()).unwrap_or(""),
                "income_level": c.get("incomeLevel").and_then(|il| il.get("value")).and_then(|v| v.as_str()).unwrap_or(""),
                "capital_city": c.get("capitalCity").and_then(|v| v.as_str()).unwrap_or(""),
                "longitude": wb_number(c.get("longitude")),
                "latitude": wb_number(c.get("latitude")),
            })
        })
        .collect();

    serde_json::json!({
        "countries": results,
        "count": results.len(),
        "matched": matched,
        "income_group_filter": income_group,
    })
}

/// `wb_list_topics`: Browse the World Bank topic tree.
pub async fn list_topics(
    client: &EconomicDataClient<'_>,
    _req: &WbListTopicsRequest,
) -> Result<Value, EconomicDataError> {
    let url = wb_url("topic", &[("per_page", "100")]);
    let body = client.fetch(WB_PROVIDER, &url).await?;
    let data = wb_extract_data(&body)?;

    let results: Vec<Value> = data
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": wb_id(t.get("id")),
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
    client: &EconomicDataClient<'_>,
    req: &WbGetIndicatorInfoRequest,
) -> Result<Value, EconomicDataError> {
    let indicator = super::path_segment("indicator_id", &req.indicator_id)?;
    let endpoint = format!("indicator/{indicator}");
    let url = wb_url(&endpoint, &[("per_page", "1")]);
    let body = client.fetch(WB_PROVIDER, &url).await?;

    // The World Bank answers an unknown indicator id with a rejection
    // message, not an empty list — both mean the caller's id is not there.
    let not_found =
        || EconomicDataError::NotFound(format!("World Bank indicator {indicator} not found"));
    if wb_rejection(&body).is_some() {
        return Err(not_found());
    }
    let data = wb_extract_data(&body)?;
    let ind = data.first().ok_or_else(not_found)?;

    let mut info = wb_indicator_json(ind);
    // Indicator info is the one endpoint that also names the source
    // organization; the shared mapping covers the rest.
    info["source_organization"] = serde_json::json!(
        ind.get("sourceOrganization")
            .and_then(|v| v.as_str())
            .unwrap_or("")
    );
    Ok(info)
}

// ── Tests ──────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// The live row shapes (captured 2026-11-10): ids, coordinates and
    /// page totals are JSON strings; the ISO3 code is the row's `id`.
    fn country(id: &str, region: &str, income: &str) -> Value {
        serde_json::json!({
            "id": id, "iso2Code": "XX", "name": id,
            "region": {"id": region, "value": "Region"},
            "incomeLevel": {"id": income, "value": "Income"},
            "capitalCity": "Capital", "longitude": "13.242", "latitude": "-8.81155"
        })
    }

    /// mcp-tool-review PM2-06 pin: the World Bank serves numeric ids as
    /// strings. Live 2026-11-10, every topic id read 0 — the documented
    /// input to `wb_search_indicators`' `topic_id` filter.
    #[test]
    fn string_typed_ids_and_coordinates_are_read_not_zero_filled() {
        assert_eq!(wb_id(Some(&serde_json::json!("3"))), Some(3));
        assert_eq!(wb_id(Some(&serde_json::json!(3))), Some(3));
        assert_eq!(wb_id(Some(&serde_json::json!(""))), None);
        assert_eq!(wb_id(None), None);
        let indicator = serde_json::json!({
            "id": "NY.GDP.PCAP.CD", "name": "GDP per capita (current US$)",
            "topics": [{"id": "3", "value": "Economy & Growth"}]
        });
        assert_eq!(wb_indicator_json(&indicator)["topics"][0]["id"], 3);
        let page = countries_page(&[country("AGO", "SSF", "LMC")], None, None, 10);
        assert_eq!(page["countries"][0]["iso3"], "AGO");
        assert_eq!(page["countries"][0]["latitude"], -8.81155);
        assert_eq!(page["countries"][0]["longitude"], 13.242);
    }

    /// mcp-tool-review PM2-06 pin: the advertised `mic` filter selects the
    /// World Bank's two middle-income levels, `all` selects every country,
    /// and the limit applies after aggregates are dropped. Live 2026-11-10,
    /// `mic` and `all` both returned zero countries.
    #[test]
    fn income_group_filter_and_limit_apply_to_countries_not_rows() {
        let rows = vec![
            country("AFE", "NA", "NA"),
            country("ABW", "LCN", "HIC"),
            country("AGO", "SSF", "LMC"),
            country("ALB", "ECS", "UMC"),
            country("AFG", "MEA", "LIC"),
        ];
        let select = |group: &str, limit: usize| {
            let levels = wb_income_levels(Some(group)).expect("valid group");
            countries_page(&rows, levels, Some(group), limit)
        };
        assert_eq!(
            select("all", 10)["count"],
            4,
            "aggregates are not countries"
        );
        assert_eq!(select("mic", 10)["count"], 2, "mic = lower + upper middle");
        assert_eq!(select("HIC", 10)["countries"][0]["iso3"], "ABW");
        let capped = select("all", 2);
        assert_eq!(capped["count"], 2);
        assert_eq!(capped["matched"], 4, "the cap is disclosed, not silent");
        let error = wb_income_levels(Some("rich")).expect_err("unknown group");
        assert!(
            error.to_string().contains("hic, mic, lmc, umc, lic"),
            "{error}"
        );
    }

    /// mcp-tool-review PM2-06 pin: a World Bank rejection (HTTP 200 with a
    /// message array) is the caller's value being refused — never an
    /// `internal` parse fault. Live 2026-11-10, an unknown indicator id
    /// returned `[internal] ... expected [meta, data] array`.
    #[test]
    fn provider_rejection_is_classified_as_the_callers_value() {
        let body = serde_json::json!([{"message": [{
            "id": "120", "key": "Invalid value",
            "value": "The provided parameter value is not valid"
        }]}]);
        let error = wb_extract_data(&body).expect_err("a rejection is not data");
        assert!(
            matches!(error, EconomicDataError::InvalidParam(_)),
            "{error:?}"
        );
        assert!(error.to_string().contains("Invalid value"), "{error}");
    }

    /// mcp-tool-review PM2-06 pin: an end year without a start year. Live
    /// 2026-11-10, the previously built `:2002` was rejected by the API.
    #[test]
    fn open_ended_date_ranges_are_closed_before_they_reach_the_api() {
        assert_eq!(wb_date_param(None, None), None);
        assert_eq!(
            wb_date_param(Some("2000"), Some("2002")).as_deref(),
            Some("2000:2002")
        );
        assert_eq!(
            wb_date_param(None, Some("2002")).as_deref(),
            Some("1900:2002")
        );
        assert_eq!(
            wb_date_param(Some("2000"), None).as_deref(),
            Some("2000:2100")
        );
        assert_eq!(
            wb_url(
                "country/USA/indicator/SP.POP.TOTL",
                &[("date", "2000:2002")]
            ),
            "https://api.worldbank.org/v2/country/USA/indicator/SP.POP.TOTL?format=json&date=2000%3A2002"
        );
    }

    /// mcp-tool-review PM2-08 pin: a search that scanned one page of a
    /// larger catalogue says so, and the caller's limit caps the matches —
    /// never the rows scanned.
    #[test]
    fn search_discloses_a_partial_scan_and_filters_before_limiting() {
        let rows: Vec<Value> = ["Access to electricity", "GDP per capita", "GDP growth"]
            .iter()
            .map(|name| serde_json::json!({"id": "X", "name": name}))
            .collect();
        let request = |query: &str| WbSearchIndicatorsRequest {
            query: query.to_string(),
            topic_id: None,
            limit: None,
        };
        let partial = search_page(&request("gdp"), &rows, Some(29_544), 1);
        assert_eq!(partial["returned"], 1, "limit caps matches");
        assert_eq!(
            partial["results"][0]["name"], "GDP per capita",
            "row 2 was scanned"
        );
        assert_eq!(partial["scanned"], 3);
        assert!(
            partial["note"]
                .as_str()
                .is_some_and(|n| n.contains("first 3 of 29544")),
            "a partial scan must be disclosed: {partial}"
        );
        let whole = search_page(&request("gdp"), &rows, Some(3), 10);
        assert_eq!(whole["returned"], 2);
        assert!(whole["note"].is_null(), "a full scan carries no note");
    }
}
