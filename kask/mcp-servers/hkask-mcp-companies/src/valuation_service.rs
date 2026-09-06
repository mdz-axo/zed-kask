//! Shared typed financial acquisition, DCF preparation, and response assembly.
//!
//! Standalone and comparable DCF use the same history, guards and assumptions.
//! Tool handlers own response formatting and forecast persistence; other valuation
//! engines retain their distinct models and reuse only historical extraction.

use crate::data_quality::ModelInputQuality;
use crate::financial_model::{HistoricalSnapshot, ProjectedModel, ProjectionAssumptions};

/// Extract non-empty financial statement arrays and the profile object from
/// the raw provider responses. Returns `None` if any required array is empty
/// or missing, so callers can surface an "insufficient data" error without
/// panicky `unwrap()` calls on guarded `Option<&[Value]>`.
pub(crate) fn extract_historical_arrays<'a>(
    income: &'a serde_json::Value,
    balance: &'a serde_json::Value,
    cf: &'a serde_json::Value,
    metrics: &'a serde_json::Value,
    profile: &'a crate::CompanyProfile,
) -> Option<(
    &'a [serde_json::Value],
    &'a [serde_json::Value],
    &'a [serde_json::Value],
    &'a [serde_json::Value],
    &'a serde_json::Value,
)> {
    let income_data = income.as_array().filter(|a| !a.is_empty())?;
    let balance_data = balance.as_array().filter(|a| !a.is_empty())?;
    let cf_data = cf.as_array().filter(|a| !a.is_empty())?;
    let metrics_data: &[serde_json::Value] = metrics.as_array().map_or(&[], |v| v);
    let profile_data = profile.first()?;
    Some((
        income_data,
        balance_data,
        cf_data,
        metrics_data,
        profile_data,
    ))
}

/// Build the `dcf_valuation` response body from the projected model and
/// historical snapshot. Pure — no I/O, no API keys, no `CompaniesServer`.
///
/// `forecast_id` is the caller-generated UUID; `symbol` and `revision_of`
/// echo the request. `shares` is `hist.shares_outstanding` passed explicitly
/// so the caller can override it (it does not today, but the explicit
/// parameter keeps the function pure over `HistoricalSnapshot`).
pub(crate) fn build_dcf_response(
    symbol: &str,
    forecast_id: &str,
    revision_of: &Option<String>,
    model: &ProjectedModel,
    assumptions: &ProjectionAssumptions,
    hist: &HistoricalSnapshot,
    signal_quality: &ModelInputQuality,
    current_price: f64,
    shares: f64,
) -> serde_json::Value {
    let margin_of_safety = if current_price > 0.0 {
        (model.intrinsic_per_share - current_price) / current_price
    } else {
        0.0
    };

    let period_summary: Vec<serde_json::Value> = model
        .periods
        .iter()
        .map(|p| {
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
        })
        .collect();

    serde_json::json!({
        "symbol": symbol,
        "forecast_id": forecast_id,
        "revision_of": revision_of,
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
    })
}

/// Owns the normalized financial inputs so profile array/object details cannot
/// escape into DCF callers. Other models can reuse extraction without adopting DCF policy.
struct FinancialInputs<'a> {
    income: serde_json::Value,
    balance: serde_json::Value,
    cash_flow: serde_json::Value,
    metrics: crate::KeyMetrics,
    profile: &'a crate::CompanyProfile,
}

impl FinancialInputs<'_> {
    fn history(&self, symbol: &str) -> Result<HistoricalSnapshot, DcfPreparationError> {
        let Some((income, balance, cash_flow, metrics, profile)) = extract_historical_arrays(
            &self.income,
            &self.balance,
            &self.cash_flow,
            self.metrics.raw(),
            self.profile,
        ) else {
            return Err(DcfPreparationError::Unavailable(
                serde_json::json!({"symbol":symbol,"error":"insufficient data for DCF"}),
            ));
        };
        // The older model has a nominal share-count fallback. DCF preparation
        // must not turn missing shares into a seemingly valid per-share value.
        let shares = income
            .first()
            .and_then(|entry| {
                entry
                    .get("weightedAverageShsOutDil")
                    .or_else(|| entry.get("weightedAverageShsOut"))
            })
            .or_else(|| {
                metrics.first().and_then(|entry| {
                    entry
                        .get("weightedAverageShsOutDil")
                        .or_else(|| entry.get("weightedAverageShsOut"))
                })
            })
            .or_else(|| profile.get("sharesOutstanding"))
            .and_then(serde_json::Value::as_f64);
        if !shares.is_some_and(|value| value.is_finite() && value > 0.0) {
            return Err(DcfPreparationError::Unavailable(
                serde_json::json!({"symbol":symbol,"error":"missing or invalid shares outstanding for DCF"}),
            ));
        }
        let history =
            HistoricalSnapshot::from_api_json(income, balance, cash_flow, metrics, profile);
        if history.revenue.len() < 2 {
            return Err(DcfPreparationError::Unavailable(
                serde_json::json!({"symbol":symbol,"error":"insufficient historical data - need at least 2 years of revenue"}),
            ));
        }
        Ok(history)
    }
}

