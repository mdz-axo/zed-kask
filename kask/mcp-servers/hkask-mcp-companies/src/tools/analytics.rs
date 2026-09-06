//! Portfolio analytics and DCF valuation tools.
use super::notes::run_store;
use crate::{
    CompaniesServer, StoredForecast, fibo, financial_model,
    research_store::PersistedForecast,
    scenarios, superforecast,
    types::{self, AttributionRequest, CharacteristicsRequest},
    validate_symbol,
};
use hkask_mcp_portfolio::TxType;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_types::time::now_rfc3339;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use uuid::Uuid;

/// An attribution row before JSON serialization. `security_return` is
/// `None` when the end price is missing — the return is unknown, not -100%.
pub(crate) struct AttributionRow {
    pub symbol: String,
    pub mv_start: f64,
    pub security_return: Option<f64>,
    pub shares_end: f64,
    pub p_end: Option<f64>,
}

/// Symbols excluded from or null-valued in the attribution table because a
/// close price is missing. Surfaced in the tool output so a data outage is
/// never indistinguishable from a computed result.
pub(crate) struct AttributionGaps {
    pub missing_start_prices: Vec<String>,
    pub missing_end_prices: Vec<String>,
}

/// Build attribution rows from start/end positions and fetched close prices.
///
/// Pre-fix behavior zero-valued a missing end price, which fabricated a
/// -100% return and a `gain_loss` of `-mv_start` for every symbol with a
/// data outage. Now a missing end price nulls the row's return/contribution
/// and lists the symbol in `missing_end_prices`; a missing start price
/// excludes the row (it cannot be weighted) and lists it in
/// `missing_start_prices`. A closed position (shares_end ≈ 0) does not need
/// an end price — its end weight is 0 by construction — so a missing end
/// price there is not reported as a gap.
pub(crate) fn build_attribution_rows(
    positions_start: &std::collections::HashMap<String, f64>,
    positions_end: &std::collections::HashMap<String, f64>,
    prices_start: &serde_json::Map<String, serde_json::Value>,
    prices_end: &serde_json::Map<String, serde_json::Value>,
) -> (Vec<AttributionRow>, AttributionGaps) {
    let mut rows = Vec::new();
    let mut gaps = AttributionGaps {
        missing_start_prices: Vec::new(),
        missing_end_prices: Vec::new(),
    };
    for (sym, shares) in positions_start {
        let Some(p_start) = prices_start
            .get(sym)
            .and_then(|v| v.as_f64())
            .filter(|p| *p > 0.0)
        else {
            gaps.missing_start_prices.push(sym.clone());
            continue;
        };
        let p_end = prices_end
            .get(sym)
            .and_then(|v| v.as_f64())
            .filter(|p| *p > 0.0);
        let shares_end = positions_end.get(sym).copied().unwrap_or(0.0);
        let security_return = match p_end {
            Some(p) => Some((p - p_start) / p_start),
            None => {
                if shares_end > 0.0001 {
                    gaps.missing_end_prices.push(sym.clone());
                }
                None
            }
        };
        rows.push(AttributionRow {
            symbol: sym.clone(),
            mv_start: shares * p_start,
            security_return,
            shares_end,
            p_end,
        });
    }
    (rows, gaps)
}

