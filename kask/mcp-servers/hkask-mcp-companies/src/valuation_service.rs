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

use crate::data_quality::SignalQuality;
use crate::financial_model::{HistoricalSnapshot, ProjectedModel, ProjectionAssumptions};

/// Build the `dcf_valuation` response body from the projected model and
/// historical snapshot. Pure — no I/O, no API keys, no `CompaniesServer`.
///
/// `forecast_id` is the caller-generated UUID; `symbol` and `revision_of`
/// echo the request. `shares` is `hist.shares_outstanding` passed explicitly
/// so the caller can override it (it does not today, but the explicit
/// parameter keeps the function pure over `HistoricalSnapshot`).
pub fn build_dcf_response(
    symbol: &str,
    forecast_id: &str,
    revision_of: &Option<String>,
    model: &ProjectedModel,
    assumptions: &ProjectionAssumptions,
    hist: &HistoricalSnapshot,
    signal_quality: &SignalQuality,
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
            "tax_rate": hist.tax_rate(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::financial_model::{ProjectedLineItems, ProjectedModel};

    /// The response builder is pure — it can be tested with a fixture model
    /// and snapshot, no HTTP, no API keys, no `CompaniesServer`. This is the
    /// testability unlock Candidate B targets.
    #[test]
    fn build_dcf_response_shapes_period_summary_and_valuation() {
        let model = ProjectedModel {
            periods: vec![ProjectedLineItems {
                period: 1,
                year: 2025,
                revenue: 100.0,
                cogs: 60.0,
                gross_profit: 40.0,
                da: 5.0,
                ebit: 35.0,
                tax: 7.0,
                nopat: 28.0,
                capex: 10.0,
                change_in_nwc: 2.0,
                free_cash_flow: 16.0,
                discount_factor: 0.9,
                present_value: 14.4,
            }],
            terminal_value: 160.0,
            terminal_pv: 100.0,
            enterprise_value: 114.4,
            net_debt: 10.0,
            equity_value: 104.4,
            intrinsic_per_share: 10.0,
        };
        let assumptions = ProjectionAssumptions {
            stage1_years: 3,
            total_years: 10,
            discount_rate: 0.10,
            terminal_growth: 0.02,
            revenue_growth: 0.05,
            gross_margin: 0.40,
            da_to_revenue: 0.05,
            capex_to_revenue: 0.10,
            nwc_to_revenue: 0.02,
            tax_rate: 0.21,
        };
        let hist = HistoricalSnapshot {
            revenue: vec![("2024".to_string(), 100.0)],
            cogs: vec![("2024".to_string(), 60.0)],
            da: vec![("2024".to_string(), 5.0)],
            capex: vec![("2024".to_string(), 10.0)],
            current_assets: vec![],
            current_liabilities: vec![],
            cash: vec![],
            long_term_debt: vec![],
            shares_outstanding: 10.0,
            tax_rate: 0.21,
        };
        let signal_quality = SignalQuality {
            overall_confidence: 0.8,
            revenue_growth: 0.9,
            gross_margin: 0.9,
            da_to_revenue: 0.8,
            capex_to_revenue: 0.8,
            nwc_to_revenue: 0.8,
            tax_rate: 0.8,
        };

        let response = build_dcf_response(
            "TEST",
            "forecast-123",
            &None,
            &model,
            &assumptions,
            &hist,
            &signal_quality,
            8.0,
            10.0,
        );

        assert_eq!(response["symbol"], "TEST");
        assert_eq!(response["forecast_id"], "forecast-123");
        assert_eq!(response["valuation"]["intrinsic_per_share"], 10.0);
        assert_eq!(response["valuation"]["current_price"], 8.0);
        // margin_of_safety = (10 - 8) / 8 = 0.25
        assert_eq!(response["valuation"]["margin_of_safety"], 0.25);
        assert_eq!(response["projections"][0]["period"], 1);
        assert_eq!(response["projections"][0]["free_cash_flow"], 16.0);
        assert_eq!(response["data_quality"]["overall_confidence"], 0.8);
    }

    #[test]
    fn build_dcf_response_zero_price_yields_zero_margin_of_safety() {
        let model = ProjectedModel {
            periods: vec![],
            terminal_value: 0.0,
            terminal_pv: 0.0,
            enterprise_value: 0.0,
            net_debt: 0.0,
            equity_value: 0.0,
            intrinsic_per_share: 10.0,
        };
        let assumptions = ProjectionAssumptions {
            stage1_years: 3,
            total_years: 10,
            discount_rate: 0.10,
            terminal_growth: 0.02,
            revenue_growth: 0.05,
            gross_margin: 0.40,
            da_to_revenue: 0.05,
            capex_to_revenue: 0.10,
            nwc_to_revenue: 0.02,
            tax_rate: 0.21,
        };
        let hist = HistoricalSnapshot {
            revenue: vec![],
            cogs: vec![],
            da: vec![],
            capex: vec![],
            current_assets: vec![],
            current_liabilities: vec![],
            cash: vec![],
            long_term_debt: vec![],
            shares_outstanding: 0.0,
            tax_rate: 0.21,
        };
        let signal_quality = SignalQuality {
            overall_confidence: 0.5,
            revenue_growth: 0.5,
            gross_margin: 0.5,
            da_to_revenue: 0.5,
            capex_to_revenue: 0.5,
            nwc_to_revenue: 0.5,
            tax_rate: 0.5,
        };

        let response = build_dcf_response(
            "TEST",
            "id",
            &None,
            &model,
            &assumptions,
            &hist,
            &signal_quality,
            0.0,
            0.0,
        );
        assert_eq!(response["valuation"]["margin_of_safety"], 0.0);
    }
}
