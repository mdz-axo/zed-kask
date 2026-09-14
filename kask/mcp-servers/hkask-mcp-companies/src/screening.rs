use crate::{
    CompaniesServer, providers,
    research_store::{ResearchStore, ScreenJobRecord},
    types::{ScreenAction, ScreenTemplateContext, ScreenerRequest},
};

use futures::{FutureExt as _, StreamExt as _};
use hkask_mcp_server::server::McpToolError;
use hkask_types::time::now_rfc3339;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    future::Future,
    panic::AssertUnwindSafe,
};

const SCREEN_TEMPLATES: &[(&str, &str)] = &[
    (
        "universal_equity",
        include_str!("../../../registry/templates/company-screen/universal_equity.j2"),
    ),
    (
        "expectations_gap",
        include_str!("../../../registry/templates/company-screen/expectations_gap.j2"),
    ),
];
const LISP_MAX_STEPS: u64 = 100_000;
const LISP_MAX_DEPTH: u64 = 256;

#[derive(Deserialize)]
struct ScreenTemplateMetadata {
    contract: ScreenTemplateContract,
}

#[derive(Deserialize)]
struct ScreenTemplateContract {
    input: BTreeMap<String, String>,
    #[serde(default)]
    server_input: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScreenDefinition {
    name: String,
    kind: String,
    as_of: String,
    reporting_currency: String,
    universe: ScreenUniverse,
    filters: Vec<ScreenFilter>,
    columns: Vec<ScreenColumn>,
    ranking: Vec<ScreenSort>,
    #[serde(default)]
    logic_env: Value,
    #[serde(default)]
    assertions: Vec<ScreenAssertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScreenUniverse {
    asset_class: String,
    exchanges: Vec<String>,
    active_only: bool,
    primary_only: bool,
    security_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScreenFilter {
    field: String,
    operator: String,
    value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScreenColumn {
    id: String,
    label: String,
    source: String,
    data_type: String,
    unit: Option<String>,
    #[serde(default)]
    provenance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScreenSort {
    column: String,
    direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScreenAssertion {
    name: String,
    form: String,
}

struct ScreenCalculation {
    rows: Vec<Value>,
    candidate_count: usize,
    exclusions: Vec<Value>,
}

struct MaterializedSecurity {
    symbol: String,
    name: String,
    market_capitalization_usd: f64,
    average_daily_dollar_volume_usd: f64,
    adjusted_close: f64,
    currency_symbol: String,
    issuer_key: String,
    lei: Option<String>,
    primary_ticker: Option<String>,
    isin: Option<String>,
    normalized_issuer_name: String,
    fundamentals: Value,
}

struct IssuerGroup {
    issuer_key: String,
    issuer_key_provenance: &'static str,
    issuer_identity_provenance: &'static str,
    securities: Vec<MaterializedSecurity>,
}

pub(crate) async fn execute(
    server: &CompaniesServer,
    req: ScreenerRequest,
) -> Result<Value, McpToolError> {
    match req.action {
        Some(ScreenAction::Calculate) => submit(server, req).await,
        Some(ScreenAction::Status) => status(server, required_job_id(&req)?).await,
        Some(ScreenAction::Results) => {
            results(
                server,
                required_job_id(&req)?,
                req.cursor.unwrap_or(0),
                req.limit,
            )
            .await
        }
        None => Err(McpToolError::invalid_argument(
            "screen action is required for saved-screen execution",
        )),
    }
}

async fn submit(server: &CompaniesServer, req: ScreenerRequest) -> Result<Value, McpToolError> {
    let acquisition_date = chrono::Utc::now().date_naive().to_string();
    let definition = resolve_definition(&req, &acquisition_date)?;
    validate_definition(&definition, &acquisition_date)?;
    let verification = verify_assertions(&definition)?;
    let definition_value = serde_json::to_value(&definition)
        .map_err(|error| McpToolError::internal(format!("serialize screen definition: {error}")))?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let job = ScreenJobRecord {
        id: id.clone(),
        status: "queued".to_string(),
        definition: definition_value,
        result: None,
        error: None,
        created_at: now.clone(),
        updated_at: now,
    };
    server
        .research
        .insert_screen_job(&job)
        .map_err(crate::map_portfolio_error)?;

    let task_server = server.clone();
    let task_id = id.clone();
    #[cfg(test)]
    let test_origin = providers::TEST_HTTP_ORIGIN.try_with(Clone::clone).ok();
    drop(tokio::spawn(async move {
        let calculation = calculate_job(task_server, task_id, definition, verification);
        #[cfg(test)]
        if let Some(origin) = test_origin {
            providers::TEST_HTTP_ORIGIN.scope(origin, calculation).await;
        } else {
            calculation.await;
        }
        #[cfg(not(test))]
        calculation.await;
    }));

    Ok(json!({
        "job_id": id,
        "status": "queued",
        "screen_name": job.definition.get("name").cloned().unwrap_or(Value::Null),
        "as_of": job.definition.get("as_of").cloned().unwrap_or(Value::Null),
    }))
}

async fn acquire_universe(
    server: &CompaniesServer,
    definition: &ScreenDefinition,
) -> Result<Vec<Value>, McpToolError> {
    let mut criteria = Map::new();
    criteria.insert(
        "exchanges".to_string(),
        json!(definition.universe.exchanges),
    );
    for filter in &definition.filters {
        let key = match filter.operator.as_str() {
            ">" | ">=" => format!("{}_min", filter.field),
            "<" | "<=" => format!("{}_max", filter.field),
            "=" => filter.field.clone(),
            operator => {
                return Err(McpToolError::invalid_argument(format!(
                    "unsupported screen filter operator {operator:?}"
                )));
            }
        };
        criteria.insert(key, filter.value.clone());
    }
    let request = serde_json::from_value(json!({
        "prompt":"",
        "limit":u32::MAX,
        "criteria_overrides":Value::Object(criteria),
    }))
    .map_err(|error| McpToolError::internal(format!("build universe request: {error}")))?;
    let output =
        Box::pin(server.company_screener(rmcp::handler::server::wrapper::Parameters(request)))
            .await?;
    let output: Value = serde_json::from_str(&output)
        .map_err(|error| McpToolError::internal(format!("parse universe result: {error}")))?;
    let output = hkask_types::tool_response::unwrap_tool_envelope(output);
    output
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| McpToolError::internal("universe result has no results array"))
}

async fn calculate_job(
    server: CompaniesServer,
    job_id: String,
    definition: ScreenDefinition,
    verification: Value,
) {
    let store = server.research.clone();
    let calculation = async {
        let universe_snapshot = acquire_universe(&server, &definition).await?;
        calculate(
            &server.client,
            &server.eodhd_api_key,
            &definition,
            verification,
            universe_snapshot,
        )
        .await
    };
    persist_screen_calculation(store, job_id, calculation).await;
}

async fn persist_screen_calculation<F>(store: ResearchStore, job_id: String, calculation: F)
where
    F: Future<Output = Result<Value, McpToolError>>,
{
    if let Err(error) = store.update_screen_job(&job_id, "executing", None, None) {
        tracing::error!(job_id, "failed to mark screen job executing: {error}");
        return;
    }
    let outcome = AssertUnwindSafe(calculation).catch_unwind().await;
    let (status, result, error) = match outcome {
        Ok(Ok(result)) => ("completed", Some(result), None),
        Ok(Err(error)) => ("failed", None, Some(error.to_string())),
        Err(payload) => {
            let panic_message = payload
                .downcast_ref::<&str>()
                .map(|message| (*message).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic payload".to_string());
            tracing::error!(job_id, panic_message, "screen calculation panicked");
            (
                "failed",
                None,
                Some(format!("screen calculation panicked: {panic_message}")),
            )
        }
    };
    if let Err(store_error) =
        store.update_screen_job(&job_id, status, result.as_ref(), error.as_deref())
    {
        tracing::error!(
            job_id,
            status,
            "failed to persist terminal screen job state: {store_error}"
        );
    }
}

async fn calculate(
    _client: &reqwest::Client,
    _eodhd_api_key: &str,
    definition: &ScreenDefinition,
    verification: Value,
    universe_snapshot: Vec<Value>,
) -> Result<Value, McpToolError> {
    let calculation = if definition.kind == "expectations_gap" {
        calculate_expectations_gap(_client, _eodhd_api_key, definition, universe_snapshot).await?
    } else {
        ScreenCalculation {
            candidate_count: universe_snapshot.len(),
            rows: universe_snapshot,
            exclusions: Vec::new(),
        }
    };
    let mut rows = calculation.rows;
    sort_rows(&mut rows, definition);
    let table = columnar_table(&rows, definition);
    let row_count = rows.len();
    let excluded_count = calculation.exclusions.len();
    Ok(json!({
        "screen_name": definition.name,
        "as_of": definition.as_of,
        "reporting_currency": definition.reporting_currency,
        "metadata": {
            "candidate_count": calculation.candidate_count,
            "passed_count": row_count,
            "excluded_count": excluded_count,
            "reconciled": calculation.candidate_count == row_count + excluded_count,
            "source": "EODHD Screener API",
            "logic_verification": verification,
        },
        "table": table,
        "exclusions": calculation.exclusions,
    }))
}

async fn calculate_expectations_gap(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    definition: &ScreenDefinition,
    universe: Vec<Value>,
) -> Result<ScreenCalculation, McpToolError> {
    let candidate_count = universe.len();
    let cap_min = definition
        .logic_env
        .get("market_cap_min")
        .and_then(Value::as_f64)
        .ok_or_else(|| McpToolError::invalid_argument("screen has no market_cap_min"))?;
    let cap_max = definition
        .logic_env
        .get("market_cap_max")
        .and_then(Value::as_f64)
        .ok_or_else(|| McpToolError::invalid_argument("screen has no market_cap_max"))?;
    let liquidity_min = definition
        .logic_env
        .get("liquidity_min_usd")
        .and_then(Value::as_f64)
        .ok_or_else(|| McpToolError::invalid_argument("screen has no liquidity_min_usd"))?;
    let ticker_fetches = definition
        .universe
        .exchanges
        .iter()
        .map(|exchange| async move {
            let tickers =
                providers::fetch_eodhd_common_stocks(client, eodhd_api_key, exchange).await?;
            Ok::<_, McpToolError>((exchange.clone(), tickers))
        });
    let mut common_stocks = HashSet::new();
    for outcome in futures::future::join_all(ticker_fetches).await {
        let (exchange, tickers) = outcome?;
        let rows = tickers.as_array().ok_or_else(|| {
            McpToolError::unavailable(format!("{exchange} ticker list is not an array"))
        })?;
        for row in rows {
            if row.get("Type").and_then(Value::as_str) == Some("Common Stock")
                && let Some(code) = row.get("Code").and_then(Value::as_str)
            {
                common_stocks.insert(format!("{exchange}:{code}"));
            }
        }
    }

    let mut currencies: Vec<String> = universe
        .iter()
        .filter_map(|row| row.get("currency_symbol").and_then(Value::as_str))
        .filter_map(|symbol| currency_spec(symbol).map(|(currency, _)| currency.to_string()))
        .filter(|currency| currency != "USD")
        .collect();
    currencies.sort();
    currencies.dedup();
    let fx_fetches = currencies.iter().cloned().map(|currency| async move {
        let (_, rate) = providers::fetch_eodhd_forex_rate(client, eodhd_api_key, &currency).await?;
        Ok::<_, McpToolError>((currency, rate))
    });
    let mut fx_rates = HashMap::new();
    fx_rates.insert("USD".to_string(), 1.0);
    for outcome in futures::future::join_all(fx_fetches).await {
        let (currency, rate) = outcome?;
        fx_rates.insert(currency, rate);
    }

    let outcomes = futures::stream::iter(universe.into_iter().map(|row| {
        let common_stocks = &common_stocks;
        let fx_rates = &fx_rates;
        async move {
            materialize_security(
                client,
                eodhd_api_key,
                row,
                common_stocks,
                fx_rates,
                cap_min,
                cap_max,
                liquidity_min,
            )
            .await
        }
    }))
    .buffered(12)
    .collect::<Vec<_>>()
    .await;
    let mut materialized = Vec::new();
    let mut exclusions = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(security) => materialized.push(security),
            Err(exclusion) => exclusions.push(exclusion),
        }
    }

    let groups = group_materialized_securities(materialized);
    let mut rows = Vec::new();
    for group in groups {
        match analyze_issuer_group(client, eodhd_api_key, &fx_rates, &group).await {
            Ok(row) => {
                let actionable_symbol = row.get("actionable_symbol").and_then(Value::as_str);
                for duplicate in group
                    .securities
                    .iter()
                    .filter(|security| Some(security.symbol.as_str()) != actionable_symbol)
                {
                    exclusions.push(json!({
                        "symbol": duplicate.symbol,
                        "reason": "issuer_deduplicated",
                        "issuer_key": group.issuer_key,
                    }));
                }
                rows.push(row);
            }
            Err(reason) => {
                for security in group.securities {
                    exclusions.push(json!({
                        "symbol": security.symbol,
                        "reason": "expectations_unavailable",
                        "detail": reason,
                    }));
                }
            }
        }
    }
    Ok(ScreenCalculation {
        rows,
        candidate_count,
        exclusions,
    })
}

async fn materialize_security(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    row: Value,
    common_stocks: &HashSet<String>,
    fx_rates: &HashMap<String, f64>,
    cap_min: f64,
    cap_max: f64,
    liquidity_min: f64,
) -> Result<MaterializedSecurity, Value> {
    let code = row.get("code").and_then(Value::as_str).unwrap_or("");
    let exchange = row.get("exchange").and_then(Value::as_str).unwrap_or("");
    let symbol = format!("{code}.{exchange}");
    if !common_stocks.contains(&format!("{exchange}:{code}")) {
        return Err(screen_exclusion(&symbol, "ineligible_security_type", None));
    }
    let cap = row
        .get("market_capitalization_usd")
        .and_then(Value::as_f64)
        .ok_or_else(|| screen_exclusion(&symbol, "market_cap_usd_unavailable", None))?;
    if cap < cap_min || cap > cap_max {
        return Err(json!({
            "symbol": symbol,
            "reason": "market_cap_out_of_band",
            "market_capitalization_usd": cap,
            "minimum_usd": cap_min,
            "maximum_usd": cap_max,
        }));
    }
    let currency_symbol = row
        .get("currency_symbol")
        .and_then(Value::as_str)
        .ok_or_else(|| screen_exclusion(&symbol, "currency_unavailable", None))?;
    let (currency, major_per_unit) = currency_spec(currency_symbol)
        .ok_or_else(|| screen_exclusion(&symbol, "currency_ambiguous", Some(currency_symbol)))?;
    let adjusted_close = row
        .get("adjusted_close")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| screen_exclusion(&symbol, "adjusted_close_unavailable", None))?;
    let average_volume = row
        .get("avgvol_200d")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| screen_exclusion(&symbol, "avgvol_200d_unavailable", None))?;
    let rate = fx_rates
        .get(currency)
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| screen_exclusion(&symbol, "fx_rate_unavailable", Some(currency)))?;
    let average = adjusted_close * major_per_unit * average_volume / rate;
    if average < liquidity_min {
        return Err(json!({
            "symbol": symbol,
            "reason": "below_liquidity_minimum",
            "average_daily_dollar_volume_usd": average,
            "minimum_usd": liquidity_min,
        }));
    }
    let fundamentals = providers::fetch_eodhd_fundamentals(client, eodhd_api_key, &symbol)
        .await
        .map_err(|error| {
            screen_exclusion(
                &symbol,
                "fundamentals_unavailable",
                Some(&error.to_string()),
            )
        })?;
    let general = fundamentals
        .get("General")
        .and_then(Value::as_object)
        .ok_or_else(|| screen_exclusion(&symbol, "identity_unavailable", None))?;
    if general.get("Type").and_then(Value::as_str) != Some("Common Stock") {
        return Err(screen_exclusion(&symbol, "ineligible_security_type", None));
    }
    if general.get("IsDelisted").and_then(Value::as_bool) == Some(true) {
        return Err(screen_exclusion(&symbol, "inactive_security", None));
    }
    let name = general
        .get("Name")
        .and_then(Value::as_str)
        .or_else(|| row.get("name").and_then(Value::as_str))
        .unwrap_or(code)
        .to_string();
    let primary_ticker = general
        .get("PrimaryTicker")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let lei = general
        .get("LEI")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let isin = general
        .get("ISIN")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let normalized_issuer_name = normalize_name(&name);
    let issuer_key = lei
        .as_ref()
        .map(|value| format!("lei:{value}"))
        .or_else(|| {
            primary_ticker
                .as_ref()
                .map(|value| format!("primary:{value}"))
        })
        .or_else(|| isin.as_ref().map(|value| format!("isin:{value}")))
        .unwrap_or_else(|| format!("name:{normalized_issuer_name}"));
    Ok(MaterializedSecurity {
        symbol,
        name,
        market_capitalization_usd: cap,
        average_daily_dollar_volume_usd: average,
        adjusted_close,
        currency_symbol: currency_symbol.to_string(),
        issuer_key,
        lei,
        primary_ticker,
        isin,
        normalized_issuer_name,
        fundamentals,
    })
}

/// expect: Qualifying home shares and ADRs for one issuer produce one auditable row.
/// [P5] Motivating: issuer-level screening must not rank the same company twice.
/// pre: securities passed the venue, type, capitalization, and liquidity gates.
/// post: shared LEI, primary ticker, ISIN, or non-conflicting normalized name evidence forms one group.
/// [P1] Constraining: conflicting LEIs are never merged through the name fallback.
fn group_materialized_securities(materialized: Vec<MaterializedSecurity>) -> Vec<IssuerGroup> {
    let mut groups: Vec<Vec<MaterializedSecurity>> = Vec::new();
    for security in materialized {
        let mut merged = vec![security];
        let mut remaining = groups;
        loop {
            let mut next = Vec::new();
            let mut merged_any = false;
            for group in remaining {
                if groups_share_identity(&merged, &group) {
                    merged.extend(group);
                    merged_any = true;
                } else {
                    next.push(group);
                }
            }
            if !merged_any {
                next.push(merged);
                groups = next;
                break;
            }
            remaining = next;
        }
    }
    groups.into_iter().map(finalize_issuer_group).collect()
}

fn groups_share_identity(left: &[MaterializedSecurity], right: &[MaterializedSecurity]) -> bool {
    let shares_strong_identity = left.iter().any(|left_security| {
        right.iter().any(|right_security| {
            optional_identity_matches(&left_security.lei, &right_security.lei)
                || optional_identity_matches(
                    &left_security.primary_ticker,
                    &right_security.primary_ticker,
                )
                || optional_identity_matches(&left_security.isin, &right_security.isin)
                || left_security.issuer_key == right_security.issuer_key
        })
    });
    if shares_strong_identity {
        return true;
    }
    let shares_name = left.iter().any(|left_security| {
        right.iter().any(|right_security| {
            left_security.normalized_issuer_name == right_security.normalized_issuer_name
        })
    });
    if !shares_name {
        return false;
    }
    let distinct_leis: BTreeSet<&str> = left
        .iter()
        .chain(right.iter())
        .filter_map(|security| security.lei.as_deref())
        .collect();
    distinct_leis.len() <= 1
}

fn optional_identity_matches(left: &Option<String>, right: &Option<String>) -> bool {
    left.as_deref()
        .zip(right.as_deref())
        .is_some_and(|(left, right)| left == right)
}

fn finalize_issuer_group(securities: Vec<MaterializedSecurity>) -> IssuerGroup {
    let leis: BTreeSet<&str> = securities
        .iter()
        .filter_map(|security| security.lei.as_deref())
        .collect();
    let primary_tickers: BTreeSet<&str> = securities
        .iter()
        .filter_map(|security| security.primary_ticker.as_deref())
        .collect();
    let isins: BTreeSet<&str> = securities
        .iter()
        .filter_map(|security| security.isin.as_deref())
        .collect();
    let normalized_names: BTreeSet<&str> = securities
        .iter()
        .map(|security| security.normalized_issuer_name.as_str())
        .collect();
    let (issuer_key, issuer_key_provenance) = if let Some(lei) = leis.iter().next() {
        (format!("lei:{lei}"), "lei")
    } else if primary_tickers.len() == 1 {
        let primary = primary_tickers.iter().next().copied().unwrap_or_default();
        (format!("primary:{primary}"), "primary_ticker")
    } else if isins.len() == 1 {
        let isin = isins.iter().next().copied().unwrap_or_default();
        (format!("isin:{isin}"), "isin")
    } else {
        let name = normalized_names.iter().next().copied().unwrap_or_default();
        (format!("name:{name}"), "normalized_name")
    };
    let issuer_identity_provenance = if securities.len() == 1 {
        issuer_key_provenance
    } else if leis.len() == 1 && securities.iter().all(|security| security.lei.is_some()) {
        "lei"
    } else if primary_tickers.len() == 1
        && securities
            .iter()
            .all(|security| security.primary_ticker.is_some())
    {
        "primary_ticker"
    } else if isins.len() == 1 && securities.iter().all(|security| security.isin.is_some()) {
        "isin"
    } else {
        "normalized_name_fallback"
    };
    IssuerGroup {
        issuer_key,
        issuer_key_provenance,
        issuer_identity_provenance,
        securities,
    }
}

async fn analyze_issuer_group(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    fx_rates: &HashMap<String, f64>,
    issuer_group: &IssuerGroup,
) -> Result<Value, String> {
    let group = &issuer_group.securities;
    let primary = group
        .iter()
        .find_map(|security| security.primary_ticker.as_deref())
        .ok_or_else(|| "EODHD PrimaryTicker is unavailable".to_string())?;
    let primary_security = group.iter().find(|security| security.symbol == primary);
    let fundamentals = match primary_security {
        Some(security) => security.fundamentals.clone(),
        None => providers::fetch_eodhd_fundamentals(client, eodhd_api_key, primary)
            .await
            .map_err(|error| error.to_string())?,
    };
    let income = providers::normalize_eodhd("income_statement", &fundamentals, primary);
    let balance = providers::normalize_eodhd("balance_sheet", &fundamentals, primary);
    let cash_flow = providers::normalize_eodhd("cash_flow_statement", &fundamentals, primary);
    let metrics = providers::normalize_eodhd("key_metrics", &fundamentals, primary);
    let profile_value = providers::normalize_eodhd("company_profile", &fundamentals, primary);
    let profile = crate::CompanyProfile::from_response(providers::ProviderResponse {
        value: profile_value,
        provider: crate::Provider::Eodhd,
        warnings: Vec::new(),
    });
    let (raw_price, listing_currency_symbol) = match primary_security {
        Some(security) => (
            security.adjusted_close,
            Some(security.currency_symbol.as_str()),
        ),
        None => {
            let quote = providers::fetch_eodhd_realtime(client, eodhd_api_key, primary)
                .await
                .map_err(|error| error.to_string())?;
            let price = quote
                .get("close")
                .or_else(|| quote.get("price"))
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value > 0.0)
                .ok_or_else(|| "primary security has no current price".to_string())?;
            (price, None)
        }
    };
    let current_price = normalize_primary_price(
        client,
        eodhd_api_key,
        fx_rates,
        &fundamentals,
        raw_price,
        listing_currency_symbol,
    )
    .await?;
    let price_currency_normalization_provenance = json!({
        "analysis_symbol": primary,
        "price_source": if primary_security.is_some() {
            "EODHD screener adjusted_close"
        } else {
            "EODHD realtime primary-security close"
        },
        "raw_price": raw_price,
        "listing_currency_symbol": listing_currency_symbol,
        "quote_currency": fundamentals.pointer("/General/CurrencyCode"),
        "statement_currency": fundamentals.pointer("/Financials/Income_Statement/currency_symbol"),
        "normalized_price_in_statement_currency": current_price,
        "method": "listing-unit normalization, then quote-currency-to-USD and USD-to-statement-currency FX conversion",
    });
    let analysis = crate::tools::expectations::solve_expectations(
        &income,
        &balance,
        &cash_flow,
        &metrics,
        &profile,
        current_price,
    );
    let report = crate::tools::expectations::build_gap_report(
        primary,
        &analysis,
        &[],
        0.05,
        &[],
        0,
        "EODHD as-of close; single fundamentals payload",
    );
    let actionable = group
        .iter()
        .max_by(|left, right| {
            left.average_daily_dollar_volume_usd
                .partial_cmp(&right.average_daily_dollar_volume_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .ok_or_else(|| "issuer group is empty".to_string())?;
    let mut caps: Vec<f64> = group
        .iter()
        .map(|security| security.market_capitalization_usd)
        .collect();
    caps.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let cap = caps
        .get(caps.len() / 2)
        .copied()
        .ok_or_else(|| "issuer has no market capitalization".to_string())?;
    let capability = report.get("capability").cloned().unwrap_or(Value::Null);
    let price_implied = report.get("price_implied").cloned().unwrap_or(Value::Null);
    let gaps = report.get("gaps").cloned().unwrap_or(Value::Null);
    let headline = capability.get("headline_measure").and_then(Value::as_str);
    let demonstrated = if headline == Some("roe") {
        capability.get("roe").cloned().unwrap_or(Value::Null)
    } else {
        capability
            .get("net_profit_margin")
            .cloned()
            .unwrap_or(Value::Null)
    };
    let implied_profitability = if headline == Some("roe") {
        price_implied
            .pointer("/implied_roe/value")
            .cloned()
            .unwrap_or(Value::Null)
    } else {
        price_implied
            .pointer("/implied_net_margin_at_demonstrated_growth/value")
            .cloned()
            .unwrap_or(Value::Null)
    };
    let demonstrated_growth = gaps
        .get("demonstrated_revenue_growth")
        .cloned()
        .unwrap_or(Value::Null);
    let growth_gap = gaps.get("growth_gap_pp").cloned().unwrap_or(Value::Null);
    let financing_growth_gap = gaps
        .get("financing_growth_gap_pp")
        .cloned()
        .unwrap_or(Value::Null);
    let profitability_gap = gaps
        .get("profitability_gap_pp")
        .cloned()
        .unwrap_or(Value::Null);
    let capability_status = report
        .pointer("/data_quality/capability_status")
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    let capability_flags = report
        .pointer("/data_quality/capability_flags")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let capability_model_sensitive = capability_status == "model_sensitive";
    let gap_status = gap_data_status(&growth_gap, &profitability_gap);
    let data_quality_status = if gap_status == "partial" {
        "partial"
    } else if capability_model_sensitive {
        "model_sensitive"
    } else {
        "complete"
    };
    Ok(json!({
        "company": actionable.name,
        "issuer_key": issuer_group.issuer_key,
        "issuer_key_provenance": issuer_group.issuer_key_provenance,
        "issuer_identity_provenance": issuer_group.issuer_identity_provenance,
        "primary_ticker": primary,
        "analysis_symbol": primary,
        "analysis_line_reason": "EODHD PrimaryTicker supplies the issuer fundamentals and price-implied analysis",
        "actionable_symbol": actionable.symbol,
        "actionable_venue": symbol_venue(&actionable.symbol),
        "actionable_line_reason": "highest normalized 200-day average daily dollar volume among eligible lines",
        "eligible_symbols": group.iter().map(|security| security.symbol.clone()).collect::<Vec<_>>(),
        "eligible_lines": group.iter().map(|security| json!({
            "symbol": security.symbol,
            "venue": symbol_venue(&security.symbol),
            "market_capitalization_usd": security.market_capitalization_usd,
            "average_daily_dollar_volume_usd": security.average_daily_dollar_volume_usd,
            "listing_currency_symbol": security.currency_symbol,
        })).collect::<Vec<_>>(),
        "market_capitalization_usd": cap,
        "average_daily_dollar_volume_usd": actionable.average_daily_dollar_volume_usd,
        "demonstrated_profitability": demonstrated,
        "demonstrated_growth": demonstrated_growth,
        "sustainable_growth": capability.get("sustainable_growth_rate").cloned().unwrap_or(Value::Null),
        "implied_growth": price_implied.pointer("/implied_growth/value").cloned().unwrap_or(Value::Null),
        "growth_gap_pp": growth_gap,
        "financing_growth_gap_pp": financing_growth_gap,
        "implied_profitability": implied_profitability,
        "profitability_gap_pp": profitability_gap,
        "gap_data_status": gap_status,
        "data_quality_status": data_quality_status,
        "capability_quality_flags": capability_flags,
        "price_currency_normalization_provenance": price_currency_normalization_provenance,
        "expectations_report": report,
    }))
}

fn screen_exclusion(symbol: &str, reason: &str, detail: Option<&str>) -> Value {
    json!({"symbol":symbol,"reason":reason,"detail":detail})
}

async fn status(server: &CompaniesServer, job_id: &str) -> Result<Value, McpToolError> {
    let job = load_job(&server.research, job_id)?;
    Ok(json!({
        "job_id": job.id,
        "status": job.status,
        "error": job.error,
        "created_at": job.created_at,
        "updated_at": job.updated_at,
    }))
}

async fn results(
    server: &CompaniesServer,
    job_id: &str,
    cursor: u32,
    limit: u32,
) -> Result<Value, McpToolError> {
    let job = load_job(&server.research, job_id)?;
    if job.status != "completed" {
        return Err(McpToolError::unavailable(format!(
            "screen job {job_id:?} is {}; results are not ready",
            job.status
        )));
    }
    let result = job
        .result
        .ok_or_else(|| McpToolError::internal("completed screen job has no result"))?;
    paginate_result(result, cursor, limit)
}

fn load_job(store: &ResearchStore, job_id: &str) -> Result<ScreenJobRecord, McpToolError> {
    store
        .get_screen_job(job_id)
        .map_err(crate::map_portfolio_error)?
        .ok_or_else(|| McpToolError::invalid_argument(format!("screen job {job_id:?} not found")))
}

fn resolve_definition(
    req: &ScreenerRequest,
    acquisition_date: &str,
) -> Result<ScreenDefinition, McpToolError> {
    match (&req.template, &req.screen_definition) {
        (Some(name), None) => {
            render_template(name, req.template_context.as_ref(), acquisition_date)
        }
        (None, Some(value)) => serde_json::from_value(value.0.clone()).map_err(|error| {
            McpToolError::invalid_argument(format!("invalid screen_definition: {error}"))
        }),
        (Some(_), Some(_)) => Err(McpToolError::invalid_argument(
            "provide template or screen_definition, not both",
        )),
        (None, None) => Err(McpToolError::invalid_argument(
            "calculate requires template or screen_definition",
        )),
    }
}

fn validate_definition(
    definition: &ScreenDefinition,
    acquisition_date: &str,
) -> Result<(), McpToolError> {
    if definition.universe.exchanges.is_empty() {
        return Err(McpToolError::invalid_argument(
            "screen universe requires at least one exchange",
        ));
    }
    if definition.as_of != acquisition_date {
        return Err(McpToolError::invalid_argument(format!(
            "screen as_of {:?} does not match current acquisition date {acquisition_date:?}",
            definition.as_of
        )));
    }
    Ok(())
}

fn parse_template_source<'a>(
    name: &str,
    source: &'a str,
) -> Result<(ScreenTemplateMetadata, &'a str), McpToolError> {
    let (metadata, body) = source.split_once("\n---\n").ok_or_else(|| {
        McpToolError::internal(format!(
            "screen template {name:?} has no metadata delimiter"
        ))
    })?;
    let metadata = metadata.strip_prefix("[inference]\n").ok_or_else(|| {
        McpToolError::internal(format!(
            "screen template {name:?} has no inference metadata header"
        ))
    })?;
    let metadata = serde_yaml_neo::from_str(metadata).map_err(|error| {
        McpToolError::internal(format!(
            "screen template {name:?} has invalid metadata: {error}"
        ))
    })?;
    Ok((metadata, body))
}

fn render_template(
    name: &str,
    context: Option<&ScreenTemplateContext>,
    acquisition_date: &str,
) -> Result<ScreenDefinition, McpToolError> {
    let source = SCREEN_TEMPLATES
        .iter()
        .find_map(|(registered_name, source)| (*registered_name == name).then_some(*source))
        .ok_or_else(|| {
            McpToolError::invalid_argument(format!("unknown screen template {name:?}"))
        })?;
    let (metadata, body) = parse_template_source(name, source)?;
    let mut context = match context {
        Some(context) => serde_json::to_value(context).map_err(|error| {
            McpToolError::internal(format!(
                "screen template {name:?} context failed to serialize: {error}"
            ))
        })?,
        None => json!({}),
    };
    let context_object = context.as_object_mut().ok_or_else(|| {
        McpToolError::internal(format!("screen template {name:?} context is not an object"))
    })?;
    context_object.insert("as_of".to_string(), json!(acquisition_date));
    let missing_context_variables: Vec<&str> = metadata
        .contract
        .input
        .keys()
        .chain(metadata.contract.server_input.keys())
        .filter(|field| !context_object.contains_key(field.as_str()))
        .map(String::as_str)
        .collect();
    if !missing_context_variables.is_empty() {
        let details = json!({
            "template": name,
            "missing_context_variables": missing_context_variables,
        });
        return Err(McpToolError::invalid_argument(format!(
            "screen template context is incomplete: {details}"
        )));
    }

    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template(name, body).map_err(|error| {
        McpToolError::internal(format!(
            "screen template {name:?} failed to compile: {error}"
        ))
    })?;
    let rendered = env
        .get_template(name)
        .and_then(|template| template.render(minijinja::Value::from_serialize(&context)))
        .map_err(|error| {
            McpToolError::invalid_argument(format!("screen template {name:?} failed: {error}"))
        })?;
    serde_json::from_str(&rendered).map_err(|error| {
        McpToolError::internal(format!(
            "screen template {name:?} rendered invalid JSON: {error}"
        ))
    })
}

fn verify_assertions(definition: &ScreenDefinition) -> Result<Value, McpToolError> {
    let mut evidence = Vec::new();
    for assertion in &definition.assertions {
        let result = hkask_lisp::eval_sandboxed_with_budget(
            &assertion.form,
            &definition.logic_env,
            LISP_MAX_STEPS,
            LISP_MAX_DEPTH,
        )
        .map_err(|error| {
            McpToolError::invalid_argument(format!(
                "screen assertion {:?} failed to evaluate: {error}",
                assertion.name
            ))
        })?;
        if result != Value::Bool(true) {
            return Err(McpToolError::invalid_argument(format!(
                "screen assertion {:?} returned {result}, expected true",
                assertion.name
            )));
        }
        evidence.push(json!({
            "name": assertion.name,
            "form": assertion.form,
            "result": result,
            "engine": "hkask-lisp sandboxed",
        }));
    }
    Ok(Value::Array(evidence))
}

async fn normalize_primary_price(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    fx_rates: &HashMap<String, f64>,
    fundamentals: &Value,
    raw_price: f64,
    listing_currency_symbol: Option<&str>,
) -> Result<f64, String> {
    let quote_code = fundamentals
        .pointer("/General/CurrencyCode")
        .and_then(Value::as_str)
        .ok_or_else(|| "primary currency is unavailable".to_string())?;
    let statement_code = fundamentals
        .pointer("/Financials/Income_Statement/currency_symbol")
        .and_then(Value::as_str)
        .ok_or_else(|| "statement currency is unavailable".to_string())?;
    let (quote_major, mut quote_unit) = currency_code_unit(quote_code);
    if listing_currency_symbol == Some("p") {
        quote_unit = 0.01;
    }
    let (statement_major, statement_unit) = currency_code_unit(statement_code);
    let quote_rate = current_rate(client, eodhd_api_key, fx_rates, &quote_major).await?;
    let statement_rate = if statement_major == quote_major {
        quote_rate
    } else {
        current_rate(client, eodhd_api_key, fx_rates, &statement_major).await?
    };
    let normalized = raw_price * quote_unit / quote_rate * statement_rate / statement_unit;
    if normalized.is_finite() && normalized > 0.0 {
        Ok(normalized)
    } else {
        Err("primary price currency conversion is invalid".to_string())
    }
}

async fn current_rate(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    fx_rates: &HashMap<String, f64>,
    currency: &str,
) -> Result<f64, String> {
    if currency == "USD" {
        return Ok(1.0);
    }
    if let Some(rate) = fx_rates.get(currency).copied() {
        return Ok(rate);
    }
    providers::fetch_eodhd_forex_rate(client, eodhd_api_key, currency)
        .await
        .map(|(_, rate)| rate)
        .map_err(|error| error.to_string())
}

fn currency_code_unit(code: &str) -> (String, f64) {
    if code.eq_ignore_ascii_case("GBX") {
        ("GBP".to_string(), 0.01)
    } else {
        (code.trim().to_uppercase(), 1.0)
    }
}

fn gap_data_status(growth_gap: &Value, profitability_gap: &Value) -> &'static str {
    if growth_gap.is_number() && profitability_gap.is_number() {
        "complete"
    } else {
        "partial"
    }
}

fn currency_spec(symbol: &str) -> Option<(&'static str, f64)> {
    match symbol {
        "$" => Some(("USD", 1.0)),
        "C$" => Some(("CAD", 1.0)),
        "₱" => Some(("MXN", 1.0)),
        "£" => Some(("GBP", 1.0)),
        "p" => Some(("GBP", 0.01)),
        "€" => Some(("EUR", 1.0)),
        "¥" => Some(("JPY", 1.0)),
        "CHF" => Some(("CHF", 1.0)),
        "zł" => Some(("PLN", 1.0)),
        "Kč" => Some(("CZK", 1.0)),
        "Ft" => Some(("HUF", 1.0)),
        "₫" => Some(("VND", 1.0)),
        _ => None,
    }
}

fn symbol_venue(symbol: &str) -> &str {
    symbol.rsplit_once('.').map_or("", |(_, exchange)| exchange)
}

fn normalize_name(value: &str) -> String {
    let mut tokens: Vec<String> = value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect();
    if let Some(adr_index) = tokens.iter().position(|token| token == "adr") {
        tokens.truncate(adr_index);
    }
    tokens.concat()
}

fn sort_rows(rows: &mut [Value], definition: &ScreenDefinition) {
    let Some(sort) = definition.ranking.first() else {
        return;
    };
    let Some(column) = definition
        .columns
        .iter()
        .find(|column| column.id == sort.column)
    else {
        return;
    };
    rows.sort_by(|left, right| {
        let left = left.get(&column.source).and_then(Value::as_f64);
        let right = right.get(&column.source).and_then(Value::as_f64);
        let order = match (left, right) {
            (Some(left), Some(right)) => left
                .partial_cmp(&right)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        };
        if sort.direction == "descending" {
            order.reverse()
        } else {
            order
        }
    });
}

fn columnar_table(rows: &[Value], definition: &ScreenDefinition) -> Value {
    let row_ids: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let code = row.get("code").and_then(Value::as_str).unwrap_or("");
            let exchange = row.get("exchange").and_then(Value::as_str).unwrap_or("");
            if !code.is_empty() {
                format!("{code}.{exchange}")
            } else {
                row.get("actionable_symbol")
                    .or_else(|| row.get("primary_ticker"))
                    .or_else(|| row.get("issuer_key"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("row-{index}"))
            }
        })
        .collect();
    let mut columns = Map::new();
    for column in &definition.columns {
        let values: Vec<Value> = rows
            .iter()
            .map(|row| {
                if column.source == "symbol" {
                    let code = row.get("code").and_then(Value::as_str).unwrap_or("");
                    let exchange = row.get("exchange").and_then(Value::as_str).unwrap_or("");
                    json!(format!("{code}.{exchange}"))
                } else {
                    row.get(&column.source).cloned().unwrap_or(Value::Null)
                }
            })
            .collect();
        columns.insert(
            column.id.clone(),
            json!({
                "label": column.label,
                "data_type": column.data_type,
                "unit": column.unit,
                "source": column.source,
                "provenance": column.provenance.clone().unwrap_or_else(|| {
                    format!("EODHD-derived:{}", column.source)
                }),
                "values": values,
            }),
        );
    }
    json!({
        "row_count": rows.len(),
        "row_ids": row_ids,
        "columns": Value::Object(columns),
    })
}

