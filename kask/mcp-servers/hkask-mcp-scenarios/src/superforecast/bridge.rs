//! Bridge — cross-validation of estimates and conversion of external server
//! outputs (companies + prediction-markets) into `ScenarioEvent`s. Adapts the
//! `hkask-mcp-companies`/`-prediction-markets` data shapes into the scenario
//! forecast model.
//!
//! Extracted from `superforecast.rs` (deep-module split).

use super::ForecastStore;
use super::calibrate_from_fermi;

use crate::types::{
    CrossValidation, ScenarioError, ScenarioEvent, ScenarioType, SubQuestion,
    SubQuestionDivergence, TimeHorizon,
};
// ── Cross-Validation ──────────────────────────────────────────────────────

/// Cross-validate two probability estimates for the same event.
///
/// Typically compares an LLM-generated estimate (from the superforecasting
/// skill) against a server-computed estimate (from scenario_calibrate).
///
/// Computes per-sub-question divergence to identify where the estimates
/// differ most. Flags for review when overall divergence exceeds the
/// threshold (default 0.15).
///
/// This closes the learning loop between LLM reasoning and computational
/// verification — the key bridge between the superforecasting skill and
/// the scenarios MCP server.
#[must_use = "validation result should be inspected"]
pub fn cross_validate(
    event_id: &str,
    source_a: &str,
    estimate_a: f64,
    sub_questions_a: &[SubQuestion],
    source_b: &str,
    estimate_b: f64,
    sub_questions_b: &[SubQuestion],
    threshold: Option<f64>,
) -> CrossValidation {
    let review_threshold = threshold.unwrap_or(0.15);
    let divergence = (estimate_a - estimate_b).abs();
    let requires_review = divergence > review_threshold;

    // Match sub-questions by index (best-effort alignment)
    let max_sq = sub_questions_a.len().max(sub_questions_b.len());
    let mut sq_divergences = Vec::new();
    for i in 0..max_sq {
        let sq_a = sub_questions_a.get(i);
        let sq_b = sub_questions_b.get(i);
        let question = sq_a
            .map(|s| s.question.as_str())
            .or_else(|| sq_b.map(|s| s.question.as_str()))
            .unwrap_or("unknown");
        let est_a = sq_a.map(|s| s.estimate).unwrap_or(0.5);
        let est_b = sq_b.map(|s| s.estimate).unwrap_or(0.5);
        let sq_div = (est_a - est_b).abs();
        sq_divergences.push(SubQuestionDivergence {
            question: question.to_string(),
            estimate_a: est_a,
            estimate_b: est_b,
            divergence: sq_div,
        });
    }

    let recommendation = if !requires_review {
        format!(
            "Estimates are consistent (divergence {:.3} <= threshold {:.3}). No review needed.",
            divergence, review_threshold
        )
    } else {
        let max_sq_div = sq_divergences
            .iter()
            .map(|d| d.divergence)
            .fold(0.0_f64, f64::max);
        let top_sq = sq_divergences
            .iter()
            .max_by(|a, b| {
                a.divergence
                    .partial_cmp(&b.divergence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|d| d.question.as_str())
            .unwrap_or("unknown");
        format!(
            "Estimates diverge ({:.3} > {:.3}). Largest sub-question divergence ({:.3}) on '{}'. Activate grill-me skill.",
            divergence, review_threshold, max_sq_div, top_sq
        )
    };

    let grill_me_questions: Vec<String> = if requires_review {
        let mut questions = vec![format!(
            "What hidden assumptions could explain the {:.1}% divergence between '{}' and '{}' on event '{}'?",
            divergence * 100.0,
            source_a,
            source_b,
            event_id
        )];
        for sq in sq_divergences.iter().take(3) {
            if sq.divergence > 0.05 {
                questions.push(format!(
                    "Sub-question '{}': why does {} estimate {:.0}% while {} estimates {:.0}%?",
                    sq.question,
                    source_a,
                    sq.estimate_a * 100.0,
                    source_b,
                    sq.estimate_b * 100.0
                ));
            }
        }
        questions
    } else {
        Vec::new()
    };

    CrossValidation {
        event_id: event_id.to_string(),
        estimate_a,
        source_a: source_a.to_string(),
        estimate_b,
        source_b: source_b.to_string(),
        divergence,
        requires_review,
        review_threshold,
        sub_question_divergences: sq_divergences,
        recommendation,
        grill_me_questions,
    }
}

// ── Companies Server Bridge ────────────────────────────────────────────────

/// Convert a companies server calibrate_forecast output into ScenarioEvents
/// that can be quantified by the scenarios pipeline.
///
/// The companies server produces Schwartz 2×2 scenario results with
/// intrinsic values per quadrant. This function converts those into
/// binomial events with Fermi sub-questions, ready for scenario_quantify
/// and scenario_calibrate.
///
/// Bridge path: companies.calibrate_forecast → this function → scenario_quantify → scenario_synthesize
pub fn convert_companies_output(
    symbol: &str,
    companies_json: &serde_json::Value,
    time_horizon: TimeHorizon,
) -> Result<Vec<ScenarioEvent>, ScenarioError> {
    let scenarios = companies_json
        .get("scenarios")
        .and_then(|s| s.as_array())
        .ok_or(ScenarioError::NoEvents)?;

    let mut events = Vec::new();
    // Derive deadline from time horizon
    let today = chrono::Utc::now().date_naive();
    let deadline = match time_horizon {
        TimeHorizon::Tactical => today + chrono::TimeDelta::days(540),
        TimeHorizon::Strategic => today + chrono::TimeDelta::days(1460),
        TimeHorizon::LongTerm => today + chrono::TimeDelta::days(2920),
    };

    for (i, scenario) in scenarios.iter().enumerate() {
        let name = scenario
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");
        let intrinsic = scenario
            .get("intrinsic_per_share")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let current_price = companies_json
            .get("current_price")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let upside = if current_price > 0.0 {
            (intrinsic - current_price) / current_price
        } else {
            0.0
        };

        let question = format!(
            "Will {} trade within 20% of the {} scenario intrinsic value ({:.2}) by {}",
            symbol,
            name.to_lowercase(),
            intrinsic,
            deadline.format("%Y-%m-%d")
        );

        let growth = scenario.get("applied_growth").and_then(|v| v.as_f64());
        let margin = scenario.get("applied_margin").and_then(|v| v.as_f64());

        let mut sub_questions = Vec::new();
        if let Some(g) = growth {
            sub_questions.push(SubQuestion {
                question: format!("Will revenue growth reach {:.0}%?", g * 100.0),
                estimate: if g > 0.1 { 0.6 } else { 0.4 },
                confidence: 0.5,
            });
        }
        if let Some(m) = margin {
            sub_questions.push(SubQuestion {
                question: format!("Will gross margins hold at {:.0}%?", m * 100.0),
                estimate: if m > 0.4 { 0.6 } else { 0.4 },
                confidence: 0.5,
            });
        }

        // Probability: Fermi-calibrate from sub-questions when available.
        let prob = if !sub_questions.is_empty() {
            calibrate_from_fermi(&sub_questions).unwrap_or_else(|_| {
                if upside > 0.2 {
                    0.65
                } else if upside > 0.0 {
                    0.55
                } else if upside > -0.2 {
                    0.40
                } else {
                    0.25
                }
            })
        } else if upside > 0.2 {
            0.65
        } else if upside > 0.0 {
            0.55
        } else if upside > -0.2 {
            0.40
        } else {
            0.25
        };

        events.push(ScenarioEvent {
            id: format!("comp-{}-{}", symbol, i),
            name: format!("{} {}", symbol, name),
            question,
            deadline,
            time_horizon,
            scenario_type: ScenarioType::CompanyAnalysis,
            subject: symbol.to_string(),
            probability: prob,
            basis: Some("financial_model".into()),
            depends_on: vec![],
            sub_questions,
            base_rate: None,
            reference_class: Some("Company DCF scenario analysis, 2×2 Schwartz matrix".into()),
            brier_score: None,
            update_count: 0,
        });
    }

    Ok(events)
}

/// Per-domain de-compression strength δ, derived from measured calibration.
///
/// Returns the weighted bias (expected_rate − hit_rate) over resolved
/// forecasts in `store` matching `category` (case-insensitive substring
/// match). A bias of 0.0 means the domain is well-calibrated (or there is
/// insufficient data — fewer than `MIN_DOMAIN_SAMPLE` resolved forecasts —
/// in which case no correction is applied, the honest default per Tetlock's
/// superforecasting discipline: corrections must come from measured
/// calibration, not hardcoded magic numbers).
///
/// Positive δ de-compresses probabilities away from 0.5 (corrects
/// underconfidence — forecasts cluster too close to 50/50). Negative δ is
/// clamped to 0.0 by `domain_bias_correction` (overconfidence is corrected
/// by a different mechanism — the calibration feedback loop, not a static
/// de-compression).
pub fn domain_bias_delta(store: Option<&ForecastStore>, category: &str) -> f64 {
    /// Minimum resolved forecasts in a domain before a data-derived bias
    /// correction is applied. Below this, the sample is too small for a
    /// reliable bias estimate (Tetlock: calibration requires "enough
    /// forecasts to make the statistics work").
    const MIN_DOMAIN_SAMPLE: usize = 5;

    let Some(store) = store else {
        return 0.0;
    };
    let resolved = store.resolved_by_category(category);
    if resolved.len() < MIN_DOMAIN_SAMPLE {
        return 0.0;
    }

    // Reuse the calibration-curve logic: weighted bias across bins with
    // enough samples. This is the same computation as
    // `compute_calibration_curve`'s `overconfidence_score`, but filtered to
    // the domain category.
    let mut bins: Vec<(u64, u64, f64)> = vec![(0, 0, 0.0); 10];
    for record in &resolved {
        let occurred = record.outcome.unwrap_or(false);
        let bin_idx = ((record.probability * 10.0) as usize).min(9);
        bins[bin_idx].0 += 1;
        if occurred {
            bins[bin_idx].1 += 1;
        }
        bins[bin_idx].2 += record.probability;
    }

    let mut weighted_bias = 0.0;
    let mut bias_weight = 0.0;
    for &(count, hits, probability_sum) in &bins {
        if count >= 5 {
            let hit_rate = hits as f64 / count as f64;
            let expected = probability_sum / count as f64;
            // bias = expected − hit_rate: positive = overconfident (forecasts
            // say X% but reality is lower), negative = underconfident.
            // For de-compression δ we want the magnitude of underconfidence
            // (forecasts compressed toward 0.5). A negative bias (underconfident)
            // maps to a positive δ; a positive bias (overconfident) maps to δ=0
            // (de-compression would make it worse).
            let bias = expected - hit_rate;
            if bias < 0.0 {
                weighted_bias += (-bias) * count as f64;
                bias_weight += count as f64;
            }
        }
    }
    if bias_weight > 0.0 {
        (weighted_bias / bias_weight).clamp(0.0, 0.5)
    } else {
        0.0
    }
}

/// Convert an annotated prediction-market record (from
/// hkask-mcp-prediction-markets `market_lookup`/`market_match`, pasted by
/// the caller — the same caller-mediated bridge pattern as
/// `scenario_from_companies`) into a ScenarioEvent anchored on the
/// market-implied base rate.
///
/// Gates (deliberate refusals, mirroring the contract's epistemic posture):
/// - `reliability_tier` low ⇒ no anchor (`base_rate: None` + warning)
/// - `match_confidence` below high/medium ⇒ no anchor (wrong-event risk)
/// - resolved/closed markets ⇒ rejected (a resolved market is not a forecast)
///
/// The domain-bias correction (hkask_forecast::domain_bias_correction) is
/// applied deterministically here — the LLM consumer never sees an
/// uncorrected politics price as the default anchor. The correction δ is
/// derived from measured per-domain calibration in `store` (when provided);
/// when `store` is `None` or has insufficient resolved forecasts for the
/// domain, δ=0.0 (no correction — the honest default per Tetlock's
/// discipline: corrections come from measured calibration, not hardcoded
/// magic numbers).
pub fn convert_market_record(
    record: &hkask_mcp_prediction_markets::types::MarketRecord,
    match_confidence: Option<&str>,
    store: Option<&ForecastStore>,
) -> Result<(ScenarioEvent, Vec<String>), ScenarioError> {
    let mut warnings = Vec::new();

    if !matches!(
        record.status,
        hkask_mcp_prediction_markets::types::MarketStatus::Open
    ) {
        return Err(ScenarioError::EmptyInput(format!(
            "market '{}' is not open (status: {:?}) — a resolved or closed market is not a forecast",
            record.market_id, record.status
        )));
    }

    let raw = record.probability;
    let delta = domain_bias_delta(store, &record.category);
    let corrected = hkask_forecast::domain_bias_correction(raw, delta);
    if delta > 0.0 {
        warnings.push(format!(
            "domain-bias correction applied ({}, δ={delta}): {raw:.3} → {corrected:.3}",
            record.category
        ));
    }

    let low_reliability = matches!(
        record.reliability_tier,
        hkask_mcp_prediction_markets::types::ReliabilityTier::Low
    );
    let weak_match = match match_confidence {
        Some("high") | Some("medium") | None => false,
        Some(_) => true,
    };

    let base_rate = if low_reliability || weak_match {
        if low_reliability {
            warnings.push(format!(
                "low reliability tier (volume={:.0}, spread={:?}) — base_rate withheld",
                record.volume, record.spread
            ));
        }
        if weak_match {
            warnings.push(format!(
                "match confidence {:?} below threshold — base_rate withheld (wrong-event risk)",
                match_confidence
            ));
        }
        None
    } else {
        Some(corrected)
    };

    let deadline = chrono::NaiveDate::parse_from_str(
        &record.deadline.chars().take(10).collect::<String>(),
        "%Y-%m-%d",
    )
    .map_err(|e| {
        // A forecasting artifact with a fabricated deadline is worse than an
        // error (same no-fabrication posture as the contract's
        // resolved_outcome gate).
        ScenarioError::EmptyInput(format!(
            "market '{}' has unparseable deadline '{}' ({e}) — refusing to fabricate one",
            record.market_id, record.deadline
        ))
    })?;

    let event = ScenarioEvent {
        id: format!("mkt-{}", record.market_id),
        name: record.question.chars().take(80).collect(),
        question: record.question.clone(),
        deadline,
        time_horizon: TimeHorizon::Strategic,
        scenario_type: ScenarioType::EmergingEconomic,
        subject: record.series.clone(),
        probability: base_rate.unwrap_or(0.5),
        basis: Some(format!("prediction_market:{:?}", record.source).to_lowercase()),
        depends_on: vec![],
        sub_questions: vec![],
        base_rate,
        reference_class: Some(format!(
            "{} market-implied probability (tier: {:?}, method: {:?})",
            record.series, record.reliability_tier, record.probability_method
        )),
        brier_score: None,
        update_count: 0,
    };
    Ok((event, warnings))
}
