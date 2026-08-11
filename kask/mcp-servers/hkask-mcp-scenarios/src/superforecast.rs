//! Superforecasting computation engine (Tetlock GJP methodology).
//!
//! Four-stage pipeline:
//! 1. Fermi decomposition — break forecast into sub-questions
//! 2. Outside view — base rate calibration from reference class
//! 3. Inside view — case-specific adjustments
//! 4. Bayesian updating — revise probabilities as evidence arrives
//!
//! Plus: event tree computation (conditional probability propagation)
//! and Brier scoring for calibration tracking.

use crate::types::{
    CrossValidation, EventTree, ScenarioError, ScenarioEvent, ScenarioType, SubQuestion,
    SubQuestionDivergence, TimeHorizon,
};
use std::collections::{HashMap, HashSet};

use hkask_forecast as forecast;

// ── Re-exports from hkask-forecast (pure pass-throughs eliminated) ───────
pub use forecast::{bayesian_update, brier_interpretation, brier_score, outside_view_adjustment};
// ── Forecast math (pure deterministic functions) ───────────────────────────
// Extracted to `superforecast/math.rs` (deep-module split: the pure math — Fermi
// decomposition, event-tree propagation, Brier scoring, sensitivity ranking — is
// independent of the stateful orchestration that remains in this file).
mod math;
pub(crate) use math::brier_score_multi;
pub use math::{
    auto_update_suggestions, build_event_tree, calibrate_from_fermi, score_forecast,
    sensitivity_ranking, structure_framing_document,
};

// ── Assessment (Chermack + Dragonfly-Eye + Calibration + Triage) ────────────
// Extracted to `superforecast/assess.rs` (deep-module split: the assessment
// concern — project scoring, perspective synthesis, calibration-curve tracking,
// triage — is independent of the forecast math and market composition).
mod assess;
pub use assess::{
    assess_project, compute_calibration_curve, synthesize_perspectives, triage_question,
};

// ── Persistence ──────────────────────────────────────────────────────────
// Extracted to `superforecast/store.rs` (deep-module split: the persistence
// concern — journal + snapshot compaction — is independent of the forecast
// math and composition concerns that remain in this file).
mod store;
pub use store::ForecastStore;

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

// ── Markets-set composition (T4a) ───────────────────────────────────────────

/// One caller-specified dependency edge for `compose_market_tree`.
///
/// The conditionals are caller-authored: the platform computes marginals and
/// joints but never invents the conditional probabilities themselves (the
/// never-fabricate rule applied to the composition layer).
#[derive(Debug, Clone)]
pub struct DependencySpec {
    /// Child event id (must match a converted event's `mkt-{market_id}`).
    pub child_market_id: String,
    /// Parent event ids (market ids of the conditioning markets).
    pub parent_market_ids: Vec<String>,
    /// P(child | parent truth assignment), bitmap-ordered, length
    /// 2^parent_market_ids.len().
    pub conditionals: Vec<f64>,
}

/// Maximum parents per dependency group (CPT size cap — variety amplifier iv).
/// 2^4 = 16 conditional entries per group; deeper conditioning belongs in
/// multiple groups (noisy-OR channels) or signals a misspecified tree.
pub const MAX_PARENTS_PER_GROUP: usize = 4;

/// Jaccard token-overlap threshold above which two market questions are
/// flagged as potential duplicates (same underlying event, not a dependency).
const DUPLICATE_OVERLAP_THRESHOLD: f64 = 0.65;

/// A warning emitted during composition — surfaced to the caller, never
/// silently dropped.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompositionWarning {
    pub kind: &'static str,
    pub detail: String,
}

/// Compose a set of prediction-market records into a validated `EventTree`.
///
/// Pipeline: per-record conversion (the existing `convert_market_record`
/// gates apply to each market) → caller-specified dependency wiring →
/// overlap diagnostics → `build_event_tree` (validation, cycle detection,
/// marginalization).
///
/// Dependency inference is deliberately NOT automatic: question overlap can
/// suggest *relatedness* but cannot determine the direction or strength of
/// causation, so `depends_on` edges come from the caller's `dependency_specs`.
/// Overlap above `DUPLICATE_OVERLAP_THRESHOLD` is flagged as a likely
/// duplicate (wiring two records of the same event into a tree double-counts
/// the signal).
pub fn compose_market_tree(
    records: &[hkask_mcp_prediction_markets::types::MarketRecord],
    match_confidences: &[Option<String>],
    dependency_specs: &[DependencySpec],
    store: Option<&ForecastStore>,
) -> Result<(EventTree, Vec<CompositionWarning>), ScenarioError> {
    if records.is_empty() {
        return Err(ScenarioError::EmptyInput(
            "compose_market_tree requires at least one market record".into(),
        ));
    }
    if match_confidences.len() != records.len() {
        return Err(ScenarioError::EmptyInput(format!(
            "match_confidences length {} must equal records length {} (use None per entry for direct lookups)",
            match_confidences.len(),
            records.len()
        )));
    }

    let mut warnings: Vec<CompositionWarning> = Vec::new();

    // 1. Convert each record through the existing gated bridge.
    let mut events: Vec<ScenarioEvent> = Vec::with_capacity(records.len());
    let mut seen_ids: HashSet<String> = HashSet::new();
    for (record, confidence) in records.iter().zip(match_confidences.iter()) {
        let (event, record_warnings) = convert_market_record(record, confidence.as_deref(), store)?;
        for warning in record_warnings {
            warnings.push(CompositionWarning {
                kind: "bridge_gate",
                detail: format!("{}: {warning}", record.market_id),
            });
        }
        if !seen_ids.insert(event.id.clone()) {
            return Err(ScenarioError::InvalidDependency(
                event.id,
                "duplicate market_id in record set — each market may appear once".into(),
            ));
        }
        events.push(event);
    }

    // 2. Wire caller-specified dependencies.
    for spec in dependency_specs {
        if spec.parent_market_ids.len() > MAX_PARENTS_PER_GROUP {
            return Err(ScenarioError::InvalidDependency(
                spec.child_market_id.clone(),
                format!(
                    "{} parents exceeds the CPT size cap of {MAX_PARENTS_PER_GROUP} — split into multiple groups or respecify the tree",
                    spec.parent_market_ids.len()
                ),
            ));
        }
        let child_id = format!("mkt-{}", spec.child_market_id);
        let parent_ids: Vec<String> = spec
            .parent_market_ids
            .iter()
            .map(|id| format!("mkt-{id}"))
            .collect();
        for parent_id in &parent_ids {
            if !seen_ids.contains(parent_id) {
                #[allow(clippy::redundant_clone)]
                // child_id is used after the loop; the clone is only redundant on this exit path
                return Err(ScenarioError::UnknownParent(
                    child_id.clone(),
                    parent_id.clone(),
                ));
            }
        }
        let child = events
            .iter_mut()
            .find(|e| e.id == child_id)
            .ok_or_else(|| ScenarioError::EventNotFound(child_id.clone()))?;
        child.depends_on.push(crate::types::EventDependency {
            parent_event_ids: parent_ids,
            conditionals: spec.conditionals.clone(),
        });
    }

    // 3. Overlap diagnostics (deterministic, matcher.rs machinery).
    for (i, a) in records.iter().enumerate() {
        for b in records.iter().skip(i + 1) {
            let overlap =
                hkask_mcp_prediction_markets::matcher::token_overlap(&a.question, &b.question);
            if overlap >= DUPLICATE_OVERLAP_THRESHOLD {
                warnings.push(CompositionWarning {
                    kind: "possible_duplicate",
                    detail: format!(
                        "questions of {} and {} overlap at {overlap:.2} — likely the same underlying event; wiring both double-counts the signal",
                        a.market_id, b.market_id
                    ),
                });
            }
        }
    }

    // 4. Build the tree (validation, cycle detection, marginalization).
    let tree = build_event_tree(&events)?;
    Ok((tree, warnings))
}

