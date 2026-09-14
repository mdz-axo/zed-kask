use crate::{
    CompaniesServer, providers,
    research_store::{ResearchStore, ScreenJobItemRecord, ScreenJobRecord},
    types::{ScreenAction, ScreenTemplateContext, ScreenerRequest},
};

use futures::{FutureExt as _, StreamExt as _};
use hkask_mcp_server::server::McpToolError;
use hkask_types::time::now_rfc3339;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
#[cfg(test)]
use std::future::Future;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    panic::AssertUnwindSafe,
    time::Duration,
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
const SCREEN_PASS_DEADLINE: Duration = Duration::from_secs(60);
const ENRICHMENT_DEADLINE: Duration = Duration::from_secs(110);

const BULK_ANALYSIS_CONCURRENCY: usize = 96;
const FALLBACK_ENRICHMENT_CONCURRENCY: usize = 12;
const FALLBACK_ISSUER_TIMEOUT: Duration = Duration::from_secs(15);

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IssuerGroup {
    issuer_key: String,
    issuer_key_provenance: String,
    issuer_identity_provenance: String,
    securities: Vec<MaterializedSecurity>,
}

struct PreparedPassSet {
    groups: Vec<IssuerGroup>,
    candidate_count: usize,
    financial_passing_security_count: usize,
    exclusions: Vec<Value>,
    fx_rates: HashMap<String, f64>,
}

pub(crate) fn resume_pending_jobs(server: &CompaniesServer) {
    let jobs = match server.research.pending_screen_jobs() {
        Ok(jobs) => jobs,
        Err(error) => {
            tracing::warn!("failed to load resumable screen jobs: {error}");
            return;
        }
    };
    for job in jobs {
        let definition: ScreenDefinition = match serde_json::from_value(job.definition) {
            Ok(definition) => definition,
            Err(error) => {
                tracing::warn!(
                    job_id = job.id,
                    "invalid resumable screen definition: {error}"
                );
                continue;
            }
        };
        let verification = match verify_assertions(&definition) {
            Ok(verification) => verification,
            Err(error) => {
                tracing::warn!(
                    job_id = job.id,
                    "resumable screen assertions failed: {error}"
                );
                continue;
            }
        };
        let server = server.clone();
        drop(tokio::spawn(async move {
            calculate_job(server, job.id, definition, verification).await;
        }));
    }
}

pub(crate) async fn execute(
    server: &CompaniesServer,
    req: ScreenerRequest,
) -> Result<Value, McpToolError> {
    match req.action {
        Some(ScreenAction::Calculate) => submit(server, req).await,
        Some(ScreenAction::Status) => status(server, required_job_id(&req)?).await,
        Some(ScreenAction::Cancel) => cancel(server, required_job_id(&req)?).await,
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
        stage: "queued".to_string(),
        processed: 0,
        total: 0,
        complete_count: 0,
        partial_count: 0,
        unavailable_count: 0,
        model_sensitive_count: 0,
        heartbeat_at: None,
        cancel_requested: false,
        checkpoint: None,
        artifact_path: None,
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
    let heartbeat_store = store.clone();
    let heartbeat_job = job_id.clone();
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let heartbeat = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(error) = heartbeat_store.heartbeat_screen_job(&heartbeat_job) {
                        tracing::warn!(job_id = heartbeat_job, "screen heartbeat failed: {error}");
                    }
                }
                changed = stop_rx.changed() => {
                    if changed.is_err() || *stop_rx.borrow() { break; }
                }
            }
        }
    });
    let outcome = AssertUnwindSafe(run_screen_job(&server, &job_id, &definition, verification))
        .catch_unwind()
        .await;
    if stop_tx.send(true).is_err() {
        tracing::debug!(job_id, "screen heartbeat already stopped");
    }
    if let Err(error) = heartbeat.await {
        tracing::warn!(job_id, "screen heartbeat task failed: {error}");
    }
    let failure = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(payload) => Some(
            payload
                .downcast_ref::<&str>()
                .map(|message| (*message).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic payload".to_string()),
        ),
    };
    if let Some(error) = failure {
        let cancelled = match store.screen_cancel_requested(&job_id) {
            Ok(cancelled) => cancelled,
            Err(store_error) => {
                tracing::warn!(
                    job_id,
                    "failed to read cancellation after screen failure: {store_error}"
                );
                false
            }
        };
        let (status, persisted_error) = if cancelled {
            ("cancelled", None)
        } else {
            ("failed", Some(error.as_str()))
        };
        if let Err(store_error) =
            store.finish_screen_job(&job_id, status, None, persisted_error, None)
        {
            tracing::error!(
                job_id,
                "failed to persist screen terminal state: {store_error}"
            );
        }
    }
}

