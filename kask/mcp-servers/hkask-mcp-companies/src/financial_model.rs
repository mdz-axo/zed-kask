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
            let rev = entry.get("revenue").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let c = entry
                .get("costOfRevenue")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let d = entry
                .get("depreciationAndAmortization")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let te = entry
                .get("incomeTaxExpense")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let pi = entry
                .get("incomeBeforeTax")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0);

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
                entry
                    .get("totalCurrentAssets")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            ));
            current_liabilities.push((
                year.to_string(),
                entry
                    .get("totalCurrentLiabilities")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
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
                entry
                    .get("longTermDebt")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
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
            let cap = entry
                .get("capitalExpenditure")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
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

// ── Gap decomposition ──────────────────────────────────────────────────────

/// Result of decomposing a forecast-vs-actual return gap.
#[derive(Debug, Clone, Serialize)]
pub struct GapDecomposition {
    pub total_return_gap: f64,
    pub revenue_growth_contribution: f64,
    pub gross_margin_contribution: f64,
    pub da_contribution: f64,
    pub capex_contribution: f64,
    pub nwc_contribution: f64,
    pub multiple_contribution: f64,
    pub net_debt_contribution: f64,
    pub residual: f64,
}

/// Decompose the gap between projected and actual intrinsic value into
/// 11-line-item drivers. Each contribution is computed by running the
/// projection model with only that one assumption changed to the actual,
/// and measuring the intrinsic value delta.
pub fn decompose_gap(
    projected: &ProjectedModel,
    projected_assumptions: &ProjectionAssumptions,
    actual_hist: &HistoricalSnapshot,
    actual_price: f64,
    actual_multiple: f64,
    _projected_intrinsic: f64,
    projected_price: f64,
) -> GapDecomposition {
    // Baseline: the original projection gives projected_intrinsic_per_share
    let base_intrinsic = projected.intrinsic_per_share;
    let base_price = projected_price;

    // Total return gap: actual price change - projected price change
    // (if we had projected price and actual price)
    let projected_return = if base_price > 0.0 {
        (base_intrinsic - base_price) / base_price
    } else {
        0.0
    };
    let actual_return = if actual_price > 0.0 && projected_price > 0.0 {
        (actual_price - projected_price) / projected_price
    } else {
        0.0
    };
    let total_return_gap = actual_return - projected_return;

    // Helper to compute what intrinsic would be with one parameter changed
    let compute_delta = |assumptions: &ProjectionAssumptions| -> f64 {
        let alt_model = project_model(actual_hist, assumptions, 0.0);
        alt_model.intrinsic_per_share - base_intrinsic
    };

    // Revenue growth contribution: use actual CAGR vs projected CAGR
    let mut growth_assumptions = projected_assumptions.clone();
    growth_assumptions.revenue_growth = actual_hist.revenue_cagr();
    let revenue_growth_delta = compute_delta(&growth_assumptions);

    // Gross margin contribution
    let mut gm_assumptions = projected_assumptions.clone();
    gm_assumptions.gross_margin = actual_hist.gross_margin();
    let gross_margin_delta = compute_delta(&gm_assumptions);

    // D&A contribution
    let mut da_assumptions = projected_assumptions.clone();
    da_assumptions.da_to_revenue = actual_hist.da_to_revenue();
    let da_delta = compute_delta(&da_assumptions);

    // Capex contribution
    let mut capex_assumptions = projected_assumptions.clone();
    capex_assumptions.capex_to_revenue = actual_hist.capex_to_revenue();
    let capex_delta = compute_delta(&capex_assumptions);

    // NWC contribution
    let mut nwc_assumptions = projected_assumptions.clone();
    nwc_assumptions.nwc_to_revenue = actual_hist.nwc_to_revenue();
    let nwc_delta = compute_delta(&nwc_assumptions);

    // Multiple contribution: (actual multiple - projected multiple) * actual_fcf
    let projected_multiple = if let Some(last) = projected.periods.last() {
        if last.free_cash_flow > 0.0 {
            projected.terminal_value / last.free_cash_flow
        } else {
            0.0
        }
    } else {
        0.0
    };
    let multiple_delta = (actual_multiple - projected_multiple) * 10.0;

    // Net debt contribution: change in net debt directly affects equity value
    let projected_net_debt = projected.net_debt;
    let actual_net_debt = actual_hist.net_debt();
    let net_debt_delta =
        (projected_net_debt - actual_net_debt) / actual_hist.shares_outstanding.max(1.0);

    // Residual: total gap minus sum of contributions
    let sum_contributions = revenue_growth_delta
        + gross_margin_delta
        + da_delta
        + capex_delta
        + nwc_delta
        + multiple_delta
        + net_debt_delta;
    let residual =
        (actual_return * base_price) - (projected_return * base_price) - sum_contributions;

    GapDecomposition {
        total_return_gap,
        revenue_growth_contribution: revenue_growth_delta,
        gross_margin_contribution: gross_margin_delta,
        da_contribution: da_delta,
        capex_contribution: capex_delta,
        nwc_contribution: nwc_delta,
        multiple_contribution: multiple_delta,
        net_debt_contribution: net_debt_delta,
        residual,
    }
}

// ── Sensitivity analysis ────────────────────────────────────────────────────

/// Result of varying one assumption and measuring intrinsic value delta.
#[derive(Debug, Clone, Serialize)]
pub struct SensitivityResult {
    pub driver: String,
    pub label: String,
    pub base_value: f64,
    pub low_value: f64,
    pub high_value: f64,
    pub intrinsic_low: f64,
    pub intrinsic_high: f64,
    pub delta_pct: f64,
    pub fibo_concept: &'static str,
}

/// Run sensitivity analysis on all key DCF drivers.
/// Varies each assumption by +/- range_pct and records intrinsic value impact.
/// Returns results sorted by absolute delta (most impactful first).
pub fn sensitivity_analysis(
    hist: &HistoricalSnapshot,
    base_assumptions: &ProjectionAssumptions,
    range_pct: f64,
) -> Vec<SensitivityResult> {
    let base = project_model(hist, base_assumptions, 0.0);
    let base_intrinsic = base.intrinsic_per_share;

    #[allow(clippy::type_complexity)]
    let drivers: [(
        &str,
        &str,
        &dyn Fn(&ProjectionAssumptions) -> f64,
        &dyn Fn(&mut ProjectionAssumptions, f64),
        &str,
    ); 6] = [
        (
            "revenue_growth",
            "Revenue Growth",
            &|a| a.revenue_growth,
            &|a, v| a.revenue_growth = v.clamp(-0.50, 1.00),
            "fibo-fbc-fct-ra:RevenueGrowthRate",
        ),
        (
            "gross_margin",
            "Gross Margin",
            &|a| a.gross_margin,
            &|a, v| a.gross_margin = v.clamp(0.05, 0.95),
            "fibo-fbc-fct-ra:GrossProfitMargin",
        ),
        (
            "da_to_revenue",
            "D&A / Revenue",
            &|a| a.da_to_revenue,
            &|a, v| a.da_to_revenue = v.clamp(0.0, 0.20),
            "fibo-fbc-fct-ra:DepreciationAndAmortization",
        ),
        (
            "capex_to_revenue",
            "Capex / Revenue",
            &|a| a.capex_to_revenue,
            &|a, v| a.capex_to_revenue = v.clamp(0.0, 0.30),
            "fibo-fbc-fct-ra:CapitalExpenditure",
        ),
        (
            "nwc_to_revenue",
            "NWC / Revenue",
            &|a| a.nwc_to_revenue,
            &|a, v| a.nwc_to_revenue = v.clamp(-0.20, 0.50),
            "fibo-fbc-fct-ra:NetWorkingCapital",
        ),
        (
            "discount_rate",
            "Discount Rate",
            &|a| a.discount_rate,
            &|a, v| a.discount_rate = v.clamp((a.terminal_growth + 0.0001).max(0.05), 0.30),
            "fibo-fbc-fct-ra:DiscountRate",
        ),
    ];

    let mut results = Vec::new();
    for (key, label, getter, setter, fibo) in &drivers {
        let base_val = getter(base_assumptions);
        let low_val = base_val * (1.0 - range_pct);
        let high_val = base_val * (1.0 + range_pct);

        let mut low_a = base_assumptions.clone();
        setter(&mut low_a, low_val);
        let intrinsic_low = project_model(hist, &low_a, 0.0).intrinsic_per_share;

        let mut high_a = base_assumptions.clone();
        setter(&mut high_a, high_val);
        let intrinsic_high = project_model(hist, &high_a, 0.0).intrinsic_per_share;

        let delta_pct = if base_intrinsic > 0.0 {
            (intrinsic_high - intrinsic_low) / base_intrinsic
        } else {
            0.0
        };

        results.push(SensitivityResult {
            driver: key.to_string(),
            label: label.to_string(),
            base_value: base_val,
            low_value: low_val,
            high_value: high_val,
            intrinsic_low,
            intrinsic_high,
            delta_pct,
            fibo_concept: fibo,
        });
    }

    results.sort_by(|a, b| {
        b.delta_pct
            .partial_cmp(&a.delta_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}

// ── Monte Carlo DCF ─────────────────────────────────────────────────────────

/// Distribution of intrinsic values from Monte Carlo simulation.
#[derive(Debug, Clone, Serialize)]
pub struct MonteCarloResult {
    pub simulations: usize,
    pub base_intrinsic: f64,
    pub mean_intrinsic: f64,
    pub std_dev: f64,
    pub min_intrinsic: f64,
    pub p10: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub p90: f64,
    pub max_intrinsic: f64,
    /// Probability intrinsic exceeds current price (if price > 0)
    pub prob_undervalued: f64,
    /// Histogram buckets: [(label, count)]
    pub histogram: Vec<(String, usize)>,
}

/// Range specification for one assumption in Monte Carlo simulation.
pub struct McRange {
    pub revenue_growth: f64,
    pub gross_margin: f64,
    pub da_to_revenue: f64,
    pub capex_to_revenue: f64,
    pub nwc_to_revenue: f64,
    pub discount_rate: f64,
}

impl McRange {
    /// Validate Monte Carlo perturbation widths before sampling.
    pub fn validate(&self) -> Result<(), ProjectionAssumptionError> {
        for (field, value) in [
            ("range_revenue_growth", self.revenue_growth),
            ("range_gross_margin", self.gross_margin),
            ("range_da", self.da_to_revenue),
            ("range_capex", self.capex_to_revenue),
            ("range_nwc", self.nwc_to_revenue),
            ("range_discount_rate", self.discount_rate),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(ProjectionAssumptionError::NotFiniteOrOutOfRange {
                    field,
                    min: 0.0,
                    max: 1.0,
                });
            }
        }
        Ok(())
    }
}

/// Validate the relative sensitivity range before varying assumptions.
pub fn validate_sensitivity_range(range_pct: f64) -> Result<(), ProjectionAssumptionError> {
    if !range_pct.is_finite() || !(0.0..=1.0).contains(&range_pct) {
        return Err(ProjectionAssumptionError::NotFiniteOrOutOfRange {
            field: "range_pct",
            min: 0.0,
            max: 1.0,
        });
    }
    Ok(())
}

impl Default for McRange {
    fn default() -> Self {
        Self {
            revenue_growth: 0.03,
            gross_margin: 0.03,
            da_to_revenue: 0.01,
            capex_to_revenue: 0.01,
            nwc_to_revenue: 0.02,
            discount_rate: 0.01,
        }
    }
}

/// Run N Monte Carlo simulations with randomized assumptions within +/- range.
pub fn monte_carlo_dcf(
    hist: &HistoricalSnapshot,
    base_assumptions: &ProjectionAssumptions,
    simulations: usize,
    ranges: &McRange,
    current_price: f64,
    rng: &mut impl rand::Rng,
) -> MonteCarloResult {
    let base = project_model(hist, base_assumptions, current_price);
    let mut values: Vec<f64> = Vec::with_capacity(simulations);

    for _ in 0..simulations {
        let mut a = base_assumptions.clone();
        a.revenue_growth =
            sample_uniform(rng, a.revenue_growth, ranges.revenue_growth).clamp(-0.50, 1.00);
        a.gross_margin = sample_uniform(rng, a.gross_margin, ranges.gross_margin).clamp(0.05, 0.95);
        a.da_to_revenue =
            sample_uniform(rng, a.da_to_revenue, ranges.da_to_revenue).clamp(0.0, 0.20);
        a.capex_to_revenue =
            sample_uniform(rng, a.capex_to_revenue, ranges.capex_to_revenue).clamp(0.0, 0.30);
        a.nwc_to_revenue =
            sample_uniform(rng, a.nwc_to_revenue, ranges.nwc_to_revenue).clamp(-0.20, 0.50);
        a.discount_rate = sample_uniform(rng, a.discount_rate, ranges.discount_rate)
            .clamp((a.terminal_growth + 0.0001).max(0.05), 0.30);
        values.push(project_model(hist, &a, current_price).intrinsic_per_share);
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    let mean = values.iter().sum::<f64>() / n as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
    let std_dev = variance.sqrt();

    // Histogram with 10 buckets
    let min_val = values[0];
    let max_val = values[n - 1];
    let bucket_width = (max_val - min_val) / 10.0;
    let mut histogram: Vec<(String, usize)> = Vec::new();
    if bucket_width > 0.0 {
        for i in 0..10 {
            let lo = min_val + i as f64 * bucket_width;
            let hi = lo + bucket_width;
            let count = values
                .iter()
                .filter(|&&v| v >= lo && (i == 9 || v < hi))
                .count();
            histogram.push((format!("{:.0}-{:.0}", lo, hi), count));
        }
    }

    let prob_undervalued = if current_price > 0.0 {
        values.iter().filter(|&&v| v > current_price).count() as f64 / n as f64
    } else {
        0.0
    };

    MonteCarloResult {
        simulations: n,
        base_intrinsic: base.intrinsic_per_share,
        mean_intrinsic: mean,
        std_dev,
        min_intrinsic: values[0],
        p10: percentile(&values, 0.10),
        p25: percentile(&values, 0.25),
        median: percentile(&values, 0.50),
        p75: percentile(&values, 0.75),
        p90: percentile(&values, 0.90),
        max_intrinsic: values[n - 1],
        prob_undervalued,
        histogram,
    }
}

fn sample_uniform(rng: &mut impl rand::Rng, center: f64, range: f64) -> f64 {
    let lo = center - range;
    let hi = center + range;
    rng.random_range(lo..hi)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
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
