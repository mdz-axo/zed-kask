//! The worked historical-snapshot fixture the financial_model tests share.
//!
//! Two years of a hand-checkable worked history (revenue 1000, COGS 600,
//! SG&A 100, D&A 50, operating income 200, net income 160; equity 1000;
//! capex −80) — the driver-model and gap-decomposition test modules each
//! rebuilt it byte-identically before the extraction (2026-10-12 ratchet
//! pass).

use serde_json::json;

use super::HistoricalSnapshot;

// ── Live-receipt fixtures (2026-10-14) — the dividends seam ──────────────
// Two provider shapes, both observed live: FMP rows carry
// commonDividendsPaid (negative) and no dividendsPaid field; EODHD-normalized
// rows carry dividendsPaid (positive) with income rows keyed by fiscalYear
// and balance/cash-flow rows by calendarYear — the cross-form year join.

/// NVDA's FMP-shaped statements (live 2026-10-14): the shape that silently
/// zeroed every FMP-routed dividend payer's payout before the field-name
/// fix. Revenue 27.0→215.9B (the 8.0x episode window), dividends
/// 0.398→0.974B.
fn fmp_nvda_statements() -> (
    Vec<serde_json::Value>,
    Vec<serde_json::Value>,
    Vec<serde_json::Value>,
) {
    let income = vec![
        json!({"fiscalYear":"2026","revenue":215_938_000_000.0,"netIncome":120_067_000_000.0,"weightedAverageShsOut":24_359_000_000.0}),
        json!({"fiscalYear":"2025","revenue":130_497_000_000.0,"netIncome":72_880_000_000.0,"weightedAverageShsOut":24_555_000_000.0}),
        json!({"fiscalYear":"2024","revenue":60_922_000_000.0,"netIncome":29_760_000_000.0,"weightedAverageShsOut":24_690_000_000.0}),
        json!({"fiscalYear":"2023","revenue":26_974_000_000.0,"netIncome":4_368_000_000.0,"weightedAverageShsOut":24_870_000_000.0}),
    ];
    let balance = (2022..=2026)
        .rev()
        .map(|year| {
            json!({"fiscalYear":year.to_string(),"totalAssets":60_000_000_000.0,"totalStockholdersEquity":30_000_000_000.0})
        })
        .collect();
    let cash_flow = vec![
        json!({"fiscalYear":"2026","commonDividendsPaid":-974_000_000.0,"capitalExpenditure":-6_042_000_000.0}),
        json!({"fiscalYear":"2025","commonDividendsPaid":-834_000_000.0,"capitalExpenditure":-3_236_000_000.0}),
        json!({"fiscalYear":"2024","commonDividendsPaid":-395_000_000.0,"capitalExpenditure":-1_069_000_000.0}),
        json!({"fiscalYear":"2023","commonDividendsPaid":-398_000_000.0,"capitalExpenditure":-1_833_000_000.0}),
    ];
    (income, balance, cash_flow)
}

/// RY's EODHD-shaped statements (live 2026-10-14): income rows keyed by
/// fiscalYear, balance/cash-flow rows by calendarYear, dividendsPaid
/// positive — the cross-form year join and sign convention.
fn eodhd_ry_statements() -> (
    Vec<serde_json::Value>,
    Vec<serde_json::Value>,
    Vec<serde_json::Value>,
) {
    let income = vec![
        json!({"fiscalYear":"2025","revenue":137_357_000_000.0,"netIncome":20_369_000_000.0}),
        json!({"fiscalYear":"2024","revenue":134_489_000_000.0,"netIncome":16_240_000_000.0}),
        json!({"fiscalYear":"2023","revenue":113_453_000_000.0,"netIncome":14_605_000_000.0}),
        json!({"fiscalYear":"2022","revenue":66_829_000_000.0,"netIncome":15_794_000_000.0}),
    ];
    let balance = (2021..=2025)
        .rev()
        .map(|year| {
            json!({"calendarYear":year.to_string(),"totalAssets":1_000_000_000_000.0,"totalStockholdersEquity":150_000_000_000.0})
        })
        .collect();
    let cash_flow = vec![
        json!({"calendarYear":"2025","dividendsPaid":8_800_000_000.0,"capitalExpenditure":2_243_000_000.0}),
        json!({"calendarYear":"2024","dividendsPaid":6_637_000_000.0,"capitalExpenditure":2_280_000_000.0}),
        json!({"calendarYear":"2023","dividendsPaid":5_549_000_000.0,"capitalExpenditure":2_730_000_000.0}),
        json!({"calendarYear":"2022","dividendsPaid":6_960_000_000.0,"capitalExpenditure":2_500_000_000.0}),
    ];
    (income, balance, cash_flow)
}

