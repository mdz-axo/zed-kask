//! The annotated market-record contract (integration report §4).
//!
//! Load-bearing design rule: this struct never carries a bare probability.
//! Every `probability` is paired with reliability covariates, calibration
//! metadata, volatility annotation, and the dual-axis ontology mapping so a
//! consumer cannot be naive by default.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::calibration::CalibrationReading;
use crate::ontology;
use crate::provider_kalshi::{KalshiEvent, KalshiMarket, parse_fp};
use crate::provider_polymarket::GammaMarket;

/// Source platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Polymarket,
    Kalshi,
}

/// Provenance of the probability value (T0: Kalshi percentile-history was
/// dropped after the endpoint 404'd live; candlesticks are the Kalshi
/// history source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbabilityMethod {
    /// Last trade price (Polymarket outcomePrices).
    LastTrade,
    /// Two-sided quote midpoint (Kalshi yes bid/ask).
    Midpoint,
}

/// Lifecycle status of the market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketStatus {
    Open,
    Closed,
    Resolved,
}

/// The grain at which `volume` is measured — differs across providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VolumeGrain {
    Market,
    Event,
}

/// Derived reliability gate (2607.08199: wide spread / thin volume ⇒ noise).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReliabilityTier {
    High,
    Medium,
    Low,
}

/// Calibration metadata for the market's domain/series. Computed from data
/// by T5; until then `domain_bias` is seeded from arXiv:2602.19520 and
/// `stale` is true (no measured Brier yet — never a synthetic 0).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calibration {
    pub brier: Option<f64>,
    pub domain_bias: Option<Cow<'static, str>>,
    /// Provenance of the bias estimate: seeded from arXiv:2602.19520 until
    /// the calibration loop measures it from resolved outcomes. Consumers
    /// must be able to distinguish a paper-seeded prior from a measurement.
    pub bias_source: Cow<'static, str>,
    pub sample_size: u64,
    /// True when the calibration signal is unavailable (not measured, read
    /// failure, thin sample). Distinct from brier: 0 — a 0 would read as
    /// "perfectly calibrated" and create a reinforcing loop.
    pub stale: bool,
}

/// Volatility annotation (2607.08199). `realized_variance` is computed from
/// price history by T4's follow-up; until history is wired it is None and
/// only structural flags are set. `dras_forecast` carries the closed-form
/// DR-AS structural conditional variance forecast when the forecast-origin
/// state (price, time-to-resolution, spread, volume) is available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volatility {
    pub realized_variance: Option<f64>,
    pub structural_flag: StructuralFlag,
    pub interpretation: Cow<'static, str>,
    /// The DR-AS structural conditional volatility forecast (arXiv:2607.08199),
    /// when the forecast-origin state is available. None when the inputs are
    /// missing or degenerate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dras_forecast: Option<crate::volatility::VolatilityForecast>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuralFlag {
    None,
    NearDeadline,
    NearCoinflip,
    NearDeadlineAndCoinflip,
}

/// PKO process-axis mapping (generated from `crate::ontology` constants).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessAxis {
    pub r#type: Cow<'static, str>,
    pub stage: Cow<'static, str>,
    pub probability_role: Cow<'static, str>,
}

/// Dublin Core state-axis mapping (vocabulary from hkask-bridge-ontology;
/// Q-O1 resolved 2026-08-05).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateAxis {
    pub identifier: String,
    pub title: String,
    pub description: String,
    pub temporal: String,
    pub provenance: Cow<'static, str>,
}

/// The dual-axis mapping every record carries to its consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyBlock {
    pub process: ProcessAxis,
    pub state: StateAxis,
    pub mapping_version: u32,
}

/// The full annotated market record.
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
    /// Fractional years from the record's `last_update` to `deadline`
    /// (negative when the deadline is past). None when either timestamp is
    /// unparsable — never a fabricated number.
    pub time_to_maturity: Option<f64>,
    pub probability: f64,
    pub probability_method: ProbabilityMethod,
    pub spread: Option<f64>,
    /// Traded volume at the market's own grain: market-level for Kalshi,
    /// event-level for Polymarket (Gamma exposes market volume as a string
    /// that is frequently empty; event volume is the reliable signal).
    /// Cross-platform comparisons must use `volume_grain`.
    pub volume: f64,
    pub volume_grain: VolumeGrain,
    pub liquidity: Option<f64>,
    pub open_interest: Option<f64>,
    pub last_update: String,
    pub volatility: Volatility,
    pub status: MarketStatus,
    pub resolved_outcome: Option<bool>,
    pub resolution_source: Cow<'static, str>,
    pub calibration: Calibration,
    pub reliability_tier: ReliabilityTier,
    pub ontology: OntologyBlock,
}

