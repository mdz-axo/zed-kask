//! Macaulay-style equity duration — extracted from `financial_model.rs`
//! (deep-module split: the cash-flow-weighted duration computation is a pure
//! function of a `ProjectedModel`, independent of the projection machinery).

use serde::{Deserialize, Serialize};

use super::ProjectedModel;

/// Cash-flow-weighted (Macaulay-style) equity duration of a projected model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityDuration {
    /// D_e = Σ_t t·PV(CF_t) / Σ_t PV(CF_t) in years. The sum runs over every
    /// projected year plus the terminal value treated as a single cash flow at
    /// the horizon year (t = total_years) — the terminal value *is* a cash flow
    /// claim on the horizon date, so it enters the numerator weighted by t = N,
    /// not spread over the implicit perpetuity.
    pub macaulay_duration_years: f64,
    /// Share of total PV sitting in the terminal value.
    pub terminal_pv_share: f64,
    /// Share of total PV from stage-1 (growth-phase) years.
    pub stage1_pv_share: f64,
    /// Share of total PV from stage-2 (fade-phase) years, excluding terminal.
    pub stage2_pv_share: f64,
    /// Denominator: Σ PV over projected years + terminal PV.
    pub total_pv: f64,
    /// Horizon year at which the terminal value is timed.
    pub horizon_years: u8,
}

/// Compute Macaulay-style equity duration from a projected model.
/// Returns `None` when total PV is zero (duration undefined — never fabricate).
pub fn equity_duration(model: &ProjectedModel, stage1_years: u8) -> Option<EquityDuration> {
    let horizon_years = model.periods.len() as u8;
    let explicit_pv: f64 = model.periods.iter().map(|p| p.present_value).sum();
    let total_pv = explicit_pv + model.terminal_pv;
    if total_pv == 0.0 {
        return None;
    }

    let weighted_time: f64 = model
        .periods
        .iter()
        .map(|p| p.year * p.present_value)
        .sum::<f64>()
        + f64::from(horizon_years) * model.terminal_pv;

    let stage1_pv: f64 = model
        .periods
        .iter()
        .filter(|p| p.period < stage1_years as usize)
        .map(|p| p.present_value)
        .sum();
    let stage2_pv = explicit_pv - stage1_pv;

    Some(EquityDuration {
        macaulay_duration_years: weighted_time / total_pv,
        terminal_pv_share: model.terminal_pv / total_pv,
        stage1_pv_share: stage1_pv / total_pv,
        stage2_pv_share: stage2_pv / total_pv,
        total_pv,
        horizon_years,
    })
}