fn paginate_result(mut result: Value, cursor: u32, limit: u32) -> Result<Value, McpToolError> {
    if limit == 0 {
        return Err(McpToolError::invalid_argument(
            "result limit must be greater than zero",
        ));
    }
    let table = result
        .get_mut("table")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| McpToolError::internal("screen result has no table"))?;
    let row_count = table
        .get("row_count")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| McpToolError::internal("screen result has invalid row_count"))?;
    let row_ids = table
        .get("row_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| McpToolError::internal("screen result has invalid row_ids"))?;
    if row_ids.len() != row_count {
        return Err(McpToolError::internal(format!(
            "screen result row_ids length {} does not match row_count {row_count}",
            row_ids.len()
        )));
    }
    let columns = table
        .get("columns")
        .and_then(Value::as_object)
        .ok_or_else(|| McpToolError::internal("screen result has invalid columns"))?;
    for (column_id, column) in columns {
        let values = column
            .get("values")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                McpToolError::internal(format!(
                    "screen result column {column_id:?} has invalid values"
                ))
            })?;
        if values.len() != row_count {
            return Err(McpToolError::internal(format!(
                "screen result column {column_id:?} length {} does not match row_count {row_count}",
                values.len()
            )));
        }
    }

    let start = usize::try_from(cursor)
        .map_err(|_| McpToolError::invalid_argument("cursor exceeds platform range"))?
        .min(row_count);
    let page_limit = usize::try_from(limit)
        .map_err(|_| McpToolError::invalid_argument("result limit exceeds platform range"))?;
    let end = start.saturating_add(page_limit).min(row_count);
    let row_ids = table
        .get_mut("row_ids")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| McpToolError::internal("screen result has invalid row_ids"))?;
    *row_ids = row_ids
        .get(start..end)
        .ok_or_else(|| McpToolError::internal("screen result row_ids page is out of bounds"))?
        .to_vec();
    let columns = table
        .get_mut("columns")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| McpToolError::internal("screen result has invalid columns"))?;
    for (column_id, column) in columns {
        let values = column
            .get_mut("values")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                McpToolError::internal(format!(
                    "screen result column {column_id:?} has invalid values"
                ))
            })?;
        *values = values
            .get(start..end)
            .ok_or_else(|| {
                McpToolError::internal(format!(
                    "screen result column {column_id:?} page is out of bounds"
                ))
            })?
            .to_vec();
    }
    table.insert("page_start".to_string(), json!(start));
    table.insert("page_count".to_string(), json!(end - start));
    table.insert(
        "next_cursor".to_string(),
        if end < row_count {
            json!(end)
        } else {
            Value::Null
        },
    );
    Ok(result)
}