/// Build the calibration block for a record: store reading (Brier, sample
/// size, staleness) + the static-or-measured domain bias for the category.
/// The server (which owns the store) calls this; the builders stay pure.
pub fn calibration_for(store_reading: Option<&CalibrationReading>, category: &str) -> Calibration {
    let bias = domain_bias_for(category);
    let bias_source = if bias.is_some() {
        Cow::Borrowed("static_2602_19520")
    } else {
        Cow::Borrowed("none")
    };
    match store_reading {
        Some(reading) if !reading.stale => Calibration {
            brier: reading.brier,
            domain_bias: bias.map(Cow::Borrowed),
            bias_source,
            sample_size: reading.sample_size,
            stale: false,
        },
        _ => Calibration {
            brier: None,
            domain_bias: bias.map(Cow::Borrowed),
            bias_source,
            sample_size: store_reading.map_or(0, |r| r.sample_size),
            stale: true,
        },
    }
}

/// Canonical calibration bucket for a category/tag. The T10 loop closes
/// through this key — if Kalshi's "Elections" and Polymarket's "Politics"
/// accrue under different buckets, the same domain never reaches the
/// demotion threshold on either side. Normalization is lowercase synonym
/// mapping; unknown categories pass through lowercased (deterministic, no
/// fabrication — an unmapped category still forms a coherent bucket).
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

/// Static per-domain bias table seeded from arXiv:2602.19520 (politics
/// chronically underconfident on both exchanges). T5/T10 replace this with
/// data-derived estimates.
pub fn domain_bias_for(category: &str) -> Option<&'static str> {
    if canonical_bucket(category) == "politics" {
        Some("underconfident")
    } else {
        None
    }
}

/// Reliability gate from observable covariates, modulated by the bucket's
/// measured calibration (T10 loop closure). Thresholds are initial
/// placeholders (Q3 in the plan — recalibrate per-domain after data accrues).
///
/// The feedback is negative (corrective): a poorly-calibrated bucket
/// (Brier > 0.25 over ≥5 resolved markets — worse than a well-informed
/// forecaster's baseline) demotes the covariate-derived tier by one step.
/// A stale/unmeasured calibration signal changes nothing — absence of
/// evidence never demotes (and never promotes either).
pub fn reliability_tier(
    volume: f64,
    spread: Option<f64>,
    calibration: &Calibration,
) -> ReliabilityTier {
    let wide_spread = spread.is_some_and(|s| s > 0.10);
    let thin_volume = volume < 1_000.0;
    let base = if thin_volume || wide_spread {
        ReliabilityTier::Low
    } else if volume < 50_000.0 || spread.is_none_or(|s| s > 0.04) {
        ReliabilityTier::Medium
    } else {
        ReliabilityTier::High
    };
    let poorly_calibrated = !calibration.stale
        && calibration.sample_size >= 5
        && calibration.brier.is_some_and(|b| b > 0.25);
    if poorly_calibrated {
        match base {
            ReliabilityTier::High => ReliabilityTier::Medium,
            lower => lower,
        }
    } else {
        base
    }
}

/// Structural volatility flags per 2607.08199: vol rises near the deadline
/// and near coin-flip prices. Days-to-deadline is computed by the caller
/// (None when the deadline is unparsable → only the coin-flip check runs).
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

/// Interpretation derived from the structural flag (2607.08199): elevated
/// expected instability near deadlines and coin-flip prices.
fn interpretation_for(flag: StructuralFlag) -> Cow<'static, str> {
    match flag {
        StructuralFlag::None => Cow::Borrowed("low"),
        StructuralFlag::NearCoinflip | StructuralFlag::NearDeadline => Cow::Borrowed("medium"),
        StructuralFlag::NearDeadlineAndCoinflip => Cow::Borrowed("high"),
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
            r#type: Cow::Borrowed("pko:ProcedureExecution"),
            stage: Cow::Borrowed(stage),
            probability_role: Cow::Borrowed("prov:wasGeneratedBy"),
        },
        state: StateAxis {
            identifier: format!("{prefix}:{market_id}"),
            title: question.to_string(),
            description: description.chars().take(500).collect(),
            temporal: deadline.to_string(),
            provenance: Cow::Borrowed(provenance),
        },
        mapping_version: ontology::MAPPING_VERSION,
    }
}

