//! Driver-based linked three-statement financial projection model.
//!
//! Projects income statement, balance sheet, and cash flow statement from five
//! key drivers, with proper accounting linkage:
//!
//! - Income statement → Balance sheet: Net income flows to retained earnings
//! - Balance sheet → Income statement: Debt drives interest expense; PP&E drives depreciation
//! - Income statement + Balance sheet → Cash flow: NI + non-cash + WC changes + capex = OCF
//! - Balance sheet identity: Assets = Liabilities + Equity (enforced every period via cash plug)
//!
//! Financial-sector companies use an equity-based path (ROE/COE residual income)
//! instead of FCF-based DCF, per Damodaran *Applied Corporate Finance* Ch. 19.
//!
//! Source references (John Brooks corpus, `extracted/researcher/`):
//! - Damodaran, *Investment Valuation* (2nd ed.): FCF, WACC, terminal value, SG&A as operating expense
//! - Fabozzi, *Financial Management & Analysis*: three-statement linkage, retained earnings, working capital
//! - Damodaran, *Applied Corporate Finance* Ch. 19: financial-sector equity-based valuation
//! - `137464131.financial-signposts.txt`: gross margin stability, working capital discipline

use serde::{Deserialize, Serialize};

use super::HistoricalSnapshot;

// ── Adjustment types ────────────────────────────────────────────────────────

/// How a driver value is applied. Each driver supports one or more of these
/// adjustment types simultaneously (e.g., revenue can grow 8% YoY AND receive
/// an explicit $500M adjustment for a planned expansion).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverAdjustment {
    /// Percentage change from prior period (e.g., 0.08 = 8% YoY growth).
    /// For margins, this is the percentage of revenue (e.g., 0.60 = 60% of revenue).
    pub percent: Option<f64>,
    /// Explicit dollar adjustment added to the computed value (e.g., +500e6 for capex).
    pub explicit: Option<f64>,
    /// Ratio target (e.g., capex/D&A ratio of 1.5, or target D/E ratio of 0.5).
    pub ratio: Option<f64>,
}

impl Default for DriverAdjustment {
    fn default() -> Self {
        Self {
            percent: None,
            explicit: None,
            ratio: None,
        }
    }
}

// ── Working capital method ──────────────────────────────────────────────────

/// Method for projecting net working capital.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NwcMethod {
    /// Days-based: DSO, DIO, DPO drive AR, inventory, AP.
    Days,
    /// Percentage of revenue: NWC = revenue × nwc_pct.
    PercentOfRevenue,
    /// Explicit dollar amount for total NWC.
    Explicit,
}

impl Default for NwcMethod {
    fn default() -> Self {
        Self::PercentOfRevenue
    }
}

// ── Driver assumptions ──────────────────────────────────────────────────────

/// The five key drivers for the linked three-statement projection.
///
/// Each driver supports three adjustment types:
/// - **Percent change** (e.g., revenue grows 8% YoY)
/// - **Percent of total / common-size** (e.g., COGS is 60% of revenue)
/// - **Explicit adjustment** (e.g., add $500M to capex for a planned expansion)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverAssumptions {
    // Driver 1: Revenue growth
    /// Revenue growth rate (YoY % change). Default: historical CAGR.
    pub revenue_growth: DriverAdjustment,
    /// Explicit revenue adjustment in dollars (e.g., acquisition contribution).
    pub revenue_explicit: DriverAdjustment,

    // Driver 2: Profit margins
    /// Gross margin (% of revenue). Default: historical gross margin.
    pub gross_margin: DriverAdjustment,
    /// SG&A as % of revenue. Default: historical SG&A/revenue. Fixes the
    /// SG&A omission defect (H1) in the original `project_model`.
    pub sga_pct: DriverAdjustment,
    /// D&A as % of revenue. Default: historical D&A/revenue.
    pub da_pct: DriverAdjustment,
    /// Effective tax rate. Default: historical tax rate.
    pub tax_rate: f64,

    // Driver 3: Capex vs depreciation
    /// Capex as % of revenue. Default: historical capex/revenue.
    pub capex_pct: DriverAdjustment,
    /// Explicit capex adjustment in dollars.
    pub capex_explicit: DriverAdjustment,
    /// Capex/D&A ratio target. When set, overrides `da_pct` — D&A is derived
    /// from capex divided by this ratio. Models asset intensity.
    pub capex_da_ratio: Option<f64>,

    // Driver 4: Net working capital
    /// Method for projecting NWC (days, % of revenue, or explicit).
    pub nwc_method: NwcMethod,
    /// Days sales outstanding (AR / (revenue/365)). Default: historical DSO.
    pub dso_days: f64,
    /// Days inventory outstanding (inventory / (cogs/365)). Default: historical DIO.
    pub dio_days: f64,
    /// Days payable outstanding (AP / (cogs/365)). Default: historical DPO.
    pub dpo_days: f64,
    /// NWC as % of revenue (when method = PercentOfRevenue). Default: historical NWC/revenue.
    pub nwc_pct: f64,
    /// Explicit NWC adjustment in dollars.
    pub nwc_explicit: DriverAdjustment,

    // Driver 5: Debt/equity issuance
    /// Explicit debt issuance in dollars per period.
    pub debt_issuance: f64,
    /// Explicit debt repayment in dollars per period.
    pub debt_repayment: f64,
    /// Target debt-to-equity ratio. When set, debt is adjusted toward this target.
    pub target_debt_equity: Option<f64>,
    /// Interest rate on debt for interest expense computation.
    pub interest_rate: f64,
    /// Explicit equity issuance in dollars per period.
    pub equity_issuance: f64,
    /// Dividend payout ratio (dividends / net income). Default: 0 (retention mode).
    pub dividend_payout_ratio: f64,

    // Valuation
    /// Discount rate. For non-financial companies, this is WACC (firm-level DCF).
    /// For financial-sector companies, this is COE (equity-level residual income).
    pub discount_rate: f64,
    /// Terminal growth rate (Gordon Growth perpetuity).
    pub terminal_growth: f64,
    /// Projection horizon in years.
    pub total_years: u8,
    /// Whether to use the financial-sector equity-based path.
    pub is_financial_sector: bool,
    /// Cost of equity for financial-sector residual income valuation.
    pub cost_of_equity: f64,
}

