//! The annotated market-record contract (integration report §4).
//!
//! Load-bearing design rule: this struct never carries a bare probability.
//! Every `probability` is paired with reliability covariates, calibration
//! metadata, volatility annotation, and the dual-axis ontology mapping so a
//! consumer cannot be naive by default.

use serde::Serialize;

use crate::ontology;
use crate::provider_kalshi::{KalshiEvent, KalshiMarket, parse_fp};
use crate::provider_polymarket::GammaMarket;

/// Source platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Polymarket,
    Kalshi,
}

/// Provenance of the probability value (T0: Kalshi percentile-history was
/// dropped after the endpoint 404'd live; candlesticks are the Kalshi
/// history source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbabilityMethod {
    /// Last trade price (Polymarket outcomePrices).
    LastTrade,
    /// Two-sided quote midpoint (Kalshi yes bid/ask).
    Midpoint,
}

/// Lifecycle status of the market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketStatus {
    Open,
    Closed,
    Resolved,
}

/// Derived reliability gate (2607.08199: wide spread / thin volume ⇒ noise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReliabilityTier {
    High,
    Medium,
    Low,
}

/// Calibration metadata for the market's domain/series. Computed from data
/// by T5; until then `domain_bias` is seeded from arXiv:2602.19520 and
/// `stale` is true (no measured Brier yet — never a synthetic 0).
#[derive(Debug, Clone, Serialize)]
pub struct Calibration {
    pub brier: Option<f64>,
    pub domain_bias: Option<&'static str>,
    pub sample_size: u64,
    /// True when the calibration signal is unavailable (not measured, read
    /// failure, thin sample). Distinct from brier: 0 — a 0 would read as
    /// "perfectly calibrated" and create a reinforcing loop.
    pub stale: bool,
}

/// Volatility annotation (2607.08199). `realized_variance` is computed from
/// price history by T4's follow-up; until history is wired it is None and
/// only structural flags are set.
#[derive(Debug, Clone, Serialize)]
pub struct Volatility {
    pub realized_variance: Option<f64>,
    pub structural_flag: StructuralFlag,
    pub interpretation: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuralFlag {
    None,
    NearDeadline,
    NearCoinflip,
    NearDeadlineAndCoinflip,
}

/// PKO process-axis mapping (generated from `crate::ontology` constants).
#[derive(Debug, Clone, Serialize)]
pub struct ProcessAxis {
    pub r#type: &'static str,
    pub stage: &'static str,
    pub probability_role: &'static str,
}

/// Dublin Core state-axis mapping (vocabulary from hkask-bridge-dublincore;
/// Q-O1 resolved 2026-08-05).
#[derive(Debug, Clone, Serialize)]
pub struct StateAxis {
    pub identifier: String,
    pub title: String,
    pub description: String,
    pub temporal: String,
    pub provenance: &'static str,
}

/// The dual-axis mapping every record carries to its consumers.
#[derive(Debug, Clone, Serialize)]
pub struct OntologyBlock {
    pub process: ProcessAxis,
    pub state: StateAxis,
    pub mapping_version: u32,
}

/// The full annotated market record.
#[derive(Debug, Clone, Serialize)]
pub struct MarketRecord {
    pub source: Source,
    pub event_id: String,
    pub market_id: String,
    pub question: String,
    pub description: String,
    pub category: String,
    pub series: String,
    pub deadline: String,
    pub probability: f64,
    pub probability_method: ProbabilityMethod,
    pub spread: Option<f64>,
    pub volume: f64,
    pub liquidity: Option<f64>,
    pub open_interest: Option<f64>,
    pub last_update: String,
    pub volatility: Volatility,
    pub status: MarketStatus,
    pub resolved_outcome: Option<bool>,
    pub resolution_source: &'static str,
    pub calibration: Calibration,
    pub reliability_tier: ReliabilityTier,
    pub ontology: OntologyBlock,
}

/// Static per-domain bias table seeded from arXiv:2602.19520 (politics
/// chronically underconfident on both exchanges). T5/T10 replace this with
/// data-derived estimates.
pub fn domain_bias_for(category: &str) -> Option<&'static str> {
    let normalized = category.to_ascii_lowercase();
    if normalized.contains("politic") || normalized.contains("election") {
        Some("underconfident")
    } else {
        None
    }
}

/// Reliability gate from observable covariates. Thresholds are initial
/// placeholders (Q3 in the plan — recalibrate per-domain after data accrues).
pub fn reliability_tier(volume: f64, spread: Option<f64>) -> ReliabilityTier {
    let wide_spread = spread.is_some_and(|s| s > 0.10);
    let thin_volume = volume < 1_000.0;
    if thin_volume || wide_spread {
        ReliabilityTier::Low
    } else if volume < 50_000.0 || spread.is_none_or(|s| s > 0.04) {
        ReliabilityTier::Medium
    } else {
        ReliabilityTier::High
    }
}

/// Structural volatility flags per 2607.08199: vol rises near the deadline
/// and near coin-flip prices. Days-to-deadline is computed by the caller
/// (None when the deadline is unparseable → only the coin-flip check runs).
pub fn structural_flag(probability: f64, days_to_deadline: Option<f64>) -> StructuralFlag {
    let near_coinflip = (probability - 0.5).abs() < 0.10;
    let near_deadline = days_to_deadline.is_some_and(|d| d < 7.0);
    match (near_deadline, near_coinflip) {
        (true, true) => StructuralFlag::NearDeadlineAndCoinflip,
        (true, false) => StructuralFlag::NearDeadline,
        (false, true) => StructuralFlag::NearCoinflip,
        (false, false) => StructuralFlag::None,
    }
}

