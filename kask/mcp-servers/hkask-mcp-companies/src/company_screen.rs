use crate::{CompaniesServer, providers, tools::notes::run_store, types};
use chrono::{Datelike as _, NaiveDate, Weekday};
use futures::StreamExt as _;
use hkask_mcp_server::server::McpToolError;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

const DEFAULT_PAGE_SIZE: u32 = 50;
const QUALIFICATION_CONCURRENCY: usize = 8;

/// Start a frozen EODHD candidate snapshot or advance one bounded page of an
/// existing run. All mutable traversal state is persisted before returning.
pub(crate) async fn advance(
    server: &CompaniesServer,
    req: types::ExpectationsScreenRequest,
) -> Result<Value, McpToolError> {
    match req.run_id {
        Some(run_id) => advance_existing(server, &run_id, req.page_size).await,
        None => start(server, req).await,
    }
}

async fn start(
    server: &CompaniesServer,
    req: types::ExpectationsScreenRequest,
) -> Result<Value, McpToolError> {
    let as_of = req.as_of.ok_or_else(|| {
        McpToolError::invalid_argument("as_of is required when starting a screen")
    })?;
    let as_of_date = NaiveDate::parse_from_str(&as_of, "%Y-%m-%d").map_err(|error| {
        McpToolError::invalid_argument(format!("as_of must be YYYY-MM-DD: {error}"))
    })?;
    let exchanges = req.exchanges.ok_or_else(|| {
        McpToolError::invalid_argument("exchanges are required when starting a screen")
    })?;
    if exchanges.is_empty() {
        return Err(McpToolError::invalid_argument(
            "at least one exchange is required",
        ));
    }
    let cap_min = required_positive(req.market_cap_min_usd, "market_cap_min_usd")?;
    let cap_max = required_positive(req.market_cap_max_usd, "market_cap_max_usd")?;
    if cap_max < cap_min {
        return Err(McpToolError::invalid_argument(
            "market_cap_max_usd must be greater than or equal to market_cap_min_usd",
        ));
    }
    let liquidity_min = required_positive(req.liquidity_min_usd, "liquidity_min_usd")?;
    let window_days = req.liquidity_window_days.ok_or_else(|| {
        McpToolError::invalid_argument("liquidity_window_days is required when starting a screen")
    })?;
    if window_days == 0 {
        return Err(McpToolError::invalid_argument(
            "liquidity_window_days must be greater than zero",
        ));
    }
    let page_size = req.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if page_size == 0 {
        return Err(McpToolError::invalid_argument(
            "page_size must be greater than zero",
        ));
    }
    let window_start = as_of_date
        .checked_sub_signed(chrono::Duration::days(i64::from(window_days)))
        .ok_or_else(|| McpToolError::invalid_argument("liquidity window underflows calendar"))?;

    // Reuse the existing, tested listing screener exactly once. It owns
    // exchange fan-out, mixed-currency market-cap conversion, instrument
    // filtering, and the listing-level USD cap field.
    let screen_output = server
        .company_screener(Parameters(types::ScreenerRequest {
            prompt: String::new(),
            limit: u32::MAX,
            criteria_overrides: hkask_types::AnyJsonValue(json!({
                "exchanges": exchanges,
                "market_capitalization_min": cap_min,
                "market_capitalization_max": cap_max,
            })),
        }))
        .await?;
    let screen_output: Value = serde_json::from_str(&screen_output).map_err(|error| {
        McpToolError::internal(format!("parse company_screener response: {error}"))
    })?;
    let screen_output = hkask_types::tool_response::unwrap_tool_envelope(screen_output);
    let candidates = screen_output
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| McpToolError::internal("company_screener response has no results array"))?;

    let calendar_fetches = exchanges.iter().map(|exchange| async move {
        let details = providers::fetch_eodhd_exchange_details(
            &server.client,
            &server.eodhd_api_key,
            exchange,
        )
        .await?;
        let sessions = exchange_sessions(&details, window_start, as_of_date)?;
        Ok::<_, McpToolError>((exchange.clone(), sessions))
    });
    let mut sessions_by_exchange = Map::new();
    for outcome in futures::future::join_all(calendar_fetches).await {
        let (exchange, sessions) = outcome?;
        sessions_by_exchange.insert(exchange, json!(sessions));
    }

    let run_id = uuid::Uuid::new_v4().to_string();
    let state = json!({
        "run_id": run_id,
        "phase": "qualify_listings",
        "as_of": as_of,
        "window_start": window_start.format("%Y-%m-%d").to_string(),
        "criteria": {
            "exchanges": exchanges,
            "market_cap_min_usd": cap_min,
            "market_cap_max_usd": cap_max,
            "liquidity_min_usd": liquidity_min,
            "liquidity_window_days": window_days,
            "page_size": page_size,
        },
        "candidate_count": candidates.len(),
        "candidate_cursor": 0,
        "candidates": candidates,
        "sessions_by_exchange": Value::Object(sessions_by_exchange),
        "qualified_listings": [],
        "excluded_listings": [],
        "issuer_results": [],
    });
    save(server, &run_id, &state).await?;
    Ok(state)
}

