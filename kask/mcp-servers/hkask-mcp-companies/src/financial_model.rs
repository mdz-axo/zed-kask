//! Simplified 11-line-item financial model.
//!
//! Projects income statement, balance sheet, and cash flow items
//! to derive free cash flow for DCF valuation. All key drivers
//! are calibrated from historical company performance.
//!
//!   Item                          Source (FMP/EODHD field)
//!   ──────────────────────────    ────────────────────────
//!   1. Revenue                    income_statement.revenue
//!   2. COGS                       income_statement.costOfRevenue
//!   3. D&A                        income_statement.depreciationAndAmortization
//!   4. Capex                      cash_flow_statement.capitalExpenditure
//!   5. Assets                     balance_sheet.totalAssets
//!   6. NWC (net of cash)         currentAssets - currentLiabilities - cash
//!   7. Cash                       balance_sheet.cashAndCashEquivalents
//!   8. Long-term debt             balance_sheet.longTermDebt
//!   9. Owner's equity             balance_sheet.totalStockholdersEquity
//!  10. Shares outstanding         key_metrics.weightedAverageShsOut or profile
//!  11. Tax rate                   incomeTaxExpense / incomeBeforeTax

use crate::types::ProjectionAssumptionOverrides;
use crate::providers::CompanyProfile;
use serde::{Deserialize, Serialize};

/// Sector classification source: FMP `company_profile` API, which returns
/// GICS sector and industry classifications. Verified against COF, JPM, BAC,
/// ALL, SCHW (Financial Services), PLD, O (Real Estate/REIT), AAPL (Technology).
/// FMP maps its sector field from the GICS (Global Industry Classification
/// Standard) taxonomy maintained by S&P Dow Jones Indices and MSCI.
/// Reference: https://www.msci.com/our-solutions/index-investment-solutions/gics
//
/// Detect whether a company is in the financial sector (banks, insurance,
/// capital markets, diversified financials). These companies have balance
/// sheets where `totalCurrentLiabilities` includes customer deposits, making
/// NWC, ROIC, invested capital, and working capital cycle meaningless.
/// Financial companies are valued using P/B, P/TBV, dividend discount models,
/// and residual income on equity — not FCF-based DCF or economic profit on
/// invested capital.
///
/// Source: Damodaran, A. (2014). "Applied Corporate Finance" (4th ed.),
/// Chapter 19: "Valuing Financial Service Firms" — banks are valued using
/// equity-based approaches (excess return on equity, dividend discount models)
/// rather than firm-based DCF because debt is a raw material, not a source
/// of capital.
pub(crate) fn is_financial_sector(profile: &CompanyProfile) -> bool {
    let sector = profile.sector().unwrap_or("");
    let industry = profile.industry().unwrap_or("");
    sector.eq_ignore_ascii_case("Financial Services")
        || sector.eq_ignore_ascii_case("Financials")
        || industry.contains("Bank")
        || industry.contains("Credit Services")
        || industry.contains("Insurance")
        || industry.contains("Capital Markets")
        || industry.contains("Diversified Financial")
}

/// Detect whether a company is a REIT (Real Estate Investment Trust).
/// REITs have balance sheets dominated by property, with rental revenue
/// that doesn't map to traditional working capital concepts. DPO/DSO/DIO
/// and the cash conversion cycle are not meaningful for REITs.
/// REITs are valued using FFO/AFFO, cap rates, and NAV.
///
/// Source: NAREIT (National Association of Real Estate Investment Trusts),
/// "REIT Industry Operations & Financial Metrics" — REITs report FFO
/// (Funds From Operations) rather than net income as the primary earnings
/// metric, and cap rates (NOI / property value) rather than ROIC.
#[allow(dead_code)]
fn is_reit(profile: &CompanyProfile) -> bool {
    let sector = profile.sector().unwrap_or("");
    let industry = profile.industry().unwrap_or("");
    sector.eq_ignore_ascii_case("Real Estate")
        && (industry.contains("REIT") || industry.contains("Real Estate"))
}

