//! Bridge — cross-validation of estimates and conversion of prediction-market
//! records into `ScenarioEvent`s. Adapts the `hkask-mcp-prediction-markets`
//! data shape into the scenario forecast model.
//!
//! Extracted from `superforecast.rs` (deep-module split).

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
pub(crate) fn cross_validate(
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

// ── Market conversion ──────────────────────────────────────────────
/// Convert an open market without altering its observed probability.
/// Liquidity and event-match gates withhold the base rate; the 0.5 scaffold
/// is not eligible for forecast declaration.
pub(crate) fn convert_market_record(
    record: &hkask_mcp_prediction_markets::types::MarketRecord,
    match_confidence: Option<&str>,
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
        Some(record.probability)
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
            "market '{}' has unparsable deadline '{}' ({e}) — refusing to fabricate one",
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