// ── R1: Composition over CMP inputs ─────────────────────────────────────────
//
// Re-points the composition machinery at CMP index probabilities instead of
// raw contract probabilities. A CMP index is a constant-maturity, constant-
// orientation synthetic portfolio — its probability is controlled (the time
// axis is taken out), so it's the right input for scenario trees. The tree
// cites the index (family, orientation, tenor, venue), not a decaying contract.

/// Convert a CMP index into a ScenarioEvent for tree composition.
///
/// The CMP index probability becomes the event's prior. Unlike
/// `convert_market_record`, no domain-bias correction or reliability-tier
/// gating is applied — the CMP index is already a controlled, portfolio-weighted
/// probability with its own reliability floor and construction method surfaced
/// in the provenance. The event ID is `cmp:{family}:{tenor}:{orientation}` —
/// the index identity, not a decaying contract ID.
///
/// `observation_date` is the date the CMP index was built (the "today" of the
/// index). The event deadline is `observation_date + target_maturity_days` —
/// the honest deadline for the constant-maturity target, not a fabricated
/// placeholder.
pub fn convert_cmp_index(
    index: &hkask_mcp_prediction_markets::cmp_index_builder::ProvenancedCmpIndex,
    observation_date: chrono::NaiveDate,
) -> ScenarioEvent {
    use hkask_mcp_prediction_markets::cmp::CmpMethod;

    let family_label = index.family.label();
    let tenor = index.index.bucket.label();
    let orientation = index.index.orientation.to_string();
    let venue = index.venue.to_string();
    let method = match index.index.portfolio.method {
        CmpMethod::Interpolated => "interpolated",
        CmpMethod::BucketedSparse => "bucketed_sparse",
    };
    let id = format!("cmp:{family_label}:{tenor}:{orientation}");
    let name = format!("{family_label} {tenor} {orientation} ({venue}, {method})");
    let question = format!(
        "CMP index: {family_label} {orientation} at {tenor} forward maturity, \
         venue={venue}, method={method}, p={:.3}, maturity_error={:.1}d, constituents={}",
        index.index.portfolio.index_probability,
        index.index.portfolio.maturity_error_days,
        index.index.portfolio.constituents.len()
    );
    // The deadline is the observation date + the target maturity — the honest
    // deadline for the constant-maturity target. The CMP index is a rolling
    // synthetic, so this deadline advances with each observation.
    let target_days = index.index.bucket.target_days() as i64;
    let deadline = observation_date + chrono::Duration::days(target_days);
    let probability = index.index.portfolio.index_probability;
    ScenarioEvent {
        id,
        name,
        question,
        deadline,
        time_horizon: TimeHorizon::Strategic,
        scenario_type: ScenarioType::EmergingEconomic,
        subject: family_label.to_string(),
        probability,
        basis: Some(format!("cmp_index:{method}")),
        depends_on: vec![],
        sub_questions: vec![],
        base_rate: Some(probability),
        reference_class: Some(format!(
            "CMP {family_label} {orientation} {tenor} ({venue}); \
             method={method}, maturity_error={:.1}d, constituents={}",
            index.index.portfolio.maturity_error_days,
            index.index.portfolio.constituents.len()
        )),
        brier_score: None,
        update_count: 0,
    }
}

/// Compose a set of CMP indices into an EventTree (R1).
///
/// Each CMP index becomes a root ScenarioEvent with its index probability as
/// the prior. The tree cites the index (family, orientation, tenor, venue) in
/// the provenance — not a decaying contract. This is the re-pointed composition
/// path: same tree machinery, CMP-controlled inputs.
///
/// `observation_date` is the date the CMP indices were built. Each event's
/// deadline is `observation_date + target_maturity_days` — the honest deadline.
///
/// CMP indices are independent root events (no caller-authored dependencies
/// in the initial implementation — the tree is a flat set of CMP priors).
/// Dependency edges between CMP indices (e.g. "oil price increase → inflation
/// increase") are a future refinement (R5 coherence analysis); for now the
/// tree is a flat prior set that downstream tools (scenario_analysis,
/// scenario_propagate) consume.
pub fn compose_cmp_tree(
    indices: &[hkask_mcp_prediction_markets::cmp_index_builder::ProvenancedCmpIndex],
    observation_date: chrono::NaiveDate,
) -> Result<EventTree, ScenarioError> {
    if indices.is_empty() {
        return Err(ScenarioError::EmptyInput(
            "compose_cmp_tree requires at least one CMP index".into(),
        ));
    }
    let events: Vec<ScenarioEvent> = indices
        .iter()
        .map(|idx| convert_cmp_index(idx, observation_date))
        .collect();
    // Check for duplicate IDs (same family/tenor/orientation from different venues).
    let mut seen = HashSet::new();
    for event in &events {
        if !seen.insert(event.id.clone()) {
            return Err(ScenarioError::InvalidDependency(
                event.id.clone(),
                "duplicate CMP index (same family/tenor/orientation) — \
                 merge venues or filter before composing"
                    .into(),
            ));
        }
    }
    build_event_tree(&events)
}

/// A caller-authored dependency edge between CMP indices.
///
/// `child_id` and `parent_ids` use the CMP index ID format
/// `cmp:{family}:{tenor}:{orientation}` — the same format `convert_cmp_index`
/// generates. The caller identifies the CMP indices by their (family, tenor,
/// orientation) triple and supplies the conditional probability table.
///
/// The conditionals are P(child | parent truth assignment), bitmap-ordered,
/// length 2^parent_ids.len(). The server validates structure but never invents
/// conditional probabilities — the caller authors them.
#[derive(Debug, Clone)]
pub struct CmpDependencySpec {
    /// The child CMP index ID: `cmp:{family}:{tenor}:{orientation}`.
    pub child_id: String,
    /// The parent CMP index IDs.
    pub parent_ids: Vec<String>,
    /// P(child | parent truth assignment), bitmap-ordered.
    pub conditionals: Vec<f64>,
}

