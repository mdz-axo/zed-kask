use crate::{
    CompaniesServer, providers,
    research_store::{ResearchStore, ScreenJobRecord},
    types::{ScreenAction, ScreenTemplateContext, ScreenerRequest},
};

use futures::StreamExt as _;
use hkask_mcp_server::server::McpToolError;
use hkask_types::time::now_rfc3339;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

const UNIVERSAL_EQUITY_TEMPLATE: &str =
    include_str!("../../../registry/templates/company-screen/universal_equity.j2");
const EXPECTATIONS_GAP_TEMPLATE: &str =
    include_str!("../../../registry/templates/company-screen/expectations_gap.j2");
const LISP_MAX_STEPS: u64 = 100_000;
const LISP_MAX_DEPTH: u64 = 256;

#[derive(Deserialize)]
struct ScreenTemplateMetadata {
    contract: ScreenTemplateContract,
}

#[derive(Deserialize)]
struct ScreenTemplateContract {
    input: BTreeMap<String, String>,
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
    primary_ticker: Option<String>,
    fundamentals: Value,
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
    let definition = resolve_definition(&req)?;
    let verification = verify_assertions(&definition)?;
    let universe_snapshot = acquire_universe(server, &definition).await?;
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

    let client = server.client.clone();
    let eodhd_api_key = server.eodhd_api_key.clone();
    let store = server.research.clone();
    let task_id = id.clone();
    #[cfg(test)]
    let test_origin = providers::TEST_HTTP_ORIGIN.try_with(Clone::clone).ok();
    drop(tokio::spawn(async move {
        let calculation = calculate_job(
            store,
            client,
            eodhd_api_key,
            task_id,
            definition,
            verification,
            universe_snapshot,
        );
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
    store: ResearchStore,
    client: reqwest::Client,
    eodhd_api_key: String,
    job_id: String,
    definition: ScreenDefinition,
    verification: Value,
    universe_snapshot: Vec<Value>,
) {
    if let Err(error) = store.update_screen_job(&job_id, "executing", None, None) {
        tracing::error!(job_id, "failed to mark screen job executing: {error}");
        return;
    }
    match calculate(
        &client,
        &eodhd_api_key,
        &definition,
        verification,
        universe_snapshot,
    )
    .await
    {
        Ok(result) => {
            if let Err(error) = store.update_screen_job(&job_id, "completed", Some(&result), None) {
                tracing::error!(job_id, "failed to persist completed screen job: {error}");
            }
        }
        Err(error) => {
            let message = error.to_string();
            if let Err(store_error) =
                store.update_screen_job(&job_id, "failed", None, Some(&message))
            {
                tracing::error!(
                    job_id,
                    "failed to persist screen job failure: {store_error}"
                );
            }
        }
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

    let mut groups: HashMap<String, Vec<MaterializedSecurity>> = HashMap::new();
    for security in materialized {
        groups
            .entry(security.issuer_key.clone())
            .or_default()
            .push(security);
    }
    let mut rows = Vec::new();
    for (issuer_key, group) in groups {
        match analyze_issuer_group(client, eodhd_api_key, &fx_rates, &issuer_key, &group).await {
            Ok(row) => {
                for duplicate in group.iter().skip(1) {
                    exclusions.push(json!({
                        "symbol": duplicate.symbol,
                        "reason": "issuer_deduplicated",
                        "issuer_key": issuer_key,
                    }));
                }
                rows.push(row);
            }
            Err(reason) => {
                for security in group {
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
    let issuer_key = general
        .get("LEI")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| format!("lei:{value}"))
        .or_else(|| {
            primary_ticker
                .as_ref()
                .map(|value| format!("primary:{value}"))
        })
        .unwrap_or_else(|| format!("name:{}", normalize_name(&name)));
    Ok(MaterializedSecurity {
        symbol,
        name,
        market_capitalization_usd: cap,
        average_daily_dollar_volume_usd: average,
        adjusted_close,
        currency_symbol: currency_symbol.to_string(),
        issuer_key,
        primary_ticker,
        fundamentals,
    })
}

async fn analyze_issuer_group(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    fx_rates: &HashMap<String, f64>,
    issuer_key: &str,
    group: &[MaterializedSecurity],
) -> Result<Value, String> {
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
            .pointer("/implied_net_margin_at_sustainable_growth/value")
            .cloned()
            .unwrap_or(Value::Null)
    };
    let growth_gap = gaps.get("growth_gap_pp").cloned().unwrap_or(Value::Null);
    let profitability_gap = gaps
        .get("profitability_gap_pp")
        .cloned()
        .unwrap_or(Value::Null);
    let score = expectation_score(&growth_gap, &profitability_gap);
    Ok(json!({
        "company": actionable.name,
        "issuer_key": issuer_key,
        "primary_ticker": primary,
        "actionable_symbol": actionable.symbol,
        "eligible_symbols": group.iter().map(|security| security.symbol.clone()).collect::<Vec<_>>(),
        "market_capitalization_usd": cap,
        "average_daily_dollar_volume_usd": actionable.average_daily_dollar_volume_usd,
        "demonstrated_profitability": demonstrated,
        "sustainable_growth": capability.get("sustainable_growth_rate").cloned().unwrap_or(Value::Null),
        "implied_growth": price_implied.pointer("/implied_growth/value").cloned().unwrap_or(Value::Null),
        "growth_gap_pp": growth_gap,
        "implied_profitability": implied_profitability,
        "profitability_gap_pp": profitability_gap,
        "expectations_gap_score": score,
        "data_quality_status": if score.is_number() { "complete" } else { "partial" },
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

fn resolve_definition(req: &ScreenerRequest) -> Result<ScreenDefinition, McpToolError> {
    match (&req.template, &req.screen_definition) {
        (Some(name), None) => render_template(name, req.template_context.as_ref()),
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

fn render_template(
    name: &str,
    context: Option<&ScreenTemplateContext>,
) -> Result<ScreenDefinition, McpToolError> {
    let source = match name {
        "universal_equity" => UNIVERSAL_EQUITY_TEMPLATE,
        "expectations_gap" => EXPECTATIONS_GAP_TEMPLATE,
        _ => {
            return Err(McpToolError::invalid_argument(format!(
                "unknown screen template {name:?}"
            )));
        }
    };
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
    let metadata: ScreenTemplateMetadata = serde_yaml_neo::from_str(metadata).map_err(|error| {
        McpToolError::internal(format!(
            "screen template {name:?} has invalid metadata: {error}"
        ))
    })?;
    let context = match context {
        Some(context) => serde_json::to_value(context).map_err(|error| {
            McpToolError::internal(format!(
                "screen template {name:?} context failed to serialize: {error}"
            ))
        })?,
        None => json!({}),
    };
    let missing_context_variables: Vec<&str> = metadata
        .contract
        .input
        .keys()
        .filter(|field| {
            !context
                .as_object()
                .is_some_and(|object| object.contains_key(field.as_str()))
        })
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

fn expectation_score(growth_gap: &Value, profitability_gap: &Value) -> Value {
    let Some(growth) = growth_gap.as_f64() else {
        return Value::Null;
    };
    let Some(profitability) = profitability_gap.as_f64() else {
        return Value::Null;
    };
    if growth >= 0.0 || profitability >= 0.0 {
        return Value::Null;
    }
    json!(((-growth / 3.0) * (-profitability / 0.5)).sqrt())
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

fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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
    let start = usize::try_from(cursor)
        .map_err(|_| McpToolError::invalid_argument("cursor exceeds platform range"))?
        .min(row_count);
    let end = start
        .saturating_add(usize::try_from(limit).unwrap_or(usize::MAX))
        .min(row_count);
    if let Some(row_ids) = table.get_mut("row_ids").and_then(Value::as_array_mut) {
        *row_ids = row_ids[start..end].to_vec();
    }
    if let Some(columns) = table.get_mut("columns").and_then(Value::as_object_mut) {
        for column in columns.values_mut() {
            if let Some(values) = column.get_mut("values").and_then(Value::as_array_mut) {
                *values = values[start..end].to_vec();
            }
        }
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
        let error = match resolve_definition(&request) {
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
                "as_of",
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
                "as_of":"2026-09-11",
                "market_cap_min":5_000_000_000.0,
                "market_cap_max":50_000_000_000.0
            },
            "prompt":"",
            "limit":100,
            "criteria_overrides":{}
        }))
        .expect("template request");
        let rendered = resolve_definition(&templated).expect("rendered definition");
        let rendered_value = serde_json::to_value(&rendered).expect("definition JSON");
        let direct: ScreenerRequest = serde_json::from_value(json!({
            "action":"calculate",
            "screen_definition":rendered_value,
            "prompt":"",
            "limit":100,
            "criteria_overrides":{}
        }))
        .expect("direct request");
        let direct = resolve_definition(&direct).expect("direct definition");
        assert_eq!(
            serde_json::to_value(rendered).expect("rendered JSON"),
            serde_json::to_value(direct).expect("direct JSON")
        );
    }
}