impl Default for DriverAssumptions {
    fn default() -> Self {
        Self {
            revenue_growth: DriverAdjustment {
                percent: Some(0.08),
                ..Default::default()
            },
            revenue_explicit: DriverAdjustment::default(),
            gross_margin: DriverAdjustment {
                percent: Some(0.40),
                ..Default::default()
            },
            sga_pct: DriverAdjustment {
                percent: Some(0.15),
                ..Default::default()
            },
            da_pct: DriverAdjustment {
                percent: Some(0.03),
                ..Default::default()
            },
            tax_rate: 0.21,
            capex_pct: DriverAdjustment {
                percent: Some(0.03),
                ..Default::default()
            },
            capex_explicit: DriverAdjustment::default(),
            capex_da_ratio: None,
            nwc_method: NwcMethod::PercentOfRevenue,
            dso_days: 45.0,
            dio_days: 60.0,
            dpo_days: 30.0,
            nwc_pct: 0.10,
            nwc_explicit: DriverAdjustment::default(),
            debt_issuance: 0.0,
            debt_repayment: 0.0,
            target_debt_equity: None,
            interest_rate: 0.05,
            equity_issuance: 0.0,
            dividend_payout_ratio: 0.0,
            discount_rate: 0.10,
            terminal_growth: 0.025,
            total_years: 10,
            is_financial_sector: false,
            cost_of_equity: 0.10,
        }
    }
}

impl DriverAssumptions {
    /// Build driver assumptions calibrated from historical data.
    pub fn from_history(hist: &HistoricalSnapshot) -> Self {
        let gross_margin = hist.gross_margin();
        let sga_pct = hist.sga_to_revenue();
        let da_pct = hist.da_to_revenue();
        let capex_pct = hist.capex_to_revenue();
        let nwc_pct = hist.nwc_to_revenue();
        let revenue_growth = hist.revenue_cagr();
        let tax_rate = hist.tax_rate;
        let interest_rate = if hist.latest_debt() > 0.0 {
            hist.interest_expense() / hist.latest_debt()
        } else {
            0.05
        };
        let dso = hist.dso_days();
        let dio = hist.dio_days();
        let dpo = hist.dpo_days();
        let dividend_payout = hist.dividend_payout_ratio();

        Self {
            revenue_growth: DriverAdjustment {
                percent: Some(revenue_growth),
                ..Default::default()
            },
            revenue_explicit: DriverAdjustment::default(),
            gross_margin: DriverAdjustment {
                percent: Some(gross_margin),
                ..Default::default()
            },
            sga_pct: DriverAdjustment {
                percent: Some(sga_pct),
                ..Default::default()
            },
            da_pct: DriverAdjustment {
                percent: Some(da_pct),
                ..Default::default()
            },
            tax_rate,
            capex_pct: DriverAdjustment {
                percent: Some(capex_pct),
                ..Default::default()
            },
            capex_explicit: DriverAdjustment::default(),
            capex_da_ratio: None,
            nwc_method: NwcMethod::PercentOfRevenue,
            dso_days: dso,
            dio_days: dio,
            dpo_days: dpo,
            nwc_pct,
            nwc_explicit: DriverAdjustment::default(),
            debt_issuance: 0.0,
            debt_repayment: 0.0,
            target_debt_equity: None,
            interest_rate: interest_rate.clamp(0.01, 0.15),
            equity_issuance: 0.0,
            dividend_payout_ratio: dividend_payout,
            discount_rate: 0.10,
            terminal_growth: 0.025,
            total_years: 10,
            is_financial_sector: false,
            cost_of_equity: 0.10,
        }
    }