fn required_job_id(req: &ScreenerRequest) -> Result<&str, McpToolError> {
    req.job_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| McpToolError::invalid_argument("job_id is required for this action"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expectations_definition(as_of: &str, exchanges: Vec<String>) -> ScreenDefinition {
        let context = ScreenTemplateContext {
            exchanges: Some(exchanges),
            market_cap_min: Some(5_000_000_000.0),
            market_cap_max: Some(50_000_000_000.0),
            liquidity_min_usd: Some(1_000_000.0),
        };
        match render_template("expectations_gap", Some(&context), as_of) {
            Ok(definition) => definition,
            Err(error) => panic!("expectations template must render: {error}"),
        }
    }

    #[test]
    fn definition_validation_rejects_empty_exchange_universe() {
        let definition = expectations_definition("2026-09-13", Vec::new());
        let error = match validate_definition(&definition, "2026-09-13") {
            Ok(()) => panic!("empty exchange universe must be rejected"),
            Err(error) => error,
        };
        assert_eq!(
            error.message,
            "screen universe requires at least one exchange"
        );
    }

    #[test]
    fn definition_validation_rejects_historical_label_for_current_data() {
        let definition = expectations_definition("2026-09-12", vec!["US".to_string()]);
        let error = match validate_definition(&definition, "2026-09-13") {
            Ok(()) => panic!("historical as_of must be rejected for current-only acquisition"),
            Err(error) => error,
        };
        assert_eq!(
            error.message,
            "screen as_of \"2026-09-12\" does not match current acquisition date \"2026-09-13\""
        );
    }

    /// expect: [P1] Gap-data completeness reports whether both requested gap
    /// legs exist; score eligibility must not masquerade as data quality.
    /// pre: growth and profitability gap values may have any sign.
    /// post: two numeric legs are complete even when no ranking score is awarded.
    #[test]
    fn gap_data_status_is_independent_of_score_sign() {
        assert_eq!(gap_data_status(&json!(4.0), &json!(-2.0)), "complete");
        assert_eq!(gap_data_status(&json!(-4.0), &json!(-2.0)), "complete");
        assert_eq!(gap_data_status(&Value::Null, &json!(-2.0)), "partial");
    }

    /// expect: [P1] The production template exposes raw gap dimensions and
    /// does not rank companies with an uncalibrated synthetic score.
    #[test]
    fn expectations_template_withholds_uncalibrated_composite_score() {
        let definition = expectations_definition("2026-09-14", vec!["US".to_string()]);
        assert!(definition.ranking.is_empty());
        assert!(
            definition
                .columns
                .iter()
                .all(|column| column.id != "expectations_gap_score")
        );
        assert!(
            definition
                .columns
                .iter()
                .any(|column| column.id == "demonstrated_growth")
        );
        assert!(
            definition
                .columns
                .iter()
                .any(|column| column.id == "financing_growth_gap_pp")
        );
    }

    #[test]
    fn pagination_rejects_inconsistent_stored_column_lengths() {
        let result = json!({
            "table": {
                "row_count": 2,
                "row_ids": ["only-one"],
                "columns": {
                    "company": {"values": ["only-one"]}
                }
            }
        });
        let error = match paginate_result(result, 0, 1) {
            Ok(_) => panic!("inconsistent stored result must be rejected"),
            Err(error) => error,
        };
        assert_eq!(
            error.message,
            "screen result row_ids length 1 does not match row_count 2"
        );
    }

    #[tokio::test]
    async fn calculation_panic_is_persisted_as_failed() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let store = ResearchStore::with_dir(directory.path().to_path_buf())?;
        store.insert_screen_job(&ScreenJobRecord {
            id: "panic-job".to_string(),
            status: "queued".to_string(),
            definition: json!({"name":"panic-contract"}),
            result: None,
            error: None,
            created_at: "2026-09-13T00:00:00Z".to_string(),
            updated_at: "2026-09-13T00:00:00Z".to_string(),
        })?;

        persist_screen_calculation(store.clone(), "panic-job".to_string(), async {
            panic!("calculation exploded");
            #[allow(unreachable_code)]
            Ok(json!({}))
        })
        .await;

        let job = store
            .get_screen_job("panic-job")?
            .ok_or_else(|| std::io::Error::other("panic job was not persisted"))?;
        assert_eq!(job.status, "failed");
        assert_eq!(job.result, None);
        assert_eq!(
            job.error.as_deref(),
            Some("screen calculation panicked: calculation exploded")
        );
        Ok(())
    }

    #[test]
    fn registered_template_inputs_match_public_context_schema() {
        let schema = schemars::schema_for!(ScreenTemplateContext);
        let schema = match serde_json::to_value(schema) {
            Ok(schema) => schema,
            Err(error) => panic!("template context schema must serialize: {error}"),
        };
        let schema_fields: std::collections::BTreeSet<String> =
            match schema.get("properties").and_then(Value::as_object) {
                Some(properties) => properties.keys().cloned().collect(),
                None => panic!("template context schema has no properties: {schema}"),
            };
        let mut contract_fields = std::collections::BTreeSet::new();
        let mut server_fields = std::collections::BTreeSet::new();
        for (name, source) in SCREEN_TEMPLATES {
            let metadata = match parse_template_source(name, source) {
                Ok((metadata, _)) => metadata,
                Err(error) => panic!("registered template metadata must parse: {error}"),
            };
            contract_fields.extend(metadata.contract.input.into_keys());
            server_fields.extend(metadata.contract.server_input.into_keys());
        }
        assert_eq!(schema_fields, contract_fields);
        assert_eq!(
            server_fields,
            std::collections::BTreeSet::from(["as_of".to_string()])
        );
    }

    #[test]
    fn template_preflight_reports_all_missing_context_variables() {
        let request = serde_json::from_value(json!({
            "action":"calculate",
            "template":"expectations_gap",
            "template_context":{},
            "prompt":"",
            "limit":100,
            "criteria_overrides":{}
        }));
        let request: ScreenerRequest = match request {
            Ok(request) => request,
            Err(error) => panic!("template request must deserialize: {error}"),
        };
        let error = match resolve_definition(&request, "2026-09-13") {
            Ok(_) => panic!("missing template context must be rejected"),
            Err(error) => error,
        };
        let details = match error
            .message
            .strip_prefix("screen template context is incomplete: ")
        {
            Some(details) => details,
            None => panic!(
                "missing structured template-context detail: {}",
                error.message
            ),
        };
        let details: Value = match serde_json::from_str(details) {
            Ok(details) => details,
            Err(parse_error) => panic!("template-context detail must be JSON: {parse_error}"),
        };
        assert_eq!(details.get("template"), Some(&json!("expectations_gap")));
        assert_eq!(
            details.get("missing_context_variables"),
            Some(&json!([
                "exchanges",
                "liquidity_min_usd",
                "market_cap_max",
                "market_cap_min"
            ]))
        );
    }

    #[test]
    fn jinja_and_api_inputs_resolve_to_the_same_screen_definition() {
        let templated: ScreenerRequest = serde_json::from_value(json!({
            "action":"calculate",
            "template":"universal_equity",
            "template_context":{
                "exchanges":["US"],
                "market_cap_min":5_000_000_000.0,
                "market_cap_max":50_000_000_000.0
            },
            "prompt":"",
            "limit":100,
            "criteria_overrides":{}
        }))
        .expect("template request");
        let rendered = resolve_definition(&templated, "2026-09-13").expect("rendered definition");
        assert_eq!(rendered.as_of, "2026-09-13");
        let rendered_value = serde_json::to_value(&rendered).expect("definition JSON");
        let direct: ScreenerRequest = serde_json::from_value(json!({
            "action":"calculate",
            "screen_definition":rendered_value,
            "prompt":"",
            "limit":100,
            "criteria_overrides":{}
        }))
        .expect("direct request");
        let direct = resolve_definition(&direct, "2026-09-13").expect("direct definition");
        assert_eq!(
            serde_json::to_value(rendered).expect("rendered JSON"),
            serde_json::to_value(direct).expect("direct JSON")
        );
    }
}
