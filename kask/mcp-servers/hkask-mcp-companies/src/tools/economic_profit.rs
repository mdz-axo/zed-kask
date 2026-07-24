//! Economic Profit valuation tools — Residual Income Model (Bergen et al. 2025).
//!
//! Tools:
//! - `ep_valuation` — Full EP-based valuation with competitive fade and IVM ratio.
use crate::{CompaniesServer, economic_profit, fibo, types, validate_symbol};
use hkask_mcp_server::server::execute_tool;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = economic_profit_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Economic Profit valuation (Bergen et al. 2025, Financial Analysts Journal). Values a company as Book Value + PV(Future Economic Profits) with competitive fade. Economic Profit = (ROIC - WACC) × Invested Capital. The IVM ratio (Intrinsic Value / Market Cap) is the primary screening metric. Decomposes value into % from assets-in-place vs % from competitive advantage. Moat classification from moat_check determines how long economic profits persist before competitors erode them."
    )]
    pub async fn ep_valuation(
        &self,
        Parameters(req): Parameters<types::EpValuationRequest>,
    ) -> String {
        execute_tool(self, "ep_valuation", async {
            validate_symbol(&req.symbol)?;

            // Fetch financial data
            let income_result = self
                .fetch("income_statement", &req.symbol, &[("limit", "5")])
                .await;
            let balance_result = self
                .fetch("balance_sheet", &req.symbol, &[("limit", "5")])
                .await;
            let metrics_result = self
                .fetch("key_metrics", &req.symbol, &[("limit", "5")])
                .await;
            let profile_result = self.fetch("company_profile", &req.symbol, &[]).await;

            let (income, balance, metrics, profile) = match (
                income_result,
                balance_result,
                metrics_result,
                profile_result,
            ) {
                (Ok(inc), Ok(bal), Ok(m), Ok(p)) => (inc, bal, m, p),
                (Err(e), _, _, _)
                | (_, Err(e), _, _)
                | (_, _, Err(e), _)
                | (_, _, _, Err(e)) => {
                    self.record_experience(
                        "ep_valuation",
                        &format!("symbol={}", req.symbol),
                        "error",
                        serde_json::json!({"error": e.to_json_string()}),
                    );
                    return Err(e);
                }
            };

            let income_arr = income.as_array();
            let balance_arr = balance.as_array();
            let metrics_arr = metrics.as_array();
            let profile_obj = profile.as_array().and_then(|a| a.first());

            if income_arr.is_none_or(|a| a.is_empty())
                || balance_arr.is_none_or(|a| a.is_empty())
                || profile_obj.is_none()
            {
                return Ok(serde_json::json!({
                    "symbol": req.symbol,
                    "error": "insufficient data — need income statement, balance sheet, and profile"
                }));
            }

            let income_data = income_arr.unwrap();
            let balance_data = balance_arr.unwrap();
            let profile_data = profile_obj.unwrap();

            // ── Extract key inputs ──────────────────────────────────────

            // Latest income statement
            let latest_income = income_data.first();
            // Latest balance sheet
            let latest_balance = balance_data.first();
            // Latest key metrics
            let latest_metrics = metrics_arr.and_then(|a| a.first());

            // Extract ROIC: prefer key_metrics (pre-computed), fall back to manual computation
            let roic = latest_metrics
                .and_then(economic_profit::extract_roic_from_metrics)
                .or_else(|| {
                    let ebit = latest_income.and_then(economic_profit::extract_ebit);
                    let ic =
                        latest_balance.and_then(economic_profit::extract_invested_capital);
                    let tax_rate = profile_data
                        .get("taxRate")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.21);
                    match (ebit, ic) {
                        (Some(e), Some(i)) => economic_profit::compute_roic(e, tax_rate, i),
                        _ => None,
                    }
                });

            let Some(roic) = roic else {
                return Ok(serde_json::json!({
                    "symbol": req.symbol,
                    "error": "Cannot compute ROIC — insufficient income statement or balance sheet data"
                }));
            };

            // Extract book value with hKask treasury stock adjustment
            let raw_book_value = latest_balance
                .and_then(economic_profit::extract_book_value)
                .unwrap_or(0.0);
            let book_value = latest_balance
                .and_then(economic_profit::adj_book_value)
                .unwrap_or(raw_book_value);
            let treasury_stock = latest_balance
                .map(economic_profit::extract_treasury_stock)
                .unwrap_or(0.0);

            // Extract invested capital with hKask treasury stock adjustment
            let raw_invested_capital = latest_metrics
                .and_then(economic_profit::extract_invested_capital_from_metrics)
                .or_else(|| {
                    latest_balance
                        .and_then(economic_profit::extract_invested_capital)
                })
                .unwrap_or(0.0);
            let invested_capital = latest_balance
                .and_then(economic_profit::adj_invested_capital)
                .unwrap_or(raw_invested_capital);

            // Current price
            let current_price = profile_data
                .get("price")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            // Shares outstanding: prefer diluted, fall back to basic
            let shares_outstanding = latest_metrics
                .and_then(|m| {
                    m.get("weightedAverageShsOutDil")
                        .or_else(|| m.get("weightedAverageShsOut"))
                        .and_then(|v| v.as_f64())
                })
                .or_else(|| {
                    profile_data
                        .get("sharesOutstanding")
                        .and_then(|v| v.as_f64())
                })
                .unwrap_or(1_000.0);

            // WACC
            let wacc = req.wacc.unwrap_or(0.10);

            // Moat classification for fade horizon
            let fade_horizon = req.moat_override
                .or(req.moat_result)
                .unwrap_or(economic_profit::FadeHorizon::Default);

            let stage1_years = req.stage1_years.unwrap_or(3);
            let ic_growth_rate = req.ic_growth_rate.unwrap_or(0.0);

            // Compute ROIC trend and variability from historical metrics
            let (roic_trend, roic_variability) = compute_roic_trend_variability(
                metrics_arr.map_or(&[], |v| v),
            );

            // ── Run EP valuation ────────────────────────────────────────

            let valuation = economic_profit::value_economic_profit(
                book_value,
                roic,
                invested_capital,
                wacc,
                shares_outstanding,
                current_price,
                fade_horizon,
                stage1_years,
                ic_growth_rate,
                roic_trend,
                roic_variability,
            );

            // ── Build output ────────────────────────────────────────────

            let period_summary: Vec<serde_json::Value> = valuation
                .periods
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "period": p.period,
                        "invested_capital": p.invested_capital,
                        "roic": p.roic,
                        "wacc": p.wacc,
                        "economic_profit": p.economic_profit,
                        "discount_factor": p.discount_factor,
                        "present_value": p.present_value,
                    })
                })
                .collect();

            let output = serde_json::json!({
                "symbol": req.symbol,
                "framework": "Residual Income Model (Bergen et al. 2025, FAJ). IV = BV + PV(Future Economic Profits). Economic Profit = (ROIC - WACC) × Invested Capital. Competitive fade: economic profits decay to zero as competitors erode advantage. IVM ratio below 1.0 suggests undervaluation.",
                "inputs": {
                    "book_value": valuation.book_value,
                    "book_value_raw": raw_book_value,
                    "treasury_stock": treasury_stock,
                    "ts_adjustment": 2.0 * treasury_stock,
                    "invested_capital": valuation.invested_capital,
                    "invested_capital_raw": raw_invested_capital,
                    "roic": valuation.current_roic,
                    "wacc": valuation.wacc,
                    "roic_wacc_spread": valuation.roic_wacc_spread,
                    "ic_growth_rate": valuation.ic_growth_rate,
                    "shares_outstanding": shares_outstanding,
                    "current_price": valuation.current_price,
                    "fade_horizon": format!("{:?}", valuation.fade_horizon),
                    "base_fade_years": valuation.base_fade_years,
                    "adjusted_fade_years": valuation.fade_years,
                    "stage1_years": valuation.stage1_years,
                    "roic_trend": roic_trend,
                    "roic_variability": roic_variability,
                },
                "valuation": {
                    "pv_economic_profits": valuation.pv_economic_profits,
                    "intrinsic_value": valuation.intrinsic_value,
                    "intrinsic_per_share": valuation.intrinsic_per_share,
                    "market_cap": valuation.market_cap,
                    "ivm_ratio": valuation.ivm_ratio,
                    "margin_of_safety": valuation.margin_of_safety,
                },
                "decomposition": {
                    "pct_from_book_value": valuation.pct_from_book_value,
                    "pct_from_economic_profits": valuation.pct_from_economic_profits,
                    "pct_from_book_value_pct": format!("{:.1}%", valuation.pct_from_book_value * 100.0),
                    "pct_from_economic_profits_pct": format!("{:.1}%", valuation.pct_from_economic_profits * 100.0),
                },
                "signal": {
                    "valuation": valuation.signal.valuation,
                    "profitability": valuation.signal.profitability,
                    "composition": valuation.signal.composition,
                    "summary": valuation.signal.summary,
                },
                "projections": period_summary,
                "fibo": {
                    "intrinsic_value_per_share": fibo::INTRINSIC_VALUE_PER_SHARE,
                    "return_on_invested_capital": fibo::RETURN_ON_INVESTED_CAPITAL,
                    "discount_rate": fibo::DISCOUNT_RATE,
                    "book_value": fibo::TOTAL_EQUITY,
                    "treasury_stock": fibo::TREASURY_STOCK,
                    "margin_of_safety": fibo::MARGIN_OF_SAFETY,
                },
                "balance_sheet_adjustment": {
                    "method": "hKask non-standard treatment: Treasury Stock is treated as committed capital, increasing Owner's Equity, Invested Capital, and Total Assets by 2× |treasury stock|. Intangible assets are correspondingly increased to preserve A = L + E.",
                    "treasury_stock_abs": treasury_stock,
                    "equity_adjustment": 2.0 * treasury_stock,
                },
            });

            self.record_experience(
                "ep_valuation",
                &format!("symbol={}", req.symbol),
                "success",
                output.clone(),
            );
            Ok(output)
        })
        .await
    }
}