/// Guard for tools that use FCF-based DCF or economic profit on invested
/// capital. Returns a structured JSON error if the company is in the
/// financial sector, or `None` if the tool should proceed.
///
/// `tool_name` is used to generate a tool-specific error message.
pub(crate) fn financial_sector_guard(
    profile: &CompanyProfile,
    symbol: &str,
    tool_name: &str,
) -> Option<serde_json::Value> {
    if !is_financial_sector(profile) {
        return None;
    }
    let sector = profile.sector().unwrap_or("");
    let industry = profile.industry().unwrap_or("");
    let (method, alternatives) = match tool_name {
        "ep_valuation" => (
            "Economic profit valuation (ROIC - WACC) × Invested Capital",
            vec!["comparable_analysis", "reverse_dcf with manual overrides", "dividend discount model"],
        ),
        _ => (
            "FCF-based DCF valuation",
            vec!["comparable_analysis", "reverse_dcf with manual overrides", "ep_valuation (equity-based)"],
        ),
    };
    Some(serde_json::json!({
        "symbol": symbol,
        "error": format!("{method} is not applicable to financial-sector companies"),
        "reason": "Banks and insurance companies have balance sheets where current liabilities include customer deposits, making NWC, ROIC, and invested capital meaningless. Financial companies are valued using P/B (price-to-book), P/TBV (tangible book value), dividend discount models, and residual income on equity — not FCF-based DCF or economic profit on invested capital.",
        "sector": sector,
        "industry": industry,
        "suggested_alternatives": alternatives,
        "source": "Damodaran, A. (2014). Applied Corporate Finance, Ch. 19: Valuing Financial Service Firms. Sector classification: GICS via FMP company_profile API."
    }))
}

/// Guard for tools that compute working capital metrics (DPO, DSO, DIO,
/// cash conversion cycle, gross margin stability). Returns a structured
/// JSON error if the company is in the financial sector or is a REIT —
/// both have balance sheet structures that make these industrial-company
/// concepts meaningless.
///
/// `tool_name` is used to generate a tool-specific error message.
#[allow(dead_code)]
pub(crate) fn working_capital_guard(
    profile: &CompanyProfile,
    symbol: &str,
    tool_name: &str,
) -> Option<serde_json::Value> {
    let sector = profile.sector().unwrap_or("");
    let industry = profile.industry().unwrap_or("");
    let (blocked_reason, alternatives) = if is_financial_sector(profile) {
        (
            "Financial-sector companies (banks, insurance) have balance sheets where current liabilities include customer deposits. DPO, DSO, DIO, and the cash conversion cycle are not meaningful — these are industrial-company metrics that measure supplier and customer payment timing, not deposit flows.",
            vec!["efficiency ratio (bank-specific)", "net interest margin", "ROE"],
        )
    } else if is_reit(profile) {
        (
            "REITs have balance sheets dominated by property assets. DPO, DSO, DIO, and the cash conversion cycle are not meaningful — REITs collect rent (not receivables) and pay property expenses (not supplier payables). Gross margin is not a meaningful concept for REITs.",
            vec!["FFO/AFFO", "cap rate (NOI/property value)", "NAV"],
        )
    } else {
        return None;
    };
    let (method, source) = match tool_name {
        "moat_check" => (
            "Moat analysis (gross margin stability, working capital days)",
            "Sector classification: GICS via FMP. Moat framework: Mauboussin & Callahan (2014), 'Calculating Return on Invested Capital'. REIT metrics: NAREIT FFO/AFFO guidance.",
        ),
        "working_capital_cycle" => (
            "Working capital cycle (DPO, DSO, DIO, cash conversion cycle)",
            "Sector classification: GICS via FMP. Working capital cycle: Richards & Laughlin (1980), 'A Cash Conversion Cycle Approach to Liquidity Analysis'. Bank metrics: FDIC Uniform Bank Performance Report.",
        ),
        "management_scorecard" => (
            "Management scorecard (ROIC trend, invested capital allocation)",
            "Sector classification: GICS via FMP. ROIC framework: Mauboussin & Callahan (2014). Bank metrics: ROE, not ROIC — see Damodaran (2014) Ch. 19.",
        ),
        _ => (
            "Working capital analysis",
            "Sector classification: GICS via FMP.",
        ),
    };
    Some(serde_json::json!({
        "symbol": symbol,
        "error": format!("{method} is not applicable to {sector} companies"),
        "reason": blocked_reason,
        "sector": sector,
        "industry": industry,
        "suggested_alternatives": alternatives,
        "source": source
    }))
}

// ── Historical data snapshot ───────────────────────────────────────────────