async fn advance_existing(
    server: &CompaniesServer,
    run_id: &str,
    requested_page_size: Option<u32>,
) -> Result<Value, McpToolError> {
    let mut state = load(server, run_id).await?;
    if state.get("phase").and_then(Value::as_str) != Some("qualify_listings") {
        return Ok(state);
    }
    let cursor = state
        .get("candidate_cursor")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| McpToolError::internal("screen state has invalid candidate_cursor"))?;
    let configured_page_size = state
        .pointer("/criteria/page_size")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| McpToolError::internal("screen state has invalid page_size"))?;
    let page_size = match requested_page_size {
        Some(0) => {
            return Err(McpToolError::invalid_argument(
                "page_size must be greater than zero",
            ));
        }
        Some(value) => usize::try_from(value)
            .map_err(|_| McpToolError::invalid_argument("page_size exceeds platform range"))?,
        None => configured_page_size,
    };
    let candidates = state
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or_else(|| McpToolError::internal("screen state has no candidates array"))?;
    let page: Vec<Value> = candidates
        .iter()
        .skip(cursor)
        .take(page_size)
        .cloned()
        .collect();
    let next_cursor = cursor
        .checked_add(page.len())
        .ok_or_else(|| McpToolError::internal("candidate cursor overflow"))?;
    let minimum_usd = state
        .pointer("/criteria/liquidity_min_usd")
        .and_then(Value::as_f64)
        .ok_or_else(|| McpToolError::internal("screen state has no liquidity minimum"))?;
    let window_start = state
        .get("window_start")
        .and_then(Value::as_str)
        .ok_or_else(|| McpToolError::internal("screen state has no window_start"))?
        .to_string();
    let as_of = state
        .get("as_of")
        .and_then(Value::as_str)
        .ok_or_else(|| McpToolError::internal("screen state has no as_of"))?
        .to_string();
    let sessions_by_exchange = state
        .get("sessions_by_exchange")
        .and_then(Value::as_object)
        .ok_or_else(|| McpToolError::internal("screen state has no exchange sessions"))?;

    let outcomes = futures::stream::iter(page.into_iter().map(|row| {
        let window_start = window_start.clone();
        let as_of = as_of.clone();
        async move {
            qualify_usd_listing(
                server,
                row,
                minimum_usd,
                &window_start,
                &as_of,
                sessions_by_exchange,
            )
            .await
        }
    }))
    .buffered(QUALIFICATION_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;

    let mut qualified = Vec::new();
    let mut excluded = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(Qualification::Qualified(row)) => qualified.push(row),
            Ok(Qualification::Excluded(row)) => excluded.push(row),
            Err(error) => excluded.push(json!({
                "reason": "data_unavailable",
                "detail": error.to_string(),
            })),
        }
    }
    append_rows(&mut state, "qualified_listings", qualified)?;
    append_rows(&mut state, "excluded_listings", excluded)?;
    state["candidate_cursor"] = json!(next_cursor);
    if next_cursor >= candidates.len() {
        state["phase"] = json!("build_issuers");
    }
    save(server, run_id, &state).await?;
    Ok(state)
}