/// Compose a set of CMP indices into an EventTree with caller-authored
/// dependency edges (R1 + H3 joint coherence support).
///
/// This is the extended version of `compose_cmp_tree` that supports dependency
/// edges between CMP indices — e.g. "oil price increase → inflation increase."
/// The dependency edges enable the H3 joint coherence test: the tree-implied
/// joint P(A ∧ B) can be compared against a parlay contract price.
///
/// `observation_date` is the date the CMP indices were built.
/// `dependency_specs` are caller-authored edges between CMP index IDs. Omit for
/// a flat (independent) tree — same as `compose_cmp_tree`.
pub fn compose_cmp_tree_with_deps(
    indices: &[hkask_mcp_prediction_markets::cmp_index_builder::ProvenancedCmpIndex],
    observation_date: chrono::NaiveDate,
    dependency_specs: &[CmpDependencySpec],
) -> Result<EventTree, ScenarioError> {
    if indices.is_empty() {
        return Err(ScenarioError::EmptyInput(
            "compose_cmp_tree_with_deps requires at least one CMP index".into(),
        ));
    }
    let mut events: Vec<ScenarioEvent> = indices
        .iter()
        .map(|idx| convert_cmp_index(idx, observation_date))
        .collect();
    // Check for duplicate IDs.
    let seen: HashSet<String> = events.iter().map(|e| e.id.clone()).collect();
    // Wire caller-specified dependencies.
    for spec in dependency_specs {
        if spec.parent_ids.len() > MAX_PARENTS_PER_GROUP {
            return Err(ScenarioError::InvalidDependency(
                spec.child_id.clone(),
                format!(
                    "{} parents exceeds the CPT size cap of {MAX_PARENTS_PER_GROUP}",
                    spec.parent_ids.len()
                ),
            ));
        }
        for parent_id in &spec.parent_ids {
            if !seen.contains(parent_id) {
                return Err(ScenarioError::UnknownParent(
                    spec.child_id.clone(),
                    parent_id.clone(),
                ));
            }
        }
        let child = events
            .iter_mut()
            .find(|e| e.id == spec.child_id)
            .ok_or_else(|| ScenarioError::EventNotFound(spec.child_id.clone()))?;
        child.depends_on.push(crate::types::EventDependency {
            parent_event_ids: spec.parent_ids.clone(),
            conditionals: spec.conditionals.clone(),
        });
    }
    build_event_tree(&events)
}

// ── Tree-level Bayesian propagation (T5) ────────────────────────────────────

/// One step in a propagation journal: a node's marginal before and after a
/// prior update elsewhere in the tree. The journal is the tâtonnement record
/// (T10): each entry is one round of the market's one-step-ahead adjustment
/// (Bhattacharya Prop. 6, arXiv:2211.03244 — see t0-keystone-mapping.md §3).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PropagationEntry {
    pub event_id: String,
    pub marginal_before: f64,
    pub marginal_after: f64,
    pub delta: f64,
}

/// Result of updating one node's prior and propagating through the tree.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PropagationResult {
    /// The updated tree (all marginals recomputed).
    pub tree: EventTree,
    /// Every node whose marginal changed (including the updated node itself),
    /// in topological order.
    pub journal: Vec<PropagationEntry>,
    /// Joint probability before and after.
    pub joint_before: f64,
    pub joint_after: f64,
}

/// Update one event's prior probability and propagate the change through the
/// tree: every descendant marginal and the joint are recomputed.
///
/// This closes the gap identified at territory-map C30 (scalar Bayes only):
/// `scenario_update` revises a probability in isolation; this function
/// recomputes the whole tree so downstream consumers (scenario-weighted
/// valuation, factor loadings) always read a coherent joint.
///
/// The update sets the node's *prior* (its stored `probability`); CPTs are
/// untouched — conditioning structure is caller-authored and stable under
/// evidence revision. Nodes not reachable from the updated node are
/// unaffected but are re-validated with the tree (cheap, and keeps one
/// validation path).
pub fn propagate_prior_update(
    events: &[ScenarioEvent],
    updated_event_id: &str,
    new_prior: f64,
) -> Result<PropagationResult, ScenarioError> {
    if !new_prior.is_finite() || !(0.0..=1.0).contains(&new_prior) {
        return Err(ScenarioError::InvalidProbability(
            updated_event_id.to_string(),
            new_prior,
        ));
    }

    // Baseline tree (before).
    let tree_before = build_event_tree(events)?;
    let marginal_before: HashMap<String, f64> = tree_before
        .nodes
        .iter()
        .map(|n| (n.event.id.clone(), n.marginal_probability))
        .collect();

    // Apply the prior update.
    let mut updated_events = events.to_vec();
    let target = updated_events
        .iter_mut()
        .find(|e| e.id == updated_event_id)
        .ok_or_else(|| ScenarioError::EventNotFound(updated_event_id.to_string()))?;
    target.probability = new_prior;
    target.update_count += 1;

    // Rebuilt tree (after).
    let tree_after = build_event_tree(&updated_events)?;

    let mut journal: Vec<PropagationEntry> = Vec::new();
    for node in &tree_after.nodes {
        let before = marginal_before
            .get(&node.event.id)
            .copied()
            .unwrap_or(node.marginal_probability);
        let delta = node.marginal_probability - before;
        if delta.abs() > 1e-12 {
            journal.push(PropagationEntry {
                event_id: node.event.id.clone(),
                marginal_before: before,
                marginal_after: node.marginal_probability,
                delta,
            });
        }
    }

    Ok(PropagationResult {
        joint_before: tree_before.joint_probability,
        joint_after: tree_after.joint_probability,
        tree: tree_after,
        journal,
    })
}