/// Extract a numeric financial field from an API JSON entry, warning when the
/// field is present but unparsable. A missing field returns 0.0 (legitimate
/// "no data"); a present-but-wrong-type field (e.g. a string where a number is
/// expected) also returns 0.0 but emits a `tracing::warn!` naming the field so
/// the operator can detect API contract drift or data corruption rather than
/// silently feeding zeros into DCF valuation math.
fn parse_financial_field(entry: &serde_json::Value, field: &str) -> f64 {
    match entry.get(field) {
        Some(v) => match v.as_f64() {
            Some(n) => n,
            None => {
                tracing::warn!(
                    target: "hkask.mcp.companies.financial_model",
                    field,
                    value = %v,
                    "financial field present but unparsable as f64 — falling back to 0.0"
                );
                0.0
            }
        },
        None => 0.0,
    }
}

/// Like `parse_financial_field` but with a custom fallback (e.g. 1.0 for
/// pre-tax income, where 0 would cause a division-by-zero in the tax-rate
/// computation).
fn parse_financial_field_or(entry: &serde_json::Value, field: &str, fallback: f64) -> f64 {
    match entry.get(field) {
        Some(v) => match v.as_f64() {
            Some(n) => n,
            None => {
                tracing::warn!(
                    target: "hkask.mcp.companies.financial_model",
                    field,
                    value = %v,
                    fallback,
                    "financial field present but unparsable as f64 — falling back to {fallback}"
                );
                fallback
            }
        },
        None => fallback,
    }
}

/// Historical financial data extracted from API responses.
#[derive(Debug, Clone)]
pub(crate) struct HistoricalSnapshot {
    pub revenue: Vec<(String, f64)>,
    pub cogs: Vec<(String, f64)>,
    pub da: Vec<(String, f64)>,
    pub capex: Vec<(String, f64)>,

    pub current_assets: Vec<(String, f64)>,
    pub current_liabilities: Vec<(String, f64)>,
    pub cash: Vec<(String, f64)>,
    pub long_term_debt: Vec<(String, f64)>,

    pub shares_outstanding: f64,
    pub tax_rate: f64,
}