/// Days in the time-to-maturity year convention (365.25 accounts for leap
/// years, matching common fixed-income convention).
const DAYS_PER_YEAR: f64 = 365.25;

/// Fractional years between an RFC3339 deadline and a reference instant.
/// Deadline in the past → negative. Unparsable deadline → None (the
/// unparsable-deadline warning is emitted once, at assembly time).
pub fn years_between(deadline: &str, reference: &chrono::DateTime<chrono::Utc>) -> Option<f64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(deadline).ok()?;
    Some(
        (parsed.with_timezone(&chrono::Utc) - *reference).num_seconds() as f64
            / (86_400.0 * DAYS_PER_YEAR),
    )
}

/// Realized variance of a price series: mean squared per-step log-odds
/// change (log-odds keeps the measure bounded at the 0/1 edges where raw
/// price deltas mechanically compress). `None` for fewer than 2 moves —
/// a variance from one step is fiction.
pub fn realized_variance(prices: &[f64]) -> Option<f64> {
    let moves: Vec<f64> = prices
        .windows(2)
        .map(|w| hkask_forecast::log_odds(w[1]) - hkask_forecast::log_odds(w[0]))
        .collect();
    if moves.len() < 2 {
        return None;
    }
    let mean = moves.iter().sum::<f64>() / moves.len() as f64;
    let variance = moves.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / moves.len() as f64;
    Some(variance)
}

/// Provider-specific record fields, extracted by each provider's adapter.
/// The shared assembly (annotation, ontology, tier, volatility) lives in
/// `assemble` — providers differ only in this extraction.
struct RecordParts {
    /// Pre-computed realized variance from price history (None when history
    /// was not fetched — the default for lookup paths).
    realized_variance: Option<f64>,
    source: Source,
    event_id: String,
    /// ID used in `dcterms:identifier` (Kalshi ticker / Polymarket condition id).
    ontology_id: String,
    market_id: String,
    question: String,
    description: String,
    category: String,
    series: String,
    deadline: String,
    /// The parsed reference instant the maturity is measured from — the
    /// record's own `last_update` when parseable, assembly `now` otherwise.
    maturity_reference: chrono::DateTime<chrono::Utc>,
    probability: f64,
    probability_method: ProbabilityMethod,
    spread: Option<f64>,
    volume: f64,
    volume_grain: VolumeGrain,
    liquidity: Option<f64>,
    open_interest: Option<f64>,
    last_update: String,
    status: MarketStatus,
    resolved_outcome: Option<bool>,
    lifecycle_stage: &'static str,
    resolution_source: &'static str,
}

/// Shared record assembly: volatility annotation, reliability tier, and the
/// dual-axis ontology block. Both providers route through here so the
/// annotation invariants live in exactly one place.
fn assemble(parts: RecordParts, calibration: Calibration) -> MarketRecord {
    let time_to_maturity = years_between(&parts.deadline, &parts.maturity_reference);
    if time_to_maturity.is_none() {
        // An unparsable deadline degrades duration semantics for this
        // record (no near-deadline vol flag, excluded from ladder tenors);
        // the warn lets an operator tell that apart from a far deadline.
        tracing::warn!(
            "unparsable deadline '{}' for market {} — time_to_maturity is None",
            parts.deadline,
            parts.market_id,
        );
    }
    let days_to_deadline = time_to_maturity.map(|years| years * DAYS_PER_YEAR);
    let flag = structural_flag(parts.probability, days_to_deadline);
    let volume = parts.volume;
    let tier = reliability_tier(volume, parts.spread, &calibration);
    MarketRecord {
        source: parts.source,
        event_id: parts.event_id,
        market_id: parts.market_id,
        question: parts.question.clone(),
        description: parts.description.clone(),
        category: parts.category,
        series: parts.series,
        deadline: parts.deadline.clone(),
        time_to_maturity,
        probability: parts.probability,
        probability_method: parts.probability_method,
        spread: parts.spread,
        volume,
        volume_grain: parts.volume_grain,
        liquidity: parts.liquidity,
        open_interest: parts.open_interest,
        last_update: parts.last_update,
        volatility: Volatility {
            realized_variance: parts.realized_variance,
            structural_flag: flag,
            interpretation: interpretation_for(flag),
            dras_forecast: None,
        },
        status: parts.status,
        resolved_outcome: parts.resolved_outcome,
        resolution_source: Cow::Borrowed(parts.resolution_source),
        calibration,
        reliability_tier: tier,
        ontology: make_ontology(
            parts.source,
            &parts.ontology_id,
            &parts.question,
            &parts.description,
            &parts.deadline,
            parts.lifecycle_stage,
            parts.resolution_source,
        ),
    }
}