#[cfg(test)]
fn test_market_record(
    category: &str,
    probability: f64,
    tier: hkask_mcp_prediction_markets::types::ReliabilityTier,
) -> hkask_mcp_prediction_markets::types::MarketRecord {
    use hkask_mcp_prediction_markets::types::*;
    use std::borrow::Cow;
    MarketRecord {
        source: Source::Kalshi,
        event_id: "KXFED-27DEC".into(),
        market_id: "KXFED-27DEC-H0".into(),
        question: "Will the Fed hold rates in December 2027?".into(),
        description: "Resolves per the Federal Reserve statement.".into(),
        category: category.into(),
        series: "KXFEDDECISION".into(),
        deadline: "2027-12-08T18:59:00Z".into(),
        time_to_maturity: None,
        probability,
        probability_method: ProbabilityMethod::Midpoint,
        spread: Some(0.02),
        volume: 250_000.0,
        volume_grain: VolumeGrain::Market,
        liquidity: Some(10_000.0),
        open_interest: Some(1_500.0),
        last_update: "2026-08-05T00:00:00Z".into(),
        volatility: Volatility {
            realized_variance: None,
            structural_flag: StructuralFlag::None,
            interpretation: Cow::Borrowed("low"),
            dras_forecast: None,
        },
        status: MarketStatus::Open,
        resolved_outcome: None,
        resolution_source: Cow::Borrowed("kalshi_exchange"),
        calibration: Calibration {
            brier: None,
            domain_bias: None,
            bias_source: std::borrow::Cow::Borrowed("none"),
            sample_size: 0,
            stale: true,
        },
        reliability_tier: tier,
        ontology: OntologyBlock {
            process: ProcessAxis {
                r#type: Cow::Borrowed("pko:ProcedureExecution"),
                stage: Cow::Borrowed("trading"),
                probability_role: Cow::Borrowed("pko:StepExecution.output"),
            },
            state: StateAxis {
                identifier: "kalshi:KXFED-27DEC-H0".into(),
                title: "t".into(),
                description: "d".into(),
                temporal: "2027-12-08T18:59:00Z".into(),
                provenance: Cow::Borrowed("kalshi_exchange"),
            },
            mapping_version: 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CertaintyTier;
    use crate::types::StoredForecastRecord;
    use hkask_mcp_prediction_markets::types::ReliabilityTier;

    #[test]
    fn politics_record_passes_through_uncorrected_without_store() {
        // No store ⇒ no measured calibration ⇒ δ=0.0 (identity). The honest
        // default per Tetlock: corrections come from measured calibration,
        // not hardcoded magic numbers. The old hardcoded δ=0.3 for politics
        // was removed — it was a magic number with no enforcement point.
        let record = test_market_record("Elections", 0.62, ReliabilityTier::High);
        let (event, warnings) =
            convert_market_record(&record, Some("high"), None).expect("converts");
        let base = event.base_rate.expect("high reliability anchors");
        assert!((base - 0.62).abs() < 1e-12, "δ=0 is identity: {base}");
        assert!(
            warnings
                .iter()
                .all(|w| !w.contains("domain-bias correction")),
            "no correction warning when δ=0"
        );
    }

    #[test]
    fn domain_bias_delta_zero_when_store_none() {
        assert_eq!(domain_bias_delta(None, "Elections"), 0.0);
    }

    #[test]
    fn domain_bias_delta_zero_when_insufficient_samples() {
        // MIN_DOMAIN_SAMPLE = 5; 4 resolved forecasts is below the threshold.
        let mut store = ForecastStore::default();
        for i in 0..4 {
            store.insert(
                format!("f{i}"),
                StoredForecastRecord {
                    schema_version: 2,
                    forecast_id: format!("f{i}"),
                    event_id: format!("e{i}"),
                    event_name: format!("e{i}"),
                    subject: "test".into(),
                    probability: 0.7,
                    created_at: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    outcome: Some(false),
                    resolved_at: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                    category: Some("Elections".into()),
                },
            );
        }
        assert_eq!(domain_bias_delta(Some(&store), "Elections"), 0.0);
    }

    #[test]
    fn domain_bias_delta_data_derived_when_underconfident() {
        // 6 resolved forecasts at p=0.3, all hit (outcome=true). The domain
        // is underconfident (forecasts say 30% but reality is 100%).
        // bias = expected − hit_rate = 0.3 − 1.0 = −0.7 (negative = underconfident).
        // δ = |bias| = 0.7, clamped to 0.5. De-compression corrects
        // underconfidence by moving probabilities away from 0.5.
        let mut store = ForecastStore::default();
        for i in 0..6 {
            store.insert(
                format!("f{i}"),
                StoredForecastRecord {
                    schema_version: 2,
                    forecast_id: format!("f{i}"),
                    event_id: format!("e{i}"),
                    event_name: format!("e{i}"),
                    subject: "test".into(),
                    probability: 0.3,
                    created_at: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    outcome: Some(true),
                    resolved_at: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                    category: Some("Elections".into()),
                },
            );
        }
        let delta = domain_bias_delta(Some(&store), "Elections");
        assert!(delta > 0.0, "underconfident domain must get δ > 0: {delta}");
        assert!(delta <= 0.5, "δ must be clamped to 0.5: {delta}");
    }

    #[test]
    fn domain_bias_delta_zero_when_overconfident() {
        // 6 resolved forecasts at p=0.7, all missed (outcome=false). The
        // domain is overconfident (forecasts say 70% but reality is 0%).
        // bias = expected − hit_rate = 0.7 − 0.0 = 0.7 (positive = overconfident).
        // De-compression would make overconfidence worse, so δ=0.0.
        let mut store = ForecastStore::default();
        for i in 0..6 {
            store.insert(
                format!("f{i}"),
                StoredForecastRecord {
                    schema_version: 2,
                    forecast_id: format!("f{i}"),
                    event_id: format!("e{i}"),
                    event_name: format!("e{i}"),
                    subject: "test".into(),
                    probability: 0.7,
                    created_at: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    outcome: Some(false),
                    resolved_at: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                    category: Some("Elections".into()),
                },
            );
        }
        assert_eq!(domain_bias_delta(Some(&store), "Elections"), 0.0);
    }

    #[test]
    fn economics_record_passes_through_uncorrected() {
        let record = test_market_record("Economics", 0.62, ReliabilityTier::High);
        let (event, _) = convert_market_record(&record, Some("high"), None).expect("converts");
        let base = event.base_rate.expect("anchors");
        assert!((base - 0.62).abs() < 1e-12, "δ=0 is identity");
    }

    #[test]
    fn low_reliability_withholds_base_rate() {
        let record = test_market_record("Economics", 0.62, ReliabilityTier::Low);
        let (event, warnings) =
            convert_market_record(&record, Some("high"), None).expect("converts");
        assert_eq!(event.base_rate, None);
        assert!(warnings.iter().any(|w| w.contains("reliability")));
    }

    #[test]
    fn low_match_confidence_withholds_base_rate() {
        let record = test_market_record("Economics", 0.62, ReliabilityTier::High);
        let (event, warnings) =
            convert_market_record(&record, Some("low"), None).expect("converts");
        assert_eq!(event.base_rate, None);
        assert!(warnings.iter().any(|w| w.contains("match confidence")));
    }

    #[test]
    fn resolved_market_is_rejected() {
        let mut record = test_market_record("Economics", 0.62, ReliabilityTier::High);
        record.status = hkask_mcp_prediction_markets::types::MarketStatus::Resolved;
        assert!(convert_market_record(&record, Some("high"), None).is_err());
    }

    #[test]
    fn event_carries_market_provenance() {
        let record = test_market_record("Economics", 0.62, ReliabilityTier::High);
        let (event, _) = convert_market_record(&record, None, None).expect("converts");
        assert!(
            event
                .basis
                .as_deref()
                .unwrap_or("")
                .contains("prediction_market")
        );
        assert!(
            event
                .reference_class
                .as_deref()
                .unwrap_or("")
                .contains("KXFEDDECISION")
        );
        assert_eq!(
            event.deadline,
            chrono::NaiveDate::from_ymd_opt(2027, 12, 8).unwrap()
        );
    }
    use crate::types::EventDependency;

    // ── T4a composition tests ────────────────────────────────────────────

    fn record_with(
        id: &str,
        question: &str,
        probability: f64,
    ) -> hkask_mcp_prediction_markets::types::MarketRecord {
        let mut record = test_market_record("Economics", probability, ReliabilityTier::High);
        record.market_id = id.into();
        record.question = question.into();
        record
    }

    #[test]
    fn compose_flat_tree_matches_independent_marginals() {
        // No dependency specs: every event is a root; marginals equal the
        // (gated) record probabilities; joint = product of roots.
        let records = vec![
            record_with("M1", "Will the Fed cut rates in January 2027?", 0.60),
            record_with("M2", "Will CPI exceed 3 percent in 2027?", 0.40),
        ];
        let (tree, warnings) =
            compose_market_tree(&records, &[None, None], &[], None).expect("composes");
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.root_ids.len(), 2);
        for node in &tree.nodes {
            let expected = if node.event.id == "mkt-M1" {
                0.60
            } else {
                0.40
            };
            assert!(
                (node.marginal_probability - expected).abs() < 1e-12,
                "root marginal must equal the record probability"
            );
        }
        assert!((tree.joint_probability - 0.24).abs() < 1e-12);
        assert!(warnings.is_empty());
    }

    #[test]
    fn compose_dependent_tree_marginalizes_like_compute_marginal_probabilities() {
        // M2 conditioned on M1: P(M2|M1)=0.9, P(M2|¬M1)=0.2.
        // Marginal: 0.9·0.6 + 0.2·0.4 = 0.62. Joint factor: 0.6 · 0.9.
        let records = vec![
            record_with("M1", "Will the Fed cut rates in January 2027?", 0.60),
            record_with("M2", "Will bank stocks rally in 2027?", 0.50),
        ];
        let specs = vec![DependencySpec {
            child_market_id: "M2".into(),
            parent_market_ids: vec!["M1".into()],
            conditionals: vec![0.2, 0.9],
        }];
        let (tree, _) =
            compose_market_tree(&records, &[None, None], &specs, None).expect("composes");
        let child = tree
            .nodes
            .iter()
            .find(|n| n.event.id == "mkt-M2")
            .expect("child present");
        assert!((child.marginal_probability - 0.62).abs() < 1e-12);
        assert_eq!(tree.root_ids, vec!["mkt-M1".to_string()]);
        assert!((tree.joint_probability - 0.54).abs() < 1e-12);
    }

    #[test]
    fn compose_rejects_cycle() {
        let records = vec![
            record_with("M1", "Will the Fed cut rates in January 2027?", 0.60),
            record_with("M2", "Will bank stocks rally in 2027?", 0.50),
        ];
        let specs = vec![
            DependencySpec {
                child_market_id: "M2".into(),
                parent_market_ids: vec!["M1".into()],
                conditionals: vec![0.2, 0.9],
            },
            DependencySpec {
                child_market_id: "M1".into(),
                parent_market_ids: vec!["M2".into()],
                conditionals: vec![0.3, 0.8],
            },
        ];
        let result = compose_market_tree(&records, &[None, None], &specs, None);
        assert!(matches!(result, Err(ScenarioError::CycleDetected)));
    }

    #[test]
    fn compose_rejects_oversized_cpt() {
        let records = vec![record_with("M1", "Will the Fed cut rates in 2027?", 0.5)];
        let specs = vec![DependencySpec {
            child_market_id: "M1".into(),
            parent_market_ids: (0..5).map(|i| format!("P{i}")).collect(),
            conditionals: vec![0.5; 32],
        }];
        let result = compose_market_tree(&records, &[None], &specs, None);
        assert!(matches!(result, Err(ScenarioError::InvalidDependency(..))));
    }

    #[test]
    fn compose_rejects_unknown_parent() {
        let records = vec![record_with("M1", "Will the Fed cut rates in 2027?", 0.5)];
        let specs = vec![DependencySpec {
            child_market_id: "M1".into(),
            parent_market_ids: vec!["GHOST".into()],
            conditionals: vec![0.4, 0.7],
        }];
        let result = compose_market_tree(&records, &[None], &specs, None);
        assert!(matches!(result, Err(ScenarioError::UnknownParent(..))));
    }

    #[test]
    fn compose_flags_duplicate_questions() {
        let records = vec![
            record_with("M1", "Will the Fed hold rates in December 2027?", 0.60),
            record_with("M2", "Will the Fed hold rates in December 2027?", 0.62),
        ];
        let (_, warnings) =
            compose_market_tree(&records, &[None, None], &[], None).expect("composes");
        assert!(
            warnings.iter().any(|w| w.kind == "possible_duplicate"),
            "identical questions must be flagged: {warnings:?}"
        );
    }

    #[test]
    fn compose_applies_per_record_gates() {
        // Low-reliability record: base_rate withheld, warning surfaced, but
        // the record still converts (probability falls back to 0.5).
        let mut low = record_with("M2", "Will oil exceed 100 dollars in 2027?", 0.70);
        low.reliability_tier = ReliabilityTier::Low;
        let records = vec![
            record_with("M1", "Will the Fed cut rates in 2027?", 0.60),
            low,
        ];
        let (tree, warnings) =
            compose_market_tree(&records, &[None, None], &[], None).expect("composes");
        let gated = tree
            .nodes
            .iter()
            .find(|n| n.event.id == "mkt-M2")
            .expect("present");
        assert_eq!(gated.event.base_rate, None);
        assert!(warnings.iter().any(|w| w.kind == "bridge_gate"));
    }

    // ── T5 propagation tests ─────────────────────────────────────────────

    #[test]
    fn propagation_recomputes_descendants_and_joint() {
        // M1 (root, 0.6) → M2 conditioned [0.2, 0.9]. Update M1's prior to
        // 0.9: M2's marginal must move from 0.62 to 0.9·0.9 + 0.1·0.2 = 0.83,
        // and the joint from 0.54 to 0.81.
        let root = make_event("E1", 0.6, vec![]);
        let child = make_event(
            "E2",
            0.5,
            vec![EventDependency {
                parent_event_ids: vec!["E1".into()],
                conditionals: vec![0.2, 0.9],
            }],
        );
        let events = vec![root, child];

        let result = propagate_prior_update(&events, "E1", 0.9).expect("propagates");

        let child_after = result
            .tree
            .nodes
            .iter()
            .find(|n| n.event.id == "E2")
            .expect("child present");
        assert!((child_after.marginal_probability - 0.83).abs() < 1e-12);
        assert!((result.joint_before - 0.54).abs() < 1e-12);
        assert!((result.joint_after - 0.81).abs() < 1e-12);

        // Journal: both nodes changed, in topo order.
        assert_eq!(result.journal.len(), 2);
        assert_eq!(result.journal[0].event_id, "E1");
        assert!((result.journal[0].marginal_before - 0.6).abs() < 1e-12);
        assert!((result.journal[0].marginal_after - 0.9).abs() < 1e-12);
        assert_eq!(result.journal[1].event_id, "E2");
        assert!((result.journal[1].delta - 0.21).abs() < 1e-12);
    }

    #[test]
    fn propagation_leaves_unrelated_nodes_untouched() {
        let root = make_event("E1", 0.6, vec![]);
        let child = make_event(
            "E2",
            0.5,
            vec![EventDependency {
                parent_event_ids: vec!["E1".into()],
                conditionals: vec![0.2, 0.9],
            }],
        );
        let independent = make_event("E3", 0.7, vec![]);
        let events = vec![root, child, independent];

        let result = propagate_prior_update(&events, "E1", 0.9).expect("propagates");
        assert!(
            result.journal.iter().all(|e| e.event_id != "E3"),
            "independent root must not appear in the journal"
        );
        let e3 = result
            .tree
            .nodes
            .iter()
            .find(|n| n.event.id == "E3")
            .expect("present");
        assert!((e3.marginal_probability - 0.7).abs() < 1e-12);
    }

    #[test]
    fn propagation_rejects_invalid_prior_and_unknown_event() {
        let events = vec![make_event("E1", 0.6, vec![])];
        assert!(matches!(
            propagate_prior_update(&events, "E1", 1.5),
            Err(ScenarioError::InvalidProbability(..))
        ));
        assert!(matches!(
            propagate_prior_update(&events, "GHOST", 0.5),
            Err(ScenarioError::EventNotFound(..))
        ));
    }

    #[test]
    fn propagation_noop_update_yields_empty_journal() {
        let events = vec![make_event("E1", 0.6, vec![])];
        let result = propagate_prior_update(&events, "E1", 0.6).expect("propagates");
        assert!(result.journal.is_empty());
        assert!((result.joint_before - result.joint_after).abs() < 1e-12);
    }

    #[test]
    fn compose_rejects_duplicate_market_ids() {
        let records = vec![
            record_with("M1", "Will the Fed cut rates in 2027?", 0.60),
            record_with("M1", "Will something else happen in 2027?", 0.40),
        ];
        let result = compose_market_tree(&records, &[None, None], &[], None);
        assert!(matches!(result, Err(ScenarioError::InvalidDependency(..))));
    }

    fn make_event(id: &str, prob: f64, deps: Vec<EventDependency>) -> ScenarioEvent {
        ScenarioEvent {
            id: id.into(),
            name: format!("Event {}", id),
            question: format!("Will {} occur?", id),
            deadline: chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
            time_horizon: TimeHorizon::Strategic,
            scenario_type: ScenarioType::CompanyAnalysis,
            subject: "TEST".into(),
            probability: prob,
            basis: None,
            depends_on: deps,
            sub_questions: vec![],
            base_rate: None,
            reference_class: None,
            brier_score: None,
            update_count: 0,
        }
    }

    #[test]
    fn test_calibrate_from_fermi_simple() {
        let sqs = vec![
            SubQuestion {
                question: "a".into(),
                estimate: 0.8,
                confidence: 0.9,
            },
            SubQuestion {
                question: "b".into(),
                estimate: 0.2,
                confidence: 0.1,
            },
        ];
        let result = calibrate_from_fermi(&sqs).unwrap();
        assert!((result - 0.74).abs() < 0.001);
    }

    #[test]
    fn test_calibrate_empty_returns_neutral() {
        assert_eq!(calibrate_from_fermi(&[]).unwrap(), 0.5);
    }

    #[test]
    fn test_calibrate_nan_rejected() {
        let sqs = vec![SubQuestion {
            question: "nan".into(),
            estimate: f64::NAN,
            confidence: 0.5,
        }];
        let result = calibrate_from_fermi(&sqs);
        assert!(result.is_err());
    }

    #[test]
    fn test_calibrate_inf_rejected() {
        let sqs = vec![SubQuestion {
            question: "inf".into(),
            estimate: f64::INFINITY,
            confidence: 0.5,
        }];
        let result = calibrate_from_fermi(&sqs);
        assert!(result.is_err());
    }

    #[test]
    fn test_outside_view_high_reference_count() {
        let (prob, conf) = outside_view_adjustment(0.7, 0.3, 1000);
        assert!(prob > 0.6);
        assert!(conf > 0.7);
    }

    #[test]
    fn test_outside_view_low_reference_count() {
        let (prob, _conf) = outside_view_adjustment(0.9, 0.5, 1);
        assert!((prob - 0.55).abs() < 0.01);
    }

    #[test]
    fn test_bayesian_update_positive_evidence() {
        let posterior = bayesian_update(0.3, 0.9, 0.3);
        assert!((posterior - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_bayesian_update_with_negative_evidence() {
        let posterior = bayesian_update(0.7, 0.1, 0.4);
        assert!((posterior - 0.175).abs() < 0.01);
    }

    #[test]
    fn test_brier_perfect() {
        assert_eq!(brier_score(1.0, true), 0.0);
        assert_eq!(brier_score(0.0, false), 0.0);
    }

    #[test]
    fn test_brier_worst() {
        assert_eq!(brier_score(0.0, true), 1.0);
        assert_eq!(brier_score(1.0, false), 1.0);
    }

    #[test]
    fn test_brier_mid() {
        assert_eq!(brier_score(0.5, true), 0.25);
        assert_eq!(brier_score(0.5, false), 0.25);
    }

    #[test]
    fn test_brier_multi_ok() {
        let result = brier_score_multi(&[0.8, 0.2], &[true, false]).unwrap();
        // (0.8-1)^2=0.04, (0.2-0)^2=0.04, avg=0.04
        assert!((result - 0.04).abs() < 0.001);
    }

    #[test]
    fn test_brier_multi_mismatch_err() {
        let result = brier_score_multi(&[0.8], &[true, false]);
        assert!(result.is_err());
    }

    #[test]
    fn test_brier_multi_empty_err() {
        let result = brier_score_multi(&[], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_brier_interpretation_excellent() {
        assert_eq!(brier_interpretation(0.03), "excellent");
    }

    #[test]
    fn test_brier_interpretation_worse() {
        assert_eq!(brier_interpretation(0.5), "worse_than_climatology");
    }

    #[test]
    fn test_event_tree_no_deps() {
        let events = vec![make_event("A", 0.8, vec![]), make_event("B", 0.6, vec![])];
        let tree = build_event_tree(&events).unwrap();
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.root_ids.len(), 2);
        assert!((tree.joint_probability - 0.48).abs() < 0.01);
    }

    #[test]
    fn test_event_tree_with_dependency() {
        let dep = vec![EventDependency {
            parent_event_ids: vec!["A".into()],
            conditionals: vec![0.2, 0.9], // [P(E|not A), P(E|A)]
        }];
        let events = vec![make_event("A", 0.5, vec![]), make_event("B", 0.7, dep)];
        let tree = build_event_tree(&events).unwrap();
        assert_eq!(tree.nodes.len(), 2);
        let b_node = tree.nodes.iter().find(|n| n.event.id == "B").unwrap();
        assert!((b_node.marginal_probability - 0.55).abs() < 0.01);
        assert!((tree.joint_probability - 0.45).abs() < 0.01);
    }

    #[test]
    fn test_event_tree_cycle_detection() {
        let dep_a = vec![EventDependency {
            parent_event_ids: vec!["B".into()],
            conditionals: vec![0.3, 0.8],
        }];
        let dep_b = vec![EventDependency {
            parent_event_ids: vec!["A".into()],
            conditionals: vec![0.3, 0.8],
        }];
        let events = vec![make_event("A", 0.5, dep_a), make_event("B", 0.5, dep_b)];
        let result = build_event_tree(&events);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ScenarioError::CycleDetected));
    }

    #[test]
    fn test_event_tree_multi_parent_independence() {
        // Two independent root events A and B, with child C depending on both.
        // P(A) = 0.5, P(B) = 0.8
        // Bitmap convention: bit j = parent_event_ids[j] (0=A, 1=B)
        //   conditionals[0b00] = P(C | ¬A, ¬B) = 0.05
        //   conditionals[0b01] = P(C |  A, ¬B) = 0.30
        //   conditionals[0b10] = P(C | ¬A,  B) = 0.40
        //   conditionals[0b11] = P(C |  A,  B) = 0.90
        //
        // Under parent independence:
        //   P(¬A,¬B) = 0.10, P(A,¬B) = 0.10, P(¬A,B) = 0.40, P(A,B) = 0.40
        // P(C) = 0.05*0.10 + 0.30*0.10 + 0.40*0.40 + 0.90*0.40 = 0.555
        // Joint P(all) = P(A) * P(B) * P(C | A=true, B=true) = 0.5 * 0.8 * 0.90 = 0.36
        let dep_c = vec![EventDependency {
            parent_event_ids: vec!["A".into(), "B".into()],
            // bitmap: 00=¬A¬B, 01=A¬B, 10=¬AB, 11=AB
            conditionals: vec![0.05, 0.30, 0.40, 0.90],
        }];
        let events = vec![
            make_event("A", 0.5, vec![]),
            make_event("B", 0.8, vec![]),
            make_event("C", 0.3, dep_c),
        ];
        let tree = build_event_tree(&events).unwrap();
        assert_eq!(tree.nodes.len(), 3);
        assert_eq!(tree.root_ids.len(), 2);

        let c_node = tree.nodes.iter().find(|n| n.event.id == "C").unwrap();
        let expected_marginal = 0.555;
        assert!(
            (c_node.marginal_probability - expected_marginal).abs() < 0.001,
            "P(C) = {} expected {} under independence",
            c_node.marginal_probability,
            expected_marginal
        );

        let expected_joint = 0.36;
        assert!(
            (tree.joint_probability - expected_joint).abs() < 0.001,
            "joint = {} expected {}",
            tree.joint_probability,
            expected_joint
        );
    }

    #[test]
    fn test_sensitivity_ranking() {
        let events = vec![
            make_event("A", 0.5, vec![]),  // max uncertainty (coin flip)
            make_event("B", 0.99, vec![]), // near certainty
        ];
        let tree = build_event_tree(&events).unwrap();
        let ranked = sensitivity_ranking(&tree);
        assert_eq!(ranked[0].0, "A");
        assert_eq!(ranked[1].0, "B");
    }

    #[test]
    fn test_validate_nan_rejected() {
        let event = make_event("A", f64::NAN, vec![]);
        let result = event.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_inf_rejected() {
        let event = make_event("A", f64::INFINITY, vec![]);
        let result = event.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_wrong_conditionals_length_rejected() {
        // 2 parents require 4 conditionals, but we provide only 3
        let dep = EventDependency {
            parent_event_ids: vec!["A".into(), "B".into()],
            conditionals: vec![0.1, 0.3, 0.7], // should be length 4
        };
        let event = make_event("C", 0.5, vec![dep]);
        let result = event.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_calibration_bias_uses_mean_forecast_probability() {
        let mut store = ForecastStore::default();
        for (id, probability, outcome) in [
            ("a", 0.81, false),
            ("b", 0.83, false),
            ("c", 0.85, false),
            ("d", 0.87, false),
            ("e", 0.89, false),
        ] {
            store.insert(
                id.into(),
                StoredForecastRecord {
                    schema_version: 1,
                    forecast_id: "forecast".into(),
                    event_id: id.into(),
                    event_name: id.into(),
                    subject: "test".into(),
                    probability,
                    created_at: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    outcome: Some(outcome),
                    resolved_at: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                    category: None,
                },
            );
        }

        let curve = compute_calibration_curve(&store).unwrap();
        let bin = curve
            .bins
            .iter()
            .find(|bin| bin.forecast_count == 5)
            .unwrap();

        assert!((bin.expected_rate - 0.85).abs() < f64::EPSILON);
        assert!((bin.bias - 0.85).abs() < f64::EPSILON);
        assert!(curve.overconfidence_score > 0.0);
        assert_eq!(curve.interpretation, "systematically_overconfident");
    }

    #[test]
    fn test_auto_update_suggestions_correct_direction() {
        let events = vec![make_event("A", 0.3, vec![])];
        let outcomes = vec![("A".into(), true)]; // event occurred but forecast was 30%
        let suggestions = auto_update_suggestions(&events, &outcomes);
        assert_eq!(suggestions.len(), 1);
        let adj = suggestions[0]["suggested_adjustment"].as_f64().unwrap();
        assert!(adj > 0.0); // should suggest raising probability
    }

    // ── E5: CertaintyTier boundary tests ──────────────────────────────────

    #[test]
    fn test_certainty_tier_exact_boundaries() {
        // Exact boundary values: Proximate ≥ 0.67, Probable in [0.33, 0.67), Possible < 0.33
        assert!(
            matches!(
                CertaintyTier::from_probability(0.67),
                CertaintyTier::Proximate
            ),
            "0.67 should be Proximate boundary (≥ 0.67)"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(0.33),
                CertaintyTier::Probable
            ),
            "0.33 should be Probable (≥ 0.33 ∧ < 0.67)"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(0.329),
                CertaintyTier::Possible
            ),
            "0.329 should be Possible (< 0.33)"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(0.669),
                CertaintyTier::Probable
            ),
            "0.669 should be Probable (< 0.67)"
        );
    }

    #[test]
    fn test_certainty_tier_range() {
        assert_eq!(CertaintyTier::Proximate.range(), "67–100%");
        assert_eq!(CertaintyTier::Probable.range(), "33–66%");
        assert_eq!(CertaintyTier::Possible.range(), "0–32%");
    }

    #[test]
    fn test_certainty_tier_edges() {
        assert!(
            matches!(
                CertaintyTier::from_probability(0.0),
                CertaintyTier::Possible
            ),
            "0.0 should be Possible"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(1.0),
                CertaintyTier::Proximate
            ),
            "1.0 should be Proximate"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(0.5),
                CertaintyTier::Probable
            ),
            "0.5 should be Probable (middle of range)"
        );
    }

    // ── Persistence round-trip tests ──────────────────────────────────────

    #[test]
    fn test_journal_insert_and_reload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("forecasts.json");

        // Phase 1: Insert records
        {
            let mut store = ForecastStore::new(Some(path.clone()));
            store.insert(
                "fcst-1:evt-A".into(),
                StoredForecastRecord {
                    schema_version: 1,
                    forecast_id: "fcst-1".into(),
                    event_id: "evt-A".into(),
                    event_name: "Event A".into(),
                    subject: "TEST".into(),
                    probability: 0.75,
                    created_at: chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                    outcome: None,
                    resolved_at: None,
                    category: None,
                },
            );
            store.insert(
                "fcst-1:evt-B".into(),
                StoredForecastRecord {
                    schema_version: 1,
                    forecast_id: "fcst-1".into(),
                    event_id: "evt-B".into(),
                    event_name: "Event B".into(),
                    subject: "TEST".into(),
                    probability: 0.30,
                    created_at: chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                    outcome: Some(true),
                    resolved_at: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 1).unwrap()),
                    category: None,
                },
            );
            store.force_compact(); // ensure snapshot is written
        }

        // Phase 2: Reload from disk
        {
            let store = ForecastStore::new(Some(path));
            assert_eq!(store.len(), 2, "both records should survive restart");

            let a = store.get("fcst-1:evt-A").expect("evt-A should exist");
            assert_eq!(a.probability, 0.75);
            assert!(a.outcome.is_none());

            let b = store.get("fcst-1:evt-B").expect("evt-B should exist");
            assert_eq!(b.probability, 0.30);
            assert_eq!(b.outcome, Some(true));
            assert_eq!(store.resolved().len(), 1);
        }
    }

    // ── R1: compose_cmp_tree tests ────────────────────────────────────────

    fn cmp_index(
        family: hkask_mcp_prediction_markets::economic_object::BaseEconomicObject,
        venue: hkask_mcp_prediction_markets::cmp_index_builder::Venue,
        bucket: hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket,
        orientation: hkask_mcp_prediction_markets::cmp_portfolio::Orientation,
        probability: f64,
        method: hkask_mcp_prediction_markets::cmp::CmpMethod,
    ) -> hkask_mcp_prediction_markets::cmp_index_builder::ProvenancedCmpIndex {
        use hkask_mcp_prediction_markets::cmp_portfolio::{
            CmpIndex, IndexPortfolio, WeightedConstituent,
        };
        hkask_mcp_prediction_markets::cmp_index_builder::ProvenancedCmpIndex {
            family,
            venue,
            index: CmpIndex {
                bucket,
                orientation,
                portfolio: IndexPortfolio {
                    constituents: vec![WeightedConstituent {
                        market_index: 0,
                        weight: 1.0,
                        days_to_expiration: 90.0,
                        probability,
                    }],
                    weighted_maturity_days: 90.0,
                    maturity_error_days: 0.0,
                    index_probability: probability,
                    method,
                },
            },
        }
    }

    #[test]
    fn compose_cmp_tree_builds_flat_tree_from_indices() {
        let indices = vec![
            cmp_index(
                hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::PolicyInterestRate,
                hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
                hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::ThreeMonth,
                hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
                0.65,
                hkask_mcp_prediction_markets::cmp::CmpMethod::Interpolated,
            ),
            cmp_index(
                hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::CrudeOilPrice,
                hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
                hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::OneMonth,
                hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
                0.40,
                hkask_mcp_prediction_markets::cmp::CmpMethod::BucketedSparse,
            ),
        ];
        let tree = compose_cmp_tree(
            &indices,
            chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
        )
        .expect("tree");
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.root_ids.len(), 2);
        // Provenance: the event IDs cite the index, not a contract.
        assert!(
            tree.nodes
                .iter()
                .any(|n| n.event.id == "cmp:policy_interest_rate:3m:increase")
        );
        assert!(
            tree.nodes
                .iter()
                .any(|n| n.event.id == "cmp:crude_oil_price:1m:increase")
        );
        // Probabilities are the CMP index probabilities.
        let rates = tree
            .nodes
            .iter()
            .find(|n| n.event.subject == "policy_interest_rate")
            .unwrap();
        assert!((rates.marginal_probability - 0.65).abs() < 1e-9);
        let oil = tree
            .nodes
            .iter()
            .find(|n| n.event.subject == "crude_oil_price")
            .unwrap();
        assert!((oil.marginal_probability - 0.40).abs() < 1e-9);
        // Basis records the CMP method.
        assert!(
            rates
                .event
                .basis
                .as_deref()
                .unwrap()
                .contains("interpolated")
        );
        assert!(
            oil.event
                .basis
                .as_deref()
                .unwrap()
                .contains("bucketed_sparse")
        );
    }

    #[test]
    fn compose_cmp_tree_rejects_empty() {
        let result = compose_cmp_tree(&[], chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn compose_cmp_tree_rejects_duplicates() {
        // Same family/tenor/orientation from the same venue → duplicate ID.
        let indices = vec![
            cmp_index(
                hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::PolicyInterestRate,
                hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
                hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::ThreeMonth,
                hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
                0.65,
                hkask_mcp_prediction_markets::cmp::CmpMethod::Interpolated,
            ),
            cmp_index(
                hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::PolicyInterestRate,
                hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
                hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::ThreeMonth,
                hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
                0.70,
                hkask_mcp_prediction_markets::cmp::CmpMethod::Interpolated,
            ),
        ];
        let result = compose_cmp_tree(
            &indices,
            chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn compose_cmp_tree_allows_same_family_different_tenor() {
        // Same family, different tenors → different IDs, no duplicate.
        let indices = vec![
            cmp_index(
                hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::PolicyInterestRate,
                hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
                hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::OneMonth,
                hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
                0.60,
                hkask_mcp_prediction_markets::cmp::CmpMethod::BucketedSparse,
            ),
            cmp_index(
                hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::PolicyInterestRate,
                hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
                hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::SixMonth,
                hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
                0.75,
                hkask_mcp_prediction_markets::cmp::CmpMethod::Interpolated,
            ),
        ];
        let tree = compose_cmp_tree(
            &indices,
            chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
        )
        .expect("tree");
        assert_eq!(tree.nodes.len(), 2);
    }

    #[test]
    fn compose_cmp_tree_with_deps_builds_dependent_tree() {
        // Oil increase (root, p=0.40) → inflation increase (child, conditional).
        // P(inflation increase | oil increase) = 0.70
        // P(inflation increase | oil not increase) = 0.20
        // Marginal P(inflation increase) = 0.70*0.40 + 0.20*0.60 = 0.28 + 0.12 = 0.40
        let indices = vec![
            cmp_index(
                hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::CrudeOilPrice,
                hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
                hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::OneMonth,
                hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
                0.40,
                hkask_mcp_prediction_markets::cmp::CmpMethod::Interpolated,
            ),
            cmp_index(
                hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::ConsumerPriceInflation,
                hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
                hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::ThreeMonth,
                hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
                0.50, // prior — will be overridden by the conditional marginal
                hkask_mcp_prediction_markets::cmp::CmpMethod::Interpolated,
            ),
        ];
        let deps = vec![CmpDependencySpec {
            child_id: "cmp:consumer_price_inflation:3m:increase".into(),
            parent_ids: vec!["cmp:crude_oil_price:1m:increase".into()],
            conditionals: vec![0.20, 0.70], // P(child|parent=false), P(child|parent=true)
        }];
        let tree = compose_cmp_tree_with_deps(
            &indices,
            chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
            &deps,
        )
        .expect("tree");
        assert_eq!(tree.nodes.len(), 2);
        // Oil is the root.
        assert!(
            tree.root_ids
                .contains(&"cmp:crude_oil_price:1m:increase".to_string())
        );
        // Oil marginal = 0.40 (root prior).
        let oil = tree
            .nodes
            .iter()
            .find(|n| n.event.subject == "crude_oil_price")
            .unwrap();
        assert!((oil.marginal_probability - 0.40).abs() < 1e-9);
        // Inflation marginal = 0.70*0.40 + 0.20*0.60 = 0.40.
        let inflation = tree
            .nodes
            .iter()
            .find(|n| n.event.subject == "consumer_price_inflation")
            .unwrap();
        assert!((inflation.marginal_probability - 0.40).abs() < 1e-9);
        // Joint probability = P(oil) * P(inflation|oil) = 0.40 * 0.70 = 0.28.
        assert!((tree.joint_probability - 0.28).abs() < 1e-9);
    }

    #[test]
    fn compose_cmp_tree_with_deps_rejects_unknown_parent() {
        let indices = vec![cmp_index(
            hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::CrudeOilPrice,
            hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
            hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::OneMonth,
            hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
            0.40,
            hkask_mcp_prediction_markets::cmp::CmpMethod::Interpolated,
        )];
        let deps = vec![CmpDependencySpec {
            child_id: "cmp:crude_oil_price:1m:increase".into(),
            parent_ids: vec!["cmp:nonexistent:1m:increase".into()],
            conditionals: vec![0.5, 0.5],
        }];
        let result = compose_cmp_tree_with_deps(
            &indices,
            chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap(),
            &deps,
        );
        assert!(result.is_err());
    }

    #[test]
    fn compose_cmp_tree_with_deps_no_deps_matches_flat_tree() {
        // With empty deps, compose_cmp_tree_with_deps should produce the same
        // result as compose_cmp_tree.
        let indices = vec![cmp_index(
            hkask_mcp_prediction_markets::economic_object::BaseEconomicObject::PolicyInterestRate,
            hkask_mcp_prediction_markets::cmp_index_builder::Venue::Kalshi,
            hkask_mcp_prediction_markets::cmp_portfolio::MaturityBucket::ThreeMonth,
            hkask_mcp_prediction_markets::cmp_portfolio::Orientation::Increase,
            0.65,
            hkask_mcp_prediction_markets::cmp::CmpMethod::Interpolated,
        )];
        let obs = chrono::NaiveDate::from_ymd_opt(2026, 8, 7).unwrap();
        let flat = compose_cmp_tree(&indices, obs).expect("flat");
        let with_deps = compose_cmp_tree_with_deps(&indices, obs, &[]).expect("with_deps");
        assert_eq!(flat.nodes.len(), with_deps.nodes.len());
        assert_eq!(flat.joint_probability, with_deps.joint_probability);
    }
}