impl HistoricalSnapshot {
    /// Build from FMP/EODHD API JSON data.
    /// All arrays (income_statements, balance_sheets, cash_flows) are iterated
    /// in reverse to produce ascending (oldest-first) year order.
    pub fn from_api_json(
        income_statements: &[serde_json::Value],
        balance_sheets: &[serde_json::Value],
        cash_flows: &[serde_json::Value],
        key_metrics: &[serde_json::Value],
        profile: &serde_json::Value,
    ) -> Self {
        // Extract revenue, COGS, D&A, tax data from income statements
        let mut revenue: Vec<(String, f64)> = Vec::new();
        let mut cogs: Vec<(String, f64)> = Vec::new();
        let mut da: Vec<(String, f64)> = Vec::new();
        let mut tax_expense: Vec<f64> = Vec::new();
        let mut pre_tax_income: Vec<f64> = Vec::new();

        for entry in income_statements.iter().rev() {
            let year = entry
                .get("calendarYear")
                .or_else(|| entry.get("fiscalYear"))
                .or_else(|| entry.get("date"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let rev = parse_financial_field(entry, "revenue");
            let c = parse_financial_field(entry, "costOfRevenue");
            let d = parse_financial_field(entry, "depreciationAndAmortization");
            let te = parse_financial_field(entry, "incomeTaxExpense");
            let pi = parse_financial_field_or(entry, "incomeBeforeTax", 1.0);

            if year.is_empty() || rev == 0.0 {
                continue;
            }
            revenue.push((year.to_string(), rev));
            cogs.push((year.to_string(), c));
            da.push((year.to_string(), d));
            tax_expense.push(te);
            pre_tax_income.push(pi);
        }

        // Extract balance sheet items

        let mut current_assets: Vec<(String, f64)> = Vec::new();
        let mut current_liabilities: Vec<(String, f64)> = Vec::new();
        let mut cash: Vec<(String, f64)> = Vec::new();
        let mut long_term_debt: Vec<(String, f64)> = Vec::new();

        for entry in balance_sheets.iter().rev() {
            let year = entry
                .get("calendarYear")
                .or_else(|| entry.get("fiscalYear"))
                .or_else(|| entry.get("date"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if year.is_empty() {
                continue;
            }

            current_assets.push((
                year.to_string(),
                parse_financial_field(entry, "totalCurrentAssets"),
            ));
            current_liabilities.push((
                year.to_string(),
                parse_financial_field(entry, "totalCurrentLiabilities"),
            ));
            cash.push((
                year.to_string(),
                entry
                    .get("cashAndCashEquivalents")
                    .or_else(|| entry.get("cashAndShortTermInvestments"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            ));
            long_term_debt.push((
                year.to_string(),
                parse_financial_field(entry, "longTermDebt"),
            ));
        }

        // Extract capex from cash flows (FMP: capex is negative)
        let mut capex: Vec<(String, f64)> = Vec::new();
        for entry in cash_flows.iter().rev() {
            let year = entry
                .get("calendarYear")
                .or_else(|| entry.get("fiscalYear"))
                .or_else(|| entry.get("date"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if year.is_empty() {
                continue;
            }
            let cap = parse_financial_field(entry, "capitalExpenditure");
            capex.push((year.to_string(), cap.abs()));
        }

        // Shares outstanding: prefer diluted from income statement (FMP stable
        // moved this from key-metrics to income-statement), fall back to basic
        // weighted average, then key_metrics (legacy/EODHD), then profile.
        let shares_outstanding = income_statements
            .first()
            .and_then(|e| {
                e.get("weightedAverageShsOutDil")
                    .or_else(|| e.get("weightedAverageShsOut"))
                    .and_then(|v| v.as_f64())
            })
            .or_else(|| {
                key_metrics.first().and_then(|e| {
                    e.get("weightedAverageShsOutDil")
                        .or_else(|| e.get("weightedAverageShsOut"))
                        .and_then(|v| v.as_f64())
                })
            })
            .or_else(|| profile.get("sharesOutstanding").and_then(|v| v.as_f64()))
            .unwrap_or(1_000.0);

        // Tax rate from most recent tax_expense / pre_tax_income
        let tax_rate = if let (Some(&te), Some(&pi)) = (tax_expense.last(), pre_tax_income.last()) {
            if pi > 0.0 {
                (te / pi).clamp(0.0, 0.50)
            } else {
                0.21
            }
        } else {
            0.21
        };

        HistoricalSnapshot {
            revenue,
            cogs,
            da,
            capex,

            current_assets,
            current_liabilities,
            cash,
            long_term_debt,

            shares_outstanding,
            tax_rate,
        }
    }

    /// Latest year's data.
    pub fn latest_revenue(&self) -> f64 {
        self.revenue.last().map(|(_, v)| *v).unwrap_or(0.0)
    }
    pub fn latest_cogs(&self) -> f64 {
        self.cogs.last().map(|(_, v)| *v).unwrap_or(0.0)
    }
    pub fn latest_da(&self) -> f64 {
        self.da.last().map(|(_, v)| *v).unwrap_or(0.0)
    }
    pub fn latest_capex(&self) -> f64 {
        self.capex.last().map(|(_, v)| *v).unwrap_or(0.0)
    }

    pub fn latest_cash(&self) -> f64 {
        self.cash.last().map(|(_, v)| *v).unwrap_or(0.0)
    }
    pub fn latest_debt(&self) -> f64 {
        self.long_term_debt.last().map(|(_, v)| *v).unwrap_or(0.0)
    }

    /// Net working capital (net of cash): current_assets - current_liabilities - cash.
    pub fn latest_nwc(&self) -> f64 {
        let ca = self.current_assets.last().map(|(_, v)| *v).unwrap_or(0.0);
        let cl = self
            .current_liabilities
            .last()
            .map(|(_, v)| *v)
            .unwrap_or(0.0);
        let ch = self.latest_cash();
        ca - cl - ch
    }

    /// Gross margin: (revenue - cogs) / revenue.
    pub fn gross_margin(&self) -> f64 {
        let rev = self.latest_revenue();
        if rev <= 0.0 {
            return 0.4;
        }
        (rev - self.latest_cogs()) / rev
    }

    /// D&A as percentage of revenue.
    pub fn da_to_revenue(&self) -> f64 {
        let rev = self.latest_revenue();
        if rev <= 0.0 {
            return 0.03;
        }
        self.latest_da() / rev
    }

    /// Capex as percentage of revenue.
    pub fn capex_to_revenue(&self) -> f64 {
        let rev = self.latest_revenue();
        if rev <= 0.0 {
            return 0.03;
        }
        self.latest_capex() / rev
    }

    /// NWC as percentage of revenue.
    pub fn nwc_to_revenue(&self) -> f64 {
        let rev = self.latest_revenue();
        if rev <= 0.0 {
            return 0.10;
        }
        self.latest_nwc() / rev
    }

    /// Revenue CAGR from historical data.
    pub fn revenue_cagr(&self) -> f64 {
        if self.revenue.len() < 2 {
            return 0.05;
        }
        let revs: Vec<f64> = self.revenue.iter().map(|(_, v)| *v).collect();
        let growths: Vec<f64> = revs
            .windows(2)
            .filter_map(|w| {
                if w[0] > 0.0 {
                    Some((w[1] - w[0]) / w[0])
                } else {
                    None
                }
            })
            .collect();
        if growths.is_empty() {
            return 0.05;
        }
        let product: f64 = growths.iter().map(|g| 1.0 + g).product();
        product.powf(1.0 / growths.len() as f64) - 1.0
    }

    /// Net debt: long_term_debt - cash.
    pub fn net_debt(&self) -> f64 {
        self.latest_debt() - self.latest_cash()
    }

    /// Compute signal quality for all 11-line-item model inputs.
    /// Returns ModelInputQuality with CV, outliers, cyclicality, and confidence.
    pub fn signal_quality(&self) -> super::data_quality::ModelInputQuality {
        let revenue: Vec<f64> = self.revenue.iter().map(|(_, v)| *v).collect();
        let cogs: Vec<f64> = self.cogs.iter().map(|(_, v)| *v).collect();
        let da: Vec<f64> = self.da.iter().map(|(_, v)| *v).collect();
        let capex: Vec<f64> = self.capex.iter().map(|(_, v)| *v).collect();
        let ca: Vec<f64> = self.current_assets.iter().map(|(_, v)| *v).collect();
        let cl: Vec<f64> = self.current_liabilities.iter().map(|(_, v)| *v).collect();
        let cash: Vec<f64> = self.cash.iter().map(|(_, v)| *v).collect();

        // Tax rate is a single value, but ModelInputQuality expects series.
        // We treat it as a constant series for quality purposes.
        let tax_expense: Vec<f64> = if self.tax_rate > 0.0 {
            vec![self.tax_rate; revenue.len()]
        } else {
            vec![0.21; revenue.len()]
        };
        let pre_tax: Vec<f64> = vec![1.0; revenue.len()];

        super::data_quality::ModelInputQuality::from_historical_series(
            &revenue,
            &cogs,
            &da,
            &capex,
            &ca,
            &cl,
            &cash,
            &tax_expense,
            &pre_tax,
            None,
        )
    }
}

// ── Projected line item ────────────────────────────────────────────────────

/// One period in the projected financial statements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectedLineItems {
    pub period: usize,
    pub year: f64,
    pub revenue: f64,
    pub cogs: f64,
    pub gross_profit: f64,
    pub da: f64,
    pub ebit: f64,
    pub tax: f64,
    pub nopat: f64,
    pub capex: f64,
    pub change_in_nwc: f64,
    pub free_cash_flow: f64,
    pub discount_factor: f64,
    pub present_value: f64,
}

/// The full projected model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectedModel {
    pub periods: Vec<ProjectedLineItems>,
    pub terminal_value: f64,
    pub terminal_pv: f64,
    pub enterprise_value: f64,
    pub net_debt: f64,
    pub equity_value: f64,
    pub intrinsic_per_share: f64,
}

/// Projection assumptions — overrideable by the user or calibrated from history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectionAssumptions {
    /// Revenue growth rate (annual).
    pub revenue_growth: f64,
    /// Gross margin: (revenue - cogs) / revenue.
    pub gross_margin: f64,
    /// D&A as % of revenue.
    pub da_to_revenue: f64,
    /// Capex as % of revenue.
    pub capex_to_revenue: f64,
    /// NWC as % of revenue.
    pub nwc_to_revenue: f64,
    /// Effective tax rate.
    pub tax_rate: f64,
    /// Discount rate (required return).
    pub discount_rate: f64,
    /// Terminal growth rate.
    pub terminal_growth: f64,
    /// Projection years.
    pub total_years: u8,
    /// Stage 1 years (growth phase).
    pub stage1_years: u8,
}

impl Default for ProjectionAssumptions {
    fn default() -> Self {
        Self {
            revenue_growth: 0.08,
            gross_margin: 0.40,
            da_to_revenue: 0.03,
            capex_to_revenue: 0.03,
            nwc_to_revenue: 0.10,
            tax_rate: 0.21,
            discount_rate: 0.10,
            terminal_growth: 0.025,
            total_years: 10,
            stage1_years: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub(crate) enum ProjectionAssumptionError {
    #[error("{field} must be finite")]
    NotFinite { field: &'static str },
    #[error("{field} must be within {min}..={max}")]
    OutOfRange {
        field: &'static str,
        min: f64,
        max: f64,
    },
    #[error("{field} must be finite and within {min}..={max}")]
    NotFiniteOrOutOfRange {
        field: &'static str,
        min: f64,
        max: f64,
    },
    #[error("projection horizon exceeds u8 capacity")]
    HorizonOverflow,
    #[error("discount_rate must be greater than terminal_growth")]
    DiscountNotGreaterThanTerminalGrowth,
}

impl ProjectionAssumptions {
    const REVENUE_GROWTH: (f64, f64) = (-0.50, 1.00);
    const GROSS_MARGIN: (f64, f64) = (0.05, 0.95);
    const DA_TO_REVENUE: (f64, f64) = (0.00, 0.20);
    const CAPEX_TO_REVENUE: (f64, f64) = (0.00, 0.30);
    const NWC_TO_REVENUE: (f64, f64) = (-0.20, 0.50);
    const TAX_RATE: (f64, f64) = (0.00, 1.00);
    const DISCOUNT_RATE: (f64, f64) = (0.05, 0.30);
    const TERMINAL_GROWTH: (f64, f64) = (0.00, 0.10);
    const STAGE1_YEARS: (u8, u8) = (1, 3);
    const STAGE2_YEARS: (u8, u8) = (2, 7);

    /// Build assumptions calibrated from history for internal model calculations.
    pub fn from_history(hist: &HistoricalSnapshot) -> Self {
        Self {
            revenue_growth: hist.revenue_cagr(),
            gross_margin: hist.gross_margin(),
            da_to_revenue: hist.da_to_revenue(),
            capex_to_revenue: hist.capex_to_revenue(),
            nwc_to_revenue: hist.nwc_to_revenue(),
            tax_rate: hist.tax_rate,
            ..Self::default()
        }
    }

    /// Construct validated DCF assumptions from history and explicit overrides.
    pub fn from_history_with_overrides(
        hist: &HistoricalSnapshot,
        overrides: ProjectionAssumptionOverrides,
    ) -> Result<Self, ProjectionAssumptionError> {
        Self::from_history(hist).with_overrides(overrides)
    }

    /// Apply and validate DCF input overrides.
    pub fn with_overrides(
        mut self,
        overrides: ProjectionAssumptionOverrides,
    ) -> Result<Self, ProjectionAssumptionError> {
        let stage1_years = overrides.stage1_years.unwrap_or(self.stage1_years);
        let stage2_years = overrides
            .stage2_years
            .unwrap_or(self.total_years - self.stage1_years);
        if !(Self::STAGE1_YEARS.0..=Self::STAGE1_YEARS.1).contains(&stage1_years) {
            return Err(ProjectionAssumptionError::OutOfRange {
                field: "stage1_years",
                min: Self::STAGE1_YEARS.0 as f64,
                max: Self::STAGE1_YEARS.1 as f64,
            });
        }
        if !(Self::STAGE2_YEARS.0..=Self::STAGE2_YEARS.1).contains(&stage2_years) {
            return Err(ProjectionAssumptionError::OutOfRange {
                field: "stage2_years",
                min: Self::STAGE2_YEARS.0 as f64,
                max: Self::STAGE2_YEARS.1 as f64,
            });
        }
        self.stage1_years = stage1_years;
        self.total_years = stage1_years
            .checked_add(stage2_years)
            .ok_or(ProjectionAssumptionError::HorizonOverflow)?;

        macro_rules! apply {
            ($field:ident) => {
                if let Some(value) = overrides.$field {
                    self.$field = value;
                }
            };
        }
        apply!(revenue_growth);
        apply!(gross_margin);
        apply!(da_to_revenue);
        apply!(capex_to_revenue);
        apply!(nwc_to_revenue);
        apply!(tax_rate);
        apply!(discount_rate);
        apply!(terminal_growth);

        self.validate(stage2_years)?;
        Ok(self)
    }

    fn validate(&self, stage2_years: u8) -> Result<(), ProjectionAssumptionError> {
        fn validate_range(
            field: &'static str,
            value: f64,
            range: (f64, f64),
        ) -> Result<(), ProjectionAssumptionError> {
            if !value.is_finite() {
                return Err(ProjectionAssumptionError::NotFinite { field });
            }
            if !(range.0..=range.1).contains(&value) {
                return Err(ProjectionAssumptionError::OutOfRange {
                    field,
                    min: range.0,
                    max: range.1,
                });
            }
            Ok(())
        }

        validate_range("revenue_growth", self.revenue_growth, Self::REVENUE_GROWTH)?;
        validate_range("gross_margin", self.gross_margin, Self::GROSS_MARGIN)?;
        validate_range("da_to_revenue", self.da_to_revenue, Self::DA_TO_REVENUE)?;
        validate_range(
            "capex_to_revenue",
            self.capex_to_revenue,
            Self::CAPEX_TO_REVENUE,
        )?;
        validate_range("nwc_to_revenue", self.nwc_to_revenue, Self::NWC_TO_REVENUE)?;
        validate_range("tax_rate", self.tax_rate, Self::TAX_RATE)?;
        validate_range("discount_rate", self.discount_rate, Self::DISCOUNT_RATE)?;
        validate_range(
            "terminal_growth",
            self.terminal_growth,
            Self::TERMINAL_GROWTH,
        )?;

        if !(Self::STAGE1_YEARS.0..=Self::STAGE1_YEARS.1).contains(&self.stage1_years) {
            return Err(ProjectionAssumptionError::OutOfRange {
                field: "stage1_years",
                min: Self::STAGE1_YEARS.0 as f64,
                max: Self::STAGE1_YEARS.1 as f64,
            });
        }
        if !(Self::STAGE2_YEARS.0..=Self::STAGE2_YEARS.1).contains(&stage2_years) {
            return Err(ProjectionAssumptionError::OutOfRange {
                field: "stage2_years",
                min: Self::STAGE2_YEARS.0 as f64,
                max: Self::STAGE2_YEARS.1 as f64,
            });
        }
        if self.discount_rate <= self.terminal_growth {
            return Err(ProjectionAssumptionError::DiscountNotGreaterThanTerminalGrowth);
        }
        Ok(())
    }
}

// ── Projection engine ──────────────────────────────────────────────────────

/// Project financial statements and compute free cash flow.
pub fn project_model(
    hist: &HistoricalSnapshot,
    assumptions: &ProjectionAssumptions,
    _current_price: f64,
) -> ProjectedModel {
    let stage2_years = assumptions.total_years - assumptions.stage1_years;
    let total_years = assumptions.total_years as usize;

    // Stage 1 growth → midpoint between historical growth and terminal
    let stage1_start = assumptions.revenue_growth;
    let stage1_mid = (stage1_start + assumptions.terminal_growth) / 2.0;

    let mut periods = Vec::with_capacity(total_years);
    let mut revenue = hist.latest_revenue();
    let mut prev_nwc = hist.latest_nwc();
    let mut prev_revenue = revenue;

    for p in 0..total_years {
        let progress = if p < assumptions.stage1_years as usize {
            let s1_p = p as f64 / (assumptions.stage1_years as f64 - 1.0).max(1.0);
            stage1_start + (stage1_mid - stage1_start) * s1_p
        } else {
            let s2_p = (p - assumptions.stage1_years as usize) as f64
                / (stage2_years as f64 - 1.0).max(1.0);
            let stage1_end = stage1_start
                + (stage1_mid - stage1_start)
                    * ((assumptions.stage1_years - 1) as f64
                        / (assumptions.stage1_years as f64 - 1.0).max(1.0));
            stage1_end + (assumptions.terminal_growth - stage1_end) * s2_p
        };

        revenue = prev_revenue * (1.0 + progress);

        let cogs = revenue * (1.0 - assumptions.gross_margin);
        let gross_profit = revenue - cogs;
        let da = revenue * assumptions.da_to_revenue;
        let ebit = gross_profit - da; // simplified: no separate SG&A
        let tax = ebit * assumptions.tax_rate;
        let nopat = ebit - tax;
        let capex = revenue * assumptions.capex_to_revenue;
        let nwc = revenue * assumptions.nwc_to_revenue;
        let change_in_nwc = nwc - prev_nwc;
        let fcf = nopat + da - capex - change_in_nwc;

        let df = 1.0 / (1.0 + assumptions.discount_rate).powi((p + 1) as i32);
        let pv = fcf * df;

        periods.push(ProjectedLineItems {
            period: p,
            year: (p + 1) as f64,
            revenue,
            cogs,
            gross_profit,
            da,
            ebit,
            tax,
            nopat,
            capex,
            change_in_nwc,
            free_cash_flow: fcf,
            discount_factor: df,
            present_value: pv,
        });

        prev_revenue = revenue;
        prev_nwc = nwc;
    }

    // Terminal value (Gordon Growth perpetuity)
    let last_fcf = periods.last().map(|p| p.free_cash_flow).unwrap_or(0.0);
    let terminal_value = last_fcf * (1.0 + assumptions.terminal_growth)
        / (assumptions.discount_rate - assumptions.terminal_growth);
    let terminal_df = 1.0 / (1.0 + assumptions.discount_rate).powi(total_years as i32);
    let terminal_pv = terminal_value * terminal_df;

    // Enterprise to equity
    let sum_pv: f64 = periods.iter().map(|p| p.present_value).sum();
    let enterprise_value = sum_pv + terminal_pv;
    let net_debt = hist.net_debt();
    let equity_value = enterprise_value - net_debt;
    let intrinsic_per_share = if hist.shares_outstanding > 0.0 {
        equity_value / hist.shares_outstanding
    } else {
        0.0
    };

    ProjectedModel {
        periods,
        terminal_value,
        terminal_pv,
        enterprise_value,
        net_debt,
        equity_value,
        intrinsic_per_share,
    }
}

// ── Implied growth (reverse DCF) ───────────────────────────────────────────

/// The growth-rate search bounds used by the reverse DCF. Callers verify the
/// price is bracketed by these bounds before searching.
pub(crate) const IMPLIED_GROWTH_LO: f64 = -0.50;
pub(crate) const IMPLIED_GROWTH_HI: f64 = 1.00;

/// Solve for the revenue-growth rate at which the projected intrinsic value
/// equals `current_price` (the Mauboussin reverse DCF).
///
/// Bisection is monotone in the right direction because intrinsic value is
/// increasing in `revenue_growth`: when the model's intrinsic exceeds the
/// price, the growth guess was too *high*, so the upper bound must shrink.
/// Getting this comparison backwards makes the search diverge from the root
/// while still returning a plausible-looking number, so it is expressed once
/// here rather than at each call site.
///
/// Returns `None` when `current_price` is not positive, or when the price is
/// not bracketed by `[IMPLIED_GROWTH_LO, IMPLIED_GROWTH_HI]` (the root lies
/// outside the searchable range — never fabricate an in-range answer).
pub(crate) fn implied_growth(
    hist: &HistoricalSnapshot,
    assumptions: &ProjectionAssumptions,
    current_price: f64,
) -> Option<f64> {
    if current_price <= 0.0 {
        return None;
    }

    let at_growth = |growth: f64| {
        project_model(
            hist,
            &ProjectionAssumptions {
                revenue_growth: growth,
                ..*assumptions
            },
            current_price,
        )
        .intrinsic_per_share
    };

    if at_growth(IMPLIED_GROWTH_LO) > current_price || at_growth(IMPLIED_GROWTH_HI) < current_price
    {
        return None;
    }

    let mut lo = IMPLIED_GROWTH_LO;
    let mut hi = IMPLIED_GROWTH_HI;
    let mut implied = 0.0_f64;
    for _ in 0..50 {
        let mid = (lo + hi) / 2.0;
        implied = mid;
        let intrinsic = at_growth(mid);
        if (intrinsic - current_price).abs() < 0.0001 {
            break;
        }
        if intrinsic > current_price {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(implied)
}

// ── Equity duration — extracted to `financial_model/equity_duration.rs`
mod equity_duration;
pub(crate) use equity_duration::equity_duration;

// ── Gap decomposition — extracted to `financial_model/gap_decomposition.rs`
mod gap_decomposition;
pub(crate) use gap_decomposition::decompose_gap;

// ── Sensitivity analysis — extracted to `financial_model/sensitivity.rs`
mod sensitivity;
pub(crate) use sensitivity::sensitivity_analysis;

// ── Monte Carlo DCF — extracted to `financial_model/monte_carlo.rs`
mod monte_carlo;
pub(crate) use monte_carlo::{McRange, monte_carlo_dcf, validate_sensitivity_range};

// ── Scenario impact valuation — extracted to `financial_model/scenario_impact.rs`
mod scenario_impact;
pub(crate) use scenario_impact::{
    ScenarioImpactError, ScenarioNodeImpact, ScenarioTreeInput, normalize_scenario_tree_json,
    scenario_impact_dcf,
};