enum Qualification {
    Qualified(Value),
    Excluded(Value),
}

async fn qualify_usd_listing(
    server: &CompaniesServer,
    row: Value,
    minimum_usd: f64,
    from: &str,
    to: &str,
    sessions_by_exchange: &Map<String, Value>,
) -> Result<Qualification, McpToolError> {
    let code = required_row_string(&row, "code")?;
    let exchange = required_row_string(&row, "exchange")?;
    let currency_symbol = required_row_string(&row, "currency_symbol")?;
    let symbol = format!("{code}.{exchange}");
    if currency_symbol != "$" {
        return Ok(Qualification::Excluded(json!({
            "symbol": symbol,
            "reason": "currency_path_pending",
            "currency_symbol": currency_symbol,
        })));
    }
    let sessions: HashSet<String> = sessions_by_exchange
        .get(exchange)
        .and_then(Value::as_array)
        .ok_or_else(|| McpToolError::internal(format!("no sessions for exchange {exchange}")))?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    if sessions.is_empty() {
        return Err(McpToolError::unavailable(format!(
            "exchange {exchange} has no scheduled sessions in the liquidity window"
        )));
    }
    let history = providers::fetch_eodhd_eod_history(
        &server.client,
        &server.eodhd_api_key,
        &symbol,
        from,
        to,
    )
    .await?;
    let (average_usd, observations) = average_usd_turnover(&history, &sessions)?;
    if average_usd < minimum_usd {
        return Ok(Qualification::Excluded(json!({
            "symbol": symbol,
            "reason": "below_liquidity_minimum",
            "average_daily_dollar_volume_usd": average_usd,
            "scheduled_sessions": sessions.len(),
            "observed_sessions": observations,
        })));
    }
    let fundamentals =
        providers::fetch_eodhd_fundamentals(&server.client, &server.eodhd_api_key, &symbol).await?;
    let general = fundamentals
        .get("General")
        .and_then(Value::as_object)
        .ok_or_else(|| McpToolError::unavailable(format!("{symbol} has no EODHD General data")))?;
    if general.get("Type").and_then(Value::as_str) != Some("Common Stock") {
        return Ok(Qualification::Excluded(json!({
            "symbol": symbol,
            "reason": "ineligible_instrument_type",
            "instrument_type": general.get("Type").cloned().unwrap_or(Value::Null),
        })));
    }
    let mut qualified = row;
    let object = qualified
        .as_object_mut()
        .ok_or_else(|| McpToolError::internal("screener candidate is not an object"))?;
    object.insert("symbol".to_string(), json!(symbol));
    object.insert(
        "average_daily_dollar_volume_usd".to_string(),
        json!(average_usd),
    );
    object.insert("scheduled_sessions".to_string(), json!(sessions.len()));
    object.insert("observed_sessions".to_string(), json!(observations));
    for (output, source) in [
        ("instrument_type", "Type"),
        ("issuer_lei", "LEI"),
        ("security_isin", "ISIN"),
        ("primary_ticker", "PrimaryTicker"),
        ("home_category", "HomeCategory"),
        ("other_listings", "Listings"),
    ] {
        object.insert(
            output.to_string(),
            general.get(source).cloned().unwrap_or(Value::Null),
        );
    }
    Ok(Qualification::Qualified(qualified))
}

