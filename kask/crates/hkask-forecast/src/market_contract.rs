//! Annotated prediction-market contract types, covariate classifiers, and
//! deterministic question token-overlap (`hkask-forecast` layer of the
//! Three-Layer Forecasting Stack).
//!
//! Every probability in [`MarketRecord`] travels with its spread/volume
//! reliability tier, descriptive calibration metadata, volatility covariates,
//! and stamped ontology metadata. Zero ontology URI constants live here (P5.4 /
//! P8.1): producer servers stamp [`OntologyBlock`] from `hkask-bridge-ontology`.

use std::borrow::Cow;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::log_odds;
use crate::volatility::VolatilityForecast;

/// Source exchange platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Polymarket,
    Kalshi,
}

/// Provenance of the observed contract probability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbabilityMethod {
    /// Last trade price (Polymarket `outcomePrices`).
    LastTrade,
    /// Two-sided quote midpoint (Kalshi yes bid/ask).
    Midpoint,
}

/// Lifecycle status of the market contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketStatus {
    Open,
    Closed,
    Resolved,
}

/// Grain at which traded `volume` is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VolumeGrain {
    Market,
    Event,
}

/// Liquidity/spread reliability tier (`arXiv:2607.08199`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReliabilityTier {
    High,
    Medium,
    Low,
}

/// Descriptive historical raw Brier loss and sample size for a domain/series bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calibration {
    pub brier: Option<f64>,
    pub sample_size: u64,
    /// True when calibration history is unavailable or empty (never fabricated as `0.0`).
    pub stale: bool,
}

/// Volatility covariates (`arXiv:2607.08199`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volatility {
    pub realized_variance: Option<f64>,
    pub structural_flag: StructuralFlag,
    pub interpretation: Cow<'static, str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dras_forecast: Option<VolatilityForecast>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuralFlag {
    None,
    NearDeadline,
    NearCoinflip,
    NearDeadlineAndCoinflip,
}

/// Stamped PKO process-axis metadata (values supplied by producer server's `ontology` module).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessAxis {
    pub r#type: Cow<'static, str>,
    pub stage: Cow<'static, str>,
    pub probability_role: Cow<'static, str>,
}

/// Stamped Dublin Core state-axis metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateAxis {
    pub identifier: String,
    pub title: String,
    pub description: String,
    pub temporal: String,
    pub provenance: Cow<'static, str>,
}

/// Dual-axis ontology metadata carried by every [`MarketRecord`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyBlock {
    pub process: ProcessAxis,
    pub state: StateAxis,
    pub mapping_version: u32,
}

/// Annotated prediction-market contract shared across forecasting consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketRecord {
    pub source: Source,
    pub event_id: String,
    pub market_id: String,
    pub question: String,
    pub description: String,
    pub category: String,
    pub series: String,
    pub deadline: String,
    pub time_to_maturity: Option<f64>,
    pub probability: f64,
    pub probability_method: ProbabilityMethod,
    pub spread: Option<f64>,
    pub volume: f64,
    pub volume_grain: VolumeGrain,
    pub liquidity: Option<f64>,
    pub open_interest: Option<f64>,
    pub last_update: String,
    #[serde(default)]
    pub clob_asset_ids: Option<Vec<String>>,
    pub volatility: Volatility,
    pub status: MarketStatus,
    pub resolved_outcome: Option<bool>,
    pub resolution_source: Cow<'static, str>,
    pub calibration: Calibration,
    pub reliability_tier: ReliabilityTier,
    pub ontology: OntologyBlock,
}

/// Normalize category/tag labels onto canonical calibration buckets.
#[must_use]
pub fn canonical_bucket(category: &str) -> String {
    let normalized = category.trim().to_lowercase();
    match normalized.as_str() {
        "politics" | "elections" | "election" => "politics".to_string(),
        "economics" | "economy" | "macro" | "finance" | "financials" => "economics".to_string(),
        "sports" | "sport" => "sports".to_string(),
        "crypto" | "cryptocurrency" => "crypto".to_string(),
        "climate" | "weather" => "climate".to_string(),
        "tech" | "technology" => "technology".to_string(),
        "entertainment" | "culture" => "culture".to_string(),
        "world" | "geopolitics" => "world".to_string(),
        other => other.to_string(),
    }
}

/// Liquidity/spread reliability tier (`arXiv:2607.08199`).
#[must_use]
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

/// Structural volatility flag near deadline (`< 7` days) and/or coin-flip (`|p - 0.5| < 0.10`).
#[must_use]
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

/// Qualitative instability label corresponding to a [`StructuralFlag`].
#[must_use]
pub fn interpretation_for(flag: StructuralFlag) -> Cow<'static, str> {
    match flag {
        StructuralFlag::None => Cow::Borrowed("low"),
        StructuralFlag::NearCoinflip | StructuralFlag::NearDeadline => Cow::Borrowed("medium"),
        StructuralFlag::NearDeadlineAndCoinflip => Cow::Borrowed("high"),
    }
}

/// Mean squared per-step log-odds change; `None` when fewer than 2 price moves exist.
#[must_use]
pub fn realized_variance(prices: &[f64]) -> Option<f64> {
    let moves: Vec<f64> = prices
        .windows(2)
        .map(|w| log_odds(w[1]) - log_odds(w[0]))
        .collect();
    if moves.len() < 2 {
        return None;
    }
    let mean = moves.iter().sum::<f64>() / moves.len() as f64;
    let variance = moves.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / moves.len() as f64;
    Some(variance)
}

/// English stopwords and generic question verbs excluded from [`token_overlap`].
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "in", "on", "of", "for", "to", "by", "at", "or", "and", "is", "be", "will",
    "what", "who", "whom", "which", "that", "this", "these", "those", "it", "its", "happen",
    "occur", "take", "place", "there", "their", "they", "them", "his", "her", "before", "after",
    "during", "above", "below", "over", "under", "than", "then", "when", "how", "many", "much",
    "does", "do", "did", "any", "some", "no", "not", "yes", "win", "become", "get", "make", "made",
    "out", "up", "down", "new", "next", "end", "year",
];

fn significant_tokens(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// Deterministic Jaccard similarity over significant tokens of two questions.
#[must_use]
pub fn token_overlap(a: &str, b: &str) -> f64 {
    let ta = significant_tokens(a);
    let tb = significant_tokens(b);
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let intersection = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    intersection / union
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_and_tier_round_trip_snake_and_lowercase() {
        let json = serde_json::to_string(&(MarketStatus::Open, ReliabilityTier::High))
            .expect("serialize tuple");
        assert_eq!(json, r#"["open","high"]"#);
    }
}
