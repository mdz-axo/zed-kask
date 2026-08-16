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
use serde::{Deserialize, Serialize};

// ── Historical data snapshot ───────────────────────────────────────────────

/// Extract a numeric financial field from an API JSON entry, warning when the
/// field is present but unparseable. A missing field returns 0.0 (legitimate
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
                    "financial field present but unparseable as f64 — falling back to 0.0"
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
                    "financial field present but unparseable as f64 — falling back to {fallback}"
                );
                fallback
            }
        },
        None => fallback,
    }
}

/// Historical financial data extracted from API responses.
#[derive(Debug, Clone)]
pub struct HistoricalSnapshot {
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
                .or_else(|| entry.get("date"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if year.is_empty() {
                continue;
            }
            let cap = parse_financial_field(entry, "capitalExpenditure");
            capex.push((year.to_string(), cap.abs()));
        }

        // Shares outstanding: prefer diluted (accounts for options/warrants/convertibles),
        // fall back to basic weighted average, then profile shares.
        let shares_outstanding = key_metrics
            .first()
            .and_then(|e| {
                e.get("weightedAverageShsOutDil")
                    .or_else(|| e.get("weightedAverageShsOut"))
                    .and_then(|v| v.as_f64())
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
pub struct ProjectedLineItems {
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
pub struct ProjectedModel {
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
pub struct ProjectionAssumptions {
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
pub enum ProjectionAssumptionError {
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
pub const IMPLIED_GROWTH_LO: f64 = -0.50;
pub const IMPLIED_GROWTH_HI: f64 = 1.00;

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
pub fn implied_growth(
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
pub use equity_duration::equity_duration;

// ── Gap decomposition — extracted to `financial_model/gap_decomposition.rs`
mod gap_decomposition;
pub use gap_decomposition::decompose_gap;

// ── Sensitivity analysis — extracted to `financial_model/sensitivity.rs`
mod sensitivity;
pub use sensitivity::sensitivity_analysis;

// ── Monte Carlo DCF — extracted to `financial_model/monte_carlo.rs`
mod monte_carlo;
pub use monte_carlo::{McRange, monte_carlo_dcf, validate_sensitivity_range};

// ── Scenario impact valuation — extracted to `financial_model/scenario_impact.rs`
mod scenario_impact;
pub use scenario_impact::{
    ScenarioImpactError, ScenarioNodeImpact, ScenarioTreeInput, scenario_impact_dcf,
};

#[cfg(test)]
mod tests {
    use super::monte_carlo::{MC_MAX_SIMULATIONS, MC_MIN_SIMULATIONS};
    use super::*;

    fn sample_hist() -> HistoricalSnapshot {
        HistoricalSnapshot {
            revenue: vec![
                ("2022".into(), 80_000.0),
                ("2023".into(), 90_000.0),
                ("2024".into(), 100_000.0),
            ],
            cogs: vec![
                ("2022".into(), 50_000.0),
                ("2023".into(), 55_000.0),
                ("2024".into(), 60_000.0),
            ],
            da: vec![
                ("2022".into(), 3_000.0),
                ("2023".into(), 3_200.0),
                ("2024".into(), 3_500.0),
            ],
            capex: vec![
                ("2022".into(), 2_500.0),
                ("2023".into(), 2_800.0),
                ("2024".into(), 3_000.0),
            ],

            current_assets: vec![("2024".into(), 50_000.0)],
            current_liabilities: vec![("2024".into(), 30_000.0)],
            cash: vec![("2024".into(), 10_000.0)],
            long_term_debt: vec![("2024".into(), 40_000.0)],

            shares_outstanding: 1_000.0,
            tax_rate: 0.21,
        }
    }

    #[test]
    fn monte_carlo_dcf_clamps_zero_simulations() {
        // `simulations = 0` previously panicked on `values[0]` because the
        // clamp lived at the tool call site, not in this public function.
        let h = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&h);
        let ranges = McRange::default();
        let mut rng = rand::rng();
        let result = monte_carlo_dcf(&h, &assumptions, 0, &ranges, 100.0, &mut rng);
        assert_eq!(
            result.simulations, MC_MIN_SIMULATIONS,
            "zero simulations must clamp up to the floor, not panic"
        );
        assert!(result.max_intrinsic >= result.min_intrinsic);
    }

    #[test]
    fn monte_carlo_dcf_clamps_excessive_simulations() {
        let h = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&h);
        let ranges = McRange::default();
        let mut rng = rand::rng();
        let result = monte_carlo_dcf(&h, &assumptions, usize::MAX, &ranges, 100.0, &mut rng);
        assert_eq!(result.simulations, MC_MAX_SIMULATIONS);
    }

    #[test]
    fn implied_growth_round_trips_through_project_model() {
        // The defining property of the reverse DCF: projecting at the returned
        // growth rate must reproduce the price it was solved for. This fails if
        // the bisection comparison is inverted (the search then converges away
        // from the root while still returning an in-range number).
        let h = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&h);
        let base = project_model(&h, &assumptions, 0.0);
        // Pick a price the bounds can bracket: the model's own intrinsic value.
        let price = base.intrinsic_per_share;
        assert!(price > 0.0, "sample must produce a positive intrinsic");

        let implied = implied_growth(&h, &assumptions, price)
            .expect("price from the model itself must be bracketed");

        let round_trip = project_model(
            &h,
            &ProjectionAssumptions {
                revenue_growth: implied,
                ..assumptions
            },
            price,
        )
        .intrinsic_per_share;

        let relative_error = ((round_trip - price) / price).abs();
        assert!(
            relative_error < 0.01,
            "implied growth {implied} reproduced {round_trip} for price {price} (relative error {relative_error})"
        );
    }

    #[test]
    fn implied_growth_refuses_unbracketed_price() {
        let h = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&h);
        // A price far above what +100% growth can justify is not bracketed.
        let unreachable = project_model(
            &h,
            &ProjectionAssumptions {
                revenue_growth: IMPLIED_GROWTH_HI,
                ..assumptions
            },
            0.0,
        )
        .intrinsic_per_share
            * 10.0;
        assert!(
            implied_growth(&h, &assumptions, unreachable).is_none(),
            "an unbracketed price must refuse, not return an in-range growth rate"
        );
        assert!(
            implied_growth(&h, &assumptions, 0.0).is_none(),
            "a non-positive price must refuse"
        );
    }

    #[test]
    fn implied_growth_is_monotone_in_price() {
        // Higher price ⇒ higher implied growth. An inverted bisection breaks
        // this ordering even when the round-trip happens to land close.
        let h = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&h);
        let base = project_model(&h, &assumptions, 0.0).intrinsic_per_share;

        let low = implied_growth(&h, &assumptions, base * 0.8).expect("bracketed");
        let high = implied_growth(&h, &assumptions, base * 1.2).expect("bracketed");
        assert!(
            high > low,
            "implied growth must increase with price: got {low} at 0.8x and {high} at 1.2x"
        );
    }

    #[test]
    fn gross_margin_from_history() {
        let h = sample_hist();
        let gm = h.gross_margin();
        assert!((gm - 0.40).abs() < 0.01);
    }

    #[test]
    fn revenue_cagr_from_history() {
        let h = sample_hist();
        let cagr = h.revenue_cagr();
        // (100/80)^(1/2) - 1 = 1.25^0.5 - 1 ~= 0.118
        assert!((cagr - 0.118).abs() < 0.01, "got {cagr}");
    }

    #[test]
    fn nwc_computation() {
        let h = sample_hist();
        // CA=50, CL=30, Cash=10 => NWC = 50-30-10 = 10
        assert!((h.latest_nwc() - 10_000.0).abs() < 1.0);
    }

    #[test]
    fn net_debt() {
        let h = sample_hist();
        // Debt=40, Cash=10 => net_debt = 40-10 = 30
        assert!((h.net_debt() - 30_000.0).abs() < 1.0);
    }

    #[test]
    fn projection_has_all_periods() {
        let h = sample_hist();
        let a = ProjectionAssumptions::from_history(&h);
        let model = project_model(&h, &a, 150.0);
        assert_eq!(model.periods.len(), 10);
    }

    #[test]
    fn free_cash_flow_is_positive() {
        let h = sample_hist();
        let a = ProjectionAssumptions::from_history(&h);
        let model = project_model(&h, &a, 150.0);
        for p in &model.periods {
            assert!(p.free_cash_flow > 0.0, "FCF should be positive");
        }
    }

    #[test]
    fn terminal_value_positive() {
        let h = sample_hist();
        let a = ProjectionAssumptions::from_history(&h);
        let model = project_model(&h, &a, 150.0);
        assert!(model.terminal_value > 0.0);
        assert!(model.terminal_pv > 0.0);
    }

    #[test]
    fn intrinsic_per_share_reasonable() {
        let h = sample_hist();
        let a = ProjectionAssumptions::from_history(&h);
        let model = project_model(&h, &a, 150.0);
        assert!(model.intrinsic_per_share > 0.0);
    }

    #[test]
    fn equity_value_net_of_debt() {
        let h = sample_hist();
        let a = ProjectionAssumptions::from_history(&h);
        let model = project_model(&h, &a, 150.0);
        // EV = sum_pv + terminal_pv, Equity = EV - net_debt
        let expected_equity = model.enterprise_value - model.net_debt;
        assert!((model.equity_value - expected_equity).abs() < 1.0);
    }

    #[test]
    fn from_api_json_extracts_correctly() {
        let income = vec![
            serde_json::json!({"calendarYear": "2024", "revenue": 100_000, "costOfRevenue": 60_000, "depreciationAndAmortization": 3_500, "incomeTaxExpense": 5_000, "incomeBeforeTax": 20_000}),
            serde_json::json!({"calendarYear": "2023", "revenue": 90_000, "costOfRevenue": 55_000, "depreciationAndAmortization": 3_200, "incomeTaxExpense": 4_500, "incomeBeforeTax": 18_000}),
        ];
        let balance = vec![
            serde_json::json!({"calendarYear": "2024", "totalAssets": 200_000, "totalCurrentAssets": 50_000, "totalCurrentLiabilities": 30_000, "cashAndCashEquivalents": 10_000, "longTermDebt": 40_000, "totalStockholdersEquity": 80_000}),
        ];
        let cf = vec![serde_json::json!({"calendarYear": "2024", "capitalExpenditure": -3_000})];
        let km: Vec<serde_json::Value> = vec![];
        let profile = serde_json::json!({"sharesOutstanding": 1_000.0});

        let hist = HistoricalSnapshot::from_api_json(&income, &balance, &cf, &km, &profile);
        assert!((hist.latest_revenue() - 100_000.0).abs() < 1.0);
        assert!((hist.latest_cogs() - 60_000.0).abs() < 1.0);
        assert!((hist.latest_capex() - 3_000.0).abs() < 1.0);
        assert!((hist.shares_outstanding - 1_000.0).abs() < 1.0);
        // Tax rate: 5000/20000 = 0.25
        assert!((hist.tax_rate - 0.25).abs() < 0.01, "got {}", hist.tax_rate);
        // Revenue is in ascending order: 2023, 2024
        assert_eq!(hist.revenue[0].0, "2023");
        assert_eq!(hist.revenue[1].0, "2024");
    }

    #[test]
    fn gap_decomposition_produces_finite_values() {
        let h = sample_hist();
        let a = ProjectionAssumptions::from_history(&h);
        let model = project_model(&h, &a, 150.0);
        let gap = decompose_gap(
            &model,
            &a,
            &h,
            150.0,
            15.0,
            model.intrinsic_per_share,
            150.0,
        );
        assert!(gap.total_return_gap.is_finite());
        assert!(gap.revenue_growth_contribution.is_finite());
        assert!(gap.gross_margin_contribution.is_finite());
        assert!(gap.residual.is_finite());
    }

    #[test]
    fn sensitivity_analysis_all_drivers_finite() {
        let h = sample_hist();
        let a = ProjectionAssumptions::from_history(&h);
        let results = sensitivity_analysis(&h, &a, 0.10);
        assert_eq!(results.len(), 6);
        for r in &results {
            assert!(r.delta_pct.is_finite());
            assert!(r.intrinsic_low > 0.0);
            assert!(r.intrinsic_high > 0.0);
        }
        // Results should be sorted by descending delta_pct
        for i in 1..results.len() {
            assert!(results[i - 1].delta_pct >= results[i].delta_pct);
        }
    }

    #[test]
    fn rejects_invalid_sensitivity_and_monte_carlo_ranges() {
        assert!(validate_sensitivity_range(f64::INFINITY).is_err());
        assert!(validate_sensitivity_range(1.01).is_err());
        assert!(
            McRange {
                revenue_growth: f64::NAN,
                ..McRange::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_non_finite_and_out_of_range_assumptions() {
        let h = sample_hist();
        for overrides in [
            ProjectionAssumptionOverrides {
                revenue_growth: Some(f64::NAN),
                ..Default::default()
            },
            ProjectionAssumptionOverrides {
                gross_margin: Some(0.99),
                ..Default::default()
            },
            ProjectionAssumptionOverrides {
                stage1_years: Some(4),
                ..Default::default()
            },
        ] {
            assert!(ProjectionAssumptions::from_history_with_overrides(&h, overrides).is_err());
        }
    }

    #[test]
    fn rejects_terminal_growth_at_or_above_discount_rate() {
        let h = sample_hist();
        let overrides = ProjectionAssumptionOverrides {
            discount_rate: Some(0.05),
            terminal_growth: Some(0.05),
            ..Default::default()
        };
        let error = ProjectionAssumptions::from_history_with_overrides(&h, overrides)
            .expect_err("terminal growth must remain below the discount rate");
        assert_eq!(
            error.to_string(),
            "discount_rate must be greater than terminal_growth"
        );
    }

    #[test]
    fn applies_valid_overrides_and_checked_horizons() {
        let h = sample_hist();
        let assumptions = ProjectionAssumptions::from_history_with_overrides(
            &h,
            ProjectionAssumptionOverrides {
                stage1_years: Some(2),
                stage2_years: Some(5),
                discount_rate: Some(0.12),
                terminal_growth: Some(0.03),
                ..Default::default()
            },
        )
        .expect("valid inputs should construct assumptions");
        assert_eq!(assumptions.stage1_years, 2);
        assert_eq!(assumptions.total_years, 7);
        assert_eq!(assumptions.discount_rate, 0.12);
    }

    #[test]
    fn monte_carlo_produces_distribution() {
        let h = sample_hist();
        let a = ProjectionAssumptions::from_history(&h);
        let ranges = McRange::default();
        let mut rng = rand::rng();
        let result = monte_carlo_dcf(&h, &a, 500, &ranges, 150.0, &mut rng);
        assert_eq!(result.simulations, 500);
        assert!(result.mean_intrinsic > 0.0);
        assert!(result.std_dev >= 0.0);
        assert!(result.median >= result.p10);
        assert!(result.p90 >= result.median);
        assert!(result.prob_undervalued >= 0.0 && result.prob_undervalued <= 1.0);
        assert_eq!(result.histogram.len(), 10);
    }
}
