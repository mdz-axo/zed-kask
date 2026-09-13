use crate::{CompaniesServer, providers, tools::notes::run_store, types};
use chrono::{Datelike as _, NaiveDate, Weekday};
use futures::StreamExt as _;
use hkask_mcp_server::server::McpToolError;
use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

const QUALIFICATION_CONCURRENCY: usize = 8;
const EXPECTATIONS_GAP_TEMPLATE: &str =
    include_str!("../../../registry/templates/company-screen/expectations_gap.j2");

#[derive(Deserialize)]
struct ScreenPlan {
    as_of: String,
    exchanges: Vec<String>,
    market_cap_min_usd: f64,
    market_cap_max_usd: f64,
    liquidity_min_usd: f64,
    liquidity_window_days: u32,
    page_size: u32,
    stages: Vec<String>,
}

/// Start a frozen EODHD candidate snapshot or advance one bounded page of an
/// existing run. All mutable traversal state is persisted before returning.
pub(crate) async fn advance(
    server: &CompaniesServer,
    req: types::ScreenerRequest,
) -> Result<Value, McpToolError> {
    let state = match req.run_id.clone() {
        Some(run_id) => advance_existing(server, &run_id, Some(req.limit)).await?,
        None => start(server, req).await?,
    };
    Ok(public_view(state))
}