#[tool_router(router = analytics_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "What moved the portfolio - each position's weight, return, and contribution, ranked by impact"
    )]
    pub async fn portfolio_attribution(
        &self,
        Parameters(req): Parameters<AttributionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_attribution", async {
            // Get transactions and compute positions at start and end
            let portfolio_name = req.portfolio.clone();
            let txs = run_store(self.research.clone(), move |portfolio| {
                portfolio.get_transactions(&portfolio_name, None, None, None, None)
            })
            .await?;

            // Compute positions at from_date and to_date
            let mut positions_start: std::collections::HashMap<String, f64> =
                std::collections::HashMap::new();
            let mut positions_end: std::collections::HashMap<String, f64> =
                std::collections::HashMap::new();
            for tx in &txs {
                if let Some(ref sym) = tx.symbol {
                    if tx.date <= req.from {
                        match tx.tx_type {
                            TxType::Buy => {
                                *positions_start.entry(sym.clone()).or_insert(0.0) +=
                                    tx.quantity.unwrap_or(0.0)
                            }
                            TxType::Sell => {
                                *positions_start.entry(sym.clone()).or_insert(0.0) -=
                                    tx.quantity.unwrap_or(0.0)
                            }
                            _ => {}
                        }
                    }
                    if tx.date <= req.to {
                        match tx.tx_type {
                            TxType::Buy => {
                                *positions_end.entry(sym.clone()).or_insert(0.0) +=
                                    tx.quantity.unwrap_or(0.0)
                            }
                            TxType::Sell => {
                                *positions_end.entry(sym.clone()).or_insert(0.0) -=
                                    tx.quantity.unwrap_or(0.0)
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Only include symbols with non-zero position at start
            positions_start.retain(|_, v| *v > 0.0001);
            if positions_start.is_empty() {
                return Ok(serde_json::json!(
                    {"attribution": [], "message": "no positions at start date"}
                ));
            }

            // Fetch prices for all symbols at both dates
            let mut prices_start = serde_json::Map::new();
            let mut prices_end = serde_json::Map::new();
            let mut errors = Vec::new();

            for sym in positions_start.keys() {
                // Fetch historical prices around each date (typed view — the
                // `close`/`adjClose` field-name knowledge lives in
                // `HistoricalPriceView::latest_close`).
                for (date, prices_map) in
                    [(&req.from, &mut prices_start), (&req.to, &mut prices_end)]
                {
                    match self.fetch_historical_price(sym, date, date).await {
                        Ok(view) => {
                            if let Some(c) = view.latest_close() {
                                prices_map
                                    .insert(sym.clone(), serde_json::Value::from(c));
                            }
                        }
                        Err(e) => {
                            errors.push(format!("{sym}@{date}: {}", e.to_json_string()));
                        }
                    }
                }
            }

            // Build attribution table. Cap at 99 holdings - if the portfolio
            // exceeds this, keep the largest by starting market value. This
            // bounds the calculation and presentation for a single portfolio.
            const MAX_HOLDINGS: usize = 99;
            let (mut rows, gaps) = build_attribution_rows(
                &positions_start,
                &positions_end,
                &prices_start,
                &prices_end,
            );

            // If over the cap, sort by market value descending and keep top 99.
            if rows.len() > MAX_HOLDINGS {
                rows.sort_by(|a, b| {
                    b.mv_start
                        .partial_cmp(&a.mv_start)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                rows.truncate(MAX_HOLDINGS);
            }

            let total_mv: f64 = rows.iter().map(|row| row.mv_start).sum();
            let mut attribution: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|row| {
                    let weight = if total_mv > 0.0 {
                        row.mv_start / total_mv
                    } else {
                        0.0
                    };
                    let contribution_bps = row
                        .security_return
                        .map(|ret| weight * ret * 10000.0);
                    // A closed position has an end weight of 0 by
                    // construction; an open one needs the end price.
                    let weight_end_pct = if total_mv <= 0.0 {
                        Some(0.0)
                    } else if row.shares_end <= 0.0001 {
                        Some(0.0)
                    } else {
                        row.p_end
                            .map(|p| row.shares_end * p / total_mv * 100.0)
                    };
                    serde_json::json!({
                        "symbol": row.symbol,
                        "weight_start_pct": (weight * 100.0),
                        "weight_end_pct": weight_end_pct,
                        "security_return_pct": row.security_return.map(|ret| ret * 100.0),
                        "contribution_bps": contribution_bps,
                        "gain_loss": row.security_return.map(|ret| row.mv_start * ret),
                    })
                })
                .collect();

            // Sort by absolute contribution
            attribution.sort_by(|a, b| {
                let ca = a["contribution_bps"].as_f64().unwrap_or(0.0).abs();
                let cb = b["contribution_bps"].as_f64().unwrap_or(0.0).abs();
                cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
            });

            let missing_prices = serde_json::json!({
                "start": gaps.missing_start_prices,
                "end": gaps.missing_end_prices,
            });
            let price_gap_note = if gaps.missing_start_prices.is_empty()
                && gaps.missing_end_prices.is_empty()
            {
                None
            } else {
                Some(format!(
                    "{} symbol(s) excluded or null-valued for missing close prices — see missing_prices; returns are unknown, not -100%",
                    gaps.missing_start_prices.len() + gaps.missing_end_prices.len()
                ))
            };

            Ok(fibo::enrich_with_ontology(serde_json::json!({
                "portfolio": req.portfolio,
                "from": req.from,
                "to": req.to,
                "attribution": attribution,
                "missing_prices": missing_prices,
                "note": price_gap_note,
                "errors": errors,
            }), "portfolio_attribution"))
        }).await
    }

    #[tool(
        description = "Weighted-average fundamentals of what the portfolio owns - valuation, profitability, leverage, growth, composition"
    )]
    pub async fn portfolio_characteristics(
        &self,
        Parameters(req): Parameters<CharacteristicsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "portfolio_characteristics", async {
            let portfolio_name = req.portfolio.clone();
            let symbols = run_store(self.research.clone(), move |portfolio| {
                portfolio.get_symbols(&portfolio_name)
            })
            .await?;

            if symbols.is_empty() {
                return Ok(serde_json::json!(
                    {"characteristics": {}, "message": "no symbols in portfolio"}
                ));
            }

            // Get positions at the as-of date
            let portfolio_name = req.portfolio.clone();
            let as_of = req.date.clone();
            let txs = run_store(self.research.clone(), move |portfolio| {
                portfolio.get_transactions(&portfolio_name, None, None, None, Some(&as_of))
            })
            .await?;
            let mut positions: std::collections::HashMap<String, f64> =
                std::collections::HashMap::new();
            for tx in &txs {
                if let Some(ref sym) = tx.symbol {
                    match tx.tx_type {
                        TxType::Buy => {
                            *positions.entry(sym.clone()).or_insert(0.0) +=
                                tx.quantity.unwrap_or(0.0)
                        }
                        TxType::Sell => {
                            *positions.entry(sym.clone()).or_insert(0.0) -=
                                tx.quantity.unwrap_or(0.0)
                        }
                        _ => {}
                    }
                }
            }
            positions.retain(|_, v| *v > 0.0001);

            // Fetch prices and market values. A quote that succeeds but
            // carries no parseable price is a data gap, not a zero-valued
            // holding — zeroing it made the symbol silently vanish from the
            // weighted averages.
            let mut market_values = Vec::new();
            let mut errors = Vec::new();
            let mut missing_prices = Vec::new();
            for sym in positions.keys() {
                match self.fetch("stock_quote", sym, &[]).await {
                    Ok(value) => {
                        let price = value
                            .as_array()
                            .and_then(|a| a.first())
                            .and_then(|q| q.get("price").and_then(|p| p.as_f64()));
                        match price {
                            Some(price) => {
                                let shares = positions.get(sym).copied().unwrap_or(0.0);
                                market_values.push((sym.clone(), shares, price, shares * price));
                            }
                            None => {
                                missing_prices
                                    .push(format!("{sym}: quote returned no parseable price"));
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!("{sym} quote: {}", e.to_json_string()));
                    }
                }
            }

            let total_mv: f64 = market_values.iter().map(|(_, _, _, mv)| mv).sum();
            if total_mv <= 0.0 {
                return Ok(serde_json::json!(
                    {"characteristics": {}, "message": "no market value", "missing_prices": missing_prices, "errors": errors}
                ));
            }

            // Cap at 99 holdings - if the portfolio exceeds this, keep the
            // largest by market value. Bounds the fetch + calculation cost.
            const MAX_HOLDINGS: usize = 99;
            if market_values.len() > MAX_HOLDINGS {
                market_values
                    .sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
                market_values.truncate(MAX_HOLDINGS);
            }
            // Recompute total_mv after truncation.
            let total_mv: f64 = market_values.iter().map(|(_, _, _, mv)| mv).sum();

            // Fetch fundamentals and compute weighted averages.
            //
            // Collect (weight, value) pairs per field, then aggregate at the
            // end using the requested method. Categorical fields (sector,
            // industry, country) are always weight-summed (not aggregated).
            let mut numeric_fields: std::collections::HashMap<
                String,
                (Vec<crate::aggregation::WeightedValue>, &'static str),
            > = std::collections::HashMap::new();
            let mut categorical_breakdowns: std::collections::HashMap<
                String,
                std::collections::HashMap<String, f64>,
            > = std::collections::HashMap::new();

            for (sym, _shares, _price, mv) in &market_values {
                let weight = mv / total_mv;

                // Fetch profile for sector/industry/country/market cap
                if let Ok(profile_val) = self.fetch("company_profile", sym, &[]).await
                    && let Some(profile) = profile_val.as_array().and_then(|a| a.first())
                {
                    for field in ["sector", "industry", "country", "mktCap"] {
                        if let Some(val) = profile.get(field) {
                            if val.is_string() {
                                let str_val =
                                    val.as_str().expect("guarded by is_string check above");
                                let sub =
                                    categorical_breakdowns.entry(field.to_string()).or_default();
                                *sub.entry(str_val.to_string()).or_insert(0.0) += weight;
                            } else if let Some(num) = val.as_f64() {
                                let metric = fibo::fmp_field_to_metric(field).unwrap_or("unknown");
                                numeric_fields
                                    .entry(field.to_string())
                                    .or_insert_with(|| (Vec::new(), metric))
                                    .0
                                    .push(crate::aggregation::WeightedValue { weight, value: num });
                            }
                        }
                    }
                }

                // Fetch key metrics for profitability/valuation
                if let Ok(metrics_val) = self.fetch("key_metrics", sym, &[("limit", "1")]).await
                    && let Some(metrics) = metrics_val.as_array().and_then(|a| a.first())
                {
                    for field in [
                        "peRatio",
                        "priceToBookRatio",
                        "priceToSalesRatio",
                        "roic",
                        "roe",
                        "grossProfitMargin",
                        "operatingProfitMargin",
                        "netProfitMargin",
                        "debtToEquity",
                        "dividendYield",
                        "revenueGrowth",
                        "epsGrowth",
                    ] {
                        if let Some(val) = metrics.get(field).and_then(|v| v.as_f64()) {
                            let metric = fibo::fmp_field_to_metric(field).unwrap_or("unknown");
                            numeric_fields
                                .entry(field.to_string())
                                .or_insert_with(|| (Vec::new(), metric))
                                .0
                                .push(crate::aggregation::WeightedValue { weight, value: val });
                        }
                    }
                }

                // Balance sheet for leverage
                if let Ok(bs_val) = self.fetch("balance_sheet", sym, &[("limit", "1")]).await
                    && let Some(bs) = bs_val.as_array().and_then(|a| a.first())
                {
                    let assets = bs.get("totalAssets").and_then(|v| v.as_f64());
                    let equity = bs.get("totalEquity").and_then(|v| v.as_f64());
                    if let (Some(a), Some(e)) = (assets, equity)
                        && e > 0.0
                    {
                        let lev = a / e;
                        let metric =
                            fibo::fmp_field_to_metric("financialLeverage").unwrap_or("unknown");
                        numeric_fields
                            .entry("financialLeverage".to_string())
                            .or_insert_with(|| (Vec::new(), metric))
                            .0
                            .push(crate::aggregation::WeightedValue { weight, value: lev });
                    }
                }
            }

            // Aggregate numeric fields using the requested method.
            let mut characteristics = serde_json::Map::new();
            for (field, (values, metric)) in &numeric_fields {
                let aggregated = crate::aggregation::aggregate(values, &req.aggregation);
                characteristics.insert(
                    field.clone(),
                    serde_json::json!({
                        "value": aggregated,
                        "metric": metric,
                        "method": req.aggregation,
                        "holdings": values.len(),
                    }),
                );
            }

            // Insert categorical breakdowns.
            for (field, breakdown) in categorical_breakdowns {
                characteristics.insert(format!("{field}_breakdown"), serde_json::json!(breakdown));
            }

            Ok(fibo::enrich_with_ontology(
                serde_json::json!({
                    "portfolio": req.portfolio,
                    "date": req.date,
                    "aggregation": req.aggregation,
                    "total_market_value": total_mv,
                    "position_count": market_values.len(),
                    "characteristics": characteristics,
                    "missing_prices": missing_prices,
                    "errors": errors,
                }),
                "portfolio_characteristics",
            ))
        })
        .await
    }

    #[tool(
        description = "Two-stage DCF valuation. Projects income statement, balance sheet, and cash flow line items to derive free cash flow, then discounts back to enterprise value and intrinsic equity per share. Projects 11 line items per period (revenue, COGS, gross profit, D&A, EBIT, tax, NOPAT, capex, change in NWC, FCF, PV). Returns a forecast_id for later decomposition via forecast_record. Default: 10yr model, 3yr stage 1, 7yr stage 2, 10% WACC, 2.5% terminal growth."
    )]
    pub async fn dcf_valuation(
        &self,
        Parameters(req): Parameters<types::DcfValuationRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "dcf_valuation", async {
            validate_symbol(&req.symbol)?;
            if let Some(ref revision_of) = req.revision_of {
                let revision_of = revision_of.clone();
                let symbol = req.symbol.clone();
                run_store(self.research.clone(), move |portfolio| {
                    portfolio.validate_forecast_revision(&revision_of, &symbol)
                })
                .await?;
            }

            let profile = self.fetch_profile(&req.symbol).await?;
            let prepared = match crate::valuation_service::prepare_dcf(
                self,
                &req.symbol,
                &profile,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .await
            {
                Ok(prepared) => prepared,
                Err(error) => return error.into_tool_result(),
            };
            let crate::valuation_service::PreparedDcf {
                history: hist,
                assumptions,
                model,
                current_price,
                provenance,
                warnings,
            } = prepared;
            let shares = hist.shares_outstanding;

            // Compute signal quality and emit Regulation span (G2: FinGPT low-SNR handling)
            let signal_quality = hist.signal_quality();
            crate::data_quality::emit_data_quality_span(
                &req.symbol,
                "dcf_valuation",
                &signal_quality,
            );

            // Generate forecast ID for later decomposition
            let forecast_id = Uuid::new_v4().to_string();

            // Persist the forecast model for later decomposition across restarts.
            let stored = StoredForecast {
                model: model.clone(),
                assumptions: assumptions.clone(),
                current_price,
                intrinsic_per_share: model.intrinsic_per_share,
            };
            self.save_forecast(PersistedForecast {
                id: forecast_id.clone(),
                symbol: req.symbol.clone(),
                revision_of: req.revision_of.clone(),
                snapshot: stored.snapshot(),
                outcomes: Vec::new(),
                created_at: now_rfc3339(),
            })
            .await?;

            // The response assembly is pure — delegate to `valuation_service`
            // so it is testable without HTTP/API keys. The tool handler retains
            // only fetch, validate, persist, and the span.
            let mut output = crate::valuation_service::build_dcf_response(
                &req.symbol,
                &forecast_id,
                &req.revision_of,
                &model,
                &assumptions,
                &hist,
                &signal_quality,
                current_price,
                shares,
            );

            output["provenance"] = provenance;
            output["warnings"] = serde_json::json!(warnings);
            Ok(fibo::enrich_with_ontology(output, "dcf_valuation"))
        })
        .await
    }

    #[tool(
        description = "Reverse DCF (Mauboussin's Expectations Investing). Solves for the revenue growth rate implied by the current stock price. \"What growth does the market expect?\" - compare to your own estimate to find mispricing. Default: 10yr model, 3yr stage 1, 7yr stage 2, 10% WACC."
    )]
    pub async fn reverse_dcf(
        &self,
        Parameters(req): Parameters<types::ReverseDcfRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reverse_dcf", async {
            validate_symbol(&req.symbol)?;

            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch_profile(&req.symbol).await;

            let (income, balance, cf, metrics, profile) =
                match (income_result, balance_result, cf_result, metrics_result, profile_result) {
                    (Ok(inc), Ok(bal), Ok(cf), Ok(m), Ok(p)) => (inc, bal, cf, m, p),
                    (Err(e), _, _, _, _)
                    | (_, Err(e), _, _, _)
                    | (_, _, Err(e), _, _)
                    | (_, _, _, Err(e), _)
                    | (_, _, _, _, Err(e)) => {
                        return Err(e);
                    }
                };

            let income_arr = income.as_array();
            let balance_arr = balance.as_array();
            let cf_arr = cf.as_array();
            let metrics_arr = metrics.as_array();
            let profile_obj = profile.raw().as_array().and_then(|a| a.first());

            let (Some(income_data), Some(balance_data), Some(cf_data), Some(profile_data)) = (
                income_arr.filter(|a| !a.is_empty()),
                balance_arr.filter(|a| !a.is_empty()),
                cf_arr.filter(|a| !a.is_empty()),
                profile_obj,
            )
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            };
            let metrics_data: &[serde_json::Value] = metrics_arr.map_or(&[], |v| v);

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data - need at least 2 years of revenue"}));
            }

            if let Some(err) = financial_model::financial_sector_guard(&profile, &req.symbol, "reverse_dcf") {
                return Ok(err);
            }

            let signal_quality = hist.signal_quality();
            crate::data_quality::emit_data_quality_span(
                &req.symbol, "reverse_dcf", &signal_quality,
            );

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            let current_price = profile.price().unwrap_or(0.0);

            if current_price <= 0.0 {
                return Err(McpToolError::invalid_argument(
                    "current price must be positive for reverse DCF",
                ));
            }

            // Solve via the shared bisection in `financial_model` — the single
            // source of truth for the search direction, shared with
            // `expectations_gap`. Report the out-of-bracket cases distinctly so
            // the caller learns which bound was violated.
            let implied_growth = match financial_model::implied_growth(
                &hist,
                &assumptions,
                current_price,
            ) {
                Some(growth) => growth,
                None => {
                    let at = |growth: f64| {
                        financial_model::project_model(
                            &hist,
                            &financial_model::ProjectionAssumptions {
                                revenue_growth: growth,
                                ..assumptions.clone()
                            },
                            current_price,
                        )
                        .intrinsic_per_share
                    };
                    let lo_intrinsic = at(financial_model::IMPLIED_GROWTH_LO);
                    if lo_intrinsic > current_price {
                        return Err(McpToolError::invalid_argument(format!(
                            "price ({current_price:.2}) below intrinsic ({lo_intrinsic:.2}) at -50% growth - stock may be distressed or data inconsistent"
                        )));
                    }
                    let hi_intrinsic = at(financial_model::IMPLIED_GROWTH_HI);
                    return Err(McpToolError::invalid_argument(format!(
                        "price ({current_price:.2}) implies growth > 100% - intrinsic at +100% growth is {hi_intrinsic:.2}"
                    )));
                }
            };

            // Final model at implied growth
            let mut final_a = assumptions.clone();
            final_a.revenue_growth = implied_growth;
            let result = financial_model::project_model(&hist, &final_a, current_price);

            let output = serde_json::json!({
                "symbol": req.symbol,
                "current_price": current_price,
                "implied_growth_rate": implied_growth,
                "intrinsic_at_implied": result.intrinsic_per_share,
                "enterprise_value": result.enterprise_value,
                "config": {
                    "stage1_years": assumptions.stage1_years,
                    "stage2_years": assumptions.total_years - assumptions.stage1_years,
                    "discount_rate": assumptions.discount_rate,
                    "terminal_growth": assumptions.terminal_growth,
                },
                "interpretation": {
                    "implied_growth_pct": format!("{:.1}%", implied_growth * 100.0),
                    "signal": if implied_growth < 0.05 { "low_expectations" } else if implied_growth > 0.15 { "high_expectations" } else { "moderate_expectations" },
                    "mauboussin_framework": "The current stock price implies a revenue growth rate. Compare this to your own estimate of sustainable growth. If your estimate is higher, the stock may be undervalued. If lower, it may be overvalued. The gap between implied and expected growth is the expectations gap - the core of Expectations Investing (Mauboussin & Rappaport, 2001).",
                },
            });

            Ok(fibo::enrich_with_ontology(output, "reverse_dcf"))
        }).await
    }

    #[tool(
        description = "Schwartz 2x2 scenario analysis. Projects four scenarios (Bull, Land Grab, Cash Cow, Bear) based on revenue growth x profit margin axes. Runs DCF under each scenario and returns the intrinsic value range. Default axes: revenue_growth x profit_margin. Adjustable multipliers let you tune scenario severity. Detailed mode (event_tree supplied) also emits the T8a risk core: a probability-weighted risk measure (expected return, sigma_scenario), APT-style factor loadings (beta per axis) over the branch revaluations, and — when the tree is CMP-built — the R4 CMP-provenance risk measure (cmp_controlled)."
    )]
    pub async fn scenario_analysis(
        &self,
        Parameters(req): Parameters<types::ScenarioAnalysisRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_analysis", async {
            validate_symbol(&req.symbol)?;

            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch_profile(&req.symbol).await;

            let (income, balance, cf, metrics, profile) =
                match (income_result, balance_result, cf_result, metrics_result, profile_result) {
                    (Ok(inc), Ok(bal), Ok(cf), Ok(m), Ok(p)) => (inc, bal, cf, m, p),
                    (Err(e), _, _, _, _)
                    | (_, Err(e), _, _, _)
                    | (_, _, Err(e), _, _)
                    | (_, _, _, Err(e), _)
                    | (_, _, _, _, Err(e)) => {
                        return Err(e);
                    }
                };

            let income_arr = income.as_array();
            let balance_arr = balance.as_array();
            let cf_arr = cf.as_array();
            let metrics_arr = metrics.as_array();
            let profile_obj = profile.raw().as_array().and_then(|a| a.first());

            let (Some(income_data), Some(balance_data), Some(cf_data), Some(profile_data)) = (
                income_arr.filter(|a| !a.is_empty()),
                balance_arr.filter(|a| !a.is_empty()),
                cf_arr.filter(|a| !a.is_empty()),
                profile_obj,
            )
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            };
            let metrics_data: &[serde_json::Value] = metrics_arr.map_or(&[], |v| v);

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data - need at least 2 years of revenue"}));
            }

            if let Some(err) = financial_model::financial_sector_guard(&profile, &req.symbol, "scenario_analysis") {
                return Ok(err);
            }

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            let current_price = profile.price().unwrap_or(0.0);

            let matrix = scenarios::ScenarioMatrix::growth_x_margin(assumptions.revenue_growth, assumptions.gross_margin);
            let results = scenarios::run_scenario_analysis(&hist, &assumptions, &matrix);

            let summary = scenarios::scenario_summary(&results);

            // T7: optional tree-weighted path (detailed mode). The 2×2 range
            // above is always computed; when the caller pastes a validated
            // event tree, quadrant probabilities are derived from its root
            // marginals and an expected intrinsic value is produced.
            let mut weighting_mode = superforecast::WeightingMode::Schwartz2x2;
            let mut weighted_output: Option<serde_json::Value> = None;
            let mut tree_warning: Option<String> = None;
            if let Some(tree_json) = &req.event_tree {
                match serde_json::from_str::<superforecast::EventTreeProjection>(tree_json) {
                    Ok(tree) => match superforecast::tree_root_probabilities(&tree) {
                        Some((growth_p, margin_p)) => {
                            let weighted = superforecast::distribute_scenario_probabilities(
                                growth_p, margin_p, &results,
                            );
                            let expected = superforecast::expected_intrinsic(&weighted);

                            // T8a risk core: probability-weighted risk measure
                            // and APT-style factor loadings over the branch
                            // revaluations. The branch return is the annualized
                            // return from the current price to the branch's
                            // intrinsic value over the DCF horizon. Skipped with
                            // a named reason (never silently) when a return is
                            // undefined.
                            let horizon_years = results
                                .first()
                                .map_or(0.0, |r| r.model.periods.len() as f64);
                            let negative_intrinsic =
                                weighted.iter().any(|w| w.intrinsic_per_share < 0.0);
                            let risk_skip_reason = if current_price <= 0.0 {
                                Some("current price is not positive")
                            } else if horizon_years <= 0.0 {
                                Some("DCF horizon is zero")
                            } else if negative_intrinsic {
                                Some("a branch intrinsic value is negative")
                            } else {
                                None
                            };
                            // `weighted` is built from `results` by index
                            // (distribute_scenario_probabilities), so the
                            // node-true vectors zip cleanly onto the branches.
                            let branches: Vec<hkask_forecast::BranchOutcome> =
                                weighted.iter().map(|w| {
                                    let branch_return = if w.intrinsic_per_share > 0.0 {
                                        (w.intrinsic_per_share / current_price)
                                            .powf(1.0 / horizon_years)
                                            - 1.0
                                    } else {
                                        // Zero intrinsic: total loss of the position.
                                        -1.0
                                    };
                                    hkask_forecast::BranchOutcome {
                                        probability: w.probability,
                                        branch_return,
                                    }
                                }).collect();
                            let risk_measure = if risk_skip_reason.is_none() {
                                hkask_forecast::scenario_risk_measure(&branches)
                            } else {
                                None
                            };
                            let growth_node_true: Vec<bool> = results
                                .iter()
                                .map(|r| r.scenario.axis1_multiplier > 1.0)
                                .collect();
                            let margin_node_true: Vec<bool> = results
                                .iter()
                                .map(|r| r.scenario.axis2_multiplier > 1.0)
                                .collect();
                            let growth_loading = if risk_skip_reason.is_none() {
                                hkask_forecast::scenario_node_loading(
                                    &branches, &growth_node_true,
                                )
                            } else {
                                None
                            };
                            let margin_loading = if risk_skip_reason.is_none() {
                                hkask_forecast::scenario_node_loading(
                                    &branches, &margin_node_true,
                                )
                            } else {
                                None
                            };
                            let factor_loading_note =
                                if risk_skip_reason.is_none()
                                    && (growth_loading.is_none()
                                        || margin_loading.is_none())
                                {
                                    Some("a conditioning set has zero probability mass — that axis loading is undefined")
                                } else {
                                    None
                                };

                            // R4: the same branches with CMP provenance. A
                            // quadrant probability derives from both tree
                            // roots, so the branch is CMP-controlled only when
                            // BOTH roots are CMP indices (a single raw root
                            // contaminates every quadrant — the measure's
                            // cmp_controlled flag then reports the confound).
                            let roots_cmp_controlled: Vec<bool> = tree
                                .root_ids
                                .iter()
                                .map(|id| {
                                    tree.cmp_provenance.iter().any(|c| c.id == *id)
                                })
                                .collect();
                            let cmp_source = match (
                                roots_cmp_controlled.first(),
                                roots_cmp_controlled.get(1),
                            ) {
                                (Some(true), Some(true)) => {
                                    tree.root_ids
                                        .first()
                                        .zip(tree.root_ids.get(1))
                                        .map(|(a, b)| format!("{a}+{b}"))
                                }
                                _ => None,
                            };
                            let cmp_branches: Vec<hkask_forecast::CmpBranchOutcome> =
                                branches.iter().map(|b| {
                                    hkask_forecast::CmpBranchOutcome {
                                        probability: b.probability,
                                        branch_return: b.branch_return,
                                        cmp_source: cmp_source.clone(),
                                    }
                                }).collect();
                            let cmp_risk_measure = if risk_skip_reason.is_none() {
                                hkask_forecast::cmp_scenario_risk_measure(&cmp_branches)
                            } else {
                                None
                            };
                            let cmp_controlled_note = match &cmp_risk_measure {
                                Some(rm) if !rm.cmp_controlled => Some(
                                    "at least one tree root is not a CMP index — the risk measure carries the maturity-transformation confound",
                                ),
                                _ => None,
                            };

                            weighting_mode = superforecast::WeightingMode::EventTree;
                            let cmp_provenance = if tree.cmp_provenance.is_empty() {
                                None
                            } else {
                                Some(&tree.cmp_provenance)
                            };
                            weighted_output = Some(serde_json::json!({
                                "growth_probability": growth_p,
                                "margin_probability": margin_p,
                                "expected_intrinsic_per_share": expected,
                                "weighted_scenarios": weighted.iter().map(|w| serde_json::json!({
                                    "name": w.name,
                                    "intrinsic_per_share": w.intrinsic_per_share,
                                    "probability": w.probability,
                                })).collect::<Vec<_>>(),
                                // T8a risk core (hkask_forecast::scenario_risk_measure).
                                "risk_measure": risk_measure.map(|rm| serde_json::json!({
                                    "expected_return": rm.expected_return,
                                    "sigma_scenario": rm.sigma_scenario,
                                    "branch_count": rm.branch_count,
                                    "probability_mass": rm.probability_mass,
                                })),
                                "risk_measure_note": risk_skip_reason.map(|reason| format!(
                                    "{reason} — scenario risk measure undefined (never fabricated)"
                                )),
                                // T8a factor exposures (hkask_forecast::scenario_node_loading):
                                // β(axis) = E[r | axis high] − E[r | axis low].
                                "factor_loadings": {
                                    "revenue_growth_beta": growth_loading,
                                    "gross_margin_beta": margin_loading,
                                },
                                "factor_loadings_note": factor_loading_note,
                                // R4 (hkask_forecast::cmp_scenario_risk_measure): the
                                // risk measure with CMP provenance.
                                "cmp_risk_measure": cmp_risk_measure.map(|rm| serde_json::json!({
                                    "expected_return": rm.inner.expected_return,
                                    "sigma_scenario": rm.inner.sigma_scenario,
                                    "branch_count": rm.inner.branch_count,
                                    "probability_mass": rm.inner.probability_mass,
                                    "cmp_controlled": rm.cmp_controlled,
                                    "cmp_branch_count": rm.cmp_branch_count,
                                })),
                                "cmp_controlled_note": cmp_controlled_note,
                                // R3: cite CMP provenance when the tree was built from CMP indices.
                                "cmp_provenance": cmp_provenance.map(|p| p.iter().map(|c| serde_json::json!({
                                    "id": c.id,
                                    "family": c.family,
                                    "tenor": c.tenor,
                                    "orientation": c.orientation,
                                    "venue": c.venue,
                                    "method": c.method,
                                    "maturity_error_days": c.maturity_error_days,
                                })).collect::<Vec<_>>()),
                            }));
                        }
                        None => {
                            tree_warning = Some(
                                "event tree does not have exactly two roots with valid marginals - falling back to simple 2x2 mode (no probabilities)".into()
                            );
                        }
                    },
                    Err(e) => {
                        tree_warning = Some(format!(
                            "event_tree JSON did not match the scenarios-server tree projection ({e}) - falling back to simple 2x2 mode"
                        ));
                    }
                }
            }

            // Compute signal quality and emit Regulation span
            let signal_quality = hist.signal_quality();
            crate::data_quality::emit_data_quality_span(
                &req.symbol, "scenario_analysis", &signal_quality,
            );

            let scenario_output: Vec<serde_json::Value> = results.iter().map(|r| {
                serde_json::json!({
                    "name": r.scenario.name,
                    "description": r.scenario.description,
                    "applied_growth": r.applied_growth,
                    "applied_margin": r.applied_margin,
                    "intrinsic_per_share": r.intrinsic_per_share,
                    "enterprise_value": r.model.enterprise_value,
                    "margin_of_safety": if current_price > 0.0 { (r.intrinsic_per_share - current_price) / current_price } else { 0.0 },
                })
            }).collect();

            let output = serde_json::json!({
                "symbol": req.symbol,
                "weighting_mode": weighting_mode,
                "tree_weighted": weighted_output,
                "tree_warning": tree_warning,
                "axes": {
                    "axis1": {"name": matrix.axis1.name, "metric": matrix.axis1.metric, "baseline": matrix.axis1.baseline},
                    "axis2": {"name": matrix.axis2.name, "metric": matrix.axis2.metric, "baseline": matrix.axis2.baseline},
                },
                "scenarios": scenario_output,
                "summary": {
                    "intrinsic_range": [summary.intrinsic_range.0, summary.intrinsic_range.1],
                    "intrinsic_average": summary.intrinsic_average,
                    "current_price": current_price,
                    "upside_pct": summary.upside_pct,
                    "downside_pct": summary.downside_pct,
                    "range_spread_pct": summary.range_spread_pct,
                },
                "data_quality": {
                    "overall_confidence": signal_quality.overall_confidence,
                    "quality_warning": signal_quality.quality_warning,
                },
                "framework": "Schwartz 2x2 scenario matrix: revenue growth x gross margin. Four scenarios: Bull (high/high), Land Grab (high/low), Cash Cow (low/high), Bear (low/low). Each scenario runs through the two-stage DCF model. The range of intrinsic values represents the uncertainty around the single-point DCF estimate. Simple mode (default) returns the range without probabilities; detailed mode (event_tree supplied) derives quadrant probabilities from the tree's root marginals - the earned upgrade on the analyst maturity ladder.",
            });

            Ok(fibo::enrich_with_ontology(output, "scenario_analysis"))
        }).await
    }
}