    /// Validate all driver assumptions before projection.
    pub fn validate(&self) -> Result<(), DriverModelError> {
        if self.total_years == 0 {
            return Err(DriverModelError::InvalidHorizon);
        }
        if !self.discount_rate.is_finite() || self.discount_rate <= 0.0 {
            return Err(DriverModelError::InvalidDiscountRate);
        }
        if !self.terminal_growth.is_finite() || self.terminal_growth < 0.0 {
            return Err(DriverModelError::InvalidTerminalGrowth);
        }
        if self.discount_rate <= self.terminal_growth {
            return Err(DriverModelError::DiscountNotGreaterThanTerminalGrowth);
        }
        if !self.tax_rate.is_finite() || !(0.0..=1.0).contains(&self.tax_rate) {
            return Err(DriverModelError::InvalidTaxRate);
        }
        if !self.interest_rate.is_finite() || self.interest_rate < 0.0 {
            return Err(DriverModelError::InvalidInterestRate);
        }
        if !self.dividend_payout_ratio.is_finite()
            || !(0.0..=1.0).contains(&self.dividend_payout_ratio)
        {
            return Err(DriverModelError::InvalidPayoutRatio);
        }
        Ok(())
    }
}

// ── Projected period ────────────────────────────────────────────────────────

/// One period of the linked three-statement projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverPeriod {
    pub year: u32,

    // Income Statement
    pub revenue: f64,
    pub cogs: f64,
    pub gross_profit: f64,
    pub sga: f64,
    pub da: f64,
    pub ebit: f64,
    pub interest_expense: f64,
    pub ebt: f64,
    pub tax: f64,
    pub net_income: f64,

    // Balance Sheet (end of period)
    pub cash: f64,
    pub accounts_receivable: f64,
    pub inventory: f64,
    pub ppe_net: f64,
    pub total_assets: f64,
    pub accounts_payable: f64,
    pub debt: f64,
    pub equity: f64,
    pub total_liabilities_equity: f64,
    /// Balance check: total_assets - total_liabilities_equity. Must be ~0 every period.
    pub balance_check: f64,

    // Cash Flow Statement
    pub cfo: f64,
    pub cfi: f64,
    pub cff: f64,
    pub net_cash_change: f64,

    // Free Cash Flow (firm-level, for non-financial DCF)
    pub free_cash_flow: f64,
    pub discount_factor: f64,
    pub present_value: f64,
}

// ── Projected model ─────────────────────────────────────────────────────────

/// Result of the driver-based three-statement projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverProjectedModel {
    pub periods: Vec<DriverPeriod>,
    pub terminal_value: f64,
    pub terminal_pv: f64,
    /// Enterprise value (non-financial) or equity value (financial-sector).
    pub enterprise_value: f64,
    /// Equity value = EV - net_debt (non-financial) or direct (financial-sector).
    pub equity_value: f64,
    pub intrinsic_per_share: f64,
    pub is_financial_sector: bool,
    /// Net debt at last projected period.
    pub net_debt: f64,
    /// Shares outstanding (from historical data).
    pub shares_outstanding: f64,
}

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, thiserror::Error)]
pub enum DriverModelError {
    #[error("projection horizon must be > 0")]
    InvalidHorizon,
    #[error("discount rate must be positive and finite")]
    InvalidDiscountRate,
    #[error("terminal growth must be non-negative and finite")]
    InvalidTerminalGrowth,
    #[error("discount rate must be greater than terminal growth")]
    DiscountNotGreaterThanTerminalGrowth,
    #[error("tax rate must be in [0, 1]")]
    InvalidTaxRate,
    #[error("interest rate must be non-negative and finite")]
    InvalidInterestRate,
    #[error("dividend payout ratio must be in [0, 1]")]
    InvalidPayoutRatio,
    #[error("insufficient historical data: need at least 2 years of revenue")]
    InsufficientHistory,
}

