//! Provider-to-`MarketRecord` assembly adapter over `hkask_forecast::market_contract`.
//!
//! Domain contract structs, reliability/structural-volatility classifiers, and
//! log-odds realized variance live in `hkask_forecast::market_contract`. This
//! module owns only the provider-specific Kalshi/Polymarket assembly and PKO/DC
//! ontology stamping (`hkask_bridge_ontology`).

use std::borrow::Cow;

pub use hkask_forecast::market_contract::{
    Calibration, MarketRecord, MarketStatus, OntologyBlock, ProbabilityMethod, ProcessAxis,
    ReliabilityTier, Source, StateAxis, StructuralFlag, Volatility, VolumeGrain, canonical_bucket,
    interpretation_for, realized_variance, reliability_tier, structural_flag,
};

use crate::calibration::CalibrationReading;
use crate::ontology;
use crate::provider_kalshi::{KalshiEvent, KalshiMarket, parse_fp};
use crate::provider_polymarket::GammaMarket;

/// Build descriptive loss metadata without manufacturing history or bias.
pub fn calibration_for(store_reading: Option<&CalibrationReading>) -> Calibration {
    match store_reading {
        Some(reading) if !reading.stale => Calibration {
            brier: reading.brier,
            sample_size: reading.sample_size,
            stale: false,
        },
        _ => Calibration {
            brier: None,
            sample_size: store_reading.map_or(0, |r| r.sample_size),
            stale: true,
        },
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
            r#type: Cow::Borrowed(hkask_bridge_ontology::pko::PROCEDURE_EXECUTION),
            stage: Cow::Borrowed(stage),
            probability_role: Cow::Borrowed(hkask_bridge_ontology::pko::WAS_GENERATED_BY),
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

const DAYS_PER_YEAR: f64 = 365.25;

pub(crate) fn years_between(
    deadline: &str,
    reference: &chrono::DateTime<chrono::Utc>,
) -> Option<f64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(deadline).ok()?;
    Some(
        (parsed.with_timezone(&chrono::Utc) - *reference).num_seconds() as f64
            / (86_400.0 * DAYS_PER_YEAR),
    )
}

struct RecordParts {
    realized_variance: Option<f64>,
    source: Source,
    event_id: String,
    ontology_id: String,
    market_id: String,
    question: String,
    description: String,
    category: String,
    series: String,
    deadline: String,
    observation_now: chrono::DateTime<chrono::Utc>,
    probability: f64,
    probability_method: ProbabilityMethod,
    spread: Option<f64>,
    volume: f64,
    volume_grain: VolumeGrain,
    liquidity: Option<f64>,
    open_interest: Option<f64>,
    last_update: String,
    clob_asset_ids: Option<Vec<String>>,
    status: MarketStatus,
    resolved_outcome: Option<bool>,
    lifecycle_stage: &'static str,
    resolution_source: &'static str,
}

fn assemble(parts: RecordParts, calibration: Calibration) -> MarketRecord {
    let time_to_maturity = years_between(&parts.deadline, &parts.observation_now);
    if time_to_maturity.is_none() {
        tracing::warn!(
            "unparsable deadline '{}' for market {} — time_to_maturity is None",
            parts.deadline,
            parts.market_id,
        );
    }
    let days_to_deadline = time_to_maturity.map(|years| years * DAYS_PER_YEAR);
    let flag = structural_flag(parts.probability, days_to_deadline);
    let volume = parts.volume;
    let tier = reliability_tier(volume, parts.spread);
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
        clob_asset_ids: parts.clob_asset_ids,
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

/// Build an annotated [`MarketRecord`] from a Kalshi market + optional parent event.
pub fn from_kalshi(
    market: &KalshiMarket,
    event: Option<&KalshiEvent>,
    calibration: Calibration,
    now: &chrono::DateTime<chrono::Utc>,
) -> Option<MarketRecord> {
    let probability = market.yes_midpoint()?;
    let status = if !market.result.is_empty() {
        MarketStatus::Resolved
    } else if market.status == "active" {
        MarketStatus::Open
    } else {
        MarketStatus::Closed
    };
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
            observation_now: *now,
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
            clob_asset_ids: None,
        },
        calibration,
    ))
}

