//! Authoritative driver-based financial model.
//!
//! Non-financial issuers use a linked income-statement, balance-sheet, cash-flow,
//! and firm-DCF projection. Financial issuers use residual income. The model
//! never forces the balance sheet to balance: unmodelled historical assets and
//! liabilities are held constant and the resulting reconciliation difference is
//! surfaced on every projected period.

use serde::{Deserialize, Serialize};

use super::HistoricalSnapshot;
use crate::types::ProjectionAssumptionOverrides;

pub(crate) const IMPLIED_GROWTH_LO: f64 = -0.50;
pub(crate) const IMPLIED_GROWTH_HI: f64 = 1.00;
pub(crate) const IMPLIED_NET_MARGIN_LO: f64 = -0.30;
pub(crate) const IMPLIED_NET_MARGIN_HI: f64 = 0.50;
/// MAIA investor hurdle documented in `MA_Guidebook_July23.md`.
pub(crate) const MAIA_INVESTOR_TARGET_RETURN: f64 = 0.15;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub(crate) enum NwcMethod {
    Days,
    #[default]
    PercentOfRevenue,
    /// Annual change in NWC equals the configured percentage of revenue.
    ChangePercentOfRevenue,
    Explicit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectionAssumptions {
    pub revenue_growth: f64,
    pub revenue_explicit: f64,
    pub gross_margin: f64,
    pub sga_to_revenue: f64,
    pub other_operating_expense_to_revenue: f64,
    pub da_to_revenue: f64,
    pub tax_rate: f64,
    pub capex_to_revenue: f64,
    pub capex_explicit: f64,
    pub capex_da_ratio: Option<f64>,
    pub nwc_method: NwcMethod,
    pub dso_days: f64,
    pub dio_days: f64,
    pub dpo_days: f64,
    pub nwc_to_revenue: f64,
    pub nwc_explicit: f64,
    pub debt_issuance: f64,
    pub debt_repayment: f64,
    pub target_debt_equity: Option<f64>,
    pub interest_rate: f64,
    pub equity_issuance: f64,
    pub dividend_payout_ratio: f64,
    /// Investor's required equity return. This replaces CAPM cost of equity.
    pub investor_target_return: f64,
    /// Modified WACC using the investor target return as the equity component.
    pub discount_rate: f64,
    pub equity_weight: f64,
    pub debt_weight: f64,
    pub terminal_growth: f64,
    pub total_years: u8,
    pub stage1_years: u8,
    pub is_financial_sector: bool,
    pub financial_roe: f64,
}

impl Default for ProjectionAssumptions {
    fn default() -> Self {
        Self {
            revenue_growth: 0.08,
            revenue_explicit: 0.0,
            gross_margin: 0.40,
            sga_to_revenue: 0.15,
            other_operating_expense_to_revenue: 0.0,
            da_to_revenue: 0.03,
            tax_rate: 0.21,
            capex_to_revenue: 0.03,
            capex_explicit: 0.0,
            capex_da_ratio: None,
            nwc_method: NwcMethod::PercentOfRevenue,
            dso_days: 45.0,
            dio_days: 60.0,
            dpo_days: 30.0,
            nwc_to_revenue: 0.10,
            nwc_explicit: 0.0,
            debt_issuance: 0.0,
            debt_repayment: 0.0,
            target_debt_equity: None,
            interest_rate: 0.05,
            equity_issuance: 0.0,
            dividend_payout_ratio: 0.0,
            investor_target_return: MAIA_INVESTOR_TARGET_RETURN,
            discount_rate: MAIA_INVESTOR_TARGET_RETURN,
            equity_weight: 1.0,
            debt_weight: 0.0,
            terminal_growth: 0.025,
            total_years: 10,
            stage1_years: 3,
            is_financial_sector: false,
            financial_roe: 0.10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub(crate) enum ProjectionError {
    #[error("historical operating expenses do not reconcile to reported operating income")]
    OperatingExpensesUnreconciled,
    #[error("missing or invalid shares outstanding")]
    InvalidShares,
    #[error("insufficient historical data: need at least two revenue periods")]
    InsufficientHistory,
    #[error("{field} must be finite and within {minimum}..={maximum}")]
    InvalidRange {
        field: &'static str,
        minimum: f64,
        maximum: f64,
    },

    #[error("{field} must be finite and within {min}..={max}")]
    NotFiniteOrOutOfRange {
        field: &'static str,
        min: f64,
        max: f64,
    },
    #[error("projection horizon exceeds u8 capacity")]
    HorizonOverflow,
    #[error("stage1_years must be less than total_years")]
    InvalidHorizon,
    #[error("discount rate must be greater than terminal growth")]
    InvalidTerminalSpread,
    #[error("capex_da_ratio must be finite and positive")]
    InvalidCapexDaRatio,
    #[error("positive shareholder equity is required to calculate investor-modified WACC")]
    InvalidCapitalStructure,
}

impl ProjectionAssumptions {
    pub(crate) fn from_history(hist: &HistoricalSnapshot) -> Result<Self, ProjectionError> {
        let other_operating_expense_to_revenue = hist
            .other_operating_expense_to_revenue()
            .ok_or(ProjectionError::OperatingExpensesUnreconciled)?;
        let debt = hist.latest_debt().max(0.0);
        let equity = hist.latest_equity();
        if !equity.is_finite() || equity <= 0.0 {
            return Err(ProjectionError::InvalidCapitalStructure);
        }
        let interest_rate = if debt > 0.0 {
            (hist.interest_expense() / debt).clamp(0.0, 0.30)
        } else {
            0.0
        };
        let total_capital = debt + equity;
        let equity_weight = equity / total_capital;
        let debt_weight = debt / total_capital;
        let discount_rate = equity_weight * MAIA_INVESTOR_TARGET_RETURN
            + debt_weight * interest_rate * (1.0 - hist.tax_rate);
        let assumptions = Self {
            revenue_growth: hist.revenue_cagr(),
            gross_margin: hist.gross_margin(),
            sga_to_revenue: hist.sga_to_revenue(),
            other_operating_expense_to_revenue,
            da_to_revenue: hist.da_to_revenue(),
            tax_rate: hist.tax_rate,
            capex_to_revenue: hist.capex_to_revenue(),
            dso_days: hist.dso_days(),
            dio_days: hist.dio_days(),
            dpo_days: hist.dpo_days(),
            nwc_to_revenue: hist.nwc_to_revenue(),
            interest_rate,
            dividend_payout_ratio: hist.dividend_payout_ratio(),
            investor_target_return: MAIA_INVESTOR_TARGET_RETURN,
            discount_rate,
            equity_weight,
            debt_weight,
            financial_roe: hist.roe(),
            ..Self::default()
        };
        assumptions.validate()?;
        Ok(assumptions)
    }

    pub(crate) fn with_investor_target_return(
        mut self,
        target_return: f64,
    ) -> Result<Self, ProjectionError> {
        if !target_return.is_finite() || !(0.0..=1.0).contains(&target_return) {
            return Err(ProjectionError::InvalidRange {
                field: "investor_target_return",
                minimum: 0.0,
                maximum: 1.0,
            });
        }
        self.investor_target_return = target_return;
        self.discount_rate = self.equity_weight * target_return
            + self.debt_weight * self.interest_rate * (1.0 - self.tax_rate);
        self.validate()?;
        Ok(self)
    }

    pub(crate) fn from_history_with_overrides(
        hist: &HistoricalSnapshot,
        overrides: ProjectionAssumptionOverrides,
    ) -> Result<Self, ProjectionError> {
        let mut assumptions = Self::from_history(hist)?;
        if let Some(value) = overrides.revenue_growth {
            assumptions.revenue_growth = value;
        }
        if let Some(value) = overrides.gross_margin {
            assumptions.gross_margin = value;
        }
        if let Some(value) = overrides.da_to_revenue {
            assumptions.da_to_revenue = value;
        }
        if let Some(value) = overrides.capex_to_revenue {
            assumptions.capex_to_revenue = value;
        }
        if let Some(value) = overrides.nwc_to_revenue {
            assumptions.nwc_to_revenue = value;
        }
        if let Some(value) = overrides.tax_rate {
            assumptions.tax_rate = value;
        }
        if let Some(value) = overrides.discount_rate {
            assumptions.discount_rate = value;
        }
        if let Some(value) = overrides.terminal_growth {
            assumptions.terminal_growth = value;
        }
        assumptions.stage1_years = overrides.stage1_years.unwrap_or(assumptions.stage1_years);
        let stage2_years = overrides.stage2_years.unwrap_or(
            assumptions
                .total_years
                .saturating_sub(assumptions.stage1_years),
        );
        assumptions.total_years = assumptions
            .stage1_years
            .checked_add(stage2_years)
            .ok_or(ProjectionError::InvalidHorizon)?;
        assumptions.validate()?;
        Ok(assumptions)
    }

    pub(crate) fn with_overrides(
        mut self,
        overrides: ProjectionAssumptionOverrides,
    ) -> Result<Self, ProjectionError> {
        if let Some(value) = overrides.revenue_growth {
            self.revenue_growth = value;
        }
        if let Some(value) = overrides.gross_margin {
            self.gross_margin = value;
        }
        if let Some(value) = overrides.da_to_revenue {
            self.da_to_revenue = value;
        }
        if let Some(value) = overrides.capex_to_revenue {
            self.capex_to_revenue = value;
        }
        if let Some(value) = overrides.nwc_to_revenue {
            self.nwc_to_revenue = value;
        }
        if let Some(value) = overrides.tax_rate {
            self.tax_rate = value;
        }
        if let Some(value) = overrides.discount_rate {
            self.discount_rate = value;
        }
        if let Some(value) = overrides.terminal_growth {
            self.terminal_growth = value;
        }
        self.stage1_years = overrides.stage1_years.unwrap_or(self.stage1_years);
        let stage2_years = overrides
            .stage2_years
            .unwrap_or(self.total_years.saturating_sub(self.stage1_years));
        self.total_years = self
            .stage1_years
            .checked_add(stage2_years)
            .ok_or(ProjectionError::HorizonOverflow)?;
        self.validate()?;
        Ok(self)
    }

    pub(crate) fn validate(&self) -> Result<(), ProjectionError> {
        fn range(
            field: &'static str,
            value: f64,
            minimum: f64,
            maximum: f64,
        ) -> Result<(), ProjectionError> {
            if !value.is_finite() || !(minimum..=maximum).contains(&value) {
                return Err(ProjectionError::InvalidRange {
                    field,
                    minimum,
                    maximum,
                });
            }
            Ok(())
        }
        range("revenue_growth", self.revenue_growth, -0.50, 1.00)?;
        range("gross_margin", self.gross_margin, -1.0, 1.0)?;
        range("sga_to_revenue", self.sga_to_revenue, 0.0, 1.0)?;
        range(
            "other_operating_expense_to_revenue",
            self.other_operating_expense_to_revenue,
            0.0,
            1.0,
        )?;
        range("da_to_revenue", self.da_to_revenue, 0.0, 0.50)?;
        range("tax_rate", self.tax_rate, 0.0, 1.0)?;
        range("capex_to_revenue", self.capex_to_revenue, 0.0, 1.0)?;
        range("nwc_to_revenue", self.nwc_to_revenue, -1.0, 1.0)?;
        range("interest_rate", self.interest_rate, 0.0, 1.0)?;
        range(
            "dividend_payout_ratio",
            self.dividend_payout_ratio,
            0.0,
            1.0,
        )?;
        range("discount_rate", self.discount_rate, 0.0, 1.0)?;
        range("terminal_growth", self.terminal_growth, 0.0, 0.20)?;
        range("financial_roe", self.financial_roe, -1.0, 2.0)?;
        range(
            "investor_target_return",
            self.investor_target_return,
            0.0,
            1.0,
        )?;
        range("equity_weight", self.equity_weight, 0.0, 1.0)?;
        range("debt_weight", self.debt_weight, 0.0, 1.0)?;
        if (self.equity_weight + self.debt_weight - 1.0).abs() > 1e-9 {
            return Err(ProjectionError::InvalidRange {
                field: "capital_weights_sum",
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        for (field, value) in [
            ("revenue_explicit", self.revenue_explicit),
            ("capex_explicit", self.capex_explicit),
            ("nwc_explicit", self.nwc_explicit),
            ("debt_issuance", self.debt_issuance),
            ("debt_repayment", self.debt_repayment),
            ("equity_issuance", self.equity_issuance),
            ("dso_days", self.dso_days),
            ("dio_days", self.dio_days),
            ("dpo_days", self.dpo_days),
        ] {
            if !value.is_finite() {
                return Err(ProjectionError::InvalidRange {
                    field,
                    minimum: f64::MIN,
                    maximum: f64::MAX,
                });
            }
        }
        if self.total_years < 2 || self.stage1_years == 0 || self.stage1_years > self.total_years {
            return Err(ProjectionError::InvalidHorizon);
        }
        if self.discount_rate <= self.terminal_growth
            || self.investor_target_return <= self.terminal_growth
        {
            return Err(ProjectionError::InvalidTerminalSpread);
        }
        if self
            .capex_da_ratio
            .is_some_and(|value| !value.is_finite() || value <= 0.0)
        {
            return Err(ProjectionError::InvalidCapexDaRatio);
        }
        if self
            .target_debt_equity
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            return Err(ProjectionError::InvalidRange {
                field: "target_debt_equity",
                minimum: 0.0,
                maximum: f64::MAX,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectedPeriod {
    pub period: usize,
    pub year: f64,
    pub revenue: f64,
    pub cogs: f64,
    pub gross_profit: f64,
    pub sga: f64,
    pub other_operating_expenses: f64,
    pub da: f64,
    pub ebit: f64,
    pub interest_expense: f64,
    pub ebt: f64,
    pub tax: f64,
    pub net_income: f64,
    pub nopat: f64,
    pub capex: f64,
    pub change_in_nwc: f64,
    pub free_cash_flow: f64,
    pub cash: f64,
    pub accounts_receivable: f64,
    pub inventory: f64,
    pub accounts_payable: f64,
    pub ppe_net: f64,
    pub debt: f64,
    pub equity: f64,
    pub total_assets: f64,
    pub total_liabilities_equity: f64,
    pub balance_reconciliation: f64,
    pub cfo: f64,
    pub cfi: f64,
    pub cff: f64,
    pub discount_factor: f64,
    pub present_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectedFinancialModel {
    pub periods: Vec<ProjectedPeriod>,
    pub terminal_value: f64,
    pub terminal_pv: f64,
    pub enterprise_value: f64,
    pub net_debt: f64,
    pub equity_value: f64,
    pub intrinsic_per_share: f64,
    pub is_financial_sector: bool,
}

pub(crate) fn project_financial_model(
    hist: &HistoricalSnapshot,
    assumptions: &ProjectionAssumptions,
) -> Result<ProjectedFinancialModel, ProjectionError> {
    assumptions.validate()?;
    if !hist.shares_outstanding.is_finite() || hist.shares_outstanding <= 0.0 {
        return Err(ProjectionError::InvalidShares);
    }
    if assumptions.is_financial_sector {
        return project_financial(hist, assumptions);
    }
    if hist.revenue.len() < 2 {
        return Err(ProjectionError::InsufficientHistory);
    }
    project_industrial(hist, assumptions)
}

fn period_growth(assumptions: &ProjectionAssumptions, period: usize) -> f64 {
    if period < assumptions.stage1_years as usize {
        return assumptions.revenue_growth;
    }
    let fade_periods = usize::from(assumptions.total_years - assumptions.stage1_years);
    let fade_index = period - assumptions.stage1_years as usize + 1;
    let weight = fade_index as f64 / fade_periods as f64;
    assumptions.revenue_growth + (assumptions.terminal_growth - assumptions.revenue_growth) * weight
}

fn working_capital(
    hist: &HistoricalSnapshot,
    assumptions: &ProjectionAssumptions,
    revenue: f64,
    cogs: f64,
    previous_nwc: f64,
) -> (f64, f64, f64, f64) {
    match assumptions.nwc_method {
        NwcMethod::Days => {
            let ar = revenue * assumptions.dso_days / 365.0;
            let inventory = cogs * assumptions.dio_days / 365.0;
            let ap = cogs * assumptions.dpo_days / 365.0;
            (ar, inventory, ap, ar + inventory - ap)
        }
        NwcMethod::PercentOfRevenue | NwcMethod::ChangePercentOfRevenue | NwcMethod::Explicit => {
            let target = if assumptions.nwc_method == NwcMethod::Explicit {
                assumptions.nwc_explicit
            } else if assumptions.nwc_method == NwcMethod::ChangePercentOfRevenue {
                previous_nwc + revenue * assumptions.nwc_to_revenue + assumptions.nwc_explicit
            } else {
                revenue * assumptions.nwc_to_revenue + assumptions.nwc_explicit
            };
            let historical = hist.latest_ar() + hist.latest_inventory() - hist.latest_ap();
            if historical.abs() > f64::EPSILON {
                let scale = target / historical;
                let ar = hist.latest_ar() * scale;
                let inventory = hist.latest_inventory() * scale;
                let ap = hist.latest_ap() * scale;
                (ar, inventory, ap, ar + inventory - ap)
            } else if target >= 0.0 {
                (target * 0.7, target * 0.5, target * 0.2, target)
            } else {
                let magnitude = target.abs();
                (magnitude * 0.2, magnitude * 0.1, magnitude * 1.3, target)
            }
        }
    }
}

fn project_industrial(
    hist: &HistoricalSnapshot,
    assumptions: &ProjectionAssumptions,
) -> Result<ProjectedFinancialModel, ProjectionError> {
    let mut periods = Vec::with_capacity(usize::from(assumptions.total_years));
    let mut previous_revenue = hist.latest_revenue();
    let mut previous_nwc = hist.latest_ar() + hist.latest_inventory() - hist.latest_ap();
    let mut previous_debt = hist.latest_debt();
    let mut previous_ppe = hist.latest_ppe_net();
    let mut previous_cash = hist.latest_cash();
    let mut previous_equity = hist.latest_equity();
    let starting_other_assets = hist.latest_total_assets()
        - (hist.latest_cash() + hist.latest_ar() + hist.latest_inventory() + hist.latest_ppe_net());
    let starting_other_liabilities =
        hist.latest_total_assets() - (hist.latest_ap() + hist.latest_debt() + hist.latest_equity());

    for period in 0..usize::from(assumptions.total_years) {
        let growth = period_growth(assumptions, period);
        let revenue = previous_revenue * (1.0 + growth) + assumptions.revenue_explicit;
        let cogs = revenue * (1.0 - assumptions.gross_margin);
        let gross_profit = revenue - cogs;
        let sga = revenue * assumptions.sga_to_revenue;
        let other_operating_expenses = revenue * assumptions.other_operating_expense_to_revenue;
        let capex = revenue * assumptions.capex_to_revenue + assumptions.capex_explicit;
        let da = assumptions
            .capex_da_ratio
            .map_or(revenue * assumptions.da_to_revenue, |ratio| capex / ratio);
        let ebit = gross_profit - sga - other_operating_expenses - da;
        let provisional_debt =
            previous_debt + assumptions.debt_issuance - assumptions.debt_repayment;
        let debt = assumptions
            .target_debt_equity
            .map_or(provisional_debt, |ratio| previous_equity * ratio);
        let interest_expense = ((previous_debt + debt) / 2.0).max(0.0) * assumptions.interest_rate;
        let ebt = ebit - interest_expense;
        let tax = ebt.max(0.0) * assumptions.tax_rate;
        let net_income = ebt - tax;
        let nopat = ebit * (1.0 - assumptions.tax_rate);
        let dividends = net_income.max(0.0) * assumptions.dividend_payout_ratio;
        let equity = previous_equity + net_income - dividends + assumptions.equity_issuance;
        let (ar, inventory, ap, nwc) =
            working_capital(hist, assumptions, revenue, cogs, previous_nwc);
        let change_in_nwc = nwc - previous_nwc;
        let ppe_net = previous_ppe + capex - da;
        let cfo = net_income + da - change_in_nwc;
        let cfi = -capex;
        let cff = debt - previous_debt + assumptions.equity_issuance - dividends;
        let cash = previous_cash + cfo + cfi + cff;
        let total_assets = cash + ar + inventory + ppe_net + starting_other_assets;
        let total_liabilities_equity = ap + debt + equity + starting_other_liabilities;
        let balance_reconciliation = total_assets - total_liabilities_equity;
        let free_cash_flow = nopat + da - capex - change_in_nwc;
        let discount_factor = 1.0 / (1.0 + assumptions.discount_rate).powf(period as f64 + 0.5);
        let present_value = free_cash_flow * discount_factor;
        periods.push(ProjectedPeriod {
            period,
            year: (period + 1) as f64,
            revenue,
            cogs,
            gross_profit,
            sga,
            other_operating_expenses,
            da,
            ebit,
            interest_expense,
            ebt,
            tax,
            net_income,
            nopat,
            capex,
            change_in_nwc,
            free_cash_flow,
            cash,
            accounts_receivable: ar,
            inventory,
            accounts_payable: ap,
            ppe_net,
            debt,
            equity,
            total_assets,
            total_liabilities_equity,
            balance_reconciliation,
            cfo,
            cfi,
            cff,
            discount_factor,
            present_value,
        });
        previous_revenue = revenue;
        previous_nwc = nwc;
        previous_debt = debt;
        previous_ppe = ppe_net;
        previous_cash = cash;
        previous_equity = equity;
    }
    finish_model(hist, assumptions, periods, false)
}

fn project_financial(
    hist: &HistoricalSnapshot,
    assumptions: &ProjectionAssumptions,
) -> Result<ProjectedFinancialModel, ProjectionError> {
    let mut periods = Vec::with_capacity(usize::from(assumptions.total_years));
    let opening_equity = hist.latest_equity();
    let mut previous_equity = opening_equity;
    let mut previous_revenue = hist.latest_revenue();
    for period in 0..usize::from(assumptions.total_years) {
        let revenue = previous_revenue * (1.0 + period_growth(assumptions, period));
        let net_income = previous_equity * assumptions.financial_roe;
        let dividends = net_income.max(0.0) * assumptions.dividend_payout_ratio;
        let equity = previous_equity + net_income - dividends + assumptions.equity_issuance;
        let residual_income =
            (assumptions.financial_roe - assumptions.investor_target_return) * previous_equity;
        let discount_factor =
            1.0 / (1.0 + assumptions.investor_target_return).powf(period as f64 + 0.5);
        periods.push(ProjectedPeriod {
            period,
            year: (period + 1) as f64,
            revenue,
            cogs: 0.0,
            gross_profit: 0.0,
            sga: 0.0,
            other_operating_expenses: 0.0,
            da: 0.0,
            ebit: net_income,
            interest_expense: 0.0,
            ebt: net_income,
            tax: 0.0,
            net_income,
            nopat: net_income,
            capex: 0.0,
            change_in_nwc: 0.0,
            free_cash_flow: residual_income,
            cash: 0.0,
            accounts_receivable: 0.0,
            inventory: 0.0,
            accounts_payable: 0.0,
            ppe_net: 0.0,
            debt: 0.0,
            equity,
            total_assets: equity,
            total_liabilities_equity: equity,
            balance_reconciliation: 0.0,
            cfo: net_income,
            cfi: 0.0,
            cff: assumptions.equity_issuance - dividends,
            discount_factor,
            present_value: residual_income * discount_factor,
        });
        previous_revenue = revenue;
        previous_equity = equity;
    }
    let last_residual = periods
        .last()
        .map(|period| period.free_cash_flow)
        .unwrap_or(0.0);
    let terminal_value = last_residual * (1.0 + assumptions.terminal_growth)
        / (assumptions.investor_target_return - assumptions.terminal_growth);
    let terminal_pv = terminal_value
        / (1.0 + assumptions.investor_target_return).powf(f64::from(assumptions.total_years) - 0.5);
    let equity_value = opening_equity
        + periods
            .iter()
            .map(|period| period.present_value)
            .sum::<f64>()
        + terminal_pv;
    Ok(ProjectedFinancialModel {
        periods,
        terminal_value,
        terminal_pv,
        enterprise_value: equity_value,
        net_debt: 0.0,
        equity_value,
        intrinsic_per_share: equity_value / hist.shares_outstanding,
        is_financial_sector: true,
    })
}

fn finish_model(
    hist: &HistoricalSnapshot,
    assumptions: &ProjectionAssumptions,
    periods: Vec<ProjectedPeriod>,
    is_financial_sector: bool,
) -> Result<ProjectedFinancialModel, ProjectionError> {
    let last_fcf = periods
        .last()
        .map(|period| period.free_cash_flow)
        .unwrap_or(0.0);
    let terminal_value = last_fcf * (1.0 + assumptions.terminal_growth)
        / (assumptions.discount_rate - assumptions.terminal_growth);
    let terminal_pv = terminal_value
        / (1.0 + assumptions.discount_rate).powf(f64::from(assumptions.total_years) - 0.5);
    let enterprise_value = periods
        .iter()
        .map(|period| period.present_value)
        .sum::<f64>()
        + terminal_pv;
    let net_debt = hist.net_debt();
    let equity_value = enterprise_value - net_debt;
    Ok(ProjectedFinancialModel {
        periods,
        terminal_value,
        terminal_pv,
        enterprise_value,
        net_debt,
        equity_value,
        intrinsic_per_share: equity_value / hist.shares_outstanding,
        is_financial_sector,
    })
}

pub(crate) fn implied_growth(
    hist: &HistoricalSnapshot,
    assumptions: &ProjectionAssumptions,
    current_price: f64,
) -> Option<f64> {
    solve_monotone(
        IMPLIED_GROWTH_LO,
        IMPLIED_GROWTH_HI,
        current_price,
        |growth| {
            let mut candidate = assumptions.clone();
            candidate.revenue_growth = growth;
            project_financial_model(hist, &candidate)
                .ok()
                .map(|model| model.intrinsic_per_share)
        },
    )
}

pub(crate) fn implied_net_margin_at_growth(
    hist: &HistoricalSnapshot,
    assumptions: &ProjectionAssumptions,
    growth: f64,
    current_price: f64,
) -> Option<f64> {
    let revenue = hist.latest_revenue();
    if revenue <= 0.0 || 1.0 - assumptions.tax_rate <= 0.01 {
        return None;
    }
    let interest_to_revenue = hist.interest_expense() / revenue;
    solve_monotone(
        IMPLIED_NET_MARGIN_LO,
        IMPLIED_NET_MARGIN_HI,
        current_price,
        |net_margin| {
            let mut candidate = assumptions.clone();
            candidate.revenue_growth = growth;
            candidate.gross_margin = net_margin / (1.0 - assumptions.tax_rate)
                + assumptions.sga_to_revenue
                + assumptions.other_operating_expense_to_revenue
                + assumptions.da_to_revenue
                + interest_to_revenue;
            project_financial_model(hist, &candidate)
                .ok()
                .map(|model| model.intrinsic_per_share)
        },
    )
}

fn solve_monotone(
    lower: f64,
    upper: f64,
    target: f64,
    value_at: impl Fn(f64) -> Option<f64>,
) -> Option<f64> {
    if !target.is_finite() || target <= 0.0 {
        return None;
    }
    if value_at(lower)? > target || value_at(upper)? < target {
        return None;
    }
    let (mut low, mut high) = (lower, upper);
    for _ in 0..60 {
        let midpoint = (low + high) / 2.0;
        let value = value_at(midpoint)?;
        if (value - target).abs() < 0.0001 {
            return Some(midpoint);
        }
        if value > target {
            high = midpoint;
        } else {
            low = midpoint;
        }
    }
    Some((low + high) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn worked_history() -> HistoricalSnapshot {
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

    /// Standard operating-income bridge: GP − SG&A − D&A − other OPEX = EBIT.
    #[test]
    fn reported_operating_income_reconciles_other_operating_expense() {
        let history = worked_history();
        assert_eq!(history.other_operating_expense_to_revenue(), Some(0.05));
    }

    /// Damodaran FCFF identity and Gordon-growth terminal value are reproduced
    /// from independently calculated values, not values emitted by the model.
    #[test]
    fn fcff_and_terminal_value_match_worked_equations() {
        let history = worked_history();
        let mut assumptions =
            ProjectionAssumptions::from_history(&history).expect("worked history reconciles");
        assumptions.revenue_growth = 0.0;
        assumptions.terminal_growth = 0.0;
        assumptions.discount_rate = 0.10;
        assumptions.total_years = 2;
        assumptions.stage1_years = 1;
        assumptions.nwc_to_revenue = 0.0;
        let model = project_financial_model(&history, &assumptions).expect("worked projection");
        let first = model.periods.first().expect("two-period model");
        // EBIT = 1000 - 600 - 100 - 50 other OPEX - 50 D&A = 200.
        assert!((first.ebit - 200.0).abs() < 1e-10);
        // FCFF = EBIT(1-t) + D&A - capex - ΔNWC = 160 + 50 - 80 = 130.
        assert!((first.free_cash_flow - 130.0).abs() < 1e-10);
        // Gordon value at g=0 is FCF/r = 130/0.10 = 1300.
        assert!((model.terminal_value - 1300.0).abs() < 1e-8);
    }

    /// MAIA modified WACC: investor target return replaces CAPM cost of equity,
    /// while issuer debt cost, tax shield, and capital weights remain observed.
    #[test]
    fn investor_target_return_drives_modified_wacc() {
        let mut history = worked_history();
        history.total_equity = vec![("2024".to_string(), 600.0), ("2025".to_string(), 600.0)];
        history.long_term_debt = vec![("2024".to_string(), 400.0), ("2025".to_string(), 400.0)];
        history.interest_expense = vec![("2024".to_string(), 32.0), ("2025".to_string(), 32.0)];
        history.tax_rate = 0.20;
        let assumptions =
            ProjectionAssumptions::from_history(&history).expect("valid capital structure");
        assert!((assumptions.equity_weight - 0.60).abs() < 1e-12);
        assert!((assumptions.debt_weight - 0.40).abs() < 1e-12);
        assert!((assumptions.investor_target_return - 0.15).abs() < 1e-12);
        assert!((assumptions.discount_rate - 0.1156).abs() < 1e-12);
        let higher_hurdle = assumptions
            .with_investor_target_return(0.20)
            .expect("valid investor hurdle");
        assert!((higher_hurdle.discount_rate - 0.1456).abs() < 1e-12);
    }

    /// Wall Street Prep's published reverse-DCF case: $100m revenue, 40% EBIT
    /// margin, 21% tax, capex at 4% of revenue, D&A at 80% of capex, annual
    /// change in NWC at 2% of revenue, 10% WACC, 2.5% terminal growth, $20m
    /// net debt, 10m shares, and a $60 price imply 12.4% five-year growth.
    /// Source: https://www.wallstreetprep.com/knowledge/reverse-dcf-model/
    #[test]
    fn reverse_dcf_reproduces_wall_street_prep_worked_case() {
        let mut history = worked_history();
        for series in [
            &mut history.revenue,
            &mut history.cogs,
            &mut history.da,
            &mut history.capex,
            &mut history.sga,
            &mut history.operating_income,
            &mut history.current_assets,
            &mut history.current_liabilities,
            &mut history.cash,
            &mut history.accounts_receivable,
            &mut history.inventory,
            &mut history.accounts_payable,
            &mut history.total_equity,
            &mut history.total_assets,
            &mut history.ppe_net,
            &mut history.interest_expense,
            &mut history.dividends_paid,
            &mut history.net_income,
        ] {
            for (_, value) in series {
                *value /= 10.0;
            }
        }
        history.shares_outstanding = 10.0;
        history.long_term_debt = vec![("2024".to_string(), 20.0), ("2025".to_string(), 20.0)];
        let assumptions = ProjectionAssumptions {
            revenue_growth: 0.124,
            gross_margin: 0.432,
            sga_to_revenue: 0.0,
            other_operating_expense_to_revenue: 0.0,
            da_to_revenue: 0.032,
            tax_rate: 0.21,
            capex_to_revenue: 0.04,
            capex_da_ratio: Some(1.25),
            nwc_method: NwcMethod::ChangePercentOfRevenue,
            nwc_to_revenue: 0.02,
            discount_rate: 0.10,
            terminal_growth: 0.025,
            total_years: 5,
            stage1_years: 5,
            ..ProjectionAssumptions::default()
        };
        let model = project_financial_model(&history, &assumptions).expect("published case model");
        assert!(
            (model.intrinsic_per_share - 60.0).abs() < 1.0,
            "intrinsic per share was {}",
            model.intrinsic_per_share
        );
        let implied = implied_growth(&history, &assumptions, 60.0).expect("published case solve");
        assert!(
            (implied - 0.124).abs() < 0.005,
            "implied growth was {implied}"
        );
    }

    #[test]
    fn revenue_growth_fades_to_terminal_growth_after_stage_one() {
        let history = worked_history();
        let mut assumptions =
            ProjectionAssumptions::from_history(&history).expect("worked history reconciles");
        assumptions.revenue_growth = 0.10;
        assumptions.terminal_growth = 0.02;
        assumptions.total_years = 3;
        assumptions.stage1_years = 1;
        let model = project_financial_model(&history, &assumptions).expect("worked projection");
        let revenues: Vec<f64> = model.periods.iter().map(|period| period.revenue).collect();
        assert!((revenues[0] - 1100.0).abs() < 1e-8);
        assert!((revenues[1] - 1166.0).abs() < 1e-8);
        assert!((revenues[2] - 1189.32).abs() < 1e-8);
    }
}