// ── Projection engine ───────────────────────────────────────────────────────

/// Project linked three-statement financials from five drivers.
///
/// For non-financial companies, this produces a firm-level DCF:
/// - Income statement: Revenue → COGS → Gross Profit → SG&A → D&A → EBIT → Interest → EBT → Tax → NI
/// - Balance sheet: Cash (plug) + AR + Inventory + PP&E = AP + Debt + Equity (retained earnings)
/// - Cash flow: CFO (NI + D&A - ΔNWC) + CFI (-Capex) + CFF (debt/equity issuance - dividends)
/// - FCF = NOPAT + D&A - Capex - ΔNWC, discounted at WACC
///
/// For financial-sector companies, this produces an equity-level residual income model:
/// - Project book equity from ROE × beginning equity
/// - Residual income = (ROE - COE) × beginning equity
/// - Equity value = book equity + PV(residual income)
///
/// Source: Damodaran, *Applied Corporate Finance* Ch. 19; Fabozzi, *Financial Management & Analysis* Ch. 6.
pub fn project_driver_model(
    hist: &HistoricalSnapshot,
    assumptions: &DriverAssumptions,
) -> Result<DriverProjectedModel, DriverModelError> {
    assumptions.validate()?;

    if hist.revenue.len() < 2 && !assumptions.is_financial_sector {
        return Err(DriverModelError::InsufficientHistory);
    }

    if assumptions.is_financial_sector {
        project_financial_sector(hist, assumptions)
    } else {
        project_industrial(hist, assumptions)
    }
}