impl MarketRecord {
    /// Build a record from a Kalshi market + its parent event.
    pub fn from_kalshi(
        market: &KalshiMarket,
        event: Option<&KalshiEvent>,
        calibration: Calibration,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Option<Self> {
        let probability = market.yes_midpoint()?;
        let status = if !market.result.is_empty() {
            MarketStatus::Resolved
        } else if market.status == "active" {
            MarketStatus::Open
        } else {
            MarketStatus::Closed
        };
        let maturity_reference = chrono::DateTime::parse_from_rfc3339(&market.updated_time)
            .map(|timestamp| timestamp.with_timezone(&chrono::Utc))
            .unwrap_or(*now);
        Some(assemble(
            RecordParts {
                realized_variance: None,
                source: Source::Kalshi,
                event_id: market.event_ticker.clone(),
                ontology_id: market.ticker.clone(),
                market_id: market.ticker.clone(),
                question: market.title.clone(),
                description: market.rules_primary.clone(),
                category: event.map(|e| e.category.clone()).unwrap_or_default(),
                series: event.map(|e| e.series_ticker.clone()).unwrap_or_default(),
                deadline: market.close_time.clone(),
                maturity_reference,
                probability,
                probability_method: ProbabilityMethod::Midpoint,
                spread: market.spread(),
                volume: parse_fp(&market.volume_fp).unwrap_or(0.0),
                volume_grain: VolumeGrain::Market,
                liquidity: parse_fp(&market.liquidity_dollars),
                open_interest: parse_fp(&market.open_interest_fp),
                last_update: market.updated_time.clone(),
                status,
                resolved_outcome: match market.result.as_str() {
                    "yes" => Some(true),
                    "no" => Some(false),
                    _ => None,
                },
                lifecycle_stage: lifecycle_stage(status, None),
                resolution_source: "kalshi_exchange",
            },
            calibration,
        ))
    }

    /// Build a record from a Polymarket Gamma market + parent event context.
    pub fn from_polymarket(
        market: &GammaMarket,
        event_id: &str,
        event_slug: &str,
        event_volume: f64,
        event_liquidity: f64,
        event_tags: &[String],
        calibration: Calibration,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Option<Self> {
        let probability = market.yes_probability()?;
        let status = if market.uma_resolution_status == "resolved" {
            MarketStatus::Resolved
        } else if market.closed {
            MarketStatus::Closed
        } else {
            MarketStatus::Open
        };
        // Resolved outcome requires definitive evidence: the Yes leg priced
        // at (approximately) 1 or 0 post-resolution. arXiv:2604.20421
        // documents "Unknown/50-50" resolutions where both legs settle at
        // 0.50 — a looser threshold would fabricate an outcome for those,
        // poisoning the T10 Brier loop with a false label. Ambiguous ⇒ None.
        let resolved_outcome = if matches!(status, MarketStatus::Resolved) {
            market.prices().first().and_then(|p| {
                if *p >= 0.99 {
                    Some(true)
                } else if *p <= 0.01 {
                    Some(false)
                } else {
                    None
                }
            })
        } else {
            None
        };
        let maturity_reference = chrono::DateTime::parse_from_rfc3339(&market.updated_at)
            .map(|timestamp| timestamp.with_timezone(&chrono::Utc))
            .unwrap_or(*now);
        Some(assemble(
            RecordParts {
                realized_variance: None,
                source: Source::Polymarket,
                event_id: event_id.to_string(),
                ontology_id: market.condition_id.clone(),
                market_id: market.id.clone(),
                question: market.question.clone(),
                description: market.description.clone(),
                // Gamma has no category field; derive from event tags
                // (T0-verified). Without this the politics-bias guardrail
                // never fires on Polymarket.
                category: event_tags.first().cloned().unwrap_or_default(),
                series: event_slug.to_string(),
                deadline: market.end_date.clone(),
                maturity_reference,
                probability,
                probability_method: ProbabilityMethod::LastTrade,
                spread: market.spread,
                volume: event_volume,
                volume_grain: VolumeGrain::Event,
                liquidity: Some(event_liquidity),
                open_interest: None,
                last_update: market.updated_at.clone(),
                status,
                resolved_outcome,
                lifecycle_stage: lifecycle_stage(status, Some(&market.uma_resolution_status)),
                resolution_source: "uma_oracle",
            },
            calibration,
        ))
    }
}