async fn start(
    server: &CompaniesServer,
    req: types::ScreenerRequest,
) -> Result<Value, McpToolError> {
    let composition = req.composition.as_deref().ok_or_else(|| {
        McpToolError::invalid_argument("composition is required when starting a composed screen")
    })?;
    let plan = render_screen_plan(composition, &req)?;
    let ScreenPlan {
        as_of,
        exchanges,
        market_cap_min_usd: cap_min,
        market_cap_max_usd: cap_max,
        liquidity_min_usd: liquidity_min,
        liquidity_window_days: window_days,
        page_size,
        stages,
    } = plan;
    let as_of_date = NaiveDate::parse_from_str(&as_of, "%Y-%m-%d").map_err(|error| {
        McpToolError::invalid_argument(format!("rendered as_of must be YYYY-MM-DD: {error}"))
    })?;
    if exchanges.is_empty()
        || !cap_min.is_finite()
        || cap_min <= 0.0
        || !cap_max.is_finite()
        || cap_max < cap_min
        || !liquidity_min.is_finite()
        || liquidity_min <= 0.0
        || window_days == 0
        || page_size == 0
    {
        return Err(McpToolError::invalid_argument(
            "rendered screen plan has invalid bounds, exchanges, window, or page size",
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
            composition: None,
            run_id: None,
            as_of: None,
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

    let mut currencies: Vec<String> = candidates
        .iter()
        .filter_map(|row| {
            row.get("currency_symbol")
                .and_then(Value::as_str)
                .and_then(currency_spec)
                .map(|(currency, _)| currency.to_string())
        })
        .filter(|currency| currency != "USD")
        .collect();
    currencies.sort();
    currencies.dedup();
    let window_start_text = window_start.format("%Y-%m-%d").to_string();
    let fx_fetches = currencies.iter().cloned().map(|currency| {
        let symbol = format!("USD{currency}.FOREX");
        let from = window_start_text.clone();
        let to = as_of.clone();
        async move {
            let history = providers::fetch_eodhd_eod_history(
                &server.client,
                &server.eodhd_api_key,
                &symbol,
                &from,
                &to,
            )
            .await?;
            Ok::<_, McpToolError>((currency, history))
        }
    });
    let mut fx_by_currency = Map::new();
    for outcome in futures::future::join_all(fx_fetches).await {
        let (currency, history) = outcome?;
        fx_by_currency.insert(currency, history);
    }

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
            "stages": stages,
        },
        "candidate_count": candidates.len(),
        "candidate_cursor": 0,
        "candidates": candidates,
        "sessions_by_exchange": Value::Object(sessions_by_exchange),
        "fx_by_currency": Value::Object(fx_by_currency),
        "qualified_listings": [],
        "excluded_listings": [],
        "latest_qualified_listings": [],
        "latest_excluded_listings": [],
        "issuer_results": [],
        "excluded_issuers": [],
        "ranked_companies": [],
        "partial_companies": [],
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
    match state.get("phase").and_then(Value::as_str) {
        Some("analyze_issuers") => {
            return advance_issuers(server, run_id, state, requested_page_size).await;
        }
        Some("complete") => return Ok(state),
        Some("qualify_listings") => {}
        Some(other) => {
            return Err(McpToolError::internal(format!(
                "company screen run has unknown phase {other:?}"
            )));
        }
        None => return Err(McpToolError::internal("company screen run has no phase")),
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
    let candidate_count = candidates.len();
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
    let fx_by_currency = state
        .get("fx_by_currency")
        .and_then(Value::as_object)
        .ok_or_else(|| McpToolError::internal("screen state has no FX histories"))?;

    let outcomes = futures::stream::iter(page.into_iter().map(|row| {
        let window_start = window_start.clone();
        let as_of = as_of.clone();
        async move {
            qualify_listing(
                server,
                row,
                minimum_usd,
                &window_start,
                &as_of,
                sessions_by_exchange,
                fx_by_currency,
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
    state["latest_qualified_listings"] = json!(qualified);
    state["latest_excluded_listings"] = json!(excluded);
    let qualified = state["latest_qualified_listings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let excluded = state["latest_excluded_listings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    append_rows(&mut state, "qualified_listings", qualified)?;
    append_rows(&mut state, "excluded_listings", excluded)?;
    state["candidate_cursor"] = json!(next_cursor);
    if next_cursor >= candidate_count {
        let qualified = state
            .get("qualified_listings")
            .and_then(Value::as_array)
            .ok_or_else(|| McpToolError::internal("screen state has no qualified listings"))?;
        state["issuers"] = json!(build_issuers(qualified));
        state["issuer_cursor"] = json!(0);
        state["phase"] = json!("analyze_issuers");
    }
    save(server, run_id, &state).await?;
    Ok(state)
}

async fn analyze_issuer(
    server: &CompaniesServer,
    issuer: Value,
    as_of: &str,
) -> Result<Value, McpToolError> {
    let primary = issuer
        .get("primary_ticker")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| McpToolError::unavailable("issuer has no EODHD PrimaryTicker"))?;
    let fundamentals = load_fundamentals(server, primary).await?;
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
    let as_of_date = NaiveDate::parse_from_str(as_of, "%Y-%m-%d").map_err(|error| {
        McpToolError::internal(format!("stored as_of date is invalid: {error}"))
    })?;
    let price_from = as_of_date
        .checked_sub_signed(chrono::Duration::days(7))
        .ok_or_else(|| McpToolError::internal("primary price window underflows calendar"))?
        .format("%Y-%m-%d")
        .to_string();
    let price_history = providers::fetch_eodhd_eod_history(
        &server.client,
        &server.eodhd_api_key,
        primary,
        &price_from,
        as_of,
    )
    .await?;
    let raw_price = latest_close(&price_history);
    let mut price_source = "EODHD EOD as-of price unavailable".to_string();
    let analysis = if let Some(raw_price) = raw_price {
        match server
            .normalize_price_for_financials(raw_price, profile.raw(), &income)
            .await
        {
            Ok((price, normalization)) => {
                price_source = format!("EODHD EOD as-of; {normalization}");
                crate::tools::expectations::solve_expectations(
                    &income, &balance, &cash_flow, &metrics, &profile, price,
                )
            }
            Err(error) => {
                price_source = format!("EODHD EOD as-of; currency_normalization_failed: {error}");
                None
            }
        }
    } else {
        None
    };
    let mut report = crate::tools::expectations::build_gap_report(
        primary,
        &analysis,
        &[],
        0.05,
        &[],
        0,
        &price_source,
    );
    report["data_quality"]["financial_source"] = json!("EODHD single fundamentals payload");
    report["data_quality"]["research_status"] = json!("not_run_in_screen");
    let eligible_lines = issuer
        .get("eligible_lines")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| McpToolError::internal("issuer has no eligible lines"))?;
    let actionable_symbol = eligible_lines
        .iter()
        .max_by(|left, right| {
            let left = left
                .get("average_daily_dollar_volume_usd")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let right = right
                .get("average_daily_dollar_volume_usd")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            left.partial_cmp(&right)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|line| line.get("symbol"))
        .cloned()
        .unwrap_or(Value::Null);
    let market_caps: Vec<f64> = eligible_lines
        .iter()
        .filter_map(|line| {
            line.get("market_capitalization_usd")
                .and_then(Value::as_f64)
        })
        .collect();
    let ranking_score = negative_gap_score(&report);
    let mut row = issuer;
    row["actionable_symbol"] = actionable_symbol;
    row["issuer_market_capitalization_usd"] = median_value(market_caps);
    row["expectations"] = report;
    row["ranking_score"] = ranking_score.map_or(Value::Null, |value| json!(value));
    row["ranking_status"] = json!(if ranking_score.is_some() {
        "complete_negative_two_legged"
    } else {
        "unranked_or_partial"
    });
    Ok(row)
}

async fn advance_issuers(
    server: &CompaniesServer,
    run_id: &str,
    mut state: Value,
    requested_page_size: Option<u32>,
) -> Result<Value, McpToolError> {
    let cursor = state
        .get("issuer_cursor")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| McpToolError::internal("screen state has invalid issuer_cursor"))?;
    let configured = state
        .pointer("/criteria/page_size")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| McpToolError::internal("screen state has invalid page_size"))?;
    let page_size = requested_page_size
        .filter(|value| *value > 0)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(configured);
    let issuers = state
        .get("issuers")
        .and_then(Value::as_array)
        .ok_or_else(|| McpToolError::internal("screen state has no issuers array"))?;
    let issuer_count = issuers.len();
    let page: Vec<Value> = issuers
        .iter()
        .skip(cursor)
        .take(page_size)
        .cloned()
        .collect();
    let next_cursor = cursor
        .checked_add(page.len())
        .ok_or_else(|| McpToolError::internal("issuer cursor overflow"))?;
    let as_of = state
        .get("as_of")
        .and_then(Value::as_str)
        .ok_or_else(|| McpToolError::internal("screen state has no as_of"))?
        .to_string();

    let outcomes = futures::stream::iter(page.into_iter().map(|issuer| {
        let as_of = as_of.clone();
        async move { analyze_issuer(server, issuer, &as_of).await }
    }))
    .buffered(4)
    .collect::<Vec<_>>()
    .await;
    let mut results = Vec::new();
    let mut excluded = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(row) => results.push(row),
            Err(error) => excluded.push(json!({
                "reason": "expectations_unavailable",
                "detail": error.to_string(),
            })),
        }
    }
    append_rows(&mut state, "issuer_results", results)?;
    append_rows(&mut state, "excluded_issuers", excluded)?;
    state["issuer_cursor"] = json!(next_cursor);
    if next_cursor >= issuer_count {
        let all_results = state
            .get("issuer_results")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| McpToolError::internal("screen state has no issuer results"))?;
        let mut ranked: Vec<Value> = all_results
            .iter()
            .filter(|row| row.get("ranking_score").and_then(Value::as_f64).is_some())
            .cloned()
            .collect();
        ranked.sort_by(|left, right| {
            let left = left
                .get("ranking_score")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let right = right
                .get("ranking_score")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            right
                .partial_cmp(&left)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (index, row) in ranked.iter_mut().enumerate() {
            row["rank"] = json!(index + 1);
        }
        let partial: Vec<Value> = all_results
            .into_iter()
            .filter(|row| row.get("ranking_score").and_then(Value::as_f64).is_none())
            .collect();
        state["ranked_companies"] = json!(ranked);
        state["partial_companies"] = json!(partial);
        state["phase"] = json!("complete");
    }
    save(server, run_id, &state).await?;
    Ok(state)
}

enum Qualification {
    Qualified(Value),
    Excluded(Value),
}

async fn qualify_listing(
    server: &CompaniesServer,
    row: Value,
    minimum_usd: f64,
    from: &str,
    to: &str,
    sessions_by_exchange: &Map<String, Value>,
    fx_by_currency: &Map<String, Value>,
) -> Result<Qualification, McpToolError> {
    let code = required_row_string(&row, "code")?;
    let exchange = required_row_string(&row, "exchange")?;
    let currency_symbol = required_row_string(&row, "currency_symbol")?;
    let symbol = format!("{code}.{exchange}");
    let (currency, major_per_price_unit) = currency_spec(currency_symbol).ok_or_else(|| {
        McpToolError::unavailable(format!(
            "{symbol} has ambiguous or unsupported currency symbol {currency_symbol:?}"
        ))
    })?;
    let fx_history = if currency == "USD" {
        None
    } else {
        Some(fx_by_currency.get(currency).ok_or_else(|| {
            McpToolError::unavailable(format!("screen run has no USD{currency}.FOREX history"))
        })?)
    };
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
    let (average_usd, observations) =
        average_usd_turnover(&history, &sessions, major_per_price_unit, fx_history)?;
    if average_usd < minimum_usd {
        return Ok(Qualification::Excluded(json!({
            "symbol": symbol,
            "reason": "below_liquidity_minimum",
            "average_daily_dollar_volume_usd": average_usd,
            "scheduled_sessions": sessions.len(),
            "observed_sessions": observations,
        })));
    }
    let fundamentals = load_fundamentals(server, &symbol).await?;
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
    object.insert(
        "liquidity_source".to_string(),
        json!(if currency == "USD" {
            "EODHD EOD close × volume".to_string()
        } else {
            format!("EODHD EOD close × volume; date-matched USD{currency}.FOREX")
        }),
    );
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

pub(crate) fn build_issuers(listings: &[Value]) -> Vec<Value> {
    let mut parents: Vec<usize> = (0..listings.len()).collect();
    let mut token_owner: HashMap<String, usize> = HashMap::new();
    for (index, listing) in listings.iter().enumerate() {
        for token in issuer_tokens(listing) {
            if let Some(previous) = token_owner.insert(token, index) {
                union(&mut parents, index, previous);
            }
        }
    }

    let mut groups: HashMap<usize, Vec<Value>> = HashMap::new();
    for (index, listing) in listings.iter().enumerate() {
        let root = find_root(&mut parents, index);
        groups.entry(root).or_default().push(listing.clone());
    }

    let mut issuers = Vec::new();
    for (_, eligible_lines) in groups {
        let mut leis = string_values(&eligible_lines, "issuer_lei");
        let mut primaries = string_values(&eligible_lines, "primary_ticker");
        leis.sort();
        leis.dedup();
        primaries.sort();
        primaries.dedup();
        let company = eligible_lines
            .iter()
            .find_map(|line| line.get("name").and_then(Value::as_str))
            .unwrap_or("unknown issuer")
            .to_string();
        let (issuer_key, identity_basis) = if let Some(lei) = leis.first() {
            (format!("lei:{lei}"), "LEI")
        } else if let Some(primary) = primaries.first() {
            (format!("primary:{primary}"), "PrimaryTicker/Listings graph")
        } else {
            (
                format!("name:{}", normalize_issuer_name(&company)),
                "normalized issuer name fallback",
            )
        };
        issuers.push(json!({
            "issuer_key": issuer_key,
            "identity_basis": identity_basis,
            "company": company,
            "primary_ticker": primaries.first().cloned(),
            "primary_ticker_conflict": primaries.len() > 1,
            "eligible_lines": eligible_lines,
        }));
    }
    issuers.sort_by(|left, right| {
        left.get("issuer_key")
            .and_then(Value::as_str)
            .cmp(&right.get("issuer_key").and_then(Value::as_str))
    });
    issuers
}

fn issuer_tokens(listing: &Value) -> Vec<String> {
    let mut tokens = Vec::new();
    if let Some(lei) = nonempty_string(listing.get("issuer_lei")) {
        tokens.push(format!("lei:{lei}"));
    }
    if let Some(primary) = nonempty_string(listing.get("primary_ticker")) {
        tokens.push(format!("security:{primary}"));
    }
    if let Some(symbol) = nonempty_string(listing.get("symbol")) {
        tokens.push(format!("security:{symbol}"));
    }
    if let Some(name) = nonempty_string(listing.get("name")) {
        tokens.push(format!("name:{}", normalize_issuer_name(name)));
    }
    if let Some(other) = listing.get("other_listings") {
        match other {
            Value::Object(values) => {
                for value in values.values() {
                    push_listing_token(&mut tokens, value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    push_listing_token(&mut tokens, value);
                }
            }
            _ => {}
        }
    }
    tokens.sort();
    tokens.dedup();
    tokens
}

fn push_listing_token(tokens: &mut Vec<String>, listing: &Value) {
    if let (Some(code), Some(exchange)) = (
        listing.get("Code").and_then(Value::as_str),
        listing.get("Exchange").and_then(Value::as_str),
    ) {
        tokens.push(format!("security:{code}.{exchange}"));
    }
}

fn string_values(rows: &[Value], key: &str) -> Vec<String> {
    rows.iter()
        .filter_map(|row| nonempty_string(row.get(key)).map(str::to_string))
        .collect()
}

fn nonempty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn normalize_issuer_name(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn find_root(parents: &mut [usize], index: usize) -> usize {
    let mut root = index;
    while parents[root] != root {
        root = parents[root];
    }
    let mut current = index;
    while parents[current] != current {
        let next = parents[current];
        parents[current] = root;
        current = next;
    }
    root
}

fn union(parents: &mut [usize], left: usize, right: usize) {
    let left_root = find_root(parents, left);
    let right_root = find_root(parents, right);
    if left_root != right_root {
        parents[right_root] = left_root;
    }
}

fn latest_close(history: &Value) -> Option<f64> {
    history
        .as_array()?
        .iter()
        .filter_map(|row| {
            let date = row.get("date")?.as_str()?;
            let close = row.get("close")?.as_f64()?;
            (close.is_finite() && close > 0.0).then_some((date, close))
        })
        .max_by_key(|(date, _)| *date)
        .map(|(_, close)| close)
}

fn negative_gap_score(report: &Value) -> Option<f64> {
    if report.get("signal").and_then(Value::as_str) != Some("price_demands_less_than_demonstrated")
    {
        return None;
    }
    let growth = report.pointer("/gaps/growth_gap_pp")?.as_f64()?;
    let profitability = report.pointer("/gaps/profitability_gap_pp")?.as_f64()?;
    if growth >= 0.0 || profitability >= 0.0 {
        return None;
    }
    Some(((-growth / 3.0) * (-profitability / 0.5)).sqrt())
}

fn median_value(mut values: Vec<f64>) -> Value {
    values.retain(|value| value.is_finite() && *value > 0.0);
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    if values.is_empty() {
        return Value::Null;
    }
    let middle = values.len() / 2;
    let median = if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    };
    json!(median)
}

fn average_usd_turnover(
    history: &Value,
    sessions: &HashSet<String>,
    major_per_price_unit: f64,
    fx_history: Option<&Value>,
) -> Result<(f64, usize), McpToolError> {
    let rows = history
        .as_array()
        .ok_or_else(|| McpToolError::unavailable("EODHD EOD history is not an array"))?;
    let mut fx_by_date = Map::new();
    if let Some(history) = fx_history {
        let rows = history
            .as_array()
            .ok_or_else(|| McpToolError::unavailable("EODHD FOREX history is not an array"))?;
        for row in rows {
            if let (Some(date), Some(rate)) = (
                row.get("date").and_then(Value::as_str),
                row.get("close").and_then(Value::as_f64),
            ) && rate.is_finite()
                && rate > 0.0
            {
                fx_by_date.insert(date.to_string(), json!(rate));
            }
        }
    }
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
        if !close.is_finite() || close <= 0.0 || !volume.is_finite() || volume < 0.0 {
            continue;
        }
        let units_per_usd = if fx_history.is_some() {
            fx_by_date
                .get(date)
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    McpToolError::unavailable(format!(
                        "no date-matched EODHD FOREX close for {date}"
                    ))
                })?
        } else {
            1.0
        };
        let turnover = close * major_per_price_unit * volume / units_per_usd;
        if turnover.is_finite() {
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
        .get("TradingHours")
        .and_then(Value::as_object)
        .and_then(|hours| hours.get("WorkingDays"))
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

fn public_view(mut state: Value) -> Value {
    let complete = state.get("phase").and_then(Value::as_str) == Some("complete");
    for (source, output) in [
        ("qualified_listings", "qualified_listing_count"),
        ("excluded_listings", "excluded_listing_count"),
        ("issuers", "issuer_count"),
        ("issuer_results", "issuer_result_count"),
        ("excluded_issuers", "excluded_issuer_count"),
    ] {
        let count = state
            .get(source)
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        state[output] = json!(count);
    }
    if let Some(object) = state.as_object_mut() {
        for key in [
            "candidates",
            "sessions_by_exchange",
            "fx_by_currency",
            "qualified_listings",
            "issuers",
            "issuer_results",
        ] {
            object.remove(key);
        }
        if complete {
            object.remove("latest_qualified_listings");
            object.remove("latest_excluded_listings");
        } else {
            object.remove("excluded_listings");
            object.remove("excluded_issuers");
            object.remove("ranked_companies");
            object.remove("partial_companies");
        }
    }
    state
}

fn render_screen_plan(
    composition: &str,
    req: &types::ScreenerRequest,
) -> Result<ScreenPlan, McpToolError> {
    let source = match composition {
        "expectations_gap" => EXPECTATIONS_GAP_TEMPLATE,
        _ => {
            return Err(McpToolError::invalid_argument(format!(
                "unknown company screening composition {composition:?}"
            )));
        }
    };
    let (_, body) = source.split_once("\n---\n").ok_or_else(|| {
        McpToolError::internal(format!(
            "company screening template {composition:?} has no metadata delimiter"
        ))
    })?;
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template(composition, body).map_err(|error| {
        McpToolError::internal(format!(
            "company screening template {composition:?} failed to compile: {error}"
        ))
    })?;
    let context = json!({
        "as_of": req.as_of,
        "page_size": req.limit,
        "criteria": req.criteria_overrides.0,
    });
    let rendered = env
        .get_template(composition)
        .and_then(|template| template.render(minijinja::Value::from_serialize(&context)))
        .map_err(|error| {
            McpToolError::invalid_argument(format!(
                "company screening composition {composition:?} could not render: {error}"
            ))
        })?;
    serde_json::from_str(&rendered).map_err(|error| {
        McpToolError::internal(format!(
            "company screening composition {composition:?} rendered an invalid plan: {error}"
        ))
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

async fn load_fundamentals(server: &CompaniesServer, symbol: &str) -> Result<Value, McpToolError> {
    if let Some(cache) = server.fibo_cache.as_ref()
        && let Some(value) = cache.get_raw(symbol, "screen_fundamentals", "none")
    {
        return Ok(value);
    }
    let value =
        providers::fetch_eodhd_fundamentals(&server.client, &server.eodhd_api_key, symbol).await?;
    if let Some(cache) = server.fibo_cache.as_ref() {
        cache.store_raw(symbol, "screen_fundamentals", "none", &value, "EODHD");
    }
    Ok(value)
}

async fn load(server: &CompaniesServer, run_id: &str) -> Result<Value, McpToolError> {
    let lookup_id = run_id.to_string();
    run_store(server.research.clone(), move |store| {
        store.get_company_screen(&lookup_id)
    })
    .await?
    .ok_or_else(|| {
        McpToolError::invalid_argument(format!("company screen run {run_id:?} was not found"))
    })
}

async fn save(server: &CompaniesServer, run_id: &str, state: &Value) -> Result<(), McpToolError> {
    let saved_id = run_id.to_string();
    let saved_state = state.clone();
    run_store(server.research.clone(), move |store| {
        store.save_company_screen(&saved_id, &saved_state)
    })
    .await
}