/// Compute ROIC trend and variability from historical key_metrics.
///
/// ROIC trend: recent trajectory (positive = improving).
/// ROIC variability: coefficient of variation across periods.
fn compute_roic_trend_variability(metrics: &[serde_json::Value]) -> (f64, f64) {
    let roics: Vec<f64> = metrics
        .iter()
        .filter_map(|m| m.get("roic").and_then(|v| v.as_f64()))
        .collect();

    let variability = if roics.len() >= 2 {
        let mean = roics.iter().sum::<f64>() / roics.len() as f64;
        let variance =
            roics.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (roics.len() - 1) as f64;
        let std_dev = variance.sqrt();
        if mean.abs() > 1e-10 {
            std_dev / mean.abs()
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Trend: average year-over-year change in ROIC (normalized by mean)
    let trend = if roics.len() >= 3 {
        let diffs: Vec<f64> = roics.windows(2).map(|w| w[1] - w[0]).collect();
        let avg_diff = diffs.iter().sum::<f64>() / diffs.len() as f64;
        let mean_abs = roics.iter().map(|v| v.abs()).sum::<f64>() / roics.len() as f64;
        if mean_abs > 1e-10 {
            avg_diff / mean_abs
        } else {
            0.0
        }
    } else {
        0.0
    };

    (trend, variability)
}
