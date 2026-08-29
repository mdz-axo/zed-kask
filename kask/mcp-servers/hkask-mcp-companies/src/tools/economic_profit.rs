//! Economic Profit valuation tools — Residual Income Model (Bergen et al. 2025).
//!
//! Tools:
//! - `ep_valuation` — Full EP-based valuation with competitive fade and IVM ratio.
use crate::{CompaniesServer, economic_profit, fibo, types, validate_symbol};
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = economic_profit_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Economic Profit valuation (Bergen et al. 2025, Financial Analysts Journal). Values a company as Book Value + PV(Future Economic Profits) with competitive fade. Economic Profit = (ROIC - WACC) × Invested Capital. The IVM ratio (Intrinsic Value / Market Cap) is the primary screening metric. Decomposes value into % from assets-in-place vs % from competitive advantage. Moat classification from moat_check determines how long economic profits persist before competitors erode them."
    )]
    pub async fn ep_valuation(
        &self,
        Parameters(req): Parameters<types::EpValuationRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool_semantic(self, "ep_valuation", Self::ontology_anchor("ep_valuation"), async {
            validate_symbol(&req.symbol)?;

            let income_result = self
                .fetch("income_statement", &req.symbol, &[("limit", "5")])
                .await;
            let balance_result = self
                .fetch("balance_sheet", &req.symbol, &[("limit", "5")])
                .await;
            let metrics_result = self.fetch_key_metrics(&req.symbol, 5).await;
            let profile_result = self.fetch_profile(&req.symbol).await;

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
                    return Err(e);
                }
            };

            let income_arr = income.as_array();
            let balance_arr = balance.as_array();
            let profile_obj = profile.raw().as_array().and_then(|a| a.first());

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

            // Sector-aware routing: financial-sector companies (banks, insurance,
            // investment firms) require equity-based valuation because debt is
            // raw material, not a source of capital. ROIC, WACC, and invested
            // capital are not meaningful for these firms.
            //
            // Source: Damodaran, A. (2002). Investment Valuation (2nd ed.),
            // Ch. 21: "Valuing Financial Service Firms" -- "it makes far more
            // sense to value equity directly at financial service firms, rather
            // than the entire firm."
            //
            // Source: Damodaran, A. (2014). Applied Corporate Finance (4th ed.),
            // Ch. 4: "Equity EVA = (Return on Equity - Cost of Equity) x
            // (Equity Invested in Project or Firm)"
            //
            // For financial-sector firms, we pass ROE as "roic", cost of equity
            // as "wacc", and book value of equity as "invested_capital" to the
            // same value_economic_profit function. The math is identical --
            // (return - cost) x capital discounted over time -- only the inputs
            // and their economic meaning change.
            let is_financial = crate::financial_model::is_financial_sector(&profile);

            let (valuation, output) = if is_financial {
                let Some(inputs) = extract_ep_inputs_equity(
                    income_data,
                    balance_data,
                    Some(metrics.years()),
                    profile_data,
                    &req,
                )? else {
                    return Ok(serde_json::json!({
                        "symbol": req.symbol,
                        "error": "Cannot compute ROE or cost of equity — insufficient data for equity-based valuation"
                    }));
                };
                let v = economic_profit::value_economic_profit(
                    inputs.book_value,
                    inputs.roe,
                    inputs.book_value,
                    inputs.coe,
                    inputs.shares_outstanding,
                    inputs.current_price,
                    inputs.fade_horizon,
                    inputs.stage1_years,
                    inputs.equity_growth_rate,
                    inputs.roe_trend,
                    inputs.roe_variability,
                );
                let out = build_ep_response_equity(&v, &req.symbol, profile_data);
                (v, out)
            } else {
                let Some(inputs) = extract_ep_inputs(
                    income_data,
                    balance_data,
                    Some(metrics.years()),
                    profile_data,
                    &req,
                )? else {
                    return Ok(serde_json::json!({
                        "symbol": req.symbol,
                        "error": "Cannot compute ROIC — insufficient income statement or balance sheet data"
                    }));
                };
                let v = economic_profit::value_economic_profit(
                    inputs.book_value,
                    inputs.roic,
                    inputs.invested_capital,
                    inputs.wacc,
                    inputs.shares_outstanding,
                    inputs.current_price,
                    inputs.fade_horizon,
                    inputs.stage1_years,
                    inputs.ic_growth_rate,
                    inputs.roic_trend,
                    inputs.roic_variability,
                );
                let out = build_ep_response(&v, &inputs, &req.symbol);
                (v, out)
            };

            let _ = valuation;
            Ok(fibo::enrich_with_ontology(output, "ep_valuation"))
        })
        .await
    }
}