#[cfg(test)]
mod attribution_tests {
    use super::*;
    use std::collections::HashMap;

    fn positions(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    fn prices(pairs: &[(&str, f64)]) -> serde_json::Map<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
            .collect()
    }

    #[test]
    fn missing_end_price_nulls_return_instead_of_fabricating_total_loss() {
        // Pre-fix: p_end missing -> unwrap_or(0.0) -> (0-100)/100 = -100%.
        let (rows, gaps) = build_attribution_rows(
            &positions(&[("AAPL", 10.0)]),
            &positions(&[("AAPL", 10.0)]),
            &prices(&[("AAPL", 100.0)]),
            &prices(&[]),
        );
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].security_return.is_none(),
            "return must be unknown, not -1.0"
        );
        assert_eq!(gaps.missing_end_prices, vec!["AAPL".to_string()]);
        assert!(gaps.missing_start_prices.is_empty());
    }

    #[test]
    fn closed_position_with_missing_end_price_is_not_a_data_gap() {
        // Sold before the to-date: the end weight is 0 by construction, so a
        // missing end price is not reported as a gap.
        let (rows, gaps) = build_attribution_rows(
            &positions(&[("AAPL", 10.0)]),
            &positions(&[]),
            &prices(&[("AAPL", 100.0)]),
            &prices(&[]),
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0].security_return.is_none());
        assert!(gaps.missing_end_prices.is_empty());
    }

    #[test]
    fn missing_start_price_excludes_row_and_is_surfaced() {
        let (rows, gaps) = build_attribution_rows(
            &positions(&[("AAPL", 10.0), ("MSFT", 5.0)]),
            &positions(&[("AAPL", 10.0), ("MSFT", 5.0)]),
            &prices(&[("MSFT", 200.0)]),
            &prices(&[("MSFT", 220.0)]),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol, "MSFT");
        assert_eq!(gaps.missing_start_prices, vec!["AAPL".to_string()]);
    }

    #[test]
    fn present_prices_compute_return() {
        let (rows, gaps) = build_attribution_rows(
            &positions(&[("AAPL", 10.0)]),
            &positions(&[("AAPL", 10.0)]),
            &prices(&[("AAPL", 100.0)]),
            &prices(&[("AAPL", 110.0)]),
        );
        assert_eq!(rows.len(), 1);
        let ret = rows[0].security_return.expect("both prices present");
        assert!((ret - 0.10).abs() < 1e-9);
        assert_eq!(rows[0].mv_start, 1000.0);
        assert!(gaps.missing_start_prices.is_empty());
        assert!(gaps.missing_end_prices.is_empty());
    }
}