pub(crate) enum DcfPreparationError {
    Tool(hkask_mcp_server::server::McpToolError),
    Unavailable(serde_json::Value),
}

impl From<hkask_mcp_server::server::McpToolError> for DcfPreparationError {
    fn from(error: hkask_mcp_server::server::McpToolError) -> Self {
        Self::Tool(error)
    }
}

impl DcfPreparationError {
    pub(crate) fn into_tool_result(
        self,
    ) -> Result<serde_json::Value, hkask_mcp_server::server::McpToolError> {
        match self {
            Self::Tool(error) => Err(error),
            Self::Unavailable(value) => Ok(value),
        }
    }
}

pub(crate) struct PreparedDcf {
    pub history: HistoricalSnapshot,
    pub assumptions: ProjectionAssumptions,
    pub model: ProjectedModel,
    pub current_price: f64,
    pub provenance: serde_json::Value,
    pub warnings: Vec<String>,
}

impl PreparedDcf {
    pub(crate) fn margin_of_safety(&self) -> f64 {
        (self.model.intrinsic_per_share - self.current_price) / self.current_price
    }
}

/// expect: [P5] Standalone and comparable DCF agree for identical inputs and assumptions.
/// pre: typed profile and normalized statements; post: shared guards, history and projection.
pub(crate) async fn prepare_dcf(
    server: &crate::CompaniesServer,
    symbol: &str,
    profile: &crate::CompanyProfile,
    overrides: crate::types::ProjectionAssumptionOverrides,
) -> Result<PreparedDcf, DcfPreparationError> {
    if let Some(error) =
        crate::financial_model::financial_sector_guard(profile, symbol, "dcf_valuation")
    {
        return Err(DcfPreparationError::Unavailable(error));
    }
    let Some(current_price) = profile
        .price()
        .filter(|price| price.is_finite() && *price > 0.0)
    else {
        return Err(DcfPreparationError::Unavailable(
            serde_json::json!({"symbol":symbol,"error":"missing or invalid current price for DCF"}),
        ));
    };
    let (income, balance, cash_flow, metrics) = tokio::try_join!(
        server.fetch_response("income_statement", symbol, &[("limit", "5")]),
        server.fetch_response("balance_sheet", symbol, &[("limit", "5")]),
        server.fetch_response("cash_flow_statement", symbol, &[("limit", "5")]),
        server.fetch_response("key_metrics", symbol, &[("limit", "5")]),
    )?;
    let provenance = serde_json::json!({
        "company_profile": profile.provider(),
        "income_statement": income.provider, "balance_sheet": balance.provider,
        "cash_flow_statement": cash_flow.provider, "key_metrics": metrics.provider,
    });
    let warnings = income
        .warnings
        .into_iter()
        .chain(balance.warnings)
        .chain(cash_flow.warnings)
        .chain(metrics.warnings)
        .collect();
    let inputs = FinancialInputs {
        income: income.value,
        balance: balance.value,
        cash_flow: cash_flow.value,
        metrics: crate::KeyMetrics::from_raw(metrics.value),
        profile,
    };
    let history = inputs.history(symbol)?;
    let assumptions = ProjectionAssumptions::from_history_with_overrides(&history, overrides)
        .map_err(|error| {
            hkask_mcp_server::server::McpToolError::invalid_argument(error.to_string())
        })?;
    let model = crate::financial_model::project_model(&history, &assumptions, current_price);
    Ok(PreparedDcf {
        history,
        assumptions,
        model,
        current_price,
        provenance,
        warnings,
    })
}