async fn run_screen_job(
    server: &CompaniesServer,
    job_id: &str,
    definition: &ScreenDefinition,
    verification: Value,
) -> Result<(), McpToolError> {
    server
        .research
        .mark_screen_job_executing(job_id)
        .map_err(crate::map_portfolio_error)?;
    if definition.kind != "expectations_gap" {
        let universe = acquire_universe(server, definition).await?;
        let result = calculate(
            &server.client,
            &server.eodhd_api_key,
            definition,
            verification,
            universe,
        )
        .await?;
        server
            .research
            .finish_screen_job(job_id, "completed", Some(&result), None, None)
            .map_err(crate::map_portfolio_error)?;
        return Ok(());
    }

    let job = load_job(&server.research, job_id)?;
    if job.checkpoint.is_none() {
        let pass_stage = async {
            let universe = acquire_universe(server, definition).await?;
            prepare_expectations_pass_set(
                &server.client,
                &server.eodhd_api_key,
                definition,
                universe,
            )
            .await
        };
        let prepared = tokio::select! {
            result = tokio::time::timeout(SCREEN_PASS_DEADLINE, pass_stage) => {
                result.map_err(|_| {
                    McpToolError::unavailable("financial screen did not complete within 60 seconds")
                })??
            }
            cancellation = wait_for_screen_cancel(&server.research, job_id) => {
                cancellation?;
                server.research.finish_screen_job(job_id, "cancelled", None, None, None)
                    .map_err(crate::map_portfolio_error)?;
                return Ok(());
            }
        };
        let mut items = Vec::with_capacity(prepared.groups.len());
        for (ordinal, group) in prepared.groups.iter().enumerate() {
            let payload = serde_json::to_value(group).map_err(|error| {
                McpToolError::internal(format!("serialize issuer checkpoint: {error}"))
            })?;
            items.push(ScreenJobItemRecord::pending(
                &group.issuer_key,
                ordinal,
                payload,
            ));
        }
        let checkpoint = json!({
            "candidate_count": prepared.candidate_count,
            "financial_passing_security_count": prepared.financial_passing_security_count,
            "exclusions": prepared.exclusions,
            "fx_rates": prepared.fx_rates,
            "logic_verification": verification,
            "pass_set_persisted_at": now_rfc3339(),
        });
        server
            .research
            .persist_screen_pass_set(job_id, &checkpoint, &items)
            .map_err(crate::map_portfolio_error)?;
        server
            .research
            .mark_screen_job_executing(job_id)
            .map_err(crate::map_portfolio_error)?;
    }

    enrich_pending_issuers(server, job_id).await?;
    if server
        .research
        .screen_cancel_requested(job_id)
        .map_err(crate::map_portfolio_error)?
    {
        server
            .research
            .finish_screen_job(job_id, "cancelled", None, None, None)
            .map_err(crate::map_portfolio_error)?;
        return Ok(());
    }

    let job = load_job(&server.research, job_id)?;
    let checkpoint = job
        .checkpoint
        .as_ref()
        .ok_or_else(|| McpToolError::internal("screen checkpoint missing"))?;
    let mut rows: Vec<Value> = server
        .research
        .all_screen_items(job_id)
        .map_err(crate::map_portfolio_error)?
        .into_iter()
        .filter_map(|item| item.row)
        .collect();
    sort_rows(&mut rows, definition);
    let exclusions = checkpoint
        .get("exclusions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let candidate_count = checkpoint
        .get("candidate_count")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    let result = build_calculation_result(
        definition,
        checkpoint
            .get("logic_verification")
            .cloned()
            .unwrap_or(Value::Null),
        ScreenCalculation {
            rows,
            candidate_count,
            exclusions,
        },
    );
    let artifact_name = format!("expectations-gap-{}-{job_id}", definition.as_of);
    let artifact_path =
        crate::tools::artifacts::save_json_artifact("report", &artifact_name, &result)?;
    server
        .research
        .finish_screen_job(
            job_id,
            "completed",
            Some(&result),
            None,
            Some(&artifact_path.to_string_lossy()),
        )
        .map_err(crate::map_portfolio_error)?;
    Ok(())
}

#[cfg(test)]
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
            let message = payload
                .downcast_ref::<&str>()
                .map(|message| (*message).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic payload".to_string());
            (
                "failed",
                None,
                Some(format!("screen calculation panicked: {message}")),
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
    let mut calculation = calculation;
    sort_rows(&mut calculation.rows, definition);
    Ok(build_calculation_result(
        definition,
        verification,
        calculation,
    ))
}

fn build_calculation_result(
    definition: &ScreenDefinition,
    verification: Value,
    calculation: ScreenCalculation,
) -> Value {
    let row_count = calculation.rows.len();
    let excluded_count = calculation.exclusions.len();
    let financial_passing_security_count = calculation
        .rows
        .iter()
        .map(|row| {
            row.get("eligible_lines")
                .and_then(Value::as_array)
                .map_or(1, Vec::len)
        })
        .sum::<usize>();
    let analysis_counts = calculation
        .rows
        .iter()
        .fold(BTreeMap::new(), |mut counts, row| {
            let status = row
                .get("data_quality_status")
                .and_then(Value::as_str)
                .unwrap_or("unclassified");
            *counts.entry(status.to_string()).or_insert(0_usize) += 1;
            counts
        });
    json!({
        "screen_name": definition.name,
        "as_of": definition.as_of,
        "reporting_currency": definition.reporting_currency,
        "metadata": {
            "candidate_count": calculation.candidate_count,
            "issuer_count": row_count,
            "financial_passing_security_count": financial_passing_security_count,
            "excluded_security_count": excluded_count,
            "candidate_securities_reconciled": calculation.candidate_count == financial_passing_security_count + excluded_count,
            "analysis_state_counts": analysis_counts,
            "issuer_states_reconciled": analysis_counts.values().sum::<usize>() == row_count,
            "source": "EODHD Screener API",
            "logic_verification": verification,
        },
        "table": columnar_table(&calculation.rows, definition),
        "exclusions": calculation.exclusions,
    })
}

async fn calculate_expectations_gap(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    definition: &ScreenDefinition,
    universe: Vec<Value>,
) -> Result<ScreenCalculation, McpToolError> {
    let prepared =
        prepare_expectations_pass_set(client, eodhd_api_key, definition, universe).await?;
    let mut rows = Vec::with_capacity(prepared.groups.len());
    for group in prepared.groups {
        let row =
            match analyze_issuer_group(client, eodhd_api_key, &prepared.fx_rates, &group, None)
                .await
            {
                Ok(row) => row,
                Err(reason) => unavailable_issuer_row(&group, &reason),
            };
        rows.push(row);
    }
    Ok(ScreenCalculation {
        rows,
        candidate_count: prepared.candidate_count,
        exclusions: prepared.exclusions,
    })
}

async fn prepare_expectations_pass_set(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    definition: &ScreenDefinition,
    universe: Vec<Value>,
) -> Result<PreparedPassSet, McpToolError> {
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
    let mut ticker_inventory = HashMap::new();
    for outcome in futures::future::join_all(ticker_fetches).await {
        let (exchange, tickers) = outcome?;
        let rows = tickers.as_array().ok_or_else(|| {
            McpToolError::unavailable(format!("{exchange} ticker list is not an array"))
        })?;
        for row in rows {
            if row.get("Type").and_then(Value::as_str) == Some("Common Stock")
                && let Some(code) = row.get("Code").and_then(Value::as_str)
            {
                ticker_inventory.insert(format!("{exchange}:{code}"), row.clone());
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
    let mut fx_rates = HashMap::from([("USD".to_string(), 1.0)]);
    for outcome in futures::future::join_all(fx_fetches).await {
        let (currency, rate) = outcome?;
        fx_rates.insert(currency, rate);
    }

    let mut materialized = Vec::new();
    let mut exclusions = Vec::new();
    for row in universe {
        match materialize_security(
            row,
            &ticker_inventory,
            &fx_rates,
            cap_min,
            cap_max,
            liquidity_min,
        ) {
            Ok(security) => materialized.push(security),
            Err(exclusion) => exclusions.push(exclusion),
        }
    }
    let financial_passing_security_count = materialized.len();
    let groups = group_materialized_securities(materialized);
    Ok(PreparedPassSet {
        groups,
        candidate_count,
        financial_passing_security_count,
        exclusions,
        fx_rates,
    })
}
fn materialize_security(
    row: Value,
    ticker_inventory: &HashMap<String, Value>,
    fx_rates: &HashMap<String, f64>,
    cap_min: f64,
    cap_max: f64,
    liquidity_min: f64,
) -> Result<MaterializedSecurity, Value> {
    let code = row.get("code").and_then(Value::as_str).unwrap_or("");
    let exchange = row.get("exchange").and_then(Value::as_str).unwrap_or("");
    let symbol = format!("{code}.{exchange}");
    let ticker = ticker_inventory
        .get(&format!("{exchange}:{code}"))
        .ok_or_else(|| screen_exclusion(&symbol, "ineligible_security_type", None))?;
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
    let name = ticker
        .get("Name")
        .and_then(Value::as_str)
        .or_else(|| row.get("name").and_then(Value::as_str))
        .unwrap_or(code)
        .to_string();
    let primary_ticker = None;
    let lei = None;
    let isin = ticker
        .get("Isin")
        .or_else(|| ticker.get("ISIN"))
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
    })
}

/// expect: Qualifying home shares and ADRs for one issuer produce one auditable row.
/// [P5] Motivating: issuer-level screening must not rank the same company twice.
/// pre: securities passed the venue, type, capitalization, and liquidity gates.
/// post: shared LEI, primary ticker, ISIN, or non-conflicting normalized name evidence forms one group.
/// [P1] Constraining: conflicting LEIs are never merged through the name fallback.
fn disjoint_root(parents: &mut [usize], index: usize) -> usize {
    if parents[index] != index {
        parents[index] = disjoint_root(parents, parents[index]);
    }
    parents[index]
}

fn merge_disjoint(parents: &mut [usize], left: usize, right: usize) {
    let left_root = disjoint_root(parents, left);
    let right_root = disjoint_root(parents, right);
    if left_root != right_root {
        parents[right_root] = left_root;
    }
}

fn group_materialized_securities(materialized: Vec<MaterializedSecurity>) -> Vec<IssuerGroup> {
    if materialized
        .iter()
        .all(|security| security.lei.is_none() && security.primary_ticker.is_none())
    {
        let mut parents: Vec<usize> = (0..materialized.len()).collect();
        let mut identity_owner: HashMap<String, usize> = HashMap::new();
        for (index, security) in materialized.iter().enumerate() {
            let mut identities = vec![format!("name:{}", security.normalized_issuer_name)];
            if let Some(isin) = &security.isin {
                identities.push(format!("isin:{isin}"));
            }
            for identity in identities {
                if let Some(owner) = identity_owner.insert(identity, index) {
                    merge_disjoint(&mut parents, index, owner);
                }
            }
        }
        let mut groups: BTreeMap<usize, Vec<MaterializedSecurity>> = BTreeMap::new();
        for (index, security) in materialized.into_iter().enumerate() {
            let root = disjoint_root(&mut parents, index);
            groups.entry(root).or_default().push(security);
        }
        return groups.into_values().map(finalize_issuer_group).collect();
    }
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

fn finalize_issuer_group(mut securities: Vec<MaterializedSecurity>) -> IssuerGroup {
    securities.sort_by(|left, right| left.symbol.cmp(&right.symbol));
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
        issuer_key_provenance: issuer_key_provenance.to_string(),
        issuer_identity_provenance: issuer_identity_provenance.to_string(),
        securities,
    }
}

async fn analyze_issuer_group(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    fx_rates: &HashMap<String, f64>,
    issuer_group: &IssuerGroup,
    fundamentals: Option<Value>,
) -> Result<Value, String> {
    let group = &issuer_group.securities;
    let actionable = group
        .iter()
        .max_by(|left, right| {
            left.average_daily_dollar_volume_usd
                .partial_cmp(&right.average_daily_dollar_volume_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .ok_or_else(|| "issuer group is empty".to_string())?;
    let analysis_symbol = actionable.symbol.as_str();
    let fundamentals = match fundamentals {
        Some(fundamentals) => fundamentals,
        None => providers::fetch_eodhd_fundamentals(client, eodhd_api_key, analysis_symbol)
            .await
            .map_err(|error| error.to_string())?,
    };
    let primary = fundamentals
        .pointer("/General/PrimaryTicker")
        .and_then(Value::as_str)
        .unwrap_or(analysis_symbol);
    let income = providers::normalize_eodhd("income_statement", &fundamentals, analysis_symbol);
    let balance = providers::normalize_eodhd("balance_sheet", &fundamentals, analysis_symbol);
    let cash_flow =
        providers::normalize_eodhd("cash_flow_statement", &fundamentals, analysis_symbol);
    let metrics = providers::normalize_eodhd("key_metrics", &fundamentals, analysis_symbol);
    let profile_value =
        providers::normalize_eodhd("company_profile", &fundamentals, analysis_symbol);
    let profile = crate::CompanyProfile::from_response(providers::ProviderResponse {
        value: profile_value,
        provider: crate::Provider::Eodhd,
        warnings: Vec::new(),
    });
    let raw_price = actionable.adjusted_close;
    let listing_currency_symbol = Some(actionable.currency_symbol.as_str());
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
        "analysis_symbol": analysis_symbol,
        "reported_primary_ticker": primary,
        "price_source": "EODHD screener adjusted_close",
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
        analysis_symbol,
        &analysis,
        &[],
        0.05,
        &[],
        0,
        "EODHD as-of close; single fundamentals payload",
    );
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
    let unavailable_reason = (gap_status == "unavailable").then_some(
        "neither requested gap leg could be solved within validated model bounds or available inputs",
    );
    let data_quality_status = if capability_model_sensitive {
        "model_sensitive"
    } else {
        gap_status
    };
    Ok(json!({
        "company": actionable.name,
        "issuer_key": issuer_group.issuer_key,
        "issuer_key_provenance": issuer_group.issuer_key_provenance,
        "issuer_identity_provenance": issuer_group.issuer_identity_provenance,
        "primary_ticker": primary,
        "analysis_symbol": analysis_symbol,
        "analysis_line_reason": "highest-liquidity eligible line supplies one issuer-level fundamentals request and the price-implied analysis",
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
        "unavailable_reason": unavailable_reason,
        "capability_quality_flags": capability_flags,
        "price_currency_normalization_provenance": price_currency_normalization_provenance,
        "expectations_report": report,
    }))
}

fn unavailable_issuer_row(group: &IssuerGroup, reason: &str) -> Value {
    let actionable = group.securities.iter().max_by(|left, right| {
        left.average_daily_dollar_volume_usd
            .partial_cmp(&right.average_daily_dollar_volume_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    json!({
        "company": actionable.map(|security| security.name.as_str()),
        "issuer_key": group.issuer_key,
        "issuer_key_provenance": group.issuer_key_provenance,
        "issuer_identity_provenance": group.issuer_identity_provenance,
        "actionable_symbol": actionable.map(|security| security.symbol.as_str()),
        "eligible_symbols": group.securities.iter().map(|security| security.symbol.clone()).collect::<Vec<_>>(),
        "eligible_lines": group.securities.iter().map(|security| json!({
            "symbol": security.symbol,
            "venue": symbol_venue(&security.symbol),
            "market_capitalization_usd": security.market_capitalization_usd,
            "average_daily_dollar_volume_usd": security.average_daily_dollar_volume_usd,
            "listing_currency_symbol": security.currency_symbol,
        })).collect::<Vec<_>>(),
        "gap_data_status": "unavailable",
        "data_quality_status": "unavailable",
        "unavailable_reason": reason,
    })
}

async fn wait_for_screen_cancel(store: &ResearchStore, job_id: &str) -> Result<(), McpToolError> {
    loop {
        if store
            .screen_cancel_requested(job_id)
            .map_err(crate::map_portfolio_error)?
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn fetch_bulk_fundamentals(
    server: &CompaniesServer,
    groups: &[IssuerGroup],
) -> HashMap<String, Result<Value, String>> {
    let mut by_exchange: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for group in groups {
        let actionable = group.securities.iter().max_by(|left, right| {
            left.average_daily_dollar_volume_usd
                .partial_cmp(&right.average_daily_dollar_volume_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some(security) = actionable {
            let exchange = security
                .symbol
                .rsplit_once('.')
                .map_or("US", |(_, exchange)| exchange);
            by_exchange
                .entry(exchange.to_string())
                .or_default()
                .push((group.issuer_key.clone(), security.symbol.clone()));
        }
    }
    let mut batches = Vec::new();
    for (exchange, issuers) in by_exchange {
        for chunk in issuers.chunks(500) {
            batches.push((exchange.clone(), chunk.to_vec()));
        }
    }
    let outcomes = futures::stream::iter(batches.into_iter().map(|(exchange, issuers)| {
        let client = server.client.clone();
        let api_key = server.eodhd_api_key.clone();
        async move {
            let symbols: Vec<String> = issuers.iter().map(|(_, symbol)| symbol.clone()).collect();
            let result =
                providers::fetch_eodhd_bulk_fundamentals(&client, &api_key, &exchange, &symbols)
                    .await;
            (issuers, result)
        }
    }))
    .buffer_unordered(8)
    .collect::<Vec<_>>()
    .await;

    let mut fundamentals = HashMap::new();
    for (issuers, outcome) in outcomes {
        match outcome {
            Ok(value) => {
                let Some(rows) = value.as_array() else {
                    for (issuer_key, _) in issuers {
                        fundamentals.insert(
                            issuer_key,
                            Err("bulk fundamentals response is not an array".to_string()),
                        );
                    }
                    continue;
                };
                let by_code: HashMap<&str, &Value> = rows
                    .iter()
                    .filter_map(|row| {
                        row.pointer("/General/Code")
                            .and_then(Value::as_str)
                            .map(|code| (code, row))
                    })
                    .collect();
                for (issuer_key, symbol) in issuers {
                    let code = symbol
                        .split_once('.')
                        .map_or(symbol.as_str(), |(code, _)| code);
                    let result = by_code
                        .get(code)
                        .map(|row| (*row).clone())
                        .ok_or_else(|| format!("bulk fundamentals omitted {symbol}"));
                    fundamentals.insert(issuer_key, result);
                }
            }
            Err(error) => {
                let reason = error.to_string();
                for (issuer_key, _) in issuers {
                    fundamentals.insert(issuer_key, Err(reason.clone()));
                }
            }
        }
    }
    fundamentals
}

async fn enrich_pending_issuers(
    server: &CompaniesServer,
    job_id: &str,
) -> Result<(), McpToolError> {
    let pending = server
        .research
        .pending_screen_items(job_id)
        .map_err(crate::map_portfolio_error)?;
    let job = load_job(&server.research, job_id)?;
    let fx_rates: HashMap<String, f64> = serde_json::from_value(
        job.checkpoint
            .as_ref()
            .and_then(|value| value.get("fx_rates"))
            .cloned()
            .unwrap_or(Value::Null),
    )
    .map_err(|error| McpToolError::internal(format!("invalid FX checkpoint: {error}")))?;
    let mut decoded = Vec::with_capacity(pending.len());
    for item in pending {
        let group: IssuerGroup = serde_json::from_value(item.payload.clone()).map_err(|error| {
            McpToolError::internal(format!("decode issuer checkpoint: {error}"))
        })?;
        decoded.push((item, group));
    }
    let groups: Vec<IssuerGroup> = decoded.iter().map(|(_, group)| group.clone()).collect();
    let mut bulk = fetch_bulk_fundamentals(server, &groups).await;
    let bulk_available = bulk.values().any(Result::is_ok);
    let concurrency = if bulk_available {
        BULK_ANALYSIS_CONCURRENCY
    } else {
        FALLBACK_ENRICHMENT_CONCURRENCY
    };
    let work = futures::stream::iter(decoded.into_iter().map(|(item, group)| {
        let client = server.client.clone();
        let api_key = server.eodhd_api_key.clone();
        let store = server.research.clone();
        let fx_rates = fx_rates.clone();
        let job_id = job_id.to_string();
        let fundamentals = bulk
            .remove(&item.issuer_key)
            .unwrap_or_else(|| Err("bulk fundamentals result missing issuer".to_string()));
        async move {
            if store.screen_cancel_requested(&job_id)? {
                return Ok::<(), crate::research_store::PortfolioError>(());
            }
            let fundamentals = match fundamentals {
                Ok(fundamentals) => Ok(fundamentals),
                Err(bulk_reason) => {
                    let actionable = group.securities.iter().max_by(|left, right| {
                        left.average_daily_dollar_volume_usd
                            .partial_cmp(&right.average_daily_dollar_volume_usd)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    match actionable {
                        Some(security) => match tokio::time::timeout(
                            FALLBACK_ISSUER_TIMEOUT,
                            providers::fetch_eodhd_fundamentals(&client, &api_key, &security.symbol),
                        ).await {
                            Ok(Ok(fundamentals)) => Ok(fundamentals),
                            Ok(Err(error)) => Err(format!("bulk unavailable ({bulk_reason}); single-symbol fallback failed: {error}")),
                            Err(_) => Err(format!("bulk unavailable ({bulk_reason}); single-symbol fallback exceeded 15 seconds")),
                        },
                        None => Err("issuer group has no actionable security".to_string()),
                    }
                }
            };
            let (row, error) = match fundamentals {
                Ok(fundamentals) => match analyze_issuer_group(
                    &client, &api_key, &fx_rates, &group, Some(fundamentals),
                ).await {
                    Ok(row) => (row, None),
                    Err(reason) => (unavailable_issuer_row(&group, &reason), Some(reason)),
                },
                Err(reason) => (unavailable_issuer_row(&group, &reason), Some(reason)),
            };
            let classification = row
                .get("data_quality_status")
                .and_then(Value::as_str)
                .unwrap_or("unavailable");
            store.complete_screen_item(
                &job_id,
                &item.issuer_key,
                classification,
                &row,
                error.as_deref(),
            )
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>();
    tokio::pin!(work);
    let deadline = tokio::time::sleep(ENRICHMENT_DEADLINE);
    tokio::pin!(deadline);
    let mut cancellation_poll = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            outcomes = &mut work => {
                for outcome in outcomes { outcome.map_err(crate::map_portfolio_error)?; }
                return Ok(());
            }
            _ = cancellation_poll.tick() => {
                if server.research.screen_cancel_requested(job_id).map_err(crate::map_portfolio_error)? {
                    return Ok(());
                }
            }
            _ = &mut deadline => {
                for item in server.research.pending_screen_items(job_id).map_err(crate::map_portfolio_error)? {
                    let group: IssuerGroup = serde_json::from_value(item.payload)
                        .map_err(|error| McpToolError::internal(format!("decode timed-out issuer checkpoint: {error}")))?;
                    let row = unavailable_issuer_row(&group, "enrichment deadline exceeded");
                    server.research.complete_screen_item(
                        job_id, &item.issuer_key, "unavailable", &row, Some("enrichment deadline exceeded"),
                    ).map_err(crate::map_portfolio_error)?;
                }
                return Ok(());
            }
        }
    }
}

fn screen_exclusion(symbol: &str, reason: &str, detail: Option<&str>) -> Value {
    json!({"symbol":symbol,"reason":reason,"detail":detail})
}

async fn status(server: &CompaniesServer, job_id: &str) -> Result<Value, McpToolError> {
    let job = load_job(&server.research, job_id)?;
    let now = chrono::Utc::now();
    let elapsed_seconds = chrono::DateTime::parse_from_rfc3339(&job.created_at)
        .ok()
        .map(|created| {
            (now - created.with_timezone(&chrono::Utc))
                .num_seconds()
                .max(0)
        });
    let heartbeat_age_seconds = job
        .heartbeat_at
        .as_deref()
        .and_then(|heartbeat| chrono::DateTime::parse_from_rfc3339(heartbeat).ok())
        .map(|heartbeat| {
            (now - heartbeat.with_timezone(&chrono::Utc))
                .num_seconds()
                .max(0)
        });
    let enrichment_elapsed_seconds = job
        .checkpoint
        .as_ref()
        .and_then(|checkpoint| checkpoint.get("pass_set_persisted_at"))
        .and_then(Value::as_str)
        .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|started| {
            (now - started.with_timezone(&chrono::Utc))
                .num_seconds()
                .max(0)
        });
    let eta_seconds = enrichment_elapsed_seconds.and_then(|stage_elapsed| {
        (job.processed > 0 && job.processed < job.total).then(|| {
            let remaining = i64::try_from(job.total - job.processed).unwrap_or(i64::MAX);
            let processed = i64::try_from(job.processed).unwrap_or(1);
            let throughput_eta = stage_elapsed.saturating_mul(remaining) / processed;
            let deadline_remaining = i64::try_from(ENRICHMENT_DEADLINE.as_secs())
                .unwrap_or(i64::MAX)
                .saturating_sub(stage_elapsed)
                .max(0);
            throughput_eta.min(deadline_remaining)
        })
    });
    if job.status == "queued" && job.stage != "queued" {
        let definition: ScreenDefinition =
            serde_json::from_value(job.definition.clone()).map_err(|error| {
                McpToolError::internal(format!("invalid persisted screen definition: {error}"))
            })?;
        let verification = verify_assertions(&definition)?;
        let server = server.clone();
        let job_id = job_id.to_string();
        drop(tokio::spawn(async move {
            calculate_job(server, job_id, definition, verification).await;
        }));
    }
    Ok(json!({
        "job_id": job.id,
        "status": job.status,
        "stage": job.stage,
        "processed": job.processed,
        "total": job.total,
        "complete_count": job.complete_count,
        "partial_count": job.partial_count,
        "unavailable_count": job.unavailable_count,
        "model_sensitive_count": job.model_sensitive_count,
        "heartbeat_at": job.heartbeat_at,
        "heartbeat_age_seconds": heartbeat_age_seconds,
        "elapsed_seconds": elapsed_seconds,
        "eta_seconds": eta_seconds,
        "cancel_requested": job.cancel_requested,
        "artifact_path": job.artifact_path,
        "error": job.error,
        "created_at": job.created_at,
        "updated_at": job.updated_at,
    }))
}

async fn cancel(server: &CompaniesServer, job_id: &str) -> Result<Value, McpToolError> {
    let accepted = server
        .research
        .request_screen_cancel(job_id)
        .map_err(crate::map_portfolio_error)?;
    Ok(
        json!({"job_id": job_id, "status": if accepted { "cancelling" } else { "unchanged" }, "accepted": accepted}),
    )
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
    match (growth_gap.is_number(), profitability_gap.is_number()) {
        (true, true) => "complete",
        (true, false) | (false, true) => "partial",
        (false, false) => "unavailable",
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
    if cursor > 0 {
        result["exclusions"] = json!([]);
        result["exclusions_page"] = json!("omitted_after_first_page");
    }
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
        assert_eq!(gap_data_status(&Value::Null, &Value::Null), "unavailable");
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

    #[test]
    fn later_result_pages_do_not_repeat_full_exclusions() {
        let result = json!({
            "exclusions": [{"symbol":"DROP.US","reason":"below_liquidity_minimum"}],
            "table": {
                "row_count": 2,
                "row_ids": ["A.US", "B.US"],
                "columns": {"company": {"values": ["A", "B"]}}
            }
        });
        let page = paginate_result(result, 1, 1).expect("second page");
        assert_eq!(page["exclusions"], json!([]));
        assert_eq!(page["exclusions_page"], json!("omitted_after_first_page"));
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
            stage: "queued".to_string(),
            processed: 0,
            total: 0,
            complete_count: 0,
            partial_count: 0,
            unavailable_count: 0,
            model_sensitive_count: 0,
            heartbeat_at: None,
            cancel_requested: false,
            checkpoint: None,
            artifact_path: None,
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

    /// expect: Distinct listing names that share an ISIN produce one durable issuer key.
    #[test]
    fn preliminary_identity_union_prevents_duplicate_durable_keys() {
        let make_security = |symbol: &str, name: &str, isin: &str| MaterializedSecurity {
            symbol: symbol.to_string(),
            name: name.to_string(),
            market_capitalization_usd: 10_000_000_000.0,
            average_daily_dollar_volume_usd: 2_000_000.0,
            adjusted_close: 20.0,
            currency_symbol: "USD".to_string(),
            issuer_key: format!("isin:{isin}"),
            lei: None,
            primary_ticker: None,
            isin: Some(isin.to_string()),
            normalized_issuer_name: normalize_name(name),
        };
        let groups = group_materialized_securities(vec![
            make_security("ACME.US", "Acme Inc", "US0000000001"),
            make_security("ACM.A.TO", "Acme Holdings", "US0000000001"),
        ]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].issuer_key, "isin:US0000000001");
        assert_eq!(groups[0].securities.len(), 2);
    }

    /// expect: Production-shaped deterministic issuer grouping preserves all passers without top-N truncation.
    #[test]
    fn production_shaped_grouping_preserves_cardinality_within_stage_budget() {
        let started = std::time::Instant::now();
        let securities: Vec<MaterializedSecurity> = (0..3_224)
            .map(|index| {
                let issuer = index % 1_270;
                MaterializedSecurity {
                    symbol: format!("S{index}.US"),
                    name: format!("Issuer {issuer}"),
                    market_capitalization_usd: 10_000_000_000.0,
                    average_daily_dollar_volume_usd: 2_000_000.0,
                    adjusted_close: 20.0,
                    currency_symbol: "USD".to_string(),
                    issuer_key: format!("name:issuer{issuer}"),
                    lei: None,
                    primary_ticker: None,
                    isin: None,
                    normalized_issuer_name: format!("issuer{issuer}"),
                }
            })
            .collect();
        let groups = group_materialized_securities(securities);
        assert_eq!(groups.len(), 1_270);
        assert_eq!(
            groups
                .iter()
                .map(|group| group.securities.len())
                .sum::<usize>(),
            3_224
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// expect: Enrichment failure retains every financially qualified security in an explicit unavailable issuer row.
    #[test]
    fn unavailable_issuer_row_preserves_the_financial_pass_set() {
        let group = IssuerGroup {
            issuer_key: "name:acme".to_string(),
            issuer_key_provenance: "normalized_name".to_string(),
            issuer_identity_provenance: "normalized_name_fallback".to_string(),
            securities: vec![MaterializedSecurity {
                symbol: "ACME.US".to_string(),
                name: "Acme".to_string(),
                market_capitalization_usd: 10_000_000_000.0,
                average_daily_dollar_volume_usd: 2_000_000.0,
                adjusted_close: 20.0,
                currency_symbol: "USD".to_string(),
                issuer_key: "name:acme".to_string(),
                lei: None,
                primary_ticker: None,
                isin: None,
                normalized_issuer_name: "acme".to_string(),
            }],
        };
        let row = unavailable_issuer_row(&group, "fundamentals unavailable");
        assert_eq!(row["data_quality_status"], json!("unavailable"));
        assert_eq!(row["eligible_symbols"], json!(["ACME.US"]));
        assert_eq!(row["unavailable_reason"], json!("fundamentals unavailable"));
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
