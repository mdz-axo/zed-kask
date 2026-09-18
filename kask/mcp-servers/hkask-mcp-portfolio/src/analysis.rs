//! Investor-oriented portfolio reports derived from the append-only ledger.
//!
//! The portfolio server remains provider-agnostic: prices come from a
//! [`PriceResolver`], while company classifications and metrics are explicit
//! observations supplied by the caller. Report calculations never mutate the
//! source ledger.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    AssetType, LedgerFilter, PortfolioError, PortfolioStore, PriceResolver, Transaction, TxType,
    parse_ymd,
};

const POSITION_EPSILON: f64 = 0.0001;
const RECONCILIATION_EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContributionReport {
    pub portfolio: String,
    pub from: String,
    pub to: String,
    pub portfolio_return: f64,
    pub total_profit: f64,
    pub start_value: f64,
    pub end_value: f64,
    pub rows: Vec<ContributionRow>,
    pub reconciliation_residual: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContributionRow {
    pub symbol: String,
    pub start_value: f64,
    pub end_value: f64,
    pub net_internal_cash_flow: f64,
    pub profit: f64,
    pub contribution_bps: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SecurityObservation {
    pub symbol: String,
    #[serde(default)]
    pub sector: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub metrics: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CharacteristicsReport {
    pub portfolio: String,
    pub date: String,
    pub total_market_value: f64,
    pub cash_weight: f64,
    pub position_count: usize,
    pub top_five_weight: f64,
    pub concentration_hhi: f64,
    pub effective_holdings: f64,
    pub sectors: BTreeMap<String, f64>,
    pub countries: BTreeMap<String, f64>,
    pub metrics: BTreeMap<String, AggregatedMetric>,
    pub missing_prices: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AggregatedMetric {
    pub value: Option<f64>,
    pub method: &'static str,
    pub weight_coverage: f64,
    pub holding_coverage: usize,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ClassificationObservation {
    pub symbol: String,
    pub group: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AttributionReport {
    pub portfolio: String,
    pub benchmark: String,
    pub from: String,
    pub to: String,
    pub model: &'static str,
    pub portfolio_return: f64,
    pub benchmark_return: f64,
    pub active_return: f64,
    pub allocation_effect: f64,
    pub selection_effect: f64,
    pub interaction_effect: f64,
    pub rows: Vec<AttributionRow>,
    pub reconciliation_residual: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AttributionRow {
    pub group: String,
    pub portfolio_weight: f64,
    pub benchmark_weight: f64,
    pub portfolio_return: f64,
    pub benchmark_return: f64,
    pub allocation_effect: f64,
    pub selection_effect: f64,
    pub interaction_effect: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WhatIfReport {
    pub portfolio: String,
    pub mode: &'static str,
    pub from: String,
    pub to: String,
    pub actual_end_value: f64,
    pub hypothetical_end_value: f64,
    pub value_difference: f64,
    pub actual_return: f64,
    pub hypothetical_return: f64,
    pub return_difference: f64,
    pub hypothetical_transactions: Vec<String>,
    pub authoritative_state_changed: bool,
    pub interpretation: &'static str,
}

#[derive(Debug, Clone)]
struct ProjectedState {
    positions: HashMap<String, f64>,
    cash: f64,
}

#[derive(Debug, Clone)]
struct ValuedState {
    positions: HashMap<String, f64>,
    position_values: HashMap<String, f64>,
    cash: f64,
    market_value: f64,
    total_value: f64,
    missing_prices: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct GroupResult {
    start_value: f64,
    profit: f64,
}

pub fn contribution(
    store: &PortfolioStore,
    portfolio: &str,
    from: &str,
    to: &str,
    prices: &dyn PriceResolver,
) -> Result<ContributionReport, PortfolioError> {
    validate_period(from, to)?;
    let transactions = store.ledger(portfolio, LedgerFilter::all())?;
    contribution_from_transactions(portfolio, &transactions, from, to, prices)
}

pub fn characteristics(
    store: &PortfolioStore,
    portfolio: &str,
    date: &str,
    prices: &dyn PriceResolver,
    observations: &[SecurityObservation],
) -> Result<CharacteristicsReport, PortfolioError> {
    parse_ymd(date, "date")?;
    let transactions = store.ledger(portfolio, LedgerFilter::all())?;
    characteristics_from_transactions(portfolio, &transactions, date, prices, observations)
}

pub fn attribution(
    store: &PortfolioStore,
    portfolio: &str,
    benchmark: &str,
    from: &str,
    to: &str,
    prices: &dyn PriceResolver,
    classifications: &[ClassificationObservation],
) -> Result<AttributionReport, PortfolioError> {
    validate_period(from, to)?;
    let portfolio_transactions = store.ledger(portfolio, LedgerFilter::all())?;
    let benchmark_transactions = store.ledger(benchmark, LedgerFilter::all())?;
    let portfolio_report =
        contribution_from_transactions(portfolio, &portfolio_transactions, from, to, prices)?;
    let benchmark_report =
        contribution_from_transactions(benchmark, &benchmark_transactions, from, to, prices)?;

    let classification_by_symbol: HashMap<&str, &str> = classifications
        .iter()
        .map(|item| (item.symbol.as_str(), item.group.as_str()))
        .collect();
    let portfolio_groups = group_contribution(&portfolio_report, &classification_by_symbol);
    let benchmark_groups = group_contribution(&benchmark_report, &classification_by_symbol);
    let groups: BTreeSet<String> = portfolio_groups
        .keys()
        .chain(benchmark_groups.keys())
        .cloned()
        .collect();

    let mut rows = Vec::with_capacity(groups.len());
    let mut allocation_effect = 0.0;
    let mut selection_effect = 0.0;
    let mut interaction_effect = 0.0;
    for group in groups {
        let portfolio_group = portfolio_groups.get(&group).cloned().unwrap_or_default();
        let benchmark_group = benchmark_groups.get(&group).cloned().unwrap_or_default();
        let portfolio_weight =
            safe_ratio(portfolio_group.start_value, portfolio_report.start_value);
        let benchmark_weight =
            safe_ratio(benchmark_group.start_value, benchmark_report.start_value);
        let portfolio_group_return =
            safe_ratio(portfolio_group.profit, portfolio_group.start_value);
        let benchmark_group_return =
            safe_ratio(benchmark_group.profit, benchmark_group.start_value);
        let allocation = (portfolio_weight - benchmark_weight)
            * (benchmark_group_return - benchmark_report.portfolio_return);
        let selection = benchmark_weight * (portfolio_group_return - benchmark_group_return);
        let interaction = (portfolio_weight - benchmark_weight)
            * (portfolio_group_return - benchmark_group_return);
        allocation_effect += allocation;
        selection_effect += selection;
        interaction_effect += interaction;
        rows.push(AttributionRow {
            group,
            portfolio_weight,
            benchmark_weight,
            portfolio_return: portfolio_group_return,
            benchmark_return: benchmark_group_return,
            allocation_effect: allocation,
            selection_effect: selection,
            interaction_effect: interaction,
        });
    }

    let active_return = portfolio_report.portfolio_return - benchmark_report.portfolio_return;
    let explained = allocation_effect + selection_effect + interaction_effect;
    Ok(AttributionReport {
        portfolio: portfolio.to_string(),
        benchmark: benchmark.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        model: "Brinson-Fachler (interaction separate)",
        portfolio_return: portfolio_report.portfolio_return,
        benchmark_return: benchmark_report.portfolio_return,
        active_return,
        allocation_effect,
        selection_effect,
        interaction_effect,
        rows,
        reconciliation_residual: active_return - explained,
    })
}

pub fn historical_what_if(
    store: &PortfolioStore,
    portfolio: &str,
    from: &str,
    to: &str,
    hypothetical_transactions: &[Transaction],
    prices: &dyn PriceResolver,
) -> Result<WhatIfReport, PortfolioError> {
    validate_period(from, to)?;
    validate_hypothetical_transactions(hypothetical_transactions, from, to)?;
    let actual_transactions = store.ledger(portfolio, LedgerFilter::all())?;
    let mut counterfactual_transactions = actual_transactions.clone();
    counterfactual_transactions.extend_from_slice(hypothetical_transactions);
    counterfactual_transactions.sort_by(|left, right| {
        left.date
            .cmp(&right.date)
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });

    let actual = contribution_from_transactions(portfolio, &actual_transactions, from, to, prices)?;
    let hypothetical =
        contribution_from_transactions(portfolio, &counterfactual_transactions, from, to, prices)?;
    validate_non_negative_state(&counterfactual_transactions, to)?;

    Ok(WhatIfReport {
        portfolio: portfolio.to_string(),
        mode: "retrospective_counterfactual",
        from: from.to_string(),
        to: to.to_string(),
        actual_end_value: actual.end_value,
        hypothetical_end_value: hypothetical.end_value,
        value_difference: hypothetical.end_value - actual.end_value,
        actual_return: actual.portfolio_return,
        hypothetical_return: hypothetical.portfolio_return,
        return_difference: hypothetical.portfolio_return - actual.portfolio_return,
        hypothetical_transactions: hypothetical_transactions
            .iter()
            .map(|transaction| transaction.id.clone())
            .collect(),
        authoritative_state_changed: false,
        interpretation: "Hindsight comparison using realized subsequent prices; not an ex-ante forecast.",
    })
}

pub fn prospective_what_if(
    store: &PortfolioStore,
    portfolio: &str,
    date: &str,
    hypothetical_transactions: &[Transaction],
    prices: &dyn PriceResolver,
    observations: &[SecurityObservation],
) -> Result<(CharacteristicsReport, CharacteristicsReport), PortfolioError> {
    parse_ymd(date, "date")?;
    validate_hypothetical_transactions(hypothetical_transactions, date, date)?;
    let actual_transactions = store.ledger(portfolio, LedgerFilter::all())?;
    let mut hypothetical = actual_transactions.clone();
    hypothetical.extend_from_slice(hypothetical_transactions);
    validate_non_negative_state(&hypothetical, date)?;
    let actual_report = characteristics_from_transactions(
        portfolio,
        &actual_transactions,
        date,
        prices,
        observations,
    )?;
    let hypothetical_report =
        characteristics_from_transactions(portfolio, &hypothetical, date, prices, observations)?;
    Ok((actual_report, hypothetical_report))
}

fn contribution_from_transactions(
    portfolio: &str,
    transactions: &[Transaction],
    from: &str,
    to: &str,
    prices: &dyn PriceResolver,
) -> Result<ContributionReport, PortfolioError> {
    let start = value_state(transactions, from, prices)?;
    let end = value_state(transactions, to, prices)?;
    reject_missing_prices(&start, from)?;
    reject_missing_prices(&end, to)?;
    if start.total_value <= 0.0 {
        return Err(
            format!("portfolio has zero or negative starting value for {from}..={to}").into(),
        );
    }

    let mut symbols: BTreeSet<String> = start
        .positions
        .keys()
        .chain(end.positions.keys())
        .cloned()
        .collect();
    for transaction in transactions {
        if transaction.date.as_str() > from
            && transaction.date.as_str() <= to
            && let Some(symbol) = &transaction.symbol
        {
            symbols.insert(symbol.clone());
        }
    }

    let mut rows = Vec::with_capacity(symbols.len() + 1);
    for symbol in symbols {
        let start_value = start.position_values.get(&symbol).copied().unwrap_or(0.0);
        let end_value = end.position_values.get(&symbol).copied().unwrap_or(0.0);
        let net_internal_cash_flow: f64 = transactions
            .iter()
            .filter(|transaction| {
                transaction.date.as_str() > from
                    && transaction.date.as_str() <= to
                    && transaction.symbol.as_deref() == Some(symbol.as_str())
                    && matches!(
                        transaction.tx_type,
                        TxType::Buy | TxType::Sell | TxType::Dividend
                    )
            })
            .map(Transaction::cash_flow)
            .sum();
        let profit = end_value - start_value + net_internal_cash_flow;
        rows.push(ContributionRow {
            symbol,
            start_value,
            end_value,
            net_internal_cash_flow,
            profit,
            contribution_bps: profit / start.total_value * 10_000.0,
        });
    }

    let external_flows: f64 = transactions
        .iter()
        .filter(|transaction| {
            transaction.date.as_str() > from
                && transaction.date.as_str() <= to
                && matches!(transaction.tx_type, TxType::Deposit | TxType::Withdrawal)
        })
        .map(Transaction::cash_flow)
        .sum();
    let total_profit = end.total_value - start.total_value - external_flows;
    let explained_profit: f64 = rows.iter().map(|row| row.profit).sum();
    let residual = total_profit - explained_profit;
    if residual.abs() > RECONCILIATION_EPSILON {
        rows.push(ContributionRow {
            symbol: "Cash / unassigned".to_string(),
            start_value: start.cash,
            end_value: end.cash,
            net_internal_cash_flow: 0.0,
            profit: residual,
            contribution_bps: residual / start.total_value * 10_000.0,
        });
    }
    rows.sort_by(|left, right| {
        right
            .profit
            .abs()
            .total_cmp(&left.profit.abs())
            .then_with(|| left.symbol.cmp(&right.symbol))
    });
    let explained_profit: f64 = rows.iter().map(|row| row.profit).sum();
    Ok(ContributionReport {
        portfolio: portfolio.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        portfolio_return: total_profit / start.total_value,
        total_profit,
        start_value: start.total_value,
        end_value: end.total_value,
        rows,
        reconciliation_residual: total_profit - explained_profit,
    })
}

fn characteristics_from_transactions(
    portfolio: &str,
    transactions: &[Transaction],
    date: &str,
    prices: &dyn PriceResolver,
    observations: &[SecurityObservation],
) -> Result<CharacteristicsReport, PortfolioError> {
    let state = value_state(transactions, date, prices)?;
    let total_market_value = state.market_value;
    let invested_total = state.total_value;
    let mut weights: Vec<(String, f64)> = state
        .position_values
        .iter()
        .filter(|(_, value)| **value > 0.0)
        .map(|(symbol, value)| {
            (
                symbol.clone(),
                if invested_total > 0.0 {
                    *value / invested_total
                } else {
                    0.0
                },
            )
        })
        .collect();
    weights.sort_by(|left, right| right.1.total_cmp(&left.1));
    let top_five_weight = weights.iter().take(5).map(|(_, weight)| weight).sum();
    let concentration_hhi: f64 = weights.iter().map(|(_, weight)| weight * weight).sum();
    let effective_holdings = if concentration_hhi > 0.0 {
        1.0 / concentration_hhi
    } else {
        0.0
    };

    let observations_by_symbol: HashMap<&str, &SecurityObservation> = observations
        .iter()
        .map(|observation| (observation.symbol.as_str(), observation))
        .collect();
    let mut sectors = BTreeMap::new();
    let mut countries = BTreeMap::new();
    let metric_names: BTreeSet<&str> = observations
        .iter()
        .flat_map(|observation| observation.metrics.keys().map(String::as_str))
        .collect();
    for (symbol, weight) in &weights {
        let Some(observation) = observations_by_symbol.get(symbol.as_str()) else {
            continue;
        };
        if let Some(sector) = &observation.sector {
            *sectors.entry(sector.clone()).or_insert(0.0) += weight;
        }
        if let Some(country) = &observation.country {
            *countries.entry(country.clone()).or_insert(0.0) += weight;
        }
    }

    let mut metrics = BTreeMap::new();
    for metric in metric_names {
        let mut covered_weight = 0.0;
        let mut holding_coverage = 0;
        let mut weighted_sum = 0.0;
        let mut harmonic_denominator = 0.0;
        for (symbol, weight) in &weights {
            let Some(value) = observations_by_symbol
                .get(symbol.as_str())
                .and_then(|observation| observation.metrics.get(metric))
                .copied()
            else {
                continue;
            };
            covered_weight += weight;
            holding_coverage += 1;
            weighted_sum += weight * value;
            if value > 0.0 {
                harmonic_denominator += weight / value;
            }
        }
        let harmonic = uses_harmonic_aggregation(metric);
        let value = if covered_weight <= 0.0 {
            None
        } else if harmonic {
            (harmonic_denominator > 0.0).then_some(covered_weight / harmonic_denominator)
        } else {
            Some(weighted_sum / covered_weight)
        };
        metrics.insert(
            metric.to_string(),
            AggregatedMetric {
                value,
                method: if harmonic {
                    "market-value-weighted harmonic mean"
                } else {
                    "market-value-weighted arithmetic mean"
                },
                weight_coverage: covered_weight,
                holding_coverage,
            },
        );
    }

    Ok(CharacteristicsReport {
        portfolio: portfolio.to_string(),
        date: date.to_string(),
        total_market_value,
        cash_weight: if invested_total > 0.0 {
            state.cash / invested_total
        } else {
            0.0
        },
        position_count: weights.len(),
        top_five_weight,
        concentration_hhi,
        effective_holdings,
        sectors,
        countries,
        metrics,
        missing_prices: state.missing_prices,
    })
}

fn group_contribution(
    report: &ContributionReport,
    classifications: &HashMap<&str, &str>,
) -> BTreeMap<String, GroupResult> {
    let mut groups: BTreeMap<String, GroupResult> = BTreeMap::new();
    for row in &report.rows {
        let group = if row.symbol == "Cash / unassigned" {
            "Cash / unassigned"
        } else {
            classifications
                .get(row.symbol.as_str())
                .copied()
                .unwrap_or("Unclassified")
        };
        let result = groups.entry(group.to_string()).or_default();
        result.start_value += row.start_value;
        result.profit += row.profit;
    }
    groups
}

fn value_state(
    transactions: &[Transaction],
    date: &str,
    prices: &dyn PriceResolver,
) -> Result<ValuedState, PortfolioError> {
    let projected = project_state(transactions, date)?;
    let mut position_values = HashMap::new();
    let mut missing_prices = Vec::new();
    for (symbol, quantity) in &projected.positions {
        if quantity.abs() <= POSITION_EPSILON {
            continue;
        }
        match prices.resolve(symbol, date) {
            Some(price) if price > 0.0 && price.is_finite() => {
                position_values.insert(symbol.clone(), quantity * price);
            }
            _ if prices.expects_prices() => missing_prices.push(symbol.clone()),
            _ => {
                position_values.insert(symbol.clone(), 0.0);
            }
        }
    }
    missing_prices.sort();
    let market_value = position_values.values().sum();
    Ok(ValuedState {
        positions: projected.positions,
        position_values,
        cash: projected.cash,
        market_value,
        total_value: market_value + projected.cash,
        missing_prices,
    })
}

fn project_state(
    transactions: &[Transaction],
    date: &str,
) -> Result<ProjectedState, PortfolioError> {
    parse_ymd(date, "date")?;
    let mut positions = HashMap::new();
    let mut cash = 0.0;
    for transaction in transactions
        .iter()
        .filter(|transaction| transaction.date.as_str() <= date)
    {
        cash += transaction.cash_flow();
        if let Some(symbol) = &transaction.symbol
            && matches!(
                transaction.tx_type,
                TxType::Buy | TxType::Sell | TxType::Roll
            )
        {
            *positions.entry(symbol.clone()).or_insert(0.0) += transaction.position_delta();
        }
    }
    positions.retain(|_, quantity| quantity.abs() > POSITION_EPSILON);
    Ok(ProjectedState { positions, cash })
}

fn validate_non_negative_state(
    transactions: &[Transaction],
    date: &str,
) -> Result<(), PortfolioError> {
    let state = project_state(transactions, date)?;
    if state.cash < -POSITION_EPSILON {
        return Err("hypothetical transactions require more cash than the portfolio holds".into());
    }
    let negative_positions: Vec<String> = state
        .positions
        .iter()
        .filter(|(_, quantity)| **quantity < -POSITION_EPSILON)
        .map(|(symbol, _)| symbol.clone())
        .collect();
    if !negative_positions.is_empty() {
        return Err(format!(
            "hypothetical transactions create unsupported short positions: {}",
            negative_positions.join(", ")
        )
        .into());
    }
    Ok(())
}

fn validate_hypothetical_transactions(
    transactions: &[Transaction],
    from: &str,
    to: &str,
) -> Result<(), PortfolioError> {
    for transaction in transactions {
        parse_ymd(&transaction.date, "hypothetical transaction date")?;
        if transaction.date.as_str() < from || transaction.date.as_str() > to {
            return Err(format!(
                "hypothetical transaction {} date {} is outside {from}..={to}",
                transaction.id, transaction.date
            )
            .into());
        }
        if transaction.asset_type != AssetType::Stock {
            return Err(format!(
                "hypothetical transaction {} is not a stock transaction",
                transaction.id
            )
            .into());
        }
        if !matches!(transaction.tx_type, TxType::Buy | TxType::Sell) {
            return Err(format!(
                "hypothetical transaction {} must be buy or sell",
                transaction.id
            )
            .into());
        }
    }
    Ok(())
}

fn reject_missing_prices(state: &ValuedState, date: &str) -> Result<(), PortfolioError> {
    if state.missing_prices.is_empty() {
        return Ok(());
    }
    Err(format!(
        "missing cached prices at {date} for: {}; seed prices before running the report",
        state.missing_prices.join(", ")
    )
    .into())
}

fn validate_period(from: &str, to: &str) -> Result<(), PortfolioError> {
    let from_date = parse_ymd(from, "from")?;
    let to_date = parse_ymd(to, "to")?;
    if from_date >= to_date {
        return Err("from must be earlier than to".into());
    }
    Ok(())
}

fn safe_ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator.abs() <= RECONCILIATION_EPSILON {
        0.0
    } else {
        numerator / denominator
    }
}

fn uses_harmonic_aggregation(metric: &str) -> bool {
    matches!(
        metric,
        "pe_ratio" | "price_to_book" | "price_to_sales" | "ev_to_ebitda"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoPrices;

    fn transaction(
        id: &str,
        date: &str,
        tx_type: TxType,
        symbol: Option<&str>,
        quantity: Option<f64>,
        price: Option<f64>,
        amount: Option<f64>,
    ) -> Transaction {
        Transaction {
            id: id.to_string(),
            date: date.to_string(),
            tx_type,
            asset_type: AssetType::Stock,
            symbol: symbol.map(str::to_string),
            quantity,
            price,
            commission: Some(0.0),
            amount,
            weight: None,
            currency: "USD".to_string(),
            notes: String::new(),
            created_at: format!("{date}T00:00:00Z"),
        }
    }

    struct FixturePrices(HashMap<(String, String), f64>);

    impl PriceResolver for FixturePrices {
        fn resolve(&self, symbol: &str, date: &str) -> Option<f64> {
            self.0.get(&(symbol.to_string(), date.to_string())).copied()
        }
    }

    fn prices(entries: &[(&str, &str, f64)]) -> FixturePrices {
        FixturePrices(
            entries
                .iter()
                .map(|(symbol, date, value)| ((symbol.to_string(), date.to_string()), *value))
                .collect(),
        )
    }

    #[test]
    fn contribution_reconciles_trades_and_dividends_to_total_profit() {
        let transactions = vec![
            transaction(
                "deposit",
                "2024-01-01",
                TxType::Deposit,
                None,
                None,
                None,
                Some(100.0),
            ),
            transaction(
                "buy-a",
                "2024-01-01",
                TxType::Buy,
                Some("A"),
                Some(5.0),
                Some(10.0),
                None,
            ),
            transaction(
                "buy-b",
                "2024-01-01",
                TxType::Buy,
                Some("B"),
                Some(5.0),
                Some(10.0),
                None,
            ),
            transaction(
                "sell-a",
                "2024-12-31",
                TxType::Sell,
                Some("A"),
                Some(1.0),
                Some(12.0),
                None,
            ),
            transaction(
                "div-b",
                "2024-12-31",
                TxType::Dividend,
                Some("B"),
                None,
                None,
                Some(5.0),
            ),
        ];
        let report = contribution_from_transactions(
            "main",
            &transactions,
            "2024-01-01",
            "2024-12-31",
            &prices(&[
                ("A", "2024-01-01", 10.0),
                ("B", "2024-01-01", 10.0),
                ("A", "2024-12-31", 12.0),
                ("B", "2024-12-31", 8.0),
            ]),
        )
        .expect("contribution should calculate");
        assert!((report.total_profit - 5.0).abs() < 1e-9);
        assert!(report.reconciliation_residual.abs() < 1e-9);
        assert!((report.rows.iter().map(|row| row.profit).sum::<f64>() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn brinson_fachler_effects_reconcile_to_active_return() {
        let portfolio = ContributionReport {
            portfolio: "p".to_string(),
            from: "2024-01-01".to_string(),
            to: "2024-12-31".to_string(),
            portfolio_return: 0.11,
            total_profit: 11.0,
            start_value: 100.0,
            end_value: 111.0,
            rows: vec![
                ContributionRow {
                    symbol: "A".to_string(),
                    start_value: 60.0,
                    end_value: 69.0,
                    net_internal_cash_flow: 0.0,
                    profit: 9.0,
                    contribution_bps: 900.0,
                },
                ContributionRow {
                    symbol: "B".to_string(),
                    start_value: 40.0,
                    end_value: 42.0,
                    net_internal_cash_flow: 0.0,
                    profit: 2.0,
                    contribution_bps: 200.0,
                },
            ],
            reconciliation_residual: 0.0,
        };
        let benchmark = ContributionReport {
            portfolio: "b".to_string(),
            from: portfolio.from.clone(),
            to: portfolio.to.clone(),
            portfolio_return: 0.09,
            total_profit: 9.0,
            start_value: 100.0,
            end_value: 109.0,
            rows: vec![
                ContributionRow {
                    symbol: "A".to_string(),
                    start_value: 50.0,
                    end_value: 56.0,
                    net_internal_cash_flow: 0.0,
                    profit: 6.0,
                    contribution_bps: 600.0,
                },
                ContributionRow {
                    symbol: "B".to_string(),
                    start_value: 50.0,
                    end_value: 53.0,
                    net_internal_cash_flow: 0.0,
                    profit: 3.0,
                    contribution_bps: 300.0,
                },
            ],
            reconciliation_residual: 0.0,
        };
        let classifications = HashMap::from([("A", "Growth"), ("B", "Value")]);
        let pg = group_contribution(&portfolio, &classifications);
        let bg = group_contribution(&benchmark, &classifications);
        let mut explained = 0.0;
        for group in ["Growth", "Value"] {
            let p = pg.get(group).expect("portfolio group");
            let b = bg.get(group).expect("benchmark group");
            let wp = p.start_value / portfolio.start_value;
            let wb = b.start_value / benchmark.start_value;
            let rp = p.profit / p.start_value;
            let rb = b.profit / b.start_value;
            explained += (wp - wb) * (rb - benchmark.portfolio_return)
                + wb * (rp - rb)
                + (wp - wb) * (rp - rb);
        }
        assert!(
            (explained - (portfolio.portfolio_return - benchmark.portfolio_return)).abs() < 1e-9
        );
    }

    #[test]
    fn characteristics_use_harmonic_multiples_and_report_coverage() {
        let transactions = vec![
            transaction(
                "deposit",
                "2024-01-01",
                TxType::Deposit,
                None,
                None,
                None,
                Some(200.0),
            ),
            transaction(
                "buy-a",
                "2024-01-01",
                TxType::Buy,
                Some("A"),
                Some(10.0),
                Some(10.0),
                None,
            ),
            transaction(
                "buy-b",
                "2024-01-01",
                TxType::Buy,
                Some("B"),
                Some(10.0),
                Some(10.0),
                None,
            ),
        ];
        let observations = vec![
            SecurityObservation {
                symbol: "A".to_string(),
                sector: Some("Tech".to_string()),
                country: Some("US".to_string()),
                metrics: BTreeMap::from([
                    ("pe_ratio".to_string(), 10.0),
                    ("roic".to_string(), 0.20),
                ]),
            },
            SecurityObservation {
                symbol: "B".to_string(),
                sector: Some("Health".to_string()),
                country: Some("US".to_string()),
                metrics: BTreeMap::from([("pe_ratio".to_string(), 20.0)]),
            },
        ];
        let report = characteristics_from_transactions(
            "main",
            &transactions,
            "2024-01-01",
            &prices(&[("A", "2024-01-01", 10.0), ("B", "2024-01-01", 10.0)]),
            &observations,
        )
        .expect("characteristics should calculate");
        let pe = report.metrics.get("pe_ratio").expect("pe metric");
        assert!((pe.value.expect("pe value") - (2.0 / 0.15)).abs() < 1e-9);
        let roic = report.metrics.get("roic").expect("roic metric");
        assert!((roic.weight_coverage - 0.5).abs() < 1e-9);
        assert_eq!(roic.holding_coverage, 1);
    }

    #[test]
    fn hypothetical_projection_rejects_short_positions() {
        let transactions = vec![transaction(
            "sell",
            "2024-01-01",
            TxType::Sell,
            Some("A"),
            Some(1.0),
            Some(10.0),
            None,
        )];
        let error = validate_non_negative_state(&transactions, "2024-01-01")
            .expect_err("short position must be rejected");
        assert!(error.to_string().contains("unsupported short positions"));
    }

    #[test]
    fn no_price_resolver_is_explicitly_supported_for_non_market_portfolios() {
        let transactions = vec![transaction(
            "deposit",
            "2024-01-01",
            TxType::Deposit,
            None,
            None,
            None,
            Some(100.0),
        )];
        let report = contribution_from_transactions(
            "cash",
            &transactions,
            "2024-01-01",
            "2024-12-31",
            &NoPrices,
        )
        .expect("cash-only report should calculate");
        assert_eq!(report.portfolio_return, 0.0);
    }
}