/// Industrial-company projection: firm-level DCF with linked three statements.
fn project_industrial(
    hist: &HistoricalSnapshot,
    assumptions: &DriverAssumptions,
) -> Result<DriverProjectedModel, DriverModelError> {
    let total_years = assumptions.total_years as usize;
    let mut periods: Vec<DriverPeriod> = Vec::with_capacity(total_years);

    // Starting balance sheet values from latest historical period
    let mut prev_revenue = hist.latest_revenue();
    let mut prev_nwc = hist.latest_nwc();
    let mut prev_debt = hist.latest_debt();
    let mut prev_equity = hist.latest_equity();
    let mut prev_ppe = hist.latest_ppe_net();
    let mut prev_cash = hist.latest_cash();

    // Extract driver values (with fallbacks)
    let growth_rate = assumptions
        .revenue_growth
        .percent
        .unwrap_or_else(|| hist.revenue_cagr());
    let revenue_adj = assumptions.revenue_explicit.explicit.unwrap_or(0.0);
    let gross_margin_pct = assumptions
        .gross_margin
        .percent
        .unwrap_or_else(|| hist.gross_margin());
    let sga_pct = assumptions.sga_pct.percent.unwrap_or_else(|| hist.sga_to_revenue());
    let capex_pct = assumptions
        .capex_pct
        .percent
        .unwrap_or_else(|| hist.capex_to_revenue());
    let capex_adj = assumptions.capex_explicit.explicit.unwrap_or(0.0);

    for p in 0..total_years {
        let year = (p + 1) as u32;

        // ── Income Statement ───────────────────────────────────────────────
        // Driver 1: Revenue growth (% change + explicit adjustment)
        let revenue = prev_revenue * (1.0 + growth_rate) + revenue_adj;
        let cogs = revenue * (1.0 - gross_margin_pct);
        let gross_profit = revenue - cogs;

        // Driver 2: SG&A as % of revenue (fixes H1 — SG&A was omitted in project_model)
        let sga = revenue * sga_pct;

        // Driver 3: D&A — either % of revenue or derived from capex/D&A ratio
        let da = if let Some(ratio) = assumptions.capex_da_ratio {
            let capex = revenue * capex_pct + capex_adj;
            if ratio > 0.0 {
                capex / ratio
            } else {
                revenue * assumptions.da_pct.percent.unwrap_or(0.03)
            }
        } else {
            revenue * assumptions.da_pct.percent.unwrap_or_else(|| hist.da_to_revenue())
        };

        let ebit = gross_profit - sga - da;

        // Balance sheet → Income statement: Debt drives interest expense
        let interest_expense = prev_debt * assumptions.interest_rate;
        let ebt = ebit - interest_expense;
        let tax = if ebt > 0.0 {
            ebt * assumptions.tax_rate
        } else {
            0.0
        };
        let net_income = ebt - tax;

        // ── Balance Sheet ──────────────────────────────────────────────────
        // Driver 4: Working capital projection
        let (ar, inventory, ap, nwc) = match assumptions.nwc_method {
            NwcMethod::Days => {
                let ar = revenue * assumptions.dso_days / 365.0;
                let inv = cogs * assumptions.dio_days / 365.0;
                let ap = cogs * assumptions.dpo_days / 365.0;
                let nwc = ar + inv - ap;
                (ar, inv, ap, nwc)
            }
            NwcMethod::PercentOfRevenue => {
                let nwc = revenue * assumptions.nwc_pct
                    + assumptions.nwc_explicit.explicit.unwrap_or(0.0);
                // Distribute NWC across AR, inventory, AP proportionally to historical mix
                let ar_ratio = hist.ar_to_nwc_ratio();
                let inv_ratio = hist.inventory_to_nwc_ratio();
                let ap_ratio = hist.ap_to_nwc_ratio();
                let ar = nwc * ar_ratio;
                let inventory = nwc * inv_ratio;
                let ap = -nwc * ap_ratio;
                (ar, inventory, ap, nwc)
            }
            NwcMethod::Explicit => {
                let nwc = assumptions.nwc_explicit.explicit.unwrap_or(0.0);
                let ar = nwc * 0.5;
                let inventory = nwc * 0.3;
                let ap = -nwc * 0.2;
                (ar, inventory, ap, nwc)
            }
        };

        // Driver 3: PP&E rolls forward: PP&E[t] = PP&E[t-1] + Capex - D&A
        let capex = revenue * capex_pct + capex_adj;
        let ppe_net = prev_ppe + capex - da;

        // Driver 5: Debt and equity
        let debt = prev_debt + assumptions.debt_issuance - assumptions.debt_repayment;
        let dividends = net_income * assumptions.dividend_payout_ratio;
        let equity = prev_equity + net_income - dividends + assumptions.equity_issuance;

        // Balance sheet identity: Assets = Liabilities + Equity
        // Cash is the plug: Cash = (AP + Debt + Equity) - (AR + Inventory + PP&E)
        let total_liabilities_equity = ap + debt + equity;
        let non_cash_assets = ar + inventory + ppe_net;
        let cash = total_liabilities_equity - non_cash_assets;

        // If cash goes negative, the company needs to raise debt to cover it.
        // This is the standard "debt plug" fallback in three-statement modelling.
        let (cash, debt, total_liabilities_equity) = if cash < 0.0 {
            let shortfall = -cash;
            let adjusted_debt = debt + shortfall;
            let adjusted_cash = 0.0;
            let adjusted_total_le = ap + adjusted_debt + equity;
            (adjusted_cash, adjusted_debt, adjusted_total_le)
        } else {
            (cash, debt, total_liabilities_equity)
        };

        let total_assets = cash + ar + inventory + ppe_net;
        let balance_check = total_assets - total_liabilities_equity;

        // ── Cash Flow Statement ────────────────────────────────────────────
        // CFO = Net Income + D&A - ΔNWC
        let change_in_nwc = nwc - prev_nwc;
        let cfo = net_income + da - change_in_nwc;
        // CFI = -Capex
        let cfi = -capex;
        // CFF = Debt issuance - Debt repayment + Equity issuance - Dividends
        let cff = assumptions.debt_issuance - assumptions.debt_repayment
            + assumptions.equity_issuance
            - dividends;
        let net_cash_change = cfo + cfi + cff;

        // FCF (firm-level) = NOPAT + D&A - Capex - ΔNWC
        // NOPAT = EBIT × (1 - tax_rate)
        let nopat = ebit * (1.0 - assumptions.tax_rate);
        let free_cash_flow = nopat + da - capex - change_in_nwc;

        // Discount at WACC
        let discount_factor = 1.0 / (1.0 + assumptions.discount_rate).powi((p + 1) as i32);
        let present_value = free_cash_flow * discount_factor;

        periods.push(DriverPeriod {
            year,
            revenue,
            cogs,
            gross_profit,
            sga,
            da,
            ebit,
            interest_expense,
            ebt,
            tax,
            net_income,
            cash,
            accounts_receivable: ar,
            inventory,
            ppe_net,
            total_assets,
            accounts_payable: ap,
            debt,
            equity,
            total_liabilities_equity,
            balance_check,
            cfo,
            cfi,
            cff,
            net_cash_change,
            free_cash_flow,
            discount_factor,
            present_value,
        });

        prev_revenue = revenue;
        prev_nwc = nwc;
        prev_debt = debt;
        prev_equity = equity;
        prev_ppe = ppe_net;
        prev_cash = cash;
    }

    // Terminal value (Gordon Growth perpetuity on last FCF)
    let last_fcf = periods.last().map(|p| p.free_cash_flow).unwrap_or(0.0);
    let terminal_value = last_fcf * (1.0 + assumptions.terminal_growth)
        / (assumptions.discount_rate - assumptions.terminal_growth);
    let terminal_df =
        1.0 / (1.0 + assumptions.discount_rate).powi(total_years as i32);
    let terminal_pv = terminal_value * terminal_df;

    // Enterprise to equity
    let sum_pv: f64 = periods.iter().map(|p| p.present_value).sum();
    let enterprise_value = sum_pv + terminal_pv;
    let net_debt = prev_debt - prev_cash;
    let equity_value = enterprise_value - net_debt;
    let intrinsic_per_share = if hist.shares_outstanding > 0.0 {
        equity_value / hist.shares_outstanding
    } else {
        0.0
    };

    Ok(DriverProjectedModel {
        periods,
        terminal_value,
        terminal_pv,
        enterprise_value,
        equity_value,
        intrinsic_per_share,
        is_financial_sector: false,
        net_debt,
        shares_outstanding: hist.shares_outstanding,
    })
}