struct EpInputs {
    roic: f64,
    book_value: f64,
    raw_book_value: f64,
    treasury_stock: f64,
    invested_capital: f64,
    raw_invested_capital: f64,
    current_price: f64,
    shares_outstanding: f64,
    wacc: f64,
    fade_horizon: economic_profit::FadeHorizon,
    stage1_years: u8,
    ic_growth_rate: f64,
    roic_trend: f64,
    roic_variability: f64,
}

fn extract_ep_inputs(
    income_data: &[serde_json::Value],
    balance_data: &[serde_json::Value],
    metrics_arr: Option<&[serde_json::Value]>,
    profile_data: &serde_json::Value,
    req: &types::EpValuationRequest,
) -> Result<Option<EpInputs>, McpToolError> {
    let latest_income = income_data.first();
    let latest_balance = balance_data.first();
    let latest_metrics = metrics_arr.and_then(|a| a.first());

    let roic = latest_metrics
        .and_then(economic_profit::extract_roic_from_metrics)
        .or_else(|| {
            let ebit = latest_income.and_then(economic_profit::extract_ebit);
            let ic = latest_balance.and_then(economic_profit::extract_invested_capital);
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
        return Ok(None);
    };

    let raw_book_value = latest_balance
        .and_then(economic_profit::extract_book_value)
        .unwrap_or(0.0);
    let book_value = latest_balance
        .and_then(economic_profit::adj_book_value)
        .unwrap_or(raw_book_value);
    let treasury_stock = latest_balance
        .map(economic_profit::extract_treasury_stock)
        .unwrap_or(0.0);

    let raw_invested_capital = latest_metrics
        .and_then(economic_profit::extract_invested_capital_from_metrics)
        .or_else(|| latest_balance.and_then(economic_profit::extract_invested_capital))
        .unwrap_or(0.0);
    let invested_capital = latest_balance
        .and_then(economic_profit::adj_invested_capital)
        .unwrap_or(raw_invested_capital);

    let current_price = profile_data
        .get("price")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let shares_outstanding = latest_income
        .and_then(|m| {
            m.get("weightedAverageShsOutDil")
                .or_else(|| m.get("weightedAverageShsOut"))
                .and_then(|v| v.as_f64())
        })
        .or_else(|| {
            latest_metrics.and_then(|m| {
                m.get("weightedAverageShsOutDil")
                    .or_else(|| m.get("weightedAverageShsOut"))
                    .and_then(|v| v.as_f64())
            })
        })
        .or_else(|| {
            profile_data
                .get("sharesOutstanding")
                .and_then(|v| v.as_f64())
        })
        .unwrap_or(1_000.0);

    let wacc = req.wacc.unwrap_or(0.10);
    let fade_horizon = req
        .moat_override
        .or(req.moat_result)
        .unwrap_or(economic_profit::FadeHorizon::Default);
    let stage1_years = req.stage1_years.unwrap_or(3);
    let ic_growth_rate = req.ic_growth_rate.unwrap_or(0.0);

    let (roic_trend, roic_variability) =
        compute_roic_trend_variability(metrics_arr.map_or(&[], |v| v));

    Ok(Some(EpInputs {
        roic,
        book_value,
        raw_book_value,
        treasury_stock,
        invested_capital,
        raw_invested_capital,
        current_price,
        shares_outstanding,
        wacc,
        fade_horizon,
        stage1_years,
        ic_growth_rate,
        roic_trend,
        roic_variability,
    }))
}

fn build_ep_response(
    valuation: &economic_profit::EpValuation,
    inputs: &EpInputs,
    symbol: &str,
) -> serde_json::Value {
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

    serde_json::json!({
        "symbol": symbol,
        "framework": "Residual Income Model (Bergen et al. 2025, FAJ). IV = BV + PV(Future Economic Profits). Economic Profit = (ROIC - WACC) × Invested Capital. Competitive fade: economic profits decay to zero as competitors erode advantage. IVM ratio below 1.0 suggests undervaluation.",
        "inputs": {
            "book_value": valuation.book_value,
            "book_value_raw": inputs.raw_book_value,
            "treasury_stock": inputs.treasury_stock,
            "ts_adjustment": 2.0 * inputs.treasury_stock,
            "invested_capital": valuation.invested_capital,
            "invested_capital_raw": inputs.raw_invested_capital,
            "roic": valuation.current_roic,
            "wacc": valuation.wacc,
            "roic_wacc_spread": valuation.roic_wacc_spread,
            "ic_growth_rate": valuation.ic_growth_rate,
            "shares_outstanding": inputs.shares_outstanding,
            "current_price": valuation.current_price,
            "fade_horizon": format!("{:?}", valuation.fade_horizon),
            "base_fade_years": valuation.base_fade_years,
            "adjusted_fade_years": valuation.fade_years,
            "stage1_years": valuation.stage1_years,
            "roic_trend": inputs.roic_trend,
            "roic_variability": inputs.roic_variability,
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
            "treasury_stock_abs": inputs.treasury_stock,
            "equity_adjustment": 2.0 * inputs.treasury_stock,
        },
    })
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

// ── Equity-based EP inputs (financial-sector firms) ────────────────────────────

struct EpEquityInputs {
    roe: f64,
    coe: f64,
    book_value: f64,
    current_price: f64,
    shares_outstanding: f64,
    fade_horizon: economic_profit::FadeHorizon,
    stage1_years: u8,
    equity_growth_rate: f64,
    roe_trend: f64,
    roe_variability: f64,
}

fn extract_ep_inputs_equity(
    income_data: &[serde_json::Value],
    balance_data: &[serde_json::Value],
    metrics_arr: Option<&[serde_json::Value]>,
    profile_data: &serde_json::Value,
    req: &types::EpValuationRequest,
) -> Result<Option<EpEquityInputs>, McpToolError> {
    let latest_income = income_data.first();
    let latest_balance = balance_data.first();
    let latest_metrics = metrics_arr.and_then(|a| a.first());

    // ROE: prefer pre-computed from key_metrics, fall back to computed.
    let roe = latest_metrics
        .and_then(economic_profit::extract_roe_from_metrics)
        .or_else(|| {
            let net_income = latest_income.and_then(economic_profit::extract_net_income);
            let bv = latest_balance.and_then(economic_profit::extract_book_value);
            match (net_income, bv) {
                (Some(ni), Some(bv)) => economic_profit::compute_roe(ni, bv),
                _ => None,
            }
        });

    let Some(roe) = roe else {
        return Ok(None);
    };

    // Book value of equity (no treasury stock adjustment for financial firms —
    // the adjustment is an hKask non-standard treatment designed for industrial
    // firms where buybacks build organizational capital. For banks, regulatory
    // capital ratios are computed on unadjusted book value, so we use raw BV.)
    let book_value = latest_balance
        .and_then(economic_profit::extract_book_value)
        .unwrap_or(0.0);

    if book_value <= 0.0 {
        return Ok(None);
    }

    // Cost of equity via CAPM: COE = rf + beta x ERP.
    // Beta from profile; rf and ERP from request overrides or defaults.
    let beta = economic_profit::extract_beta(profile_data).unwrap_or(1.0);
    let coe = economic_profit::cost_of_equity(beta, req.risk_free_rate, req.equity_risk_premium);

    let current_price = profile_data
        .get("price")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let shares_outstanding = latest_income
        .and_then(|m| {
            m.get("weightedAverageShsOutDil")
                .or_else(|| m.get("weightedAverageShsOut"))
                .and_then(|v| v.as_f64())
        })
        .or_else(|| {
            latest_metrics.and_then(|m| {
                m.get("weightedAverageShsOutDil")
                    .or_else(|| m.get("weightedAverageShsOut"))
                    .and_then(|v| v.as_f64())
            })
        })
        .or_else(|| {
            profile_data
                .get("sharesOutstanding")
                .and_then(|v| v.as_f64())
        })
        .unwrap_or(1_000.0);

    let fade_horizon = req
        .moat_override
        .or(req.moat_result)
        .unwrap_or(economic_profit::FadeHorizon::Default);
    let stage1_years = req.stage1_years.unwrap_or(3);
    let equity_growth_rate = req.ic_growth_rate.unwrap_or(0.0);

    let (roe_trend, roe_variability) =
        compute_roe_trend_variability(metrics_arr.map_or(&[], |v| v));

    Ok(Some(EpEquityInputs {
        roe,
        coe,
        book_value,
        current_price,
        shares_outstanding,
        fade_horizon,
        stage1_years,
        equity_growth_rate,
        roe_trend,
        roe_variability,
    }))
}

/// Compute ROE trend and variability from historical key_metrics.
fn compute_roe_trend_variability(metrics: &[serde_json::Value]) -> (f64, f64) {
    let roes: Vec<f64> = metrics
        .iter()
        .filter_map(|m| m.get("returnOnEquity").and_then(|v| v.as_f64()))
        .collect();

    let variability = if roes.len() >= 2 {
        let mean = roes.iter().sum::<f64>() / roes.len() as f64;
        let variance =
            roes.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (roes.len() - 1) as f64;
        let std_dev = variance.sqrt();
        if mean.abs() > 1e-10 {
            std_dev / mean.abs()
        } else {
            0.0
        }
    } else {
        0.0
    };

    let trend = if roes.len() >= 3 {
        let diffs: Vec<f64> = roes.windows(2).map(|w| w[1] - w[0]).collect();
        let avg_diff = diffs.iter().sum::<f64>() / diffs.len() as f64;
        let mean_abs = roes.iter().map(|v| v.abs()).sum::<f64>() / roes.len() as f64;
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

/// Build the EP response for equity-based valuation (financial-sector firms).
///
/// Uses equity-based labels (ROE, cost of equity, book value of equity) instead
/// of the ROIC/WACC/invested-capital labels from the industrial-firm path.
/// Includes source citations from Damodaran's Investment Valuation (Ch. 21)
/// and Applied Corporate Finance (Ch. 4).
fn build_ep_response_equity(
    valuation: &economic_profit::EpValuation,
    symbol: &str,
    profile_data: &serde_json::Value,
) -> serde_json::Value {
    let period_summary: Vec<serde_json::Value> = valuation
        .periods
        .iter()
        .map(|p| {
            serde_json::json!({
                "period": p.period,
                "book_value_of_equity": p.invested_capital,
                "roe": p.roic,
                "cost_of_equity": p.wacc,
                "excess_equity_return": p.economic_profit,
                "discount_factor": p.discount_factor,
                "present_value": p.present_value,
            })
        })
        .collect();

    let sector = profile_data
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let industry = profile_data
        .get("industry")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let beta = economic_profit::extract_beta(profile_data).unwrap_or(1.0);

    serde_json::json!({
        "symbol": symbol,
        "framework": "Equity-based Excess Return Model (Damodaran, Investment Valuation Ch. 21). Value of Equity = Book Value of Equity + PV(Excess Equity Returns). Excess Equity Return = (ROE - Cost of Equity) x Book Value of Equity. For financial-sector firms, debt is raw material, not a source of capital — ROIC and WACC are not meaningful. Competitive fade: excess returns decay to zero as competitors erode advantage.",
        "sector": sector,
        "industry": industry,
        "valuation_approach": "equity-based",
        "inputs": {
            "book_value_of_equity": valuation.book_value,
            "roe": valuation.current_roic,
            "cost_of_equity": valuation.wacc,
            "roe_coe_spread": valuation.roic_wacc_spread,
            "beta": beta,
            "equity_growth_rate": valuation.ic_growth_rate,
            "shares_outstanding": valuation.market_cap / valuation.current_price.max(1e-10),
            "current_price": valuation.current_price,
            "fade_horizon": format!("{:?}", valuation.fade_horizon),
            "base_fade_years": valuation.base_fade_years,
            "adjusted_fade_years": valuation.fade_years,
            "stage1_years": valuation.stage1_years,
        },
        "valuation": {
            "pv_excess_equity_returns": valuation.pv_economic_profits,
            "intrinsic_value_of_equity": valuation.intrinsic_value,
            "intrinsic_per_share": valuation.intrinsic_per_share,
            "market_cap": valuation.market_cap,
            "ivm_ratio": valuation.ivm_ratio,
            "margin_of_safety": valuation.margin_of_safety,
        },
        "decomposition": {
            "pct_from_book_value": valuation.pct_from_book_value,
            "pct_from_excess_returns": valuation.pct_from_economic_profits,
            "pct_from_book_value_pct": format!("{:.1}%", valuation.pct_from_book_value * 100.0),
            "pct_from_excess_returns_pct": format!("{:.1}%", valuation.pct_from_economic_profits * 100.0),
        },
        "signal": {
            "valuation": valuation.signal.valuation,
            "profitability": valuation.signal.profitability,
            "composition": valuation.signal.composition,
            "summary": valuation.signal.summary,
        },
        "projections": period_summary,
        "sources": [
            "Damodaran, A. (2002). Investment Valuation (2nd ed.), Ch. 21: Valuing Financial Service Firms. \"Given the difficulty associated with defining total capital in a financial service firm, it makes far more sense to focus on just equity when using an excess return model.\"",
            "Damodaran, A. (2014). Applied Corporate Finance (4th ed.), Ch. 4: \"Equity EVA = (Return on Equity - Cost of Equity) x (Equity Invested in Project or Firm)\"",
            "Bergen, Franzoni, Obrycki, Resendes (2025). Intrinsic Value: A Solution to the Declining Performance of Value Strategies. Financial Analysts Journal."
        ],
    })
}
