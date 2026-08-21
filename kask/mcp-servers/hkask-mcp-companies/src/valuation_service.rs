//! Valuation response assembly — pure functions over typed inputs.
//!
//! Extracts the JSON-response shaping logic from the tool handlers so it is
//! testable without a `reqwest::Client` or API keys. Each function takes the
//! already-fetched domain inputs (`HistoricalSnapshot`, `ProjectedModel`,
//! `ProjectionAssumptions`, `SignalQuality`) and returns the shaped
//! `serde_json::Value` the tool handler enriches with FIBO ontology.
//!
//! The tool handler retains only: fetch, validate, persist, and the
//! `execute_tool_semantic` span. The assembly lives here.

use crate::data_quality::ModelInputQuality;
use crate::financial_model::{HistoricalSnapshot, ProjectedModel, ProjectionAssumptions};

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
