//! The worked historical-snapshot fixture the financial_model tests share.
//!
//! Two years of a hand-checkable worked history (revenue 1000, COGS 600,
//! SG&A 100, D&A 50, operating income 200, net income 160; equity 1000;
//! capex −80) — the driver-model and gap-decomposition test modules each
//! rebuilt it byte-identically before the extraction (2026-10-12 ratchet
//! pass).

use serde_json::json;

use super::HistoricalSnapshot;

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