/// Receipt: NVDA (FMP) — retention must read commonDividendsPaid, not
/// solve 1.0. Red before the field-name fix (dividendsPaid absent →
/// payout 0 → retention exactly 1.0 for a dividend payer).
#[test]
fn dupont_retention_reads_fmp_common_dividends_field() {
    let (income, balance, cash_flow) = fmp_nvda_statements();
    let hist = HistoricalSnapshot::from_api_json(&income, &balance, &cash_flow, &[], &json!({}));
    let dupont = hist.dupont().expect("dupont solves");
    // Median of per-year retentions ≈ 0.9876 (2023: 0.909, 2024: 0.9867,
    // 2025: 0.9886, 2026: 0.9919) — NOT 1.0.
    assert!(
        dupont.retention > 0.98 && dupont.retention < 0.995,
        "retention {} should reflect the paid dividend, not 1.0",
        dupont.retention
    );
    assert!(dupont.sustainable_growth_rate < dupont.roe);
}

/// Receipt: RY (EODHD) — a ~43% payout must solve retention ≈ 0.61, not
/// 1.0. Pins the fiscalYear↔calendarYear cross-form year join and the
/// positive-sign dividendsPaid convention.
#[test]
fn dupont_retention_reads_eodhd_dividends_and_cross_form_year_join() {
    let (income, balance, cash_flow) = eodhd_ry_statements();
    let hist = HistoricalSnapshot::from_api_json(&income, &balance, &cash_flow, &[], &json!({}));
    let dupont = hist.dupont().expect("dupont solves");
    // Median per-year retention ≈ 0.606 (2022: 0.559, 2023: 0.620,
    // 2024: 0.591, 2025: 0.568).
    assert!(
        dupont.retention > 0.55 && dupont.retention < 0.65,
        "retention {} should reflect RY's ~43% payout",
        dupont.retention
    );
}

/// Receipt: the episode-window signal — NVDA's 27.0→215.9B window is
/// 8.0x (episode); the flat worked history is 1.0x (clean).
#[test]
fn revenue_window_multiple_flags_episode_windows() {
    let (income, balance, cash_flow) = fmp_nvda_statements();
    let nvda = HistoricalSnapshot::from_api_json(&income, &balance, &cash_flow, &[], &json!({}));
    let multiple = nvda.revenue_window_multiple().expect("multiple solves");
    assert!((7.9..8.1).contains(&multiple), "NVDA window {multiple}");
    assert_eq!(worked_history().revenue_window_multiple(), Some(1.0));
}

/// The worked two-year history the financial_model tests project, drive
/// and decompose. Values stay exact — the tests' oracles are computed
/// against these figures.
pub(crate) fn worked_history() -> HistoricalSnapshot {
    HistoricalSnapshot::from_api_json(
        &[
            json!({"calendarYear":"2025","revenue":1000.0,"costOfRevenue":600.0,"sellingGeneralAndAdministrativeExpenses":100.0,"depreciationAndAmortization":50.0,"operatingIncome":200.0,"interestExpense":0.0,"incomeTaxExpense":40.0,"incomeBeforeTax":200.0,"netIncome":160.0,"weightedAverageShsOut":100.0}),
            json!({"calendarYear":"2024","revenue":1000.0,"costOfRevenue":600.0,"sellingGeneralAndAdministrativeExpenses":100.0,"depreciationAndAmortization":50.0,"operatingIncome":200.0,"interestExpense":0.0,"incomeTaxExpense":40.0,"incomeBeforeTax":200.0,"netIncome":160.0,"weightedAverageShsOut":100.0}),
        ],
        &[
            json!({"calendarYear":"2025","totalCurrentAssets":0.0,"totalCurrentLiabilities":0.0,"cashAndCashEquivalents":0.0,"longTermDebt":0.0,"netReceivables":0.0,"inventory":0.0,"accountsPayable":0.0,"totalStockholdersEquity":1000.0,"totalAssets":1000.0,"netPPE":0.0}),
            json!({"calendarYear":"2024","totalCurrentAssets":0.0,"totalCurrentLiabilities":0.0,"cashAndCashEquivalents":0.0,"longTermDebt":0.0,"netReceivables":0.0,"inventory":0.0,"accountsPayable":0.0,"totalStockholdersEquity":1000.0,"totalAssets":1000.0,"netPPE":0.0}),
        ],
        &[
            json!({"calendarYear":"2025","capitalExpenditure":-80.0,"dividendsPaid":0.0}),
            json!({"calendarYear":"2024","capitalExpenditure":-80.0,"dividendsPaid":0.0}),
        ],
        &[],
        &json!({}),
    )
}