fn average_usd_turnover(
    history: &Value,
    sessions: &HashSet<String>,
) -> Result<(f64, usize), McpToolError> {
    let rows = history
        .as_array()
        .ok_or_else(|| McpToolError::unavailable("EODHD EOD history is not an array"))?;
    let mut total = 0.0;
    let mut observations = 0_usize;
    for row in rows {
        let Some(date) = row.get("date").and_then(Value::as_str) else {
            continue;
        };
        if !sessions.contains(date) {
            continue;
        }
        let Some(close) = row.get("close").and_then(Value::as_f64) else {
            continue;
        };
        let Some(volume) = row.get("volume").and_then(Value::as_f64) else {
            continue;
        };
        let turnover = close * volume;
        if close.is_finite()
            && close > 0.0
            && volume.is_finite()
            && volume >= 0.0
            && turnover.is_finite()
        {
            total += turnover;
            observations += 1;
        }
    }
    Ok((total / sessions.len() as f64, observations))
}

fn exchange_sessions(
    details: &Value,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<String>, McpToolError> {
    let data = details
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| McpToolError::unavailable("exchange details has no data object"))?;
    let working_days = data
        .pointer("/TradingHours/WorkingDays")
        .and_then(Value::as_str)
        .ok_or_else(|| McpToolError::unavailable("exchange details has no WorkingDays"))?;
    let working_days: HashSet<Weekday> = working_days
        .split(',')
        .filter_map(|day| parse_weekday(day.trim()))
        .collect();
    if working_days.is_empty() {
        return Err(McpToolError::unavailable(
            "exchange details has no recognized working days",
        ));
    }
    let holidays = data.get("ExchangeHolidays").and_then(Value::as_object);
    let mut sessions = Vec::new();
    let mut date = from;
    while date <= to {
        let key = date.format("%Y-%m-%d").to_string();
        let closed = holidays
            .and_then(|values| values.get(&key))
            .and_then(|holiday| holiday.get("Type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| !kind.eq_ignore_ascii_case("EarlyClose"));
        if working_days.contains(&date.weekday()) && !closed {
            sessions.push(key);
        }
        date = date
            .succ_opt()
            .ok_or_else(|| McpToolError::internal("exchange calendar overflow"))?;
    }
    Ok(sessions)
}

fn parse_weekday(value: &str) -> Option<Weekday> {
    match value {
        "Mon" => Some(Weekday::Mon),
        "Tue" => Some(Weekday::Tue),
        "Wed" => Some(Weekday::Wed),
        "Thu" => Some(Weekday::Thu),
        "Fri" => Some(Weekday::Fri),
        "Sat" => Some(Weekday::Sat),
        "Sun" => Some(Weekday::Sun),
        _ => None,
    }
}

fn required_positive(value: Option<f64>, name: &str) -> Result<f64, McpToolError> {
    value
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| {
            McpToolError::invalid_argument(format!("{name} must be positive and finite"))
        })
}

fn required_row_string<'a>(row: &'a Value, key: &str) -> Result<&'a str, McpToolError> {
    row.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| McpToolError::unavailable(format!("screener row has no {key}")))
}

fn append_rows(state: &mut Value, key: &str, rows: Vec<Value>) -> Result<(), McpToolError> {
    state
        .get_mut(key)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| McpToolError::internal(format!("screen state has no {key} array")))?
        .extend(rows);
    Ok(())
}

async fn load(server: &CompaniesServer, run_id: &str) -> Result<Value, McpToolError> {
    let lookup_id = run_id.to_string();
    run_store(server.research.clone(), move |store| {
        store.get_expectations_screen(&lookup_id)
    })
    .await?
    .ok_or_else(|| {
        McpToolError::invalid_argument(format!("expectations screen run {run_id:?} was not found"))
    })
}

async fn save(server: &CompaniesServer, run_id: &str, state: &Value) -> Result<(), McpToolError> {
    let saved_id = run_id.to_string();
    let saved_state = state.clone();
    run_store(server.research.clone(), move |store| {
        store.save_expectations_screen(&saved_id, &saved_state)
    })
    .await
}