/// Financial-sector projection: equity-based residual income model.
///
/// Per Damodaran *Applied Corporate Finance* Ch. 19, banks and insurance
/// companies are valued using equity-based approaches because debt is a raw
/// material, not a source of capital. The residual income model:
///
///   Equity Value = Book Equity + Σ PV(Residual Income)
///   Residual Income = (ROE - COE) × Beginning Book Equity
///
/// This avoids the meaningless NWC, ROIC, and invested capital concepts
/// that the `financial_sector_guard` correctly blocks.
fn project_financial_sector(
    hist: &HistoricalSnapshot,
    assumptions: &DriverAssumptions,
) -> Result<DriverProjectedModel, DriverModelError> {
    let total_years = assumptions.total_years as usize;
    let mut periods: Vec<DriverPeriod> = Vec::with_capacity(total_years);

    let mut prev_revenue = hist.latest_revenue();
    let mut prev_equity = hist.latest_equity();
    let mut prev_debt = hist.latest_debt();
    let mut prev_cash = hist.latest_cash();

    let growth_rate = assumptions
        .revenue_growth
        .percent
        .unwrap_or_else(|| hist.revenue_cagr());
    let revenue_adj = assumptions.revenue_explicit.explicit.unwrap_or(0.0);

    // For financial-sector, the "margin" driver is ROE, not operating margin.
    // ROE = Net Income / Beginning Equity
    let roe = assumptions
        .gross_margin
        .percent
        .unwrap_or_else(|| hist.roe());
    let coe = assumptions.cost_of_equity;

    for p in 0..total_years {
        let year = (p + 1) as u32;

        // Revenue grows, but the valuation driver is ROE on equity
        let revenue = prev_revenue * (1.0 + growth_rate) + revenue_adj;

        // Net income from ROE × beginning equity
        let net_income = prev_equity * roe;

        // Dividends
        let dividends = net_income * assumptions.dividend_payout_ratio;

        // Equity rolls forward: Equity[t] = Equity[t-1] + NI - Dividends + Issuance
        let equity = prev_equity + net_income - dividends + assumptions.equity_issuance;

        // Debt (deposits for banks) grows with revenue
        let debt = prev_debt + assumptions.debt_issuance - assumptions.debt_repayment;

        // Simplified balance sheet: Cash + Other Assets = Debt + Equity
        // For banks, "other assets" = loans + securities
        let total_liabilities_equity = debt + equity;
        let cash = (total_liabilities_equity * 0.1).max(0.0); // 10% cash buffer
        let other_assets = total_liabilities_equity - cash;
        let total_assets = cash + other_assets;
        let balance_check = total_assets - total_liabilities_equity;

        // Residual income = (ROE - COE) × beginning equity
        let residual_income = (roe - coe) * prev_equity;

        // Discount residual income at COE (equity-level)
        let discount_factor = 1.0 / (1.0 + coe).powi((p + 1) as i32);
        let present_value = residual_income * discount_factor;

        // For financial-sector, FCF is not meaningful — use residual income
        let free_cash_flow = residual_income;

        periods.push(DriverPeriod {
            year,
            revenue,
            cogs: 0.0,
            gross_profit: revenue,
            sga: 0.0,
            da: 0.0,
            ebit: net_income, // NI is the primary metric for financials
            interest_expense: 0.0,
            ebt: net_income,
            tax: 0.0,
            net_income,
            cash,
            accounts_receivable: 0.0,
            inventory: 0.0,
            ppe_net: other_assets,
            total_assets,
            accounts_payable: 0.0,
            debt,
            equity,
            total_liabilities_equity,
            balance_check,
            cfo: net_income,
            cfi: 0.0,
            cff: assumptions.equity_issuance - dividends,
            net_cash_change: net_income + assumptions.equity_issuance - dividends,
            free_cash_flow,
            discount_factor,
            present_value,
        });

        prev_revenue = revenue;
        prev_equity = equity;
        prev_debt = debt;
        prev_cash = cash;
    }

    // Terminal value: perpetuity of residual income at terminal growth
    let last_ri = periods.last().map(|p| p.free_cash_flow).unwrap_or(0.0);
    let terminal_value =
        last_ri * (1.0 + assumptions.terminal_growth) / (coe - assumptions.terminal_growth);
    let terminal_df = 1.0 / (1.0 + coe).powi(total_years as i32);
    let terminal_pv = terminal_value * terminal_df;

    // Equity value = book equity + PV of residual income
    let sum_pv: f64 = periods.iter().map(|p| p.present_value).sum();
    let equity_value = prev_equity + sum_pv + terminal_pv;
    let intrinsic_per_share = if hist.shares_outstanding > 0.0 {
        equity_value / hist.shares_outstanding
    } else {
        0.0
    };

    Ok(DriverProjectedModel {
        periods,
        terminal_value,
        terminal_pv,
        enterprise_value: equity_value, // For financials, EV = equity value
        equity_value,
        intrinsic_per_share,
        is_financial_sector: true,
        net_debt: 0.0, // Net debt is not meaningful for financials
        shares_outstanding: hist.shares_outstanding,
    })
}