fn lifecycle_stage(status: MarketStatus, uma_status: Option<&str>) -> &'static str {
    if matches!(status, MarketStatus::Resolved) {
        "settlement"
    } else if uma_status == Some("disputed") {
        "dispute"
    } else if matches!(status, MarketStatus::Closed) {
        "proposal"
    } else {
        "trading"
    }
}

fn make_ontology(
    source: Source,
    market_id: &str,
    question: &str,
    description: &str,
    deadline: &str,
    stage: &'static str,
    provenance: &'static str,
) -> OntologyBlock {
    let prefix = match source {
        Source::Polymarket => "polymarket",
        Source::Kalshi => "kalshi",
    };
    OntologyBlock {
        process: ProcessAxis {
            r#type: "pko:ProcedureExecution",
            stage,
            probability_role: "pko:StepExecution.output",
        },
        state: StateAxis {
            identifier: format!("{prefix}:{market_id}"),
            title: question.to_string(),
            description: description.chars().take(500).collect(),
            temporal: deadline.to_string(),
            provenance,
        },
        mapping_version: ontology::MAPPING_VERSION,
    }
}

fn days_between(later: &str, earlier: &chrono::DateTime<chrono::Utc>) -> Option<f64> {
    let deadline = chrono::DateTime::parse_from_rfc3339(later).ok()?;
    Some((deadline.with_timezone(&chrono::Utc) - *earlier).num_seconds() as f64 / 86_400.0)
}

impl MarketRecord {
    /// Build a record from a Kalshi market + its parent event.
    pub fn from_kalshi(
        market: &KalshiMarket,
        event: Option<&KalshiEvent>,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Option<Self> {
        let probability = market.yes_midpoint()?;
        let spread = market.spread();
        let volume = parse_fp(&market.volume_fp).unwrap_or(0.0);
        let category = event.map(|e| e.category.clone()).unwrap_or_default();
        let status = if !market.result.is_empty() {
            MarketStatus::Resolved
        } else if market.status == "active" {
            MarketStatus::Open
        } else {
            MarketStatus::Closed
        };
        let resolved_outcome = match market.result.as_str() {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        };
        let flag = structural_flag(probability, days_between(&market.close_time, now));
        Some(Self {
            source: Source::Kalshi,
            event_id: market.event_ticker.clone(),
            market_id: market.ticker.clone(),
            question: market.title.clone(),
            description: market.rules_primary.clone(),
            category: category.clone(),
            series: event.map(|e| e.series_ticker.clone()).unwrap_or_default(),
            deadline: market.close_time.clone(),
            probability,
            probability_method: ProbabilityMethod::Midpoint,
            spread,
            volume,
            liquidity: parse_fp(&market.liquidity_dollars),
            open_interest: parse_fp(&market.open_interest_fp),
            last_update: market.updated_time.clone(),
            volatility: Volatility {
                realized_variance: None,
                structural_flag: flag,
                interpretation: "low",
            },
            status,
            resolved_outcome,
            resolution_source: "kalshi_exchange",
            calibration: Calibration {
                brier: None,
                domain_bias: domain_bias_for(&category),
                sample_size: 0,
                stale: true,
            },
            reliability_tier: reliability_tier(volume, spread),
            ontology: make_ontology(
                Source::Kalshi,
                &market.ticker,
                &market.title,
                &market.rules_primary,
                &market.close_time,
                lifecycle_stage(status, None),
                "kalshi_exchange",
            ),
        })
    }

    /// Build a record from a Polymarket Gamma market + parent event context.
    pub fn from_polymarket(
        market: &GammaMarket,
        event_id: &str,
        event_slug: &str,
        event_volume: f64,
        event_liquidity: f64,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Option<Self> {
        let probability = market.yes_probability()?;
        let spread = market.spread;
        let status = if market.uma_resolution_status == "resolved" {
            MarketStatus::Resolved
        } else if market.closed {
            MarketStatus::Closed
        } else {
            MarketStatus::Open
        };
        let resolved_outcome = if matches!(status, MarketStatus::Resolved) {
            // For a resolved binary market the winning price is 1.
            market.prices().first().map(|p| *p >= 0.5)
        } else {
            None
        };
        let category = String::new(); // Gamma events carry tags, not a category field
        let flag = structural_flag(probability, days_between(&market.end_date, now));
        Some(Self {
            source: Source::Polymarket,
            event_id: event_id.to_string(),
            market_id: market.id.clone(),
            question: market.question.clone(),
            description: market.description.clone(),
            category: category.clone(),
            series: event_slug.to_string(),
            deadline: market.end_date.clone(),
            probability,
            probability_method: ProbabilityMethod::LastTrade,
            spread,
            volume: event_volume,
            liquidity: Some(event_liquidity),
            open_interest: None,
            last_update: market.updated_at.clone(),
            volatility: Volatility {
                realized_variance: None,
                structural_flag: flag,
                interpretation: "low",
            },
            status,
            resolved_outcome,
            resolution_source: "uma_oracle",
            calibration: Calibration {
                brier: None,
                domain_bias: domain_bias_for(&category),
                sample_size: 0,
                stale: true,
            },
            reliability_tier: reliability_tier(event_volume, spread),
            ontology: make_ontology(
                Source::Polymarket,
                &market.condition_id,
                &market.question,
                &market.description,
                &market.end_date,
                lifecycle_stage(status, Some(&market.uma_resolution_status)),
                "uma_oracle",
            ),
        })
    }
}
