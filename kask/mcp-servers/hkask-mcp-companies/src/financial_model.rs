//! Historical financial normalization and the authoritative driver-model facade.
//!
//! The projection implementation lives in `financial_model/driver_model.rs`.
//! Historical operating expenses must reconcile to reported operating income
//! before a non-financial valuation can run; missing components are unavailable,
//! not silently converted into a complete model.

use crate::providers::CompanyProfile;

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
            vec![
                "comparable_analysis",
                "reverse_dcf with manual overrides",
                "dividend discount model",
            ],
        ),
        _ => (
            "FCF-based DCF valuation",
            vec![
                "comparable_analysis",
                "reverse_dcf with manual overrides",
                "ep_valuation (equity-based)",
            ],
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
fn parse_optional_financial_field(entry: &serde_json::Value, fields: &[&str]) -> Option<f64> {
    fields
        .iter()
        .find_map(|field| entry.get(*field))
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite())
}

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
    /// SG&A expenses (sellingGeneralAndAdministrativeExpenses from FMP).
    pub sga: Vec<(String, f64)>,
    /// Reported EBIT/operating income. Required to reconcile operating expenses.
    pub operating_income: Vec<(String, f64)>,

    pub current_assets: Vec<(String, f64)>,
    pub current_liabilities: Vec<(String, f64)>,
    pub cash: Vec<(String, f64)>,
    pub long_term_debt: Vec<(String, f64)>,
    /// Accounts receivable (netReceivables from FMP).
    pub accounts_receivable: Vec<(String, f64)>,
    /// Inventory (inventory from FMP).
    pub inventory: Vec<(String, f64)>,
    /// Accounts payable (accountsPayable from FMP).
    pub accounts_payable: Vec<(String, f64)>,
    /// Total stockholders equity (totalStockholdersEquity from FMP).
    pub total_equity: Vec<(String, f64)>,
    /// Net PP&E (netPPE or propertyPlantEquipmentNet from FMP).
    pub ppe_net: Vec<(String, f64)>,
    /// Interest expense (interestExpense from FMP income statement).
    pub interest_expense: Vec<(String, f64)>,
    /// Dividends paid (dividendsPaid from FMP cash flow).
    pub dividends_paid: Vec<(String, f64)>,
    /// Net income (netIncome from FMP/EODHD income statements) — the DuPont
    /// profitability input.
    pub net_income: Vec<(String, f64)>,
    /// Total assets (totalAssets from balance sheets) — the DuPont
    /// asset-turnover input.
    pub total_assets: Vec<(String, f64)>,

    pub shares_outstanding: f64,
    pub tax_rate: f64,
}

/// DuPont decomposition of demonstrated capability (operator ruling
/// 2026-09-10): ROE = net profit margin × asset turnover × equity
/// multiplier, plus the Higgins sustainable growth rate
/// SGR = ROE × retention — the growth a company can self-fund without
/// external financing. Industry-aware headline: ROE for financial-sector
/// companies, net margin for everyone else.
pub(crate) struct DuPontAnalysis {
    pub net_profit_margin: f64,
    pub asset_turnover: f64,
    pub equity_multiplier: f64,
    pub roe: f64,
    pub retention: f64,
    pub sustainable_growth_rate: f64,
    pub years: usize,
}