// ── Markdown report ──────────────────────────────────────────────────────────

/// Generate a human-readable forecast summary in Markdown.
pub fn generate_markdown_report(
    model: &DriverProjectedModel,
    assumptions: &DriverAssumptions,
    symbol: &str,
    current_price: f64,
) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Driver-Based Forecast: {symbol}\n\n"));

    // Assumptions table
    md.push_str("## Assumptions\n\n");
    md.push_str("| Driver | Value | Method |\n");
    md.push_str("|--------|-------|--------|\n");
    md.push_str(&format!(
        "| Revenue growth | {:.1}% | % change |\n",
        assumptions.revenue_growth.percent.unwrap_or(0.0) * 100.0
    ));
    md.push_str(&format!(
        "| Gross margin | {:.1}% | % of revenue |\n",
        assumptions.gross_margin.percent.unwrap_or(0.0) * 100.0
    ));
    md.push_str(&format!(
        "| SG&A / revenue | {:.1}% | % of revenue |\n",
        assumptions.sga_pct.percent.unwrap_or(0.0) * 100.0
    ));
    md.push_str(&format!(
        "| D&A / revenue | {:.1}% | % of revenue |\n",
        assumptions.da_pct.percent.unwrap_or(0.0) * 100.0
    ));
    md.push_str(&format!(
        "| Capex / revenue | {:.1}% | % of revenue |\n",
        assumptions.capex_pct.percent.unwrap_or(0.0) * 100.0
    ));
    md.push_str(&format!(
        "| NWC method | {:?} | — |\n",
        assumptions.nwc_method
    ));
    if assumptions.nwc_method == NwcMethod::Days {
        md.push_str(&format!(
            "| DSO / DIO / DPO | {:.0} / {:.0} / {:.0} days | Days |\n",
            assumptions.dso_days, assumptions.dio_days, assumptions.dpo_days
        ));
    } else {
        md.push_str(&format!(
            "| NWC / revenue | {:.1}% | % of revenue |\n",
            assumptions.nwc_pct * 100.0
        ));
    }
    md.push_str(&format!(
        "| Interest rate | {:.1}% | on debt |\n",
        assumptions.interest_rate * 100.0
    ));
    md.push_str(&format!(
        "| Tax rate | {:.1}% | effective |\n",
        assumptions.tax_rate * 100.0
    ));
    md.push_str(&format!(
        "| Discount rate | {:.1}% | {} |\n",
        assumptions.discount_rate * 100.0,
        if assumptions.is_financial_sector {
            "COE"
        } else {
            "WACC"
        }
    ));
    md.push_str(&format!(
        "| Terminal growth | {:.1}% | Gordon Growth |\n",
        assumptions.terminal_growth * 100.0
    ));
    md.push_str(&format!(
        "| Horizon | {} years | — |\n",
        assumptions.total_years
    ));
    md.push_str(&format!(
        "| Sector path | {} | — |\n",
        if assumptions.is_financial_sector {
            "Equity-based (ROE/COE)"
        } else {
            "Firm-level DCF (WACC)"
        }
    ));
    md.push('\n');

    // Valuation summary
    md.push_str("## Valuation Summary\n\n");
    md.push_str(&format!(
        "| Metric | Value |\n|--------|-------|\n"
    ));
    md.push_str(&format!(
        "| Intrinsic value/share | ${:.2} |\n",
        model.intrinsic_per_share
    ));
    md.push_str(&format!(
        "| Current price | ${:.2} |\n",
        current_price
    ));
    if current_price > 0.0 {
        let margin_of_safety =
            (model.intrinsic_per_share - current_price) / current_price;
        md.push_str(&format!(
            "| Margin of safety | {:.1}% |\n",
            margin_of_safety * 100.0
        ));
    }
    md.push_str(&format!(
        "| Enterprise value | ${:.0}M |\n",
        model.enterprise_value / 1e6
    ));
    md.push_str(&format!(
        "| Equity value | ${:.0}M |\n",
        model.equity_value / 1e6
    ));
    md.push_str(&format!(
        "| Terminal value | ${:.0}M |\n",
        model.terminal_value / 1e6
    ));
    md.push_str(&format!(
        "| Terminal PV share | {:.1}% |\n",
        if model.enterprise_value > 0.0 {
            model.terminal_pv / model.enterprise_value * 100.0
        } else {
            0.0
        }
    ));
    md.push('\n');

    // Projected statements (abbreviated)
    md.push_str("## Projected Income Statement\n\n");
    md.push_str("| Year | Revenue | Gross Profit | SG&A | EBIT | Net Income |\n");
    md.push_str("|------|---------|-------------|-----|------|------------|\n");
    for p in &model.periods {
        md.push_str(&format!(
            "| {} | ${:.0}M | ${:.0}M | ${:.0}M | ${:.0}M | ${:.0}M |\n",
            p.year,
            p.revenue / 1e6,
            p.gross_profit / 1e6,
            p.sga / 1e6,
            p.ebit / 1e6,
            p.net_income / 1e6
        ));
    }
    md.push('\n');

    md.push_str("## Projected Balance Sheet\n\n");
    md.push_str("| Year | Cash | AR | Inventory | PP&E | Debt | Equity | A=L+E? |\n");
    md.push_str("|------|------|-----|----------|------|------|--------|--------|\n");
    for p in &model.periods {
        md.push_str(&format!(
            "| {} | ${:.0}M | ${:.0}M | ${:.0}M | ${:.0}M | ${:.0}M | ${:.0}M | {:.1}M |\n",
            p.year,
            p.cash / 1e6,
            p.accounts_receivable / 1e6,
            p.inventory / 1e6,
            p.ppe_net / 1e6,
            p.debt / 1e6,
            p.equity / 1e6,
            p.balance_check / 1e6
        ));
    }
    md.push('\n');

    md.push_str("## Projected Cash Flow\n\n");
    md.push_str("| Year | CFO | CFI | CFF | Net Change | FCF |\n");
    md.push_str("|------|-----|-----|-----|-----------|-----|\n");
    for p in &model.periods {
        md.push_str(&format!(
            "| {} | ${:.0}M | ${:.0}M | ${:.0}M | ${:.0}M | ${:.0}M |\n",
            p.year,
            p.cfo / 1e6,
            p.cfi / 1e6,
            p.cff / 1e6,
            p.net_cash_change / 1e6,
            p.free_cash_flow / 1e6
        ));
    }
    md.push('\n');

    // Source notes
    md.push_str("## Source Notes\n\n");
    md.push_str("- FCF formula: NOPAT + D&A - Capex - ΔNWC (Damodaran, *Investment Valuation*)\n");
    md.push_str("- SG&A included in EBIT (Fabozzi, *Financial Management & Analysis* Ch. 6)\n");
    md.push_str("- Balance sheet identity enforced via cash plug (Fabozzi, three-statement linkage)\n");
    md.push_str("- Interest expense linked to debt balance (Damodaran, *Applied Corporate Finance*)\n");
    md.push_str("- PP&E rolls forward: PP&E[t] = PP&E[t-1] + Capex - D&A\n");
    md.push_str("- Retained earnings: Equity[t] = Equity[t-1] + NI - Dividends + Issuance\n");
    if model.is_financial_sector {
        md.push_str("- Financial-sector path: residual income = (ROE - COE) × equity (Damodaran Ch. 19)\n");
    }
    md.push_str("- Terminal value: Gordon Growth perpetuity (FCF × (1+g) / (r-g))\n");

    md
}
