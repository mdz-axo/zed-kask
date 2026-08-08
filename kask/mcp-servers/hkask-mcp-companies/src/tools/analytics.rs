//! Portfolio analytics and DCF valuation tools.
use super::portfolio::run_portfolio;
use crate::{
    CompaniesServer, StoredForecast, fibo, financial_model,
    portfolio::{PersistedForecast, TxType},
    scenarios, superforecast,
    types::{self, AttributionRequest, CharacteristicsRequest},
    validate_symbol,
};
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_types::time::now_rfc3339;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use uuid::Uuid;

#[tool_router(router = analytics_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "What moved the portfolio - each position's weight, return, and contribution, ranked by impact"
    )]
    pub async fn portfolio_attribution(
        &self,
        Parameters(req): Parameters<AttributionRequest>,
    ) -> String {
        execute_tool(self, "portfolio_attribution", async {
            // Get transactions and compute positions at start and end
            let portfolio_name = req.portfolio.clone();
            let txs = run_portfolio(self.portfolio.clone(), move |portfolio| {
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
                // Fetch historical prices around each date
                for (date, prices_map) in
                    [(&req.from, &mut prices_start), (&req.to, &mut prices_end)]
                {
                    match self.fetch(
                            "historical_price",
                            sym,
                            &[("from", date), ("to", date)],
                    )
                    .await
                    {
                        Ok(value) => {
                            let historical =
                                value.get("historical").and_then(|h| h.as_array());
                            if let Some(days) = historical
                                && let Some(day) = days.first()
                            {
                                let close = day
                                    .get("close")
                                    .or_else(|| day.get("adjClose"))
                                    .and_then(|v| v.as_f64());
                                if let Some(c) = close {
                                    prices_map
                                        .insert(sym.clone(), serde_json::Value::from(c));
                                }
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
            let mut rows = Vec::new();
            for (sym, shares) in &positions_start {
                let p_start = prices_start
                    .get(sym)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let p_end = prices_end
                    .get(sym)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                if p_start <= 0.0 {
                    continue;
                }
                let security_return = (p_end - p_start) / p_start;
                let mv_start = shares * p_start;
                rows.push((sym.clone(), mv_start, security_return));
            }

            // If over the cap, sort by market value descending and keep top 99.
            if rows.len() > MAX_HOLDINGS {
                rows.sort_by(|a, b| {
                    b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                });
                rows.truncate(MAX_HOLDINGS);
            }

            let total_mv: f64 = rows.iter().map(|(_, mv, _)| mv).sum();
            let mut attribution: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(sym, mv_start, ret)| {
                    let weight = if total_mv > 0.0 {
                        mv_start / total_mv
                    } else {
                        0.0
                    };
                    let contribution_bps = weight * ret * 10000.0;
                    let shares_end = positions_end.get(&sym).copied().unwrap_or(0.0);
                    let p_end = prices_end
                        .get(&sym)
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    serde_json::json!({
                        "symbol": sym,
                        "weight_start_pct": (weight * 100.0),
                        "weight_end_pct": if total_mv > 0.0 { shares_end * p_end / total_mv * 100.0 } else { 0.0 },
                        "security_return_pct": (ret * 100.0),
                        "contribution_bps": contribution_bps,
                        "gain_loss": mv_start * ret,
                    })
                })
                .collect();

            // Sort by absolute contribution
            attribution.sort_by(|a, b| {
                let ca = a["contribution_bps"].as_f64().unwrap_or(0.0).abs();
                let cb = b["contribution_bps"].as_f64().unwrap_or(0.0).abs();
                cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
            });

            Ok(fibo::enrich_with_ontology(serde_json::json!({
                "portfolio": req.portfolio,
                "from": req.from,
                "to": req.to,
                "attribution": attribution,
                "errors": errors,
                "fibo": {
                    "attribution_analysis": fibo::ATTRIBUTION_ANALYSIS,
                },
            }), "portfolio_attribution"))
        }).await
    }

    #[tool(
        description = "Weighted-average fundamentals of what the portfolio owns - valuation, profitability, leverage, growth, composition"
    )]
    pub async fn portfolio_characteristics(
        &self,
        Parameters(req): Parameters<CharacteristicsRequest>,
    ) -> String {
        execute_tool(self, "portfolio_characteristics", async {
            let portfolio_name = req.portfolio.clone();
            let symbols = run_portfolio(self.portfolio.clone(), move |portfolio| {
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
            let txs = run_portfolio(self.portfolio.clone(), move |portfolio| {
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

            // Fetch prices and market values
            let mut market_values = Vec::new();
            let mut errors = Vec::new();
            for sym in positions.keys() {
                match self.fetch("stock_quote", sym, &[]).await {
                    Ok(value) => {
                        let price = value
                            .as_array()
                            .and_then(|a| a.first())
                            .and_then(|q| q.get("price").and_then(|p| p.as_f64()))
                            .unwrap_or(0.0);
                        let shares = positions.get(sym).copied().unwrap_or(0.0);
                        market_values.push((sym.clone(), shares, price, shares * price));
                    }
                    Err(e) => {
                        errors.push(format!("{sym} quote: {}", e.to_json_string()));
                    }
                }
            }

            let total_mv: f64 = market_values.iter().map(|(_, _, _, mv)| mv).sum();
            if total_mv <= 0.0 {
                return Ok(serde_json::json!(
                    {"characteristics": {}, "message": "no market value"}
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
                                let fibo_uri = fibo::fmp_field_to_fibo(field).unwrap_or("unknown");
                                numeric_fields
                                    .entry(field.to_string())
                                    .or_insert_with(|| (Vec::new(), fibo_uri))
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
                            let fibo_uri = fibo::fmp_field_to_fibo(field).unwrap_or("unknown");
                            numeric_fields
                                .entry(field.to_string())
                                .or_insert_with(|| (Vec::new(), fibo_uri))
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
                        let fibo_uri =
                            fibo::fmp_field_to_fibo("financialLeverage").unwrap_or("unknown");
                        numeric_fields
                            .entry("financialLeverage".to_string())
                            .or_insert_with(|| (Vec::new(), fibo_uri))
                            .0
                            .push(crate::aggregation::WeightedValue { weight, value: lev });
                    }
                }
            }

            // Aggregate numeric fields using the requested method.
            let mut characteristics = serde_json::Map::new();
            for (field, (values, fibo_uri)) in &numeric_fields {
                let aggregated = crate::aggregation::aggregate(values, &req.aggregation);
                characteristics.insert(
                    field.clone(),
                    serde_json::json!({
                        "value": aggregated,
                        "fibo": fibo_uri,
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
    ) -> String {
        execute_tool(self, "dcf_valuation", async {
            validate_symbol(&req.symbol)?;
            if let Some(ref revision_of) = req.revision_of {
                let revision_of = revision_of.clone();
                let symbol = req.symbol.clone();
                run_portfolio(self.portfolio.clone(), move |portfolio| {
                    portfolio.validate_forecast_revision(&revision_of, &symbol)
                })
                .await?;
            }

            // Fetch all required financial statements
            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch("company_profile", &req.symbol, &[]).await;

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
            let profile_obj = profile.as_array().and_then(|a| a.first());

            if income_arr.is_none_or(|a| a.is_empty())
                || balance_arr.is_none_or(|a| a.is_empty())
                || cf_arr.is_none_or(|a| a.is_empty())
                || profile_obj.is_none()
            {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            }

            let income_data = income_arr.unwrap();
            let balance_data = balance_arr.unwrap();
            let cf_data = cf_arr.unwrap();
            let metrics_data: &[serde_json::Value] = metrics_arr.map_or(&[], |v| v);
            let profile_data = profile_obj.unwrap();

            // Build historical snapshot from API data
            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data - need at least 2 years of revenue"}));
            }

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            let current_price = profile_data.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let shares = hist.shares_outstanding;

            // Run the projection engine
            let model = financial_model::project_model(&hist, &assumptions, current_price);

            // Compute signal quality and emit Regulation span (G2: FinGPT low-SNR handling)
            let signal_quality = hist.signal_quality();
            crate::data_quality::emit_data_quality_span(
                &req.symbol, "dcf_valuation", &signal_quality,
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

            // Margin of safety
            let margin_of_safety = if current_price > 0.0 {
                (model.intrinsic_per_share - current_price) / current_price
            } else {
                0.0
            };

            // Build period summary for JSON output (all 13 line items)
            let period_summary: Vec<serde_json::Value> = model.periods.iter().map(|p| {
                serde_json::json!({
                    "period": p.period,
                    "year": p.year,
                    "revenue": p.revenue,
                    "cogs": p.cogs,
                    "gross_profit": p.gross_profit,
                    "da": p.da,
                    "ebit": p.ebit,
                    "tax": p.tax,
                    "nopat": p.nopat,
                    "capex": p.capex,
                    "change_in_nwc": p.change_in_nwc,
                    "free_cash_flow": p.free_cash_flow,
                    "discount_factor": p.discount_factor,
                    "present_value": p.present_value,
                })
            }).collect();

            let output = serde_json::json!({
                "symbol": req.symbol,
                "forecast_id": forecast_id,
                "revision_of": req.revision_of,
                "config": {
                    "stage1_years": assumptions.stage1_years,
                    "stage2_years": assumptions.total_years - assumptions.stage1_years,
                    "total_years": assumptions.total_years,
                    "discount_rate": assumptions.discount_rate,
                    "terminal_growth": assumptions.terminal_growth,
                    "revenue_growth": assumptions.revenue_growth,
                    "gross_margin": assumptions.gross_margin,
                    "da_to_revenue": assumptions.da_to_revenue,
                    "capex_to_revenue": assumptions.capex_to_revenue,
                    "nwc_to_revenue": assumptions.nwc_to_revenue,
                    "tax_rate": assumptions.tax_rate,
                },
                "history": {
                    "revenue_cagr": hist.revenue_cagr(),
                    "gross_margin": hist.gross_margin(),
                    "da_to_revenue": hist.da_to_revenue(),
                    "capex_to_revenue": hist.capex_to_revenue(),
                    "nwc_to_revenue": hist.nwc_to_revenue(),
                    "tax_rate": hist.tax_rate,
                    "latest_revenue": hist.latest_revenue(),
                    "shares_outstanding": shares,
                    "net_debt": hist.net_debt(),
                },
                "projections": period_summary,
                "valuation": {
                    "pv_cash_flows": model.periods.iter().map(|p| p.present_value).sum::<f64>(),
                    "terminal_value": model.terminal_value,
                    "terminal_pv": model.terminal_pv,
                    "enterprise_value": model.enterprise_value,
                    "net_debt": model.net_debt,
                    "equity_value": model.equity_value,
                    "intrinsic_per_share": model.intrinsic_per_share,
                    "current_price": current_price,
                    "margin_of_safety": margin_of_safety,
                },
                "data_quality": {
                    "overall_confidence": signal_quality.overall_confidence,
                    "revenue_growth": serde_json::json!(signal_quality.revenue_growth),
                    "gross_margin": serde_json::json!(signal_quality.gross_margin),
                    "da_to_revenue": serde_json::json!(signal_quality.da_to_revenue),
                    "capex_to_revenue": serde_json::json!(signal_quality.capex_to_revenue),
                    "nwc_to_revenue": serde_json::json!(signal_quality.nwc_to_revenue),
                    "tax_rate": serde_json::json!(signal_quality.tax_rate),
                },
                "framework": "Two-stage 11-line-item DCF: History-calibrated projections through income statement (revenue, COGS, D&A) and balance sheet (NWC, capex) to FCF. Terminal value via Gordon Growth perpetuity (capped at r - 0.5%). Enterprise value to equity bridge via net debt. Damodaran (2012) Investment Valuation. Use forecast_record with the forecast_id to decompose actual outcomes against these projections.",
            });

            Ok(fibo::enrich_with_ontology(output, "dcf_valuation"))
        }).await
    }

    #[tool(
        description = "Reverse DCF (Mauboussin's Expectations Investing). Solves for the revenue growth rate implied by the current stock price. \"What growth does the market expect?\" - compare to your own estimate to find mispricing. Default: 10yr model, 3yr stage 1, 7yr stage 2, 10% WACC."
    )]
    pub async fn reverse_dcf(
        &self,
        Parameters(req): Parameters<types::ReverseDcfRequest>,
    ) -> String {
        execute_tool(self, "reverse_dcf", async {
            validate_symbol(&req.symbol)?;

            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch("company_profile", &req.symbol, &[]).await;

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
            let profile_obj = profile.as_array().and_then(|a| a.first());

            if income_arr.is_none_or(|a| a.is_empty())
                || balance_arr.is_none_or(|a| a.is_empty())
                || cf_arr.is_none_or(|a| a.is_empty())
                || profile_obj.is_none()
            {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            }

            let income_data = income_arr.unwrap();
            let balance_data = balance_arr.unwrap();
            let cf_data = cf_arr.unwrap();
            let metrics_data: &[serde_json::Value] = metrics_arr.map_or(&[], |v| v);
            let profile_data = profile_obj.unwrap();

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data - need at least 2 years of revenue"}));
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

            let current_price = profile_data.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);

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
                "fibo": {
                    "implied_growth_rate": fibo::REVENUE_GROWTH_RATE,
                    "discount_rate": fibo::DISCOUNT_RATE,
                    "terminal_growth_rate": fibo::TERMINAL_GROWTH_RATE,
                    "enterprise_value": fibo::ENTERPRISE_VALUE,
                    "intrinsic_value_per_share": fibo::INTRINSIC_VALUE_PER_SHARE,
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
        description = "Schwartz 2x2 scenario analysis. Projects four scenarios (Bull, Land Grab, Cash Cow, Bear) based on revenue growth x profit margin axes. Runs DCF under each scenario and returns the intrinsic value range. Default axes: revenue_growth x profit_margin. Adjustable multipliers let you tune scenario severity."
    )]
    pub async fn scenario_analysis(
        &self,
        Parameters(req): Parameters<types::ScenarioAnalysisRequest>,
    ) -> String {
        execute_tool(self, "scenario_analysis", async {
            validate_symbol(&req.symbol)?;

            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch("company_profile", &req.symbol, &[]).await;

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
            let profile_obj = profile.as_array().and_then(|a| a.first());

            if income_arr.is_none_or(|a| a.is_empty())
                || balance_arr.is_none_or(|a| a.is_empty())
                || cf_arr.is_none_or(|a| a.is_empty())
                || profile_obj.is_none()
            {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            }

            let income_data = income_arr.unwrap();
            let balance_data = balance_arr.unwrap();
            let cf_data = cf_arr.unwrap();
            let metrics_data: &[serde_json::Value] = metrics_arr.map_or(&[], |v| v);
            let profile_data = profile_obj.unwrap();

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data - need at least 2 years of revenue"}));
            }

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            let current_price = profile_data.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);

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
                    "axis1": {"name": matrix.axis1.name, "fibo": matrix.axis1.fibo_concept, "baseline": matrix.axis1.baseline},
                    "axis2": {"name": matrix.axis2.name, "fibo": matrix.axis2.fibo_concept, "baseline": matrix.axis2.baseline},
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
                "fibo": {
                    "discount_rate": fibo::DISCOUNT_RATE,
                    "terminal_growth_rate": fibo::TERMINAL_GROWTH_RATE,
                    "enterprise_value": fibo::ENTERPRISE_VALUE,
                    "intrinsic_value_per_share": fibo::INTRINSIC_VALUE_PER_SHARE,
                    "margin_of_safety": fibo::MARGIN_OF_SAFETY,
                    "scenario_probability": fibo::SCENARIO_PROBABILITY,
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