/// Resolve the first numeric share count in provider precedence order. Null,
/// missing and nonnumeric fields cannot resolve; numeric zero/negative values
/// remain explicit so DCF validation rejects them instead of hiding bad data.
pub(crate) fn resolve_shares_outstanding(
    income_statements: &[serde_json::Value],
    key_metrics: &[serde_json::Value],
    profile: &serde_json::Value,
) -> Option<f64> {
    let income = income_statements.first();
    let metrics = key_metrics.first();
    [
        income.and_then(|entry| entry.get("weightedAverageShsOutDil")),
        income.and_then(|entry| entry.get("weightedAverageShsOut")),
        metrics.and_then(|entry| entry.get("weightedAverageShsOutDil")),
        metrics.and_then(|entry| entry.get("weightedAverageShsOut")),
        profile.get("sharesOutstanding"),
    ]
    .into_iter()
    .find_map(|candidate| candidate.and_then(serde_json::Value::as_f64))
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
        // Extract revenue, COGS, D&A, SG&A, interest, tax data from income statements
        let mut revenue: Vec<(String, f64)> = Vec::new();
        let mut cogs: Vec<(String, f64)> = Vec::new();
        let mut da: Vec<(String, f64)> = Vec::new();
        let mut sga: Vec<(String, f64)> = Vec::new();
        let mut operating_income: Vec<(String, f64)> = Vec::new();
        let mut interest_expense: Vec<(String, f64)> = Vec::new();
        let mut net_income: Vec<(String, f64)> = Vec::new();
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
            let s = parse_financial_field(entry, "sellingGeneralAndAdministrativeExpenses");
            let reported_operating_income = parse_optional_financial_field(
                entry,
                &["operatingIncome", "operatingIncomeLoss", "ebit"],
            );
            let ie = parse_financial_field(entry, "interestExpense");
            let te = parse_financial_field(entry, "incomeTaxExpense");
            let pi = parse_financial_field_or(entry, "incomeBeforeTax", 1.0);
            let ni = parse_financial_field(entry, "netIncome");

            if year.is_empty() || rev == 0.0 {
                continue;
            }
            revenue.push((year.to_string(), rev));
            cogs.push((year.to_string(), c));
            da.push((year.to_string(), d));
            sga.push((year.to_string(), s));
            if let Some(value) = reported_operating_income {
                operating_income.push((year.to_string(), value));
            }
            interest_expense.push((year.to_string(), ie));
            net_income.push((year.to_string(), ni));
            tax_expense.push(te);
            pre_tax_income.push(pi);
        }

        // Extract balance sheet items

        let mut current_assets: Vec<(String, f64)> = Vec::new();
        let mut current_liabilities: Vec<(String, f64)> = Vec::new();
        let mut cash: Vec<(String, f64)> = Vec::new();
        let mut long_term_debt: Vec<(String, f64)> = Vec::new();
        let mut accounts_receivable: Vec<(String, f64)> = Vec::new();
        let mut inventory: Vec<(String, f64)> = Vec::new();
        let mut accounts_payable: Vec<(String, f64)> = Vec::new();
        let mut total_equity: Vec<(String, f64)> = Vec::new();
        let mut total_assets: Vec<(String, f64)> = Vec::new();
        let mut ppe_net: Vec<(String, f64)> = Vec::new();

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
            accounts_receivable.push((
                year.to_string(),
                entry
                    .get("netReceivables")
                    .or_else(|| entry.get("accountsReceivables"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            ));
            inventory.push((
                year.to_string(),
                entry
                    .get("inventory")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            ));
            accounts_payable.push((
                year.to_string(),
                entry
                    .get("accountsPayable")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            ));
            total_equity.push((
                year.to_string(),
                entry
                    .get("totalStockholdersEquity")
                    .or_else(|| entry.get("totalStockholderEquity"))
                    .or_else(|| entry.get("totalEquity"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            ));
            total_assets.push((
                year.to_string(),
                parse_financial_field(entry, "totalAssets"),
            ));
            ppe_net.push((
                year.to_string(),
                entry
                    .get("netPPE")
                    .or_else(|| entry.get("propertyPlantEquipmentNet"))
                    .or_else(|| entry.get("totalNonCurrentAssets"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            ));
        }

        // Extract capex and dividends from cash flows (FMP: capex is negative)
        let mut capex: Vec<(String, f64)> = Vec::new();
        let mut dividends_paid: Vec<(String, f64)> = Vec::new();
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
            let div = parse_financial_field(entry, "dividendsPaid");
            dividends_paid.push((year.to_string(), div.abs()));
        }

        let shares_outstanding =
            resolve_shares_outstanding(income_statements, key_metrics, profile).unwrap_or(1_000.0);

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
            sga,
            operating_income,

            current_assets,
            current_liabilities,
            cash,
            long_term_debt,
            accounts_receivable,
            inventory,
            accounts_payable,
            total_equity,
            total_assets,
            ppe_net,
            interest_expense,
            dividends_paid,
            net_income,

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

    /// Demonstrated full-period revenue CAGR from positive reported annual
    /// observations. Unlike model defaults, this never fabricates a fallback.
    pub fn demonstrated_revenue_cagr(&self) -> Option<f64> {
        let periods = self.revenue.len().checked_sub(1)?;
        let first = self.revenue.first().map(|(_, value)| *value)?;
        let last = self.revenue.last().map(|(_, value)| *value)?;
        if first <= 0.0 || last <= 0.0 {
            return None;
        }
        let growth = (last / first).powf(1.0 / periods as f64) - 1.0;
        growth.is_finite().then_some(growth)
    }

    /// Revenue CAGR from historical data, with the legacy model default when
    /// demonstrated growth cannot be measured.
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

    /// DuPont decomposition over income periods with matching beginning and
    /// ending balance sheets. Asset turnover and ROE use average assets and
    /// average equity; components are robust per-period medians. The identity
    /// NPM × AT × EM = ROE holds exactly within each measured period and only
    /// approximately across independently aggregated medians. Retention is
    /// 1 − dividends/net income clamped to [0, 1]. SGR = median ROE × median
    /// retention; with no profitable year the self-funding rate is zero.
    pub fn dupont(&self) -> Option<DuPontAnalysis> {
        let net_by_year: std::collections::HashMap<&str, f64> = self
            .net_income
            .iter()
            .map(|(year, value)| (year.as_str(), *value))
            .collect();
        let average_assets_by_year: std::collections::HashMap<&str, f64> = self
            .total_assets
            .windows(2)
            .filter_map(|window| {
                let [(previous_year, previous), (year, current)] = window else {
                    return None;
                };
                if previous_year == year || *previous <= 0.0 || *current <= 0.0 {
                    return None;
                }
                Some((year.as_str(), (previous + current) / 2.0))
            })
            .collect();
        let average_equity_by_year: std::collections::HashMap<&str, f64> = self
            .total_equity
            .windows(2)
            .filter_map(|window| {
                let [(previous_year, previous), (year, current)] = window else {
                    return None;
                };
                if previous_year == year || *previous <= 0.0 || *current <= 0.0 {
                    return None;
                }
                Some((year.as_str(), (previous + current) / 2.0))
            })
            .collect();
        let dividends_by_year: std::collections::HashMap<&str, f64> = self
            .dividends_paid
            .iter()
            .map(|(year, value)| (year.as_str(), *value))
            .collect();

        let mut net_profit_margins = Vec::new();
        let mut asset_turnovers = Vec::new();
        let mut equity_multipliers = Vec::new();
        let mut roes = Vec::new();
        let mut retentions = Vec::new();
        for (year, revenue) in &self.revenue {
            let (Some(&net_income), Some(&average_assets), Some(&average_equity)) = (
                net_by_year.get(year.as_str()),
                average_assets_by_year.get(year.as_str()),
                average_equity_by_year.get(year.as_str()),
            ) else {
                continue;
            };
            if *revenue <= 0.0 || average_assets <= 0.0 || average_equity <= 0.0 {
                continue;
            }
            net_profit_margins.push(net_income / *revenue);
            asset_turnovers.push(*revenue / average_assets);
            equity_multipliers.push(average_assets / average_equity);
            roes.push(net_income / average_equity);
            if net_income > 0.0 {
                let payout = dividends_by_year.get(year.as_str()).copied().unwrap_or(0.0);
                retentions.push((1.0 - payout / net_income).clamp(0.0, 1.0));
            }
        }
        if net_profit_margins.is_empty() {
            return None;
        }
        let years = net_profit_margins.len();
        let median = |mut values: Vec<f64>| {
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mid = values.len() / 2;
            if values.len().is_multiple_of(2) {
                (values[mid - 1] + values[mid]) / 2.0
            } else {
                values[mid]
            }
        };
        let roe = median(roes);
        let retention = if retentions.is_empty() {
            0.0
        } else {
            median(retentions)
        };
        Some(DuPontAnalysis {
            net_profit_margin: median(net_profit_margins),
            asset_turnover: median(asset_turnovers),
            equity_multiplier: median(equity_multipliers),
            roe,
            retention,
            sustainable_growth_rate: roe * retention,
            years,
        })
    }

    // ── New accessors for the driver-based three-statement model ──────────

    /// Latest SG&A expense.
    pub fn latest_sga(&self) -> f64 {
        self.sga.last().map(|(_, v)| *v).unwrap_or(0.0)
    }

    /// SG&A as percentage of revenue.
    pub fn sga_to_revenue(&self) -> f64 {
        let revenue = self.latest_revenue();
        if revenue <= 0.0 {
            return 0.0;
        }
        self.latest_sga() / revenue
    }

    /// Residual operating expense share required to reconcile gross profit to
    /// reported operating income after SG&A and D&A. A negative residual means
    /// the provider components overlap or disagree and is therefore unavailable.
    pub fn other_operating_expense_to_revenue(&self) -> Option<f64> {
        let revenue = self.latest_revenue();
        let operating_income = self.operating_income.last().map(|(_, value)| *value)?;
        if revenue <= 0.0 {
            return None;
        }
        let residual = self.latest_revenue()
            - self.latest_cogs()
            - self.latest_sga()
            - self.latest_da()
            - operating_income;
        let ratio = residual / revenue;
        (ratio.is_finite() && ratio >= -1e-6).then_some(ratio.max(0.0))
    }

    pub fn latest_total_assets(&self) -> f64 {
        self.total_assets
            .last()
            .map(|(_, value)| *value)
            .unwrap_or(0.0)
    }

    /// Latest total stockholders equity.
    pub fn latest_equity(&self) -> f64 {
        self.total_equity.last().map(|(_, v)| *v).unwrap_or(0.0)
    }

    /// Latest accounts receivable.
    pub fn latest_ar(&self) -> f64 {
        self.accounts_receivable
            .last()
            .map(|(_, v)| *v)
            .unwrap_or(0.0)
    }

    /// Latest inventory.
    pub fn latest_inventory(&self) -> f64 {
        self.inventory.last().map(|(_, v)| *v).unwrap_or(0.0)
    }

    /// Latest accounts payable.
    pub fn latest_ap(&self) -> f64 {
        self.accounts_payable.last().map(|(_, v)| *v).unwrap_or(0.0)
    }

    /// Latest net PP&E.
    pub fn latest_ppe_net(&self) -> f64 {
        self.ppe_net.last().map(|(_, v)| *v).unwrap_or(0.0)
    }

    /// Latest interest expense.
    pub fn interest_expense(&self) -> f64 {
        self.interest_expense.last().map(|(_, v)| *v).unwrap_or(0.0)
    }

    /// Latest dividends paid.
    pub fn latest_dividends(&self) -> f64 {
        self.dividends_paid.last().map(|(_, v)| *v).unwrap_or(0.0)
    }

    /// Dividend payout ratio from reported net income.
    pub fn dividend_payout_ratio(&self) -> f64 {
        let net_income = self
            .net_income
            .last()
            .map(|(_, value)| *value)
            .unwrap_or(0.0);
        if net_income > 0.0 {
            (self.latest_dividends() / net_income).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// ROE from reported net income and latest equity. DuPont reporting uses
    /// average balances separately; this accessor seeds forward projections.
    pub fn roe(&self) -> f64 {
        let equity = self.latest_equity();
        let net_income = self
            .net_income
            .last()
            .map(|(_, value)| *value)
            .unwrap_or(0.0);
        if equity > 0.0 {
            net_income / equity
        } else {
            0.0
        }
    }

    /// Days sales outstanding: AR / (revenue / 365).
    pub fn dso_days(&self) -> f64 {
        let rev = self.latest_revenue();
        let ar = self
            .accounts_receivable
            .last()
            .map(|(_, v)| *v)
            .unwrap_or(0.0);
        if rev > 0.0 {
            (ar / rev * 365.0).clamp(0.0, 365.0)
        } else {
            45.0
        }
    }

    /// Days inventory outstanding: inventory / (cogs / 365).
    pub fn dio_days(&self) -> f64 {
        let cogs = self.latest_cogs();
        let inv = self.inventory.last().map(|(_, v)| *v).unwrap_or(0.0);
        if cogs > 0.0 {
            (inv / cogs * 365.0).clamp(0.0, 365.0)
        } else {
            60.0
        }
    }

    /// Days payable outstanding: AP / (cogs / 365).
    pub fn dpo_days(&self) -> f64 {
        let cogs = self.latest_cogs();
        let ap = self.accounts_payable.last().map(|(_, v)| *v).unwrap_or(0.0);
        if cogs > 0.0 {
            (ap / cogs * 365.0).clamp(0.0, 365.0)
        } else {
            30.0
        }
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

/// Closed-form implied ROE from the justified price-to-book identity
/// P/B = (ROE − g) / (COE − g) — residual income on equity, the standard
/// reverse solve for financial-sector companies whose FCF is not
/// meaningful (banks' deposits make NWC and invested capital
/// incomputable). Solving for ROE: ROE = P/B × (COE − g) + g. Returns
/// `None` when price or book value is not positive, or when COE ≤ g (the
/// identity is not invertible in that regime — a price-to-book above the
/// perpetuity bound).
pub(crate) fn implied_roe_from_price_to_book(
    price: f64,
    book_value_per_share: f64,
    cost_of_equity: f64,
    growth: f64,
) -> Option<f64> {
    if price <= 0.0 || book_value_per_share <= 0.0 || cost_of_equity <= growth {
        return None;
    }
    let price_to_book = price / book_value_per_share;
    Some(price_to_book * (cost_of_equity - growth) + growth)
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

// ── Authoritative driver-based financial model
mod driver_model;
pub(crate) use driver_model::{
    IMPLIED_GROWTH_HI, IMPLIED_GROWTH_LO, ProjectedFinancialModel, ProjectionAssumptions,
    ProjectionError, implied_growth, implied_net_margin_at_growth, project_financial_model,
};