/// Build an annotated [`MarketRecord`] from a Polymarket Gamma market + parent event context.
#[expect(clippy::too_many_arguments)]
pub fn from_polymarket(
    market: &GammaMarket,
    event_id: &str,
    event_slug: &str,
    event_volume: f64,
    event_liquidity: f64,
    event_tags: &[String],
    calibration: Calibration,
    now: &chrono::DateTime<chrono::Utc>,
) -> Option<MarketRecord> {
    let probability = market.yes_probability()?;
    let status = if market.uma_resolution_status == "resolved" {
        MarketStatus::Resolved
    } else if market.closed {
        MarketStatus::Closed
    } else {
        MarketStatus::Open
    };
    let resolved_outcome = if matches!(status, MarketStatus::Resolved) {
        if probability >= 0.99 {
            Some(true)
        } else if probability <= 0.01 {
            Some(false)
        } else {
            None
        }
    } else {
        None
    };
    let clob_asset_ids = market.clob_token_ids();
    Some(assemble(
        RecordParts {
            realized_variance: None,
            source: Source::Polymarket,
            event_id: event_id.to_string(),
            ontology_id: market.condition_id.clone(),
            market_id: market.id.clone(),
            question: market.question.clone(),
            description: market.description.clone(),
            category: event_tags.first().cloned().unwrap_or_default(),
            series: event_slug.to_string(),
            deadline: market.end_date.clone(),
            observation_now: *now,
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
            clob_asset_ids: (!clob_asset_ids.is_empty()).then_some(clob_asset_ids),
        },
        calibration,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_kalshi::KalshiMarket;
    use crate::provider_polymarket::GammaMarket;

    fn utc(rfc3339: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(rfc3339)
            .expect("test fixture timestamp")
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn raw_loss_history_does_not_demote_liquidity_tier_or_correct_price() {
        let mut store = crate::calibration::CalibrationStore::new();
        for i in 0..5 {
            store.record(
                "politics",
                crate::calibration::ResolvedObservation {
                    probability: 0.9,
                    outcome: false,
                    market_key: Some(format!("market-{i}")),
                },
            );
        }
        let reading = crate::calibration::read_calibration(&store, "politics");
        let market = KalshiMarket {
            ticker: "TEST".into(),
            event_ticker: "TEST".into(),
            title: "Political outcome".into(),
            status: "active".into(),
            yes_bid_dollars: "0.89".into(),
            yes_ask_dollars: "0.91".into(),
            volume_fp: "60000".into(),
            close_time: "2026-12-31T00:00:00Z".into(),
            ..Default::default()
        };
        let record = from_kalshi(
            &market,
            None,
            calibration_for(Some(&reading)),
            &utc("2026-10-20T00:00:00Z"),
        )
        .expect("market");
        assert_eq!(record.reliability_tier, ReliabilityTier::High);
        assert!((record.probability - 0.9).abs() < 1e-12);
        assert_eq!(record.calibration.sample_size, 5);
        assert!((record.calibration.brier.expect("raw loss") - 0.81).abs() < 1e-12);
        assert_eq!(reliability_tier(10.0, Some(0.02)), ReliabilityTier::Low);
    }

    #[test]
    fn maturity_is_measured_from_observation_not_quote_vintage() {
        let now = utc("2026-09-20T00:00:00Z");
        let market = KalshiMarket {
            ticker: "KXFEDDECISION-26OCT-H25".into(),
            event_ticker: "KXFEDDECISION-26OCT".into(),
            title: "Will the Federal Reserve Hike rates by 25bps".into(),
            status: "active".into(),
            yes_bid_dollars: "0.54".into(),
            yes_ask_dollars: "0.55".into(),
            close_time: "2026-10-28T16:00:00Z".into(),
            updated_time: "2026-04-09T12:00:00Z".into(),
            ..Default::default()
        };
        let record =
            from_kalshi(&market, None, calibration_for(None), &now).expect("record assembles");
        let expected = years_between("2026-10-28T16:00:00Z", &now).expect("parses");
        assert_eq!(record.time_to_maturity, Some(expected));
        assert_eq!(
            record.last_update, "2026-04-09T12:00:00Z",
            "quote vintage carried separately, never folded into duration"
        );
        let days = expected * DAYS_PER_YEAR;
        assert!(
            (36.0..=42.0).contains(&days),
            "Oct-28 rung is ≈38 days from Sep 20, got {days}"
        );
    }

    #[test]
    fn polymarket_record_exposes_clob_asset_ids() {
        let now = utc("2026-09-20T00:00:00Z");
        let market = GammaMarket {
            id: "pm-1".into(),
            question: "Will X happen?".into(),
            outcomes: "[\"Yes\", \"No\"]".into(),
            outcome_prices: "[\"0.55\", \"0.45\"]".into(),
            clob_token_ids: "[\"tok-yes\", \"tok-no\"]".into(),
            end_date: "2026-12-31T00:00:00Z".into(),
            ..Default::default()
        };
        let record = from_polymarket(
            &market,
            "ev-1",
            "slug-x",
            100.0,
            50.0,
            &[],
            calibration_for(None),
            &now,
        )
        .expect("record assembles");
        assert_eq!(
            record.clob_asset_ids,
            Some(vec!["tok-yes".to_string(), "tok-no".to_string()]),
            "CLOB asset IDs surfaced for the subscribe path (§7 followup)"
        );
    }

    #[test]
    fn cached_record_without_clob_field_deserializes_as_none() {
        let now = utc("2026-09-20T00:00:00Z");
        let market = GammaMarket {
            id: "pm-2".into(),
            question: "Will Y happen?".into(),
            outcomes: "[\"Yes\", \"No\"]".into(),
            outcome_prices: "[\"0.60\", \"0.40\"]".into(),
            clob_token_ids: "[\"tok-a\"]".into(),
            end_date: "2026-12-31T00:00:00Z".into(),
            ..Default::default()
        };
        let record = from_polymarket(
            &market,
            "ev-2",
            "slug-y",
            100.0,
            50.0,
            &[],
            calibration_for(None),
            &now,
        )
        .expect("record assembles");
        let mut value = serde_json::to_value(&record).expect("serializes");
        value
            .as_object_mut()
            .expect("record serializes as an object")
            .remove("clob_asset_ids");
        let reloaded: MarketRecord =
            serde_json::from_value(value).expect("loads without the field");
        assert!(
            reloaded.clob_asset_ids.is_none(),
            "absent field loads as None, never breaks the cache"
        );
    }
}
